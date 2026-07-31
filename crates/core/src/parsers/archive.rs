use anyhow::{Context, Result};
use colored::Colorize;
use std::path::Path;

/// Maximum total uncompressed bytes we will write when extracting a single archive.
/// Guards against decompression bombs (e.g. 42.zip).
const MAX_TOTAL_UNCOMPRESSED: u64 = 2 * 1024 * 1024 * 1024; // 2 GiB
/// Maximum number of entries we will extract from a single archive.
const MAX_ENTRIES: usize = 100_000;

/// Parses and extracts archives (.zip, .gz, .tgz) by unpacking them into
/// a temporary directory and recursively processing the contents.
pub struct ArchiveGobbler;

impl ArchiveGobbler {
    /// Extracts the archive and returns the extracted documents, remapping internal
    /// paths to `{archive_name}/{internal_path}`.
    pub fn extract_archive(
        &self,
        path: &Path,
        args: &crate::cli::Cli,
    ) -> Result<Vec<crate::pipeline::GobbleDocument>> {
        // Refuse archives nested inside other archives. `gobble_local` forces
        // `--horde` when recursing into an extracted temp dir, so without this
        // guard a zip-quine (or deeply nested archive) would recurse unbounded.
        if args.no_nested_archives {
            anyhow::bail!(
                "Refusing to extract a nested archive (bomb/quine guard): {}",
                path.display()
            );
        }

        // Dispatch by extension, falling back to magic bytes so renamed /
        // extension-less archives (routed here by format detection) still extract.
        let mut ext = path
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_lowercase();
        if !matches!(ext.as_str(), "zip" | "gz" | "tgz") {
            let header = crate::format::read_header(path, 4);
            ext = if header.starts_with(b"PK") {
                "zip".to_string()
            } else if header.starts_with(&[0x1f, 0x8b]) {
                "gz".to_string()
            } else {
                ext
            };
        }

        if !args.quiet {
            eprintln!(
                "{} Extracting archive: {}",
                "📦".truecolor(0, 200, 255),
                path.display()
            );
        }

        let temp_dir =
            tempfile::tempdir().context("Failed to create temp directory for archive")?;

        match ext.as_str() {
            "zip" => extract_zip(path, temp_dir.path())?,
            "gz" | "tgz" => extract_tar_gz(path, temp_dir.path())?,
            other => anyhow::bail!("Unsupported archive format: {}", other),
        }

        let mut archive_args = args.clone();
        archive_args.horde = true; // Force horde to recurse into the extracted tree.
        archive_args.no_nested_archives = true; // ...but do not extract inner archives.

        let temp_path_str = temp_dir.path().to_string_lossy().to_string();
        let mut extracted = crate::gobble_local(&temp_path_str, &archive_args)?;

        // Map ugly temp paths back to {archive_name}/{internal_path}
        let target_name = path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();

        for doc in &mut extracted {
            if doc.path == "_tree.md" {
                continue;
            }
            if let Some(stripped) = doc.path.strip_prefix(&temp_path_str) {
                let cleaned = stripped.trim_start_matches(['/', '\\']);
                doc.path = format!("{}/{}", target_name, cleaned);
            }
        }

        Ok(extracted)
    }
}

/// Extracts a zip archive with per-entry size accounting and a zip-slip guard.
fn extract_zip(path: &Path, dest: &Path) -> Result<()> {
    extract_zip_capped(path, dest, MAX_TOTAL_UNCOMPRESSED)
}

