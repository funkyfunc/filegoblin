use anyhow::{Context, Result};
use colored::Colorize;
use std::path::Path;

/// Maximum total uncompressed bytes we will write when extracting a single archive.
/// Guards against decompression bombs (e.g. 42.zip).
const MAX_TOTAL_UNCOMPRESSED: u64 = 2 * 1024 * 1024 * 1024; // 2 GiB
/// Maximum number of entries we will extract from a single archive.
const MAX_ENTRIES: usize = 100_000;

/// Parses and extracts archives (.zip, .tar, .gz, .tgz) by unpacking them into
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
        let name = path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        self.extract_archive_as(path, &name, args)
    }

    /// [`Self::extract_archive`] with the name members are reported under —
    /// the URL, for an archive fetched over HTTP, rather than its temp file.
    pub fn extract_archive_as(
        &self,
        path: &Path,
        display_name: &str,
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
        if !matches!(ext.as_str(), "zip" | "gz" | "tgz" | "tar") {
            let header = crate::format::read_header(path, 512);
            ext = if header.starts_with(b"PK") {
                "zip".to_string()
            } else if header.starts_with(&[0x1f, 0x8b]) {
                "gz".to_string()
            } else if header.get(257..262) == Some(b"ustar".as_slice()) {
                "tar".to_string()
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
            "gz" | "tgz" => extract_gzip(path, temp_dir.path())?,
            "tar" => extract_tar(path, temp_dir.path())?,
            other => anyhow::bail!("Unsupported archive format: {}", other),
        }

        let mut archive_args = args.clone();
        archive_args.horde = true; // Force horde to recurse into the extracted tree.
        archive_args.no_nested_archives = true; // ...but do not extract inner archives.

        let temp_path_str = temp_dir.path().to_string_lossy().to_string();
        let mut extracted = crate::gobble_local(&temp_path_str, &archive_args)?;

        // Report members as {archive_name}/{internal_path}. The walk already
        // yields paths relative to the temp dir, so the old remapping (which
        // only fired on a temp-dir prefix) never ran: two archives that both
        // held a README.md produced two documents named `README.md`, and
        // `--split` wrote the second over the first.
        for doc in &mut extracted {
            if doc.path == "_tree.md" {
                continue;
            }
            let inner = doc.path.strip_prefix(&temp_path_str).unwrap_or(&doc.path);
            doc.path = format!("{}/{}", display_name, inner.trim_start_matches(['/', '\\']));
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

/// Handles both shapes a `.gz` can take: a gzipped **tar**, and a single
/// gzipped **file** (`access.log.gz`), which is extremely common and used to
/// hit the tar reader and fail with a misleading "Corrupt tar entry".
fn extract_gzip(path: &Path, dest: &Path) -> Result<()> {
    match extract_tar_gz(path, dest) {
        // Zero entries is not a tarball: a gzipped file that starts with two
        // NUL blocks reads as an empty tar, and the run printed an empty tree.
        Ok(0) => extract_single_gz(path, dest),
        Ok(_) => Ok(()),
        Err(tar_err) => match extract_single_gz(path, dest) {
            Ok(()) => Ok(()),
            // Report the tar failure: if it is neither, "corrupt tar" is the
            // more informative of the two for a `.tgz`.
            Err(_) => Err(tar_err),
        },
    }
}

/// Inflates a single-member gzip to `dest/<stem>`, bounded by the same cap.
fn extract_single_gz(path: &Path, dest: &Path) -> Result<()> {
    extract_single_gz_capped(path, dest, MAX_TOTAL_UNCOMPRESSED)
}

fn extract_single_gz_capped(path: &Path, dest: &Path, cap: u64) -> Result<()> {
    use std::io::Read;
    let file = std::fs::File::open(path).context("Failed to open gzip file")?;
    let mut decoder = flate2::read::GzDecoder::new(file);

    // Strip the .gz: access.log.gz -> access.log
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "decompressed".to_string());

    // Straight to disk: buffering the whole member first held up to the 2 GiB
    // cap in memory on top of the copy the parser then reads back (a 7 MB
    // `.log.gz` peaked at 3.2 GB resident).
    let mut out =
        std::fs::File::create(dest.join(stem)).context("Failed to write decompressed file")?;
    let written = std::io::copy(&mut decoder.by_ref().take(cap + 1), &mut out)
        .context("Failed to inflate gzip file")?;
    if written > cap {
        anyhow::bail!(
            "Gzip inflates past the {} byte uncompressed limit (bomb guard)",
            cap
        );
    }
    if written == 0 {
        anyhow::bail!("Gzip file decompressed to nothing");
    }
    Ok(())
}

/// Extracts a gzipped tar through the same capped unpacker as a plain tar.
fn extract_tar_gz(path: &Path, dest: &Path) -> Result<usize> {
    let file = std::fs::File::open(path).context("Failed to open tar.gz archive")?;
    unpack_tar(flate2::read::GzDecoder::new(file), dest)
}

/// Extracts an uncompressed tar. The README always listed tar, but a plain
/// `.tar` was never routed here and failed as "plaintext".
fn extract_tar(path: &Path, dest: &Path) -> Result<()> {
    let file = std::fs::File::open(path).context("Failed to open tar archive")?;
    unpack_tar(file, dest).map(|_| ())
}

/// Unpacks a tar stream with entry-count and total-size caps. `tar`'s `unpack`
/// handles `..` and absolute-path traversal internally; we bound the volume.
/// Returns how many entries it unpacked.
fn unpack_tar<R: std::io::Read>(reader: R, dest: &Path) -> Result<usize> {
    let mut archive = tar::Archive::new(reader);

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
        // `unpack_in` returns Ok(false) for a path that escapes `dest`. That
        // return value used to be discarded, so a hostile tar produced an
        // archive quietly missing files — while zip loudly bails on exactly the
        // same condition. Match the zip behaviour.
        let entry_path = entry
            .path()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|_| "<unreadable>".to_string());
        if !entry
            .unpack_in(dest)
            .context("Failed to unpack tar entry")?
        {
            anyhow::bail!("Refusing tar entry with unsafe path: {}", entry_path);
        }
    }
    Ok(count)
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

    fn make_tar(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut builder = tar::Builder::new(Vec::new());
        for (name, data) in entries {
            let mut header = tar::Header::new_ustar();
            header.set_size(data.len() as u64);
            header.set_mode(0o644);
            header.set_cksum();
            builder.append_data(&mut header, name, *data).unwrap();
        }
        builder.into_inner().unwrap()
    }

    /// Regression: a plain `.tar` failed with "Failed to read file as plaintext".
    #[test]
    fn plain_tar_extracts() {
        let tar_bytes = make_tar(&[("d/x.txt", b"hello tar\n"), ("d/y.md", b"# y\n")]);
        let archive = tempfile::Builder::new().suffix(".tar").tempfile().unwrap();
        std::fs::write(archive.path(), &tar_bytes).unwrap();
        let dest = tempfile::tempdir().unwrap();

        extract_tar(archive.path(), dest.path()).unwrap();
        let x = std::fs::read_to_string(dest.path().join("d/x.txt")).unwrap();
        assert_eq!(x, "hello tar\n");
        assert!(dest.path().join("d/y.md").exists());
    }

    /// The whole route: a `.tar` (or an extension-less one, by its ustar
    /// magic) is extracted and its entries converted, like a zip.
    #[test]
    fn tar_routes_through_the_archive_path() {
        use clap::Parser;
        let tar_bytes = make_tar(&[("d/x.txt", b"hello tar\n")]);
        let dir = tempfile::tempdir().unwrap();
        for name in ["t.tar", "t_no_ext"] {
            let path = dir.path().join(name);
            std::fs::write(&path, &tar_bytes).unwrap();
            let args = crate::cli::Cli::parse_from(["filegoblin", "-q", "dummy"]);
            let docs = crate::gobble_local(&path.to_string_lossy(), &args).unwrap();
            let doc = docs
                .iter()
                .find(|d| d.path.ends_with("d/x.txt"))
                .unwrap_or_else(|| panic!("{name}: no d/x.txt document in {docs:?}"));
            assert!(doc.error.is_none(), "{name}: {:?}", doc.error);
            assert!(doc.content.contains("hello tar"), "{name}");
        }
    }

    /// Regression: a tar symlink to a file outside the archive was extracted
    /// as a link and then read, so `notes.txt -> /etc/hosts` printed the
    /// host's file as if it were archive content.
    #[cfg(unix)]
    #[test]
    fn tar_symlinks_cannot_read_files_outside_the_archive() {
        use clap::Parser;
        let outside = tempfile::tempdir().unwrap();
        let secret = outside.path().join("secret.txt");
        std::fs::write(&secret, "TOP SECRET OUTSIDE THE ARCHIVE").unwrap();

        let mut builder = tar::Builder::new(Vec::new());
        let mut header = tar::Header::new_ustar();
        header.set_size(6);
        header.set_mode(0o644);
        header.set_cksum();
        builder
            .append_data(&mut header, "readme.txt", &b"hello\n"[..])
            .unwrap();
        for (name, target) in [
            ("abs.txt", secret.to_string_lossy().into_owned()),
            ("rel.txt", "../../../../../../../../etc/hosts".to_string()),
        ] {
            let mut link = tar::Header::new_ustar();
            link.set_entry_type(tar::EntryType::Symlink);
            link.set_size(0);
            builder.append_link(&mut link, name, target).unwrap();
        }
        let dir = tempfile::tempdir().unwrap();
        let archive = dir.path().join("links.tar");
        std::fs::write(&archive, builder.into_inner().unwrap()).unwrap();

        let args = crate::cli::Cli::parse_from(["filegoblin", "-q", "dummy"]);
        let docs = crate::gobble_local(&archive.to_string_lossy(), &args).unwrap();
        let all: String = docs.iter().map(|d| d.content.as_str()).collect();
        assert!(all.contains("hello"), "{docs:?}");
        assert!(!all.contains("TOP SECRET"), "{docs:?}");
        assert!(!all.contains("localhost"), "{docs:?}");
    }

    /// Regression: members were named by their inner path alone, so two
    /// archives holding a `README.md` collided (and `--split` overwrote one).
    #[test]
    fn members_are_named_under_their_archive() {
        use clap::Parser;
        let dir = tempfile::tempdir().unwrap();
        let tar_bytes = make_tar(&[("README.md", b"alpha\n")]);
        let archive = dir.path().join("alpha.tar");
        std::fs::write(&archive, &tar_bytes).unwrap();
        let args = crate::cli::Cli::parse_from(["filegoblin", "-q", "dummy"]);
        let docs = crate::gobble_local(&archive.to_string_lossy(), &args).unwrap();
        let paths: Vec<&str> = docs.iter().map(|d| d.path.as_str()).collect();
        assert!(paths.contains(&"alpha.tar/README.md"), "{paths:?}");
    }

    #[test]
    fn single_gzip_cap_counts_actual_bytes() {
        use std::io::Write;
        let dir = tempfile::tempdir().unwrap();
        let gz = dir.path().join("big.log.gz");
        let mut enc =
            flate2::write::GzEncoder::new(std::fs::File::create(&gz).unwrap(), Default::default());
        enc.write_all(&vec![b'a'; 64 * 1024]).unwrap();
        enc.finish().unwrap();

        let dest = tempfile::tempdir().unwrap();
        let err = extract_single_gz_capped(&gz, dest.path(), 1024).unwrap_err();
        assert!(err.to_string().contains("bomb guard"), "got: {err}");
        extract_single_gz_capped(&gz, dest.path(), 1 << 20).unwrap();
        assert_eq!(
            std::fs::metadata(dest.path().join("big.log"))
                .unwrap()
                .len(),
            64 * 1024
        );
    }

    /// Regression: a gzipped file whose first 1 KiB is NUL reads as an empty
    /// tar, and the single-file fallback never ran.
    #[test]
    fn gzip_that_reads_as_an_empty_tar_is_a_single_file() {
        use std::io::Write;
        let dir = tempfile::tempdir().unwrap();
        let gz = dir.path().join("dump.bin.gz");
        let mut payload = vec![0u8; 4096];
        payload.extend_from_slice(b"tail text");
        let mut enc =
            flate2::write::GzEncoder::new(std::fs::File::create(&gz).unwrap(), Default::default());
        enc.write_all(&payload).unwrap();
        enc.finish().unwrap();

        let dest = tempfile::tempdir().unwrap();
        extract_gzip(&gz, dest.path()).unwrap();
        assert_eq!(
            std::fs::read(dest.path().join("dump.bin")).unwrap(),
            payload
        );
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
