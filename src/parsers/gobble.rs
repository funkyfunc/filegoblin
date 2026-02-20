use std::path::Path;
use anyhow::Result;

/// The primary trait that all `filegoblin` document parsers must implement.
///
/// This ensures a unified interface for ingesting diverse file formats into
/// a target string representation (Markdown, XML, YAML) based on the chosen output flavor.
pub trait Gobble {
    /// Consumes a file at the given path and returns the extracted, structured string.
    fn gobble(&self, path: &Path) -> Result<String>;
    
    /// Consumes an in-memory string directly and returns the structured string.
    fn gobble_str(&self, _content: &str) -> Result<String> {
        anyhow::bail!("gobble_str natively unsupported by this target")
    }
}
