use crate::parsers::gobble::{Gobble, ParseOptions};
use anyhow::{Context, Result};
use std::path::Path;

pub struct EpubGobbler;

impl Gobble for EpubGobbler {
    fn gobble(&self, path: &Path, opts: &ParseOptions) -> Result<String> {
        let file = std::fs::File::open(path).context("Failed to open epub file")?;
        let mut archive =
            zip::ZipArchive::new(file).context("Failed to read epub as zip archive")?;

        // Reading order comes from the OPF spine. Sorting by filename instead
        // put `chapter10` before `chapter2`, so any book past nine chapters was
        // silently reordered into 1, 10, 11, …, 2.
        let spine = spine_order(&mut archive);

        let mut files: Vec<(String, String)> = Vec::new();
        for i in 0..archive.len() {
            let mut file = archive.by_index(i)?;
            let name = file.name().to_string();
            if name.ends_with(".html") || name.ends_with(".xhtml") || name.ends_with(".htm") {
                let content = crate::parsers::read_zip_entry_to_string(&mut file, &name)?;
                files.push((name, content));
            }
        }

        files.sort_by_key(|(name, _)| {
            // Spine order first; anything the spine doesn't list (stray files)
            // trails behind in a stable, natural-numeric order.
            (
                spine
                    .iter()
                    .position(|s| s == name || name.ends_with(s.as_str()))
                    .unwrap_or(usize::MAX),
                natural_key(name),
            )
        });

        // Chapters get their own `## <name>` headings below, so per-chapter
        // front-matter would only add noise.
        let html_gobbler = crate::parsers::html::HtmlGobbler {
            extract_full: opts.full,
            skip_front_matter: true,
            ..Default::default()
        };

        let mut output = String::new();
        let mut failed: Vec<String> = Vec::new();

        for (name, content) in files {
            // Reuse the HTML→markdown engine to strip tags and convert natively.
            match html_gobbler.gobble_str(&content, opts) {
                Ok(markdown) if !markdown.trim().is_empty() => {
                    output.push_str(&format!("\n\n## {}\n\n", name));
                    output.push_str(&markdown);
                }
                // A chapter that fails to convert used to vanish, leaving a
                // book that looks complete but isn't.
                Ok(_) => {}
                Err(e) => failed.push(format!("{}: {}", name, e)),
            }
        }

        if output.trim().is_empty() {
            if !failed.is_empty() {
                anyhow::bail!(
                    "Every chapter in this EPUB failed to convert:\n{}",
                    failed.join("\n")
                );
            }
            anyhow::bail!("No readable text found in this EPUB (DRM-protected or image-only?)");
        }
        if !failed.is_empty() {
            output.push_str(&format!(
                "\n\n> [{} chapter(s) could not be converted: {}]",
                failed.len(),
                failed.join("; ")
            ));
        }

        Ok(output.trim().to_string())
    }
}

/// Href list from the OPF spine, in reading order. Empty when the container or
/// the OPF can't be read — callers fall back to a natural-order sort.
fn spine_order<R: std::io::Read + std::io::Seek>(archive: &mut zip::ZipArchive<R>) -> Vec<String> {
    read_spine(archive).unwrap_or_default()
}

