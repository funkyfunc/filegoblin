//! Content-addressed conversion cache under `~/.filegoblin/cache/<kind>/<sha256>`.
//!
//! Agents re-read the same sources constantly; URL fetches, OCR, and
//! transcription are the expensive steps worth remembering. Local parses are
//! cheap and NOT cached. `--no-cache` disables both read and write for a run.
//!
//! Kinds and freshness:
//! - `url`: keyed by URL; entries expire after [`URL_TTL`] (remote content
//!   changes) and stale files are swept opportunistically.
//! - `ocr` / `transcript`: keyed by a hash of the input bytes (+ model +
//!   render options), so entries never go *stale* — but they are not
//!   unbounded: a total-size budget evicts the least recently used entries.
//!   `filegoblin --cache-clear` empties everything.

use sha2::{Digest, Sha256};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};

static ENABLED: AtomicBool = AtomicBool::new(true);

const URL_TTL: std::time::Duration = std::time::Duration::from_secs(15 * 60);

/// Size budget for the content-addressed kinds (`ocr`, `transcript`). These
/// never expire by age, so without a budget they grew forever — the only
/// documented remedy was `rm -rf ~/.filegoblin/cache`.
const BLOB_CACHE_BUDGET_BYTES: u64 = 512 * 1024 * 1024;

/// Called once at process entry with `!args.no_cache`.
pub fn set_enabled(enabled: bool) {
    ENABLED.store(enabled, Ordering::Relaxed);
}

fn enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

/// Hex sha256 of arbitrary key material.
pub fn key(material: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(material);
    format!("{:x}", h.finalize())
}

fn entry_path(kind: &str, key: &str) -> Option<PathBuf> {
    let dir = home::home_dir()?
        .join(".filegoblin")
        .join("cache")
        .join(kind);
    Some(dir.join(key))
}

/// Reads a cached value. `ttl` of `None` means content-addressed (never stale).
fn get_with_ttl(kind: &str, key: &str, ttl: Option<std::time::Duration>) -> Option<Vec<u8>> {
    if !enabled() {
        return None;
    }
    let path = entry_path(kind, key)?;
    if let Some(ttl) = ttl {
        let age = std::fs::metadata(&path)
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.elapsed().ok())?;
        if age > ttl {
            let _ = std::fs::remove_file(&path);
            return None;
        }
    }
    std::fs::read(&path).ok()
}

/// Writes a cached value atomically (`.part` + rename); failures are silent —
/// caching is best-effort, never load-bearing.
fn put(kind: &str, key: &str, value: &[u8]) {
    if !enabled() {
        return;
    }
    let Some(path) = entry_path(kind, key) else {
        return;
    };
    let Some(dir) = path.parent() else { return };
    if std::fs::create_dir_all(dir).is_err() {
        return;
    }
    let part = path.with_extension("part");
    if std::fs::write(&part, value).is_ok() {
        let _ = std::fs::rename(&part, &path);
    }
}

pub fn get_url(url: &str) -> Option<Vec<u8>> {
    get_with_ttl("url", &key(url.as_bytes()), Some(URL_TTL))
}

pub fn put_url(url: &str, value: &[u8]) {
    put("url", &key(url.as_bytes()), value);
    sweep_stale_urls();
}

pub fn get_blob(kind: &str, k: &str) -> Option<String> {
    get_with_ttl(kind, k, None).and_then(|b| String::from_utf8(b).ok())
}

pub fn put_blob(kind: &str, k: &str, value: &str) {
    put(kind, k, value.as_bytes());
    enforce_blob_budget(kind);
}

