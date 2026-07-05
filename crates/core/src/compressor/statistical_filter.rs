use crate::compressor::TokenTransformer;
use compression_prompt::statistical_filter::{StatisticalFilter, StatisticalFilterConfig};
use std::borrow::Cow;

pub struct StatFilter {
    filter: StatisticalFilter,
}

impl StatFilter {
    pub fn new() -> Self {
        // Initialize the StatisticalFilter from compression-prompt crate
        Self {
            filter: StatisticalFilter::new(StatisticalFilterConfig::default()),
        }
    }
}

impl Default for StatFilter {
    fn default() -> Self {
        Self::new()
    }
}

impl TokenTransformer for StatFilter {
    fn transform<'a>(&self, input: &'a str) -> Cow<'a, str> {
        // Apply the statistical filter
        let compressed = self.filter.compress(input);

        // If it compressed, return owned; else borrowed
        if compressed.len() < input.len() {
            Cow::Owned(compressed)
        } else {
            Cow::Borrowed(input)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_statistical_filtering() {
        let transformer = StatFilter::new();
        let input = "This is a very long and verbose sentence that contains many unnecessary words that can be safely removed without losing the core semantic meaning of the sentence.";
        let result = transformer.transform(input);

        // Ensure it compressed the text
        assert!(result.len() < input.len());
    }

    #[test]
    fn test_statistical_filtering_protects_code() {
        let transformer = StatFilter::new();
        let input = "```rust\nfn main() {\n    println!(\"Hello World\");\n}\n```";
        let result = transformer.transform(input);

        // Depending on the crate's implementation, code blocks might be fully protected
        // or slightly modified, but it shouldn't be completely mangled.
        // We will just ensure it doesn't crash and returns something reasonable.
        assert!(result.contains("fn main()"));
        assert!(result.contains("println!"));
    }
}
