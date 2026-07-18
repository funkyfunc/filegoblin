/// A single extracted document flowing through the pipeline. This is the one
/// document type used end-to-end: parsers, routing, curation, and formatting all
/// speak `GobbleDocument` (no `(String, String)` pair boundary), so per-file
/// metadata — detected format, byte size, and parse errors — rides along instead
/// of being discarded and re-derived.
#[derive(Debug, Clone, Default)]
pub struct GobbleDocument {
    pub path: String,
    pub content: String,
    /// Coarse detected format label (e.g. `"pdf"`, `"email"`); see `crate::format`.
    pub format: Option<String>,
    /// Original file size in bytes, when known.
    pub bytes: Option<u64>,
    /// Set when extraction failed; `content` is empty in that case. Surfaced as an
    /// in-band `{path, error}` record in `--json` rather than silently dropped.
    pub error: Option<String>,
    pub token_count: Option<usize>,
    pub relevance_score: Option<f32>,
}

impl GobbleDocument {
    pub fn new(path: String, content: String) -> Self {
        Self {
            path,
            content,
            ..Default::default()
        }
    }

    /// A successfully extracted document with format/size metadata.
    pub fn extracted(path: String, content: String, format: &str, bytes: Option<u64>) -> Self {
        Self {
            path,
            content,
            format: Some(format.to_string()),
            bytes,
            ..Default::default()
        }
    }

    /// A failed extraction: no content, an error message, and the detected format.
    pub fn failed(path: String, format: &str, error: String) -> Self {
        Self {
            path,
            content: String::new(),
            format: Some(format.to_string()),
            error: Some(error),
            ..Default::default()
        }
    }

    /// True when this document represents a successful, non-empty extraction.
    pub fn is_content(&self) -> bool {
        self.error.is_none() && !self.content.trim().is_empty() && self.path != "_tree.md"
    }

    /// Estimated LLM token count for this document's current content: the cached
    /// `token_count` when set, otherwise a fresh heuristic estimate. Centralizes the
    /// estimate so callers don't re-implement the fallback everywhere.
    pub fn tokens(&self) -> usize {
        self.token_count.unwrap_or_else(|| {
            crate::compressor::heuristic::estimate_tokens(&self.content, &self.path)
        })
    }
}