/// Evicts least-recently-modified entries in `kind` until the directory fits
/// [`BLOB_CACHE_BUDGET_BYTES`].
fn enforce_blob_budget(kind: &str) {
    let Some(dir) = entry_path(kind, "").and_then(|p| p.parent().map(PathBuf::from)) else {
        return;
    };
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return;
    };

    let mut files: Vec<(std::time::SystemTime, u64, PathBuf)> = entries
        .flatten()
        .filter_map(|e| {
            let meta = e.metadata().ok()?;
            if !meta.is_file() {
                return None;
            }
            Some((
                meta.modified().unwrap_or(std::time::UNIX_EPOCH),
                meta.len(),
                e.path(),
            ))
        })
        .collect();

    let total: u64 = files.iter().map(|(_, len, _)| *len).sum();
    if total <= BLOB_CACHE_BUDGET_BYTES {
        return;
    }

    // Oldest first, deleting until we're back under budget.
    files.sort_by_key(|(mtime, _, _)| *mtime);
    let mut remaining = total;
    for (_, len, path) in files {
        if remaining <= BLOB_CACHE_BUDGET_BYTES {
            break;
        }
        if std::fs::remove_file(&path).is_ok() {
            remaining = remaining.saturating_sub(len);
        }
    }
}

/// Empties the whole cache. Backs `--cache-clear`.
pub fn clear_all() -> std::io::Result<u64> {
    let Some(root) = home::home_dir().map(|h| h.join(".filegoblin").join("cache")) else {
        return Ok(0);
    };
    if !root.exists() {
        return Ok(0);
    }
    let mut freed = 0u64;
    for kind in std::fs::read_dir(&root)?.flatten() {
        let Ok(entries) = std::fs::read_dir(kind.path()) else {
            continue;
        };
        for e in entries.flatten() {
            if let Ok(meta) = e.metadata()
                && meta.is_file()
                && std::fs::remove_file(e.path()).is_ok()
            {
                freed = freed.saturating_add(meta.len());
            }
        }
    }
    Ok(freed)
}

/// Deletes expired url entries so the cache can't grow unboundedly. Runs
/// opportunistically on writes; sweeps anything older than 4× the TTL.
fn sweep_stale_urls() {
    let Some(dir) = entry_path("url", "").and_then(|p| p.parent().map(PathBuf::from)) else {
        return;
    };
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let stale = e
            .metadata()
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.elapsed().ok())
            .is_some_and(|age| age > URL_TTL * 4);
        if stale {
            let _ = std::fs::remove_file(e.path());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blob_roundtrip_and_disable() {
        let k = key(b"cache-test-input");
        put_blob("test", &k, "cached value");
        assert_eq!(get_blob("test", &k).as_deref(), Some("cached value"));

        set_enabled(false);
        assert_eq!(get_blob("test", &k), None);
        set_enabled(true);

        // Cleanup
        if let Some(p) = entry_path("test", &k) {
            let _ = std::fs::remove_file(p);
        }
    }

    /// The TTL path is what keeps remote content from going stale forever;
    /// it had no test at all, so a broken comparison would have been invisible.
    #[test]
    fn ttl_expiry_evicts_and_content_addressed_entries_survive() {
        let k = key(b"ttl-test-input");
        put("test", &k, b"value");

        // A TTL far in the future keeps the entry.
        assert!(get_with_ttl("test", &k, Some(std::time::Duration::from_secs(3600))).is_some());
        // A zero TTL treats anything already written as expired, and removes it.
        assert!(get_with_ttl("test", &k, Some(std::time::Duration::ZERO)).is_none());
        assert!(
            get_with_ttl("test", &k, None).is_none(),
            "an expired entry must actually be deleted, not just reported missing"
        );

        // Content-addressed reads (ttl None) never expire.
        put("test", &k, b"value");
        assert!(get_with_ttl("test", &k, None).is_some());
        if let Some(p) = entry_path("test", &k) {
            let _ = std::fs::remove_file(p);
        }
    }

    #[test]
    fn keys_are_stable_hex() {
        assert_eq!(key(b"x").len(), 64);
        assert_eq!(key(b"x"), key(b"x"));
        assert_ne!(key(b"x"), key(b"y"));
    }
}
