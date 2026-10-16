//! `--install-models`: downloads this version's PDF vision model archive
//! (`filegoblin-vision-models.tar.gz`, about 500 MB) from the GitHub release,
//! checks its SHA-256 against the published `.sha256`, and unpacks it into
//! `~/.filegoblin/models` or a directory of the user's choosing.
//!
//! `FILEGOBLIN_MODELS_URL` replaces the download URL (the `.sha256` is
//! fetched from the same URL plus `.sha256`), for internal mirrors and
//! air-gapped installs.
//!
//! Nothing here runs unless the user asks for it: filegoblin never downloads
//! models on its own.

use anyhow::{Context, Result, bail};
use std::path::{Component, Path, PathBuf};

/// `~/.filegoblin/models`.
pub fn default_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(|h| PathBuf::from(h).join(".filegoblin").join("models"))
}

/// The archive for this version, or `FILEGOBLIN_MODELS_URL`.
pub fn archive_url() -> String {
    std::env::var("FILEGOBLIN_MODELS_URL").unwrap_or_else(|_| {
        format!(
            "https://github.com/funkyfunc/filegoblin/releases/download/v{}/filegoblin-vision-models.tar.gz",
            env!("CARGO_PKG_VERSION")
        )
    })
}

/// Where an archive entry goes inside the install directory: the archive's
/// top-level `models/` is dropped, and anything that is not a plain relative
/// path (absolute, `..`) is refused.
fn entry_target(dir: &Path, entry: &Path) -> Option<PathBuf> {
    let mut parts: Vec<&std::ffi::OsStr> = Vec::new();
    for c in entry.components() {
        match c {
            Component::Normal(p) => parts.push(p),
            Component::CurDir => {}
            _ => return None,
        }
    }
    if parts.first().is_some_and(|p| *p == "models") {
        parts.remove(0);
    }
    if parts.is_empty() || parts.iter().any(|p| p.to_string_lossy().starts_with('.')) {
        return None;
    }
    Some(parts.iter().fold(dir.to_path_buf(), |acc, p| acc.join(p)))
}

/// Unpacks a `.tar.gz` model archive into `dir`.
fn unpack(archive: &Path, dir: &Path) -> Result<usize> {
    let file = std::fs::File::open(archive)?;
    let mut tar = tar::Archive::new(flate2::read::GzDecoder::new(file));
    let mut n = 0;
    for entry in tar.entries()? {
        let mut entry = entry?;
        if !entry.header().entry_type().is_file() {
            continue;
        }
        let path = entry.path()?.into_owned();
        let Some(target) = entry_target(dir, &path) else {
            continue;
        };
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)?;
        }
        // Write beside the target, then rename: an interrupted install never
        // leaves a truncated model behind under the real name.
        let tmp = target.with_extension("partial");
        let mut out = std::fs::File::create(&tmp)?;
        std::io::copy(&mut entry, &mut out)?;
        drop(out);
        std::fs::rename(&tmp, &target)?;
        n += 1;
    }
    Ok(n)
}

/// Verifies (against `<archive>.sha256` when it exists) and unpacks a local
/// model archive into `dir`.
fn install_local(archive: &Path, dir: &Path, quiet: bool) -> Result<usize> {
    use sha2::{Digest, Sha256};
    let sha_file = PathBuf::from(format!("{}.sha256", archive.display()));
    if let Ok(text) = std::fs::read_to_string(&sha_file) {
        let expected = text
            .split_whitespace()
            .next()
            .unwrap_or("")
            .to_ascii_lowercase();
        let mut hasher = Sha256::new();
        std::io::copy(&mut std::fs::File::open(archive)?, &mut hasher)?;
        let actual = format!("{:x}", hasher.finalize());
        if actual != expected {
            bail!(
                "checksum mismatch for {}: expected {expected}, got {actual}",
                archive.display()
            );
        }
    } else if !quiet {
        eprintln!(
            "(no {} beside the archive: checksum not verified)",
            sha_file.display()
        );
    }
    unpack(archive, dir).context("cannot unpack the model archive")
}

/// Downloads (or, for a local path or `file://` URL, reads), verifies and
/// unpacks the model archive into `dir` (default `~/.filegoblin/models`).
/// Returns the directory.
pub fn install(dir: Option<PathBuf>, quiet: bool) -> Result<PathBuf> {
    let dir = match dir {
        Some(d) => d,
        None => {
            default_dir().context("cannot tell where the home directory is; pass a directory")?
        }
    };
    std::fs::create_dir_all(&dir).with_context(|| format!("cannot create {}", dir.display()))?;
    let url = archive_url();
    let local = url.strip_prefix("file://").map(PathBuf::from).or_else(|| {
        let p = PathBuf::from(&url);
        p.is_file().then_some(p)
    });
    let n = match local {
        Some(archive) => install_local(&archive, &dir, quiet)?,
        None => download_and_unpack(&url, &dir, quiet)?,
    };
    if !quiet {
        eprintln!("Installed {n} files into {}", dir.display());
    }
    Ok(dir)
}

