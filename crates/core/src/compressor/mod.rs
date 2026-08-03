pub mod heuristic;
pub mod level1;
pub mod level2;
use crate::cli::CompressionLevel;
use std::borrow::Cow;

pub trait TokenTransformer {
    /// Applies the transformation to a markdown string segment
    fn transform<'a>(&self, input: &'a str) -> Cow<'a, str>;
}

pub struct CompressionPipeline {
    transformers: Vec<Box<dyn TokenTransformer>>,
}

/// Maps a file path to the language name [`level2::CommentStripper`] expects.
///
/// Two call sites passed the *path* straight in as the "language", so
/// `CommentStripper::new("src/main.rs")` matched nothing and contextual
/// comment-stripping was a silent no-op in streaming mode and in the
/// `--max-tokens` skeletonization fallback — while the buffered path, which
/// passes a real language from markdown code fences, did the work. Same flags,
/// different output, depending only on whether something forced buffering.
pub fn language_for_path(path: &str) -> Option<&'static str> {
    let ext = std::path::Path::new(path)
        .extension()
        .and_then(|e| e.to_str())?
        .to_ascii_lowercase();
    Some(match ext.as_str() {
        "rs" => "rust",
        "js" | "mjs" | "cjs" | "jsx" => "js",
        "ts" | "tsx" => "ts",
        "c" | "h" => "c",
        "cpp" | "cc" | "cxx" | "hpp" => "cpp",
        "java" => "java",
        "go" => "go",
        _ => return None,
    })
}

impl CompressionPipeline {
    pub fn new(level: &CompressionLevel, language: Option<&str>) -> Self {
        let mut transformers: Vec<Box<dyn TokenTransformer>> = Vec::new();

        // Level 1 (Safe) - Always applied if compression is on
        transformers.push(Box::new(level1::TrailingWhitespaceFolder));
        transformers.push(Box::new(level1::NewlineDeduplicator));

        if *level == CompressionLevel::Contextual {
            // Level 2
            if let Some(lang) = language {
                transformers.push(Box::new(level2::CommentStripper::new(lang)));
            }
            transformers.push(Box::new(level2::Minifier));
        }

        Self { transformers }
    }

    pub fn process(&self, input: &str) -> String {
        let mut result = Cow::Borrowed(input);
        for transformer in &self.transformers {
            // We need to allow the cow to be owned by the loop iteration, so we convert to a new Cow
            let transformed = transformer.transform(result.as_ref()).into_owned();
            result = Cow::Owned(transformed);
        }

        result.into_owned()
    }
}

#[cfg(test)]
mod tests;
