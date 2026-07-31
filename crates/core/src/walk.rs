//! Directory-walk helpers for `--horde`: tree rendering and glob filtering.
//! Extracted from `lib.rs` so each step is independently testable.

use colored::Colorize;
use globset::{Glob, GlobSet, GlobSetBuilder};
use std::path::{Path, PathBuf};

/// Walks `target` (respecting `.gitignore` via `ignore::WalkBuilder`) and returns a
/// rendered ASCII tree plus the list of files to process.
pub fn walk_tree(target: &str, max_depth: Option<usize>) -> (String, Vec<PathBuf>) {
    let mut walk_builder = ignore::WalkBuilder::new(target);
    if let Some(d) = max_depth {
        walk_builder.max_depth(Some(d));
    }
    // OS readdir order is arbitrary; sort so runs are deterministic (the --json
    // contract promises reproducible output, and golden tests depend on it).
    walk_builder.sort_by_file_name(|a, b| a.cmp(b));

    let mut tree_out = String::from("```tree\n.");
    let mut files = Vec::new();

    for entry in walk_builder.build().flatten() {
        let path = entry.path();
        let depth = entry.depth();
        let prefix = " ".repeat(depth * 2);
        let name = path.file_name().unwrap_or_default().to_string_lossy();
        if path.is_dir() {
            if depth > 0 {
                tree_out.push_str(&format!("\n{}├── {}/", prefix, name));
            }
        } else {
            tree_out.push_str(&format!("\n{}├── {}", prefix, name));
            files.push(path.to_path_buf());
        }
    }
    tree_out.push_str("\n```\n\n");
    (tree_out, files)
}

/// Translates a user-friendly pattern into a real glob string:
/// - explicit glob metacharacters are kept verbatim (`*.rs`, `*test*`, `src/**`)
/// - a leading-dot extension becomes a suffix glob (`.rs` → `*.rs`)
/// - a bare word becomes a substring glob (`test` → `*test*`)
fn normalize_pattern(pattern: &str) -> String {
    if pattern.contains(['*', '?', '[']) {
        pattern.to_string()
    } else if let Some(ext) = pattern.strip_prefix('.') {
        format!("*.{ext}")
    } else {
        format!("*{pattern}*")
    }
}

/// Builds a `GlobSet` from user patterns; unparseable patterns are skipped.
pub fn build_globset(patterns: &[String]) -> GlobSet {
    let mut builder = GlobSetBuilder::new();
    for p in patterns {
        if let Ok(glob) = Glob::new(&normalize_pattern(p)) {
            builder.add(glob);
        }
    }
    builder.build().unwrap_or_else(|_| GlobSet::empty())
}

/// Returns true if the file name (or its full path, for `/`-bearing patterns)
/// matches any glob in the set.
pub fn matches_globset(set: &GlobSet, path: &Path) -> bool {
    let name = path.file_name().unwrap_or_default();
    set.is_match(name) || set.is_match(path)
}

/// Applies `--include`/`--exclude` glob filters in place and logs the effect.
pub fn apply_glob_filters(
    files: &mut Vec<PathBuf>,
    include: &[String],
    exclude: &[String],
    quiet: bool,
) {
    if !include.is_empty() {
        let set = build_globset(include);
        let before = files.len();
        files.retain(|p| matches_globset(&set, p));
        if !quiet {
            eprintln!(
                "{}",
                format!(
                    "🎯 Filtered horde from {} to {} files matching: {}",
                    before,
                    files.len(),
                    include.join(", ")
                )
                .truecolor(0, 200, 255)
            );
        }
    }

    if !exclude.is_empty() {
        let set = build_globset(exclude);
        let before = files.len();
        files.retain(|p| !matches_globset(&set, p));
        if !quiet {
            eprintln!(
                "{}",
                format!(
                    "🚫 Excluded {} files matching: {}",
                    before - files.len(),
                    exclude.join(", ")
                )
                .truecolor(255, 99, 71)
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn glob_patterns_match_expected() {
        let set = build_globset(&["*.rs".to_string(), ".toml".to_string()]);
        assert!(matches_globset(&set, Path::new("src/main.rs")));
        assert!(matches_globset(&set, Path::new("Cargo.toml")));
        assert!(!matches_globset(&set, Path::new("readme.md")));
    }

    #[test]
    fn bare_word_is_substring() {
        let set = build_globset(&["test".to_string()]);
        assert!(matches_globset(&set, Path::new("my_test_file.rs")));
        assert!(!matches_globset(&set, Path::new("main.rs")));
    }
}
