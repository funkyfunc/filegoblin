use crate::parsers::gobble::{Gobble, ParseOptions};
use anyhow::{Context, Result};
use std::io::Read;
use std::path::Path;

pub struct EpubGobbler;

impl Gobble for EpubGobbler {
    fn gobble(&self, path: &Path, opts: &ParseOptions) -> Result<String> {
        let file = std::fs::File::open(path).context("Failed to open epub file")?;
        let mut archive =
            zip::ZipArchive::new(file).context("Failed to read epub as zip archive")?;

        let mut files: Vec<(String, String)> = Vec::new();

        for i in 0..archive.len() {
            let mut file = archive.by_index(i)?;
            let name = file.name().to_string();
            if name.ends_with(".html") || name.ends_with(".xhtml") || name.ends_with(".htm") {
                let mut content = String::new();
                if file.read_to_string(&mut content).is_ok() {
                    files.push((name, content));
                }
            }
        }

        // Sort by filename to keep chapter order relatively intact without parsing OPF spine
        files.sort_by(|a, b| a.0.cmp(&b.0));

        let html_gobbler = crate::parsers::html::HtmlGobbler {
            extract_full: opts.full,
        };

        let mut output = String::new();

        for (name, content) in files {
            // Reuse the HTML→markdown engine to strip tags and convert natively.
            if let Ok(markdown) = html_gobbler.gobble_str(&content, opts)
                && !markdown.trim().is_empty()
            {
                output.push_str(&format!("\n\n## {}\n\n", name));
                output.push_str(&markdown);
            }
        }

        if output.trim().is_empty() {
            return Ok("No readable text found in EPUB.".to_string());
        }

        Ok(output.trim().to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;
    use std::io::Write;

    #[test]
    fn test_epub_parsing() {
        // Create a dummy zip archive representing an epub
        let mut file = tempfile::NamedTempFile::new().unwrap();
        {
            let mut zip = zip::ZipWriter::new(file.as_file_mut());
            let options = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Stored);

            zip.start_file("chapter1.xhtml", options).unwrap();
            zip.write_all(
                b"<html><body><h1>Chapter 1</h1><p>It was the best of times.</p></body></html>",
            )
            .unwrap();

            zip.start_file("chapter2.html", options).unwrap();
            zip.write_all(
                b"<html><body><h1>Chapter 2</h1><p>It was the worst of times.</p></body></html>",
            )
            .unwrap();

            zip.finish().unwrap();
        }

        let gobbler = EpubGobbler;
        let args = crate::cli::Cli::parse_from(["filegoblin", "dummy"]);
        let result = gobbler.gobble(file.path(), &(&args).into()).unwrap();

        assert!(result.contains("## chapter1.xhtml"));
        assert!(result.contains("Chapter 1"));
        assert!(result.contains("It was the best of times."));
        assert!(result.contains("## chapter2.html"));
        assert!(result.contains("Chapter 2"));
        assert!(result.contains("It was the worst of times."));
    }
}
