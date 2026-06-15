use super::TokenTransformer;
use std::borrow::Cow;
use stop_words::{LANGUAGE, get};

pub struct StopwordPruner;

impl TokenTransformer for StopwordPruner {
    fn transform<'a>(&self, input: &'a str) -> Cow<'a, str> {
        let stop_words = get(LANGUAGE::English);

        // Note: For prose, we only want to strip from the raw text, not the structural markers.
        // It's safer to tokenize by words and re-assemble.
        let mut result = String::with_capacity(input.len());

        for word in input.split_whitespace() {
            let cleaned = word
                .to_lowercase()
                .trim_matches(|c: char| !c.is_alphanumeric())
                .to_string();

            if !stop_words.iter().any(|s| s == &cleaned) {
                result.push_str(word);
                result.push(' ');
            }
        }

        if result.is_empty() {
            Cow::Borrowed(input) // fallback if something goes weird
        } else {
            Cow::Owned(result.trim_end().to_string())
        }
    }
}
