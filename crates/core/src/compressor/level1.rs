use super::TokenTransformer;
use regex::Regex;
use std::borrow::Cow;
use std::sync::LazyLock;

static NEWLINE_REGEX: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\n{3,}").unwrap());
static TRAILING_SPACE_REGEX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?m)^[ \t]+|[ \t]+$").unwrap());

pub struct NewlineDeduplicator;

impl TokenTransformer for NewlineDeduplicator {
    fn transform<'a>(&self, input: &'a str) -> Cow<'a, str> {
        NEWLINE_REGEX.replace_all(input, "\n\n")
    }
}

pub struct TrailingWhitespaceFolder;

impl TokenTransformer for TrailingWhitespaceFolder {
    fn transform<'a>(&self, input: &'a str) -> Cow<'a, str> {
        TRAILING_SPACE_REGEX.replace_all(input, "")
    }
}
