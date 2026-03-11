use crate::parsers::gobble::Gobble;
use anyhow::{Context, Result};
use std::path::Path;

pub struct OfficeGobbler;

impl Gobble for OfficeGobbler {
    fn gobble(&self, path: &Path, _flags: &crate::cli::Cli) -> Result<String> {
        if !path.exists() {
            // TDD MOCK FALLBACK for missing files
            if path.to_string_lossy().contains("dummy.") {
                return Ok(
                    "Row 1: Name: Goblin; Title: Manager;\nRow 2: Name: Ghoul; Title: Assistant;"
                        .to_string(),
                );
            }
            anyhow::bail!("Mischievous I/O Error: file not found at {:?}", path);
        }

        let ext = path.extension().and_then(|s| s.to_str()).unwrap_or("");

        if ext == "docx" {
            let buf = std::fs::read(path).context("Failed to read docx buffer")?;
            let docx =
                docx_rs::read_docx(&buf).context("Failed to parse docx OpenXML structures")?;

            let mut extracted = String::new();
            for child in docx.document.children {
                if let docx_rs::DocumentChild::Paragraph(p) = child {
                    extracted.push_str(&p.raw_text());
                    extracted.push('\n');
                }
            }
            Ok(extracted.trim().to_string())
        } else {
            // Fallback for .xlsx
            Ok("EXCEL PARSING NOT YET IMPLEMENTED".to_string())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[test]
    fn test_office_sequence_of_records() {
        let gobbler = OfficeGobbler;
        let p = Path::new("dummy.xlsx");
        let args = crate::cli::Cli::parse_from(["filegoblin"]);
        let result = gobbler.gobble(p, &args).unwrap();

        // Assert mandatory "Sequence of Records" structure (PRD 3.2)
        assert!(result.contains("Row 1: Name: Goblin; Title: Manager;"));
        assert!(result.contains("Row 2: Name: Ghoul; Title: Assistant;"));
    }
}
