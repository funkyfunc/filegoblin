use crate::parsers::gobble::Gobble;
use std::io::Read;
use std::path::Path;

pub struct PptxGobbler;

impl Gobble for PptxGobbler {
    fn gobble(&self, path: &Path, _flags: &crate::cli::Cli) -> anyhow::Result<String> {
        let file = std::fs::File::open(path)?;
        let mut archive = zip::ZipArchive::new(file)?;

        let mut slide_contents: Vec<(usize, String)> = Vec::new();

        for i in 0..archive.len() {
            let mut file = archive.by_index(i)?;
            let name = file.name().to_string();

            // We only care about slide files: "ppt/slides/slideN.xml"
            if name.starts_with("ppt/slides/slide") && name.ends_with(".xml") {
                // Extract the slide number N
                let num_str = name
                    .trim_start_matches("ppt/slides/slide")
                    .trim_end_matches(".xml");

                if let Ok(slide_num) = num_str.parse::<usize>() {
                    let mut xml_content = String::new();
                    file.read_to_string(&mut xml_content)?;

                    let text = extract_text_from_slide_xml(&xml_content);
                    if !text.trim().is_empty() {
                        slide_contents.push((slide_num, text));
                    }
                }
            }
        }

        // Sort by slide number
        slide_contents.sort_by_key(|(num, _)| *num);

        let mut output = String::new();
        for (num, content) in slide_contents {
            output.push_str(&format!("## Slide {}\n\n{}", num, content));
            output.push_str("\n\n---\n\n");
        }

        if output.is_empty() {
            return Ok("No text could be extracted from this presentation.".to_string());
        }

        Ok(output)
    }
}

/// A simple quick-xml reader that grabs all text inside <a:t> nodes
fn extract_text_from_slide_xml(xml: &str) -> String {
    use quick_xml::Reader;
    use quick_xml::events::Event;

    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);

    let mut output = String::new();
    let mut in_text_node = false;

    // A paragraph usually contains multiple runs. We'll add newlines after a paragraph.
    // For simplicity, we just extract text strings sequentially.
    loop {
        match reader.read_event() {
            Ok(Event::Start(ref e)) | Ok(Event::Empty(ref e)) => {
                match e.name().as_ref() {
                    b"a:t" => in_text_node = true,
                    b"a:p" => output.push_str("\n- "), // Start of a paragraph/bullet
                    _ => (),
                }
            }
            Ok(Event::Text(e)) => {
                if in_text_node {
                    // Try to decode as UTF-8 string, manually replacing common XML escapes if quick-xml's unescape isn't available
                    let raw_str = std::str::from_utf8(e.as_ref()).unwrap_or_default();
                    let decoded = raw_str
                        .replace("&amp;", "&")
                        .replace("&lt;", "<")
                        .replace("&gt;", ">")
                        .replace("&quot;", "\"")
                        .replace("&apos;", "'");
                    output.push_str(&decoded);
                }
            }
            Ok(Event::End(ref e)) => match e.name().as_ref() {
                b"a:t" => in_text_node = false,
                b"a:p" => output.push('\n'),
                _ => (),
            },
            Ok(Event::Eof) => break,
            Err(_) => break, // Silently ignore malformed XML chunks rather than crashing the whole horde
            _ => (),
        }
    }

    // Clean up excessive newlines/bullets generated from empty paragraphs
    output
        .lines()
        .filter(|line| !line.trim().is_empty() && line.trim() != "-")
        .collect::<Vec<&str>>()
        .join("\n")
}