#[cfg(not(feature = "web"))]
fn download_and_unpack(url: &str, _dir: &Path, _quiet: bool) -> Result<usize> {
    bail!(
        "this build has no network support. Download {url} and set FILEGOBLIN_MODELS_URL \
         to the downloaded file (with its .sha256 beside it)"
    )
}

#[cfg(feature = "web")]
fn download_and_unpack(url: &str, dir: &Path, quiet: bool) -> Result<usize> {
    use sha2::{Digest, Sha256};
    use std::io::{Read, Write};

    let client = reqwest::blocking::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(30))
        .timeout(None)
        .user_agent(concat!("filegoblin/", env!("CARGO_PKG_VERSION")))
        .build()?;

    let sha_url = format!("{url}.sha256");
    let resp = client
        .get(&sha_url)
        .send()
        .with_context(|| format!("cannot reach {sha_url}"))?;
    if !resp.status().is_success() {
        bail!(
            "no model archive at {url} (HTTP {}). This version may not have a published release yet: \
             set FILEGOBLIN_MODELS_URL to an archive, or use the filegoblin-full download",
            resp.status()
        );
    }
    let expected = resp
        .text()?
        .split_whitespace()
        .next()
        .map(str::to_ascii_lowercase)
        .context("empty .sha256 file")?;

    let mut resp = client
        .get(url)
        .send()
        .with_context(|| format!("cannot reach {url}"))?;
    if !resp.status().is_success() {
        bail!("download failed: {url} (HTTP {})", resp.status());
    }
    let total = resp.content_length();
    let tmp = dir.join(format!(".download-{}.tar.gz", std::process::id()));
    let mut out = std::fs::File::create(&tmp)?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    let (mut got, mut shown) = (0u64, 0u64);
    if !quiet {
        eprintln!("Downloading {url}");
    }
    loop {
        let n = resp.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
        out.write_all(&buf[..n])?;
        got += n as u64;
        if !quiet && got - shown >= 16 << 20 {
            shown = got;
            match total {
                Some(t) => eprint!("\r  {} / {} MB", got >> 20, t >> 20),
                None => eprint!("\r  {} MB", got >> 20),
            }
        }
    }
    drop(out);
    if !quiet {
        eprintln!("\r  {} MB downloaded", got >> 20);
    }
    let actual = format!("{:x}", hasher.finalize());
    if actual != expected {
        let _ = std::fs::remove_file(&tmp);
        bail!("checksum mismatch for {url}: expected {expected}, got {actual}");
    }
    let n = unpack(&tmp, dir);
    let _ = std::fs::remove_file(&tmp);
    n.context("cannot unpack the model archive")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn archive_entries_stay_inside_the_install_dir() {
        let d = Path::new("/x/models");
        assert_eq!(
            entry_target(d, Path::new("models/pp-doclayout-v3.onnx")),
            Some(PathBuf::from("/x/models/pp-doclayout-v3.onnx"))
        );
        assert_eq!(
            entry_target(d, Path::new("./LICENSES.md")),
            Some(PathBuf::from("/x/models/LICENSES.md"))
        );
        assert_eq!(entry_target(d, Path::new("models/../../etc/passwd")), None);
        assert_eq!(entry_target(d, Path::new("/etc/passwd")), None);
        assert_eq!(entry_target(d, Path::new("models/.work/x")), None);
        assert_eq!(entry_target(d, Path::new("models/")), None);
    }

    #[test]
    fn an_archive_unpacks_without_its_top_directory() {
        let base = std::env::temp_dir().join(format!("fg-install-{}", std::process::id()));
        std::fs::create_dir_all(&base).unwrap();
        let archive = base.join("m.tar.gz");
        {
            let gz = flate2::write::GzEncoder::new(
                std::fs::File::create(&archive).unwrap(),
                flate2::Compression::fast(),
            );
            let mut b = tar::Builder::new(gz);
            let mut h = tar::Header::new_gnu();
            h.set_size(3);
            h.set_mode(0o644);
            h.set_cksum();
            b.append_data(&mut h, "models/a.onnx", &b"abc"[..]).unwrap();
            b.into_inner().unwrap().finish().unwrap();
        }
        let out = base.join("out");
        assert_eq!(unpack(&archive, &out).unwrap(), 1);
        assert_eq!(std::fs::read(out.join("a.onnx")).unwrap(), b"abc");
        let _ = std::fs::remove_dir_all(&base);
    }
}
