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
