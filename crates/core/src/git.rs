//! Thin, injection-safe wrappers around the `git` CLI used by `--git-diff`.

use anyhow::{Result, bail};
use std::path::Path;
use std::process::Command;

/// Rejects refs that could be interpreted as `git` option flags.
///
/// `git diff <ref>` requires the revision before any `--` separator, so we cannot
/// neutralize a leading `-` positionally; instead we refuse it outright. This blocks
/// tricks like `--output=<path>` (which `git diff` honors) reaching the process.
pub fn validate_ref(git_ref: &str) -> Result<()> {
    if git_ref.starts_with('-') {
        bail!(
            "Refusing git ref '{git_ref}' — it looks like a command-line option, not a revision."
        );
    }
    Ok(())
}

/// Returns the list of files changed vs `git_ref` (paths relative to `root`).
pub fn changed_files(root: &Path, git_ref: &str) -> Result<Vec<String>> {
    validate_ref(git_ref)?;
    let output = Command::new("git")
        .args(["diff", "--name-only", git_ref])
        .current_dir(root)
        .output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("git diff failed: {}", stderr.trim());
    }

    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(|l| l.to_string())
        .collect())
}

/// Returns the unified diff for a single file vs `git_ref`, or `None` if empty/failed.
/// The file path is passed after `--` so it can never be read as an option.
pub fn file_diff(root: &Path, git_ref: &str, file: &Path) -> Option<String> {
    validate_ref(git_ref).ok()?;
    let file_str = file.to_str()?;
    let output = Command::new("git")
        .args(["diff", git_ref, "--", file_str])
        .current_dir(root)
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }
    let diff = String::from_utf8_lossy(&output.stdout).to_string();
    if diff.trim().is_empty() {
        None
    } else {
        Some(diff)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_option_like_refs() {
        assert!(validate_ref("--output=/tmp/x").is_err());
        assert!(validate_ref("-p").is_err());
        assert!(validate_ref("HEAD").is_ok());
        assert!(validate_ref("main~3").is_ok());
    }
}
