use crate::parsers::gobble::Gobble;
use anyhow::{Context, Result};
use colored::Colorize;
use std::path::Path;

/// Parses and extracts archives (.zip, .gz, .tgz) by unpacking them into
/// a temporary directory and recursively processing the contents.
pub struct ArchiveGobbler;

impl ArchiveGobbler {
    /// Extracts the archive and returns the file pairs, maintaining internal paths.
    pub fn extract_archive(
        &self,
        path: &Path,
        args: &crate::cli::Cli,
    ) -> Result<Vec<(String, String)>> {
        let ext = path
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_lowercase();

        if !args.quiet {
            eprintln!(
                "{} Extracting archive: {}",
                "📦".truecolor(0, 200, 255),
                path.display()
            );
        }

        let temp_dir =
            tempfile::tempdir().context("Failed to create temp directory for archive")?;

        if ext == "zip" {
            let file = std::fs::File::open(path).context("Failed to open zip archive")?;
            let mut archive = zip::ZipArchive::new(file).context("Failed to read zip archive")?;
            archive
                .extract(temp_dir.path())
                .context("Failed to extract zip archive")?;
        } else if ext == "gz" || ext == "tgz" {
            let file = std::fs::File::open(path).context("Failed to open tar.gz archive")?;
            let tar = flate2::read::GzDecoder::new(file);
            let mut archive = tar::Archive::new(tar);
            archive
                .unpack(temp_dir.path())
                .context("Failed to unpack tar.gz archive")?;
        } else {
            anyhow::bail!("Unsupported archive format: {}", ext);
        }

        let mut archive_args = args.clone();
        archive_args.horde = true; // Force horde to recurse

        let temp_path_str = temp_dir.path().to_string_lossy().to_string();
        // Since we are moving this out of lib.rs, we need to call crate::gobble_local or crate::routing::route_local_target
        // We will call `crate::gobble_local` for now, but update it to `crate::routing::route_local_target` once we extract routing.
        let mut extracted_pairs = crate::gobble_local(&temp_path_str, &archive_args)?;

        // Map ugly temp paths back to {archive_name}/{internal_path}
        let target_name = path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();

        for (p, _) in &mut extracted_pairs {
            if p == "_tree.md" {
                continue;
            }
            if let Some(stripped) = p.strip_prefix(&temp_path_str) {
                let cleaned = stripped.trim_start_matches(['/', '\\']);
                *p = format!("{}/{}", target_name, cleaned);
            }
        }

        Ok(extracted_pairs)
    }
}

impl Gobble for ArchiveGobbler {
    /// Implements the standard Gobble trait by extracting the archive and
    /// flattening the files into a single formatted string.
    fn gobble(&self, path: &Path, flags: &crate::cli::Cli) -> Result<String> {
        let pairs = self.extract_archive(path, flags)?;
        let mut out = String::new();
        for (p, c) in pairs {
            if p == "_tree.md" {
                out.push_str(&c);
            } else {
                out.push_str(&format!("// --- FILE_START: {} ---\n{}\n\n", p, c));
            }
        }
        Ok(out)
    }
}
