use super::*;
use crate::cli::CompressionLevel;

#[test]
fn test_level1_whitespace_folding() {
    let input = "Paragraph 1. \n\n\n\n Paragraph 2.  \n";
    let pipeline = CompressionPipeline::new(&CompressionLevel::Safe, None);
    let output = pipeline.process(input);
    assert_eq!(output, "Paragraph 1.\n\nParagraph 2.\n");
}

#[test]
fn test_level2_rust_comment_stripping() {
    let input = "/// Core rust\nfn main() { // comment here\n/* block */\n}";
    let pipeline = CompressionPipeline::new(&CompressionLevel::Contextual, Some("rust"));
    let output = pipeline.process(input);
    // Doc comments should remain, line/block comments should disappear
    assert_eq!(output, "/// Core rust\nfn main() { \n\n}");
}

#[test]
fn test_level2_json_minification() {
    let input = "{\n  \"key\": \"value\" \n}";
    let pipeline = CompressionPipeline::new(&CompressionLevel::Contextual, None);
    let output = pipeline.process(input);
    assert_eq!(output, "{\"key\":\"value\"}");
}

/// Regression: the streaming path and the `--max-tokens` fallback passed a
/// *path* where a language was expected, so `CommentStripper::new("x.rs")`
/// matched nothing and comment stripping silently did nothing there.
#[test]
fn language_is_resolved_from_a_file_path() {
    assert_eq!(language_for_path("src/main.rs"), Some("rust"));
    assert_eq!(language_for_path("/abs/path/app.tsx"), Some("ts"));
    assert_eq!(language_for_path("lib.cpp"), Some("cpp"));
    assert_eq!(language_for_path("notes.md"), None);
    assert_eq!(language_for_path("no_extension"), None);

    // End to end: the same content compressed via a path must lose its
    // comment, exactly as it does when the language is named directly.
    let input = "fn main() { // strip me\n}";
    let by_path = CompressionPipeline::new(
        &CompressionLevel::Contextual,
        language_for_path("src/main.rs"),
    )
    .process(input);
    assert!(!by_path.contains("strip me"), "got: {by_path}");
}

/// Regression: `r#*.*#*` matched any token starting with `r` through to the end
/// of the line, so a comment after `return`/`ref`/`result` survived.
#[test]
fn comments_after_r_identifiers_are_stripped() {
    let input = "fn f() -> u8 {\n    return 1; // secret comment\n}";
    let output =
        CompressionPipeline::new(&CompressionLevel::Contextual, Some("rust")).process(input);
    assert!(!output.contains("secret comment"), "got: {output}");
    assert!(output.contains("return 1;"), "code must survive: {output}");
}

/// A real raw string is still left intact.
#[test]
fn raw_strings_survive_comment_stripping() {
    let input = "let re = r\"//not a comment\";";
    let output =
        CompressionPipeline::new(&CompressionLevel::Contextual, Some("rust")).process(input);
    assert!(output.contains("//not a comment"), "got: {output}");
}

/// `/* … **/` could not be matched by the old block-comment pattern.
#[test]
fn block_comments_ending_in_double_star_are_stripped() {
    let input = "fn a() {}\n/* doc block **/\nfn b() {}";
    let output =
        CompressionPipeline::new(&CompressionLevel::Contextual, Some("rust")).process(input);
    assert!(!output.contains("doc block"), "got: {output}");
}