fn extract_zip_capped(path: &Path, dest: &Path, cap: u64) -> Result<()> {
    use std::io::Read;

    let file = std::fs::File::open(path).context("Failed to open zip archive")?;
    let mut archive = zip::ZipArchive::new(file).context("Failed to read zip archive")?;

    if archive.len() > MAX_ENTRIES {
        anyhow::bail!(
            "Archive has {} entries, exceeding the {} limit (bomb guard)",
            archive.len(),
            MAX_ENTRIES
        );
    }

    let mut total_written: u64 = 0;
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i)?;

        // Zip-slip guard: reject entries whose sanitized path escapes `dest`.
        let Some(rel) = entry.enclosed_name() else {
            anyhow::bail!("Refusing zip entry with unsafe path: {}", entry.name());
        };
        let out_path = dest.join(&rel);

        if entry.is_dir() {
            std::fs::create_dir_all(&out_path).ok();
            continue;
        }

        if let Some(parent) = out_path.parent() {
            std::fs::create_dir_all(parent).ok();
        }
        let mut out = std::fs::File::create(&out_path)
            .with_context(|| format!("Failed to create {}", out_path.display()))?;

        // Bomb guard on bytes ACTUALLY decompressed — never on the entry's
        // declared size, which is attacker-controlled metadata a bomb can
        // simply lie about. `take(remaining + 1)` bounds the write; landing
        // past the cap means the archive inflates beyond the budget.
        let remaining = cap.saturating_sub(total_written);
        let written = std::io::copy(&mut (&mut entry).take(remaining + 1), &mut out)
            .context("Failed while writing zip entry")?;
        total_written = total_written.saturating_add(written);
        if total_written > cap {
            anyhow::bail!(
                "Archive inflates past the {} byte uncompressed limit (bomb guard)",
                cap
            );
        }
    }
    Ok(())
}

/// Extracts a gzipped tar with a total-size cap. `tar`'s `unpack` handles `..`
/// and absolute-path traversal internally; we bound the decompressed volume.
fn extract_tar_gz(path: &Path, dest: &Path) -> Result<()> {
    let file = std::fs::File::open(path).context("Failed to open tar.gz archive")?;
    let decoder = flate2::read::GzDecoder::new(file);
    let mut archive = tar::Archive::new(decoder);

    let mut total: u64 = 0;
    let mut count: usize = 0;
    for entry in archive.entries().context("Failed to read tar entries")? {
        let mut entry = entry.context("Corrupt tar entry")?;
        count += 1;
        if count > MAX_ENTRIES {
            anyhow::bail!(
                "Archive exceeds the {} entry limit (bomb guard)",
                MAX_ENTRIES
            );
        }
        // Unlike zip, tar's header size IS the write bound: `unpack_in` copies
        // exactly `entry.size()` bytes from the stream, so summing declared
        // sizes here bounds the actual bytes written.
        total = total.saturating_add(entry.size());
        if total > MAX_TOTAL_UNCOMPRESSED {
            anyhow::bail!(
                "Archive exceeds the {} GiB uncompressed limit (bomb guard)",
                MAX_TOTAL_UNCOMPRESSED / (1024 * 1024 * 1024)
            );
        }
        // `unpack_in` refuses paths that escape `dest` (returns Ok(false)).
        entry
            .unpack_in(dest)
            .context("Failed to unpack tar entry")?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn make_zip(entries: &[(&str, &[u8])]) -> tempfile::NamedTempFile {
        let mut file = tempfile::Builder::new().suffix(".zip").tempfile().unwrap();
        {
            let mut zip = zip::ZipWriter::new(file.as_file_mut());
            let options = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Deflated);
            for (name, data) in entries {
                zip.start_file(*name, options).unwrap();
                zip.write_all(data).unwrap();
            }
            zip.finish().unwrap();
        }
        file
    }

    #[test]
    fn zip_bomb_cap_counts_actual_bytes() {
        // 64 KiB of zeros compresses to almost nothing; a 1 KiB cap must trip
        // on ACTUAL inflation, regardless of what any header declares.
        let payload = vec![0u8; 64 * 1024];
        let zip = make_zip(&[("big.txt", &payload)]);
        let dest = tempfile::tempdir().unwrap();

        let err = extract_zip_capped(zip.path(), dest.path(), 1024).unwrap_err();
        assert!(err.to_string().contains("bomb guard"), "got: {err}");
    }

    #[test]
    fn honest_zip_extracts_within_cap() {
        let zip = make_zip(&[("dir/note.md", b"# hello goblin\n")]);
        let dest = tempfile::tempdir().unwrap();

        extract_zip(zip.path(), dest.path()).unwrap();
        let content = std::fs::read_to_string(dest.path().join("dir/note.md")).unwrap();
        assert_eq!(content, "# hello goblin\n");
    }
}
