//! Optional JSON configuration for the Privacy Shield: an allowlist of values
//! never to redact, plus user-defined custom patterns. Loaded explicitly via
//! `--scrub-config <path>` or auto-discovered from `./.filegoblin-scrub.json`.
//!
//! Example `.filegoblin-scrub.json`:
//! ```json
//! {
//!   "allowlist": ["support@example.com", "AKIAIOSFODNN7EXAMPLE"],
//!   "patterns": [
//!     { "name": "employee_id", "regex": "EMP-[0-9]{6}" }
//!   ]
//! }
//! ```

use serde::Deserialize;
use std::path::Path;

/// Default config file discovered in the current directory when no path is given.
pub const DEFAULT_CONFIG_FILE: &str = ".filegoblin-scrub.json";

#[derive(Debug, Deserialize, Default, Clone)]
pub struct ScrubConfig {
    /// Exact values that must never be redacted.
    #[serde(default)]
    pub allowlist: Vec<String>,
    /// User-defined regex rules, each redacted under its own `[NAME]` label.
    #[serde(default)]
    pub patterns: Vec<CustomPattern>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct CustomPattern {
    pub name: String,
    pub regex: String,
}

impl ScrubConfig {
    /// Loads a config from an explicit path, or auto-discovers the default file in
    /// the current directory. Returns `Ok(None)` when neither exists. A malformed
    /// file is a hard error (the user asked for it); a missing default is not.
    pub fn discover(explicit: Option<&str>) -> anyhow::Result<Option<Self>> {
        match explicit {
            Some(path) => Ok(Some(Self::load(Path::new(path))?)),
            None => {
                let default = Path::new(DEFAULT_CONFIG_FILE);
                if default.exists() {
                    Ok(Some(Self::load(default)?))
                } else {
                    Ok(None)
                }
            }
        }
    }

    fn load(path: &Path) -> anyhow::Result<Self> {
        let text = std::fs::read_to_string(path).map_err(|e| {
            anyhow::anyhow!("Failed to read scrub config {}: {}", path.display(), e)
        })?;
        serde_json::from_str(&text)
            .map_err(|e| anyhow::anyhow!("Invalid scrub config {}: {}", path.display(), e))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn parses_allowlist_and_patterns() {
        let json =
            r#"{ "allowlist": ["a@b.com"], "patterns": [{"name":"emp","regex":"EMP-[0-9]+"}] }"#;
        let cfg: ScrubConfig = serde_json::from_str(json).unwrap();
        assert_eq!(cfg.allowlist, vec!["a@b.com"]);
        assert_eq!(cfg.patterns.len(), 1);
        assert_eq!(cfg.patterns[0].name, "emp");
    }

    #[test]
    fn empty_object_is_valid() {
        let cfg: ScrubConfig = serde_json::from_str("{}").unwrap();
        assert!(cfg.allowlist.is_empty());
        assert!(cfg.patterns.is_empty());
    }

    #[test]
    fn discover_explicit_missing_errors() {
        assert!(ScrubConfig::discover(Some("/no/such/scrub.json")).is_err());
    }

    #[test]
    fn discover_reads_explicit_file() {
        let mut f = tempfile::Builder::new().suffix(".json").tempfile().unwrap();
        write!(f, r#"{{ "allowlist": ["x"] }}"#).unwrap();
        let cfg = ScrubConfig::discover(Some(f.path().to_str().unwrap()))
            .unwrap()
            .unwrap();
        assert_eq!(cfg.allowlist, vec!["x"]);
    }
}
