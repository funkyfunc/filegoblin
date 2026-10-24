use anyhow::Result;
use std::path::Path;

/// The subset of CLI options a parser may need. Parsers take this instead of the
/// whole `Cli` struct so they stay decoupled from the command-line surface and
/// remain reusable as a library / from other front-ends.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct ParseOptions {
    /// Extract the full document instead of any heuristic minification (`--full`).
    pub full: bool,
    /// Structural extraction mode (`--extract`).
    pub extract: Option<crate::cli::ExtractMode>,
    /// Suppress auxiliary stderr notes (`--quiet`).
    pub quiet: bool,
    /// This HTML already came out of a headless-browser render, so a thin page
    /// must not suggest `--browser` again — which it did, verbatim, on pages it
    /// had just rendered.
    pub already_rendered: bool,
    /// When PDFs use the vision models (`--vision`).
    pub vision: crate::cli::VisionMode,
}

impl From<&crate::cli::Cli> for ParseOptions {
    fn from(cli: &crate::cli::Cli) -> Self {
        Self {
            full: cli.full,
            extract: cli.extract,
            quiet: cli.quiet,
            already_rendered: false,
            vision: cli.vision,
        }
    }
}

/// The primary trait that all `filegoblin` document parsers must implement.
///
/// This ensures a unified interface for ingesting diverse file formats into
/// a target string representation (Markdown, XML, YAML).
pub trait Gobble {
    /// Consumes a file at the given path and returns the extracted, structured string.
    fn gobble(&self, path: &Path, opts: &ParseOptions) -> Result<String>;

    /// Consumes an in-memory string directly and returns the structured string.
    fn gobble_str(&self, _content: &str, _opts: &ParseOptions) -> Result<String> {
        anyhow::bail!("gobble_str natively unsupported by this target")
    }
}
