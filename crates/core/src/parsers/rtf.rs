use crate::parsers::gobble::Gobble;
use anyhow::{Context, Result};
use rtf_parser::{Lexer, Parser};
use std::path::Path;

/// Extracts plain text from Rich Text Format (`.rtf`) documents by lexing and
/// parsing the RTF control words and returning only the body text.
pub struct RtfGobbler;

impl Gobble for RtfGobbler {
    fn gobble(&self, path: &Path, _flags: &crate::cli::Cli) -> Result<String> {
        // RTF is ASCII-based control words with escaped Unicode; read leniently.
        let bytes = std::fs::read(path).context("Failed to read RTF file")?;
        let raw = String::from_utf8_lossy(&bytes);

        let tokens = Lexer::scan(&raw)
            .map_err(|e| anyhow::anyhow!("Mischievous RTF lexing error: {:?}", e))?;
        let document = Parser::new(tokens)
            .parse()
            .map_err(|e| anyhow::anyhow!("Mischievous RTF parsing error: {:?}", e))?;

        let text = document.get_text();
        if text.trim().is_empty() {
            return Ok("No text could be extracted from this RTF document.".to_string());
        }
        Ok(text.trim().to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser as _;
    use std::io::Write;

    #[test]
    fn test_rtf_text_extraction() {
        // Minimal valid RTF containing "Hello Goblin".
        let rtf = r"{\rtf1\ansi\deff0 {\fonttbl {\f0 Times New Roman;}} \f0\fs24 Hello Goblin}";
        let mut f = tempfile::Builder::new().suffix(".rtf").tempfile().unwrap();
        f.write_all(rtf.as_bytes()).unwrap();

        let gobbler = RtfGobbler;
        let args = crate::cli::Cli::parse_from(["filegoblin", "dummy"]);
        let result = gobbler.gobble(f.path(), &args).unwrap();
        assert!(result.contains("Hello Goblin"), "got: {result}");
    }

    #[test]
    fn test_rtf_missing_file_errors() {
        let gobbler = RtfGobbler;
        let args = crate::cli::Cli::parse_from(["filegoblin", "dummy"]);
        assert!(gobbler.gobble(Path::new("nope.rtf"), &args).is_err());
    }
}