fn read_spine<R: std::io::Read + std::io::Seek>(
    archive: &mut zip::ZipArchive<R>,
) -> Option<Vec<String>> {
    let opf_name = (0..archive.len()).find_map(|i| {
        let name = archive.by_index(i).ok()?.name().to_string();
        name.ends_with(".opf").then_some(name)
    })?;
    let opf = {
        let mut entry = archive.by_name(&opf_name).ok()?;
        crate::parsers::read_zip_entry_to_string(&mut entry, &opf_name).ok()?
    };

    // manifest: id -> href, then spine: idref order.
    let mut manifest: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    let mut order: Vec<String> = Vec::new();
    let mut reader = quick_xml::Reader::from_str(&opf);
    loop {
        match reader.read_event() {
            Ok(quick_xml::events::Event::Start(ref e))
            | Ok(quick_xml::events::Event::Empty(ref e)) => {
                let attr = |key: &[u8]| {
                    e.attributes()
                        .flatten()
                        .find(|a| a.key.as_ref() == key)
                        .and_then(|a| a.unescape_value().ok().map(|v| v.into_owned()))
                };
                match e.name().as_ref() {
                    b"item" => {
                        if let (Some(id), Some(href)) = (attr(b"id"), attr(b"href")) {
                            manifest.insert(id, href);
                        }
                    }
                    b"itemref" => {
                        if let Some(idref) = attr(b"idref") {
                            order.push(idref);
                        }
                    }
                    _ => {}
                }
            }
            Ok(quick_xml::events::Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    Some(
        order
            .into_iter()
            .filter_map(|id| manifest.get(&id).cloned())
            .collect(),
    )
}

/// Sort key that orders `chapter2` before `chapter10` by comparing digit runs
/// numerically instead of lexicographically. Text compares before the number in
/// each segment, so `a2` still sorts before `b1`.
fn natural_key(name: &str) -> Vec<(String, u64)> {
    let mut out = Vec::new();
    let mut chars = name.chars().peekable();
    let mut text = String::new();
    while let Some(c) = chars.next() {
        if c.is_ascii_digit() {
            let mut num = c.to_digit(10).unwrap() as u64;
            while let Some(d) = chars.peek().and_then(|c| c.to_digit(10)) {
                num = num.saturating_mul(10).saturating_add(d as u64);
                chars.next();
            }
            out.push((std::mem::take(&mut text), num));
        } else {
            text.push(c);
        }
    }
    if !text.is_empty() {
        out.push((text, 0));
    }
    out
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

    fn write_epub(entries: &[(&str, &str)]) -> tempfile::NamedTempFile {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        {
            let mut zip = zip::ZipWriter::new(file.as_file_mut());
            let options = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Stored);
            for (name, body) in entries {
                zip.start_file(*name, options).unwrap();
                zip.write_all(body.as_bytes()).unwrap();
            }
            zip.finish().unwrap();
        }
        file
    }

    fn chapter(n: usize) -> String {
        format!("<html><body><h1>Chapter {n}</h1><p>Body of {n}.</p></body></html>")
    }

    /// Regression: filename sort put chapter10 before chapter2, silently
    /// reordering every book longer than nine chapters.
    #[test]
    fn double_digit_chapters_keep_reading_order() {
        let c2 = chapter(2);
        let c10 = chapter(10);
        let file = write_epub(&[("chapter10.xhtml", &c10), ("chapter2.xhtml", &c2)]);
        let args = crate::cli::Cli::parse_from(["filegoblin", "dummy"]);
        let result = EpubGobbler.gobble(file.path(), &(&args).into()).unwrap();

        let pos2 = result.find("Chapter 2").unwrap();
        let pos10 = result.find("Chapter 10").unwrap();
        assert!(pos2 < pos10, "chapter 2 must precede chapter 10:\n{result}");
    }

    /// The OPF spine wins over filenames when it is present.
    #[test]
    fn spine_order_beats_filename_order() {
        let intro = "<html><body><p>Intro text</p></body></html>";
        let body = "<html><body><p>Main text</p></body></html>";
        let opf = r#"<package><manifest>
                <item id="a" href="zzz_intro.xhtml"/>
                <item id="b" href="aaa_body.xhtml"/>
            </manifest><spine>
                <itemref idref="a"/>
                <itemref idref="b"/>
            </spine></package>"#;
        let file = write_epub(&[
            ("content.opf", opf),
            ("aaa_body.xhtml", body),
            ("zzz_intro.xhtml", intro),
        ]);
        let args = crate::cli::Cli::parse_from(["filegoblin", "dummy"]);
        let result = EpubGobbler.gobble(file.path(), &(&args).into()).unwrap();

        let intro_pos = result.find("Intro text").unwrap();
        let body_pos = result.find("Main text").unwrap();
        assert!(
            intro_pos < body_pos,
            "spine order must win over filenames:\n{result}"
        );
    }

    /// An EPUB with no readable chapters errors instead of returning a
    /// sentence that reads like document content.
    #[test]
    fn empty_epub_fails_loudly() {
        let file = write_epub(&[("cover.jpg", "not html")]);
        let args = crate::cli::Cli::parse_from(["filegoblin", "dummy"]);
        let err = EpubGobbler
            .gobble(file.path(), &(&args).into())
            .unwrap_err();
        assert!(err.to_string().contains("No readable text"), "got: {err}");
    }
}
