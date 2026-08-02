//! Content-addressed conversion cache under `~/.filegoblin/cache/<kind>/<sha256>`.
//!
//! Agents re-read the same sources constantly; URL fetches, OCR, and
//! transcription are the expensive steps worth remembering. Local parses are
//! cheap and NOT cached. `--no-cache` disables both read and write for a run.
//!
//! Kinds and freshness:
//! - `url`: keyed by URL; entries expire after [`URL_TTL`] (remote content
//!   changes) and stale files are swept opportunistically.
//! - `ocr` / `transcript`: keyed by a hash of the input bytes (+ model), so
//!   entries never go stale and live until manually cleared.

use sha2::{Digest, Sha256};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};

static ENABLED: AtomicBool = AtomicBool::new(true);

const URL_TTL: std::time::Duration = std::time::Duration::from_secs(15 * 60);

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

    #[test]
    fn keys_are_stable_hex() {
        assert_eq!(key(b"x").len(), 64);
        assert_eq!(key(b"x"), key(b"x"));
        assert_ne!(key(b"x"), key(b"y"));
    }
}
