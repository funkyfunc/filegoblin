pub mod heuristic;
pub mod level1;
pub mod level2;
pub mod level3;

use crate::cli::CompressionLevel;
use std::borrow::Cow;

pub trait TokenTransformer {
    /// Applies the transformation to a markdown string segment
    fn transform<'a>(&self, input: &'a str) -> Cow<'a, str>;
}

pub struct CompressionPipeline {
    transformers: Vec<Box<dyn TokenTransformer>>,
}

impl CompressionPipeline {
    pub fn new(level: &CompressionLevel, language: Option<&str>) -> Self {
        let mut transformers: Vec<Box<dyn TokenTransformer>> = Vec::new();

        // Level 1 (Safe) - Always applied if compression is on
        transformers.push(Box::new(level1::TrailingWhitespaceFolder));
        transformers.push(Box::new(level1::NewlineDeduplicator));

        if *level == CompressionLevel::Contextual || *level == CompressionLevel::Aggressive {
            // Level 2
            if let Some(lang) = language {
                transformers.push(Box::new(level2::CommentStripper::new(lang)));
            }
            transformers.push(Box::new(level2::Minifier));
        }

        if *level == CompressionLevel::Aggressive {
            // Level 3 (Aggressive)
            transformers.push(Box::new(level3::StopwordPruner));
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
