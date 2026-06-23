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

fn extract_text_from_slide_xml(xml: &str) -> String {
    use quick_xml::Reader;
    use quick_xml::events::Event;

    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);

    let mut output = String::new();
    let mut in_text_node = false;

    // Table state trackers
    let mut in_table = false;
    let mut current_row_idx = 0;
    let mut current_col_idx = 0;
    let mut current_cell_text = String::new();
    let mut headers: Vec<String> = Vec::new();

    loop {
        match reader.read_event() {
            Ok(Event::Start(ref e)) | Ok(Event::Empty(ref e)) => {
                match e.name().as_ref() {
                    b"a:tbl" => {
                        in_table = true;
                        current_row_idx = 0;
                        output.push('\n'); // Pad before table
                    }
                    b"a:tr" if in_table => {
                        current_col_idx = 0;
                        if current_row_idx > 0 {
                            output.push_str(&format!("\nRow {}: ", current_row_idx));
                        }
                    }
                    b"a:tc" if in_table => {
                        current_cell_text.clear();
                    }
                    b"a:t" => in_text_node = true,
                    b"a:p" if !in_table => output.push_str("\n- "), // Start of a paragraph/bullet outside tables
                    _ => (),
                }
            }
            Ok(Event::Text(e)) => {
                if in_text_node {
                    // Try to decode as UTF-8 string, manually replacing common XML escapes
                    let raw_str = std::str::from_utf8(e.as_ref()).unwrap_or_default();
                    let decoded = raw_str
                        .replace("&amp;", "&")
                        .replace("&lt;", "<")
                        .replace("&gt;", ">")
                        .replace("&quot;", "\"")
                        .replace("&apos;", "'");

                    if in_table {
                        current_cell_text.push_str(&decoded);
                    } else {
                        output.push_str(&decoded);
                    }
                }
            }
            Ok(Event::End(ref e)) => match e.name().as_ref() {
                b"a:tbl" => {
                    in_table = false;
                    headers.clear();
                    output.push('\n'); // Pad after table
                }
                b"a:tr" if in_table => {
                    current_row_idx += 1;
                }
                b"a:tc" if in_table => {
                    let text = current_cell_text.trim().to_string();
                    if current_row_idx == 0 {
                        // Header row
                        headers.push(if text.is_empty() {
                            format!("Col {}", current_col_idx)
                        } else {
                            text
                        });
                    } else {
                        let header = headers
                            .get(current_col_idx)
                            .cloned()
                            .unwrap_or_else(|| format!("Col {}", current_col_idx));
                        output.push_str(&format!("{}: {}; ", header, text));
                    }
                    current_col_idx += 1;
                }
                b"a:t" => in_text_node = false,
                b"a:p" if !in_table => output.push('\n'),
                _ => (),
            },
            Ok(Event::Eof) => break,
            Err(_) => break, // Silently ignore malformed XML chunks
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_powerpoint_table_extraction() {
        let xml = r#"
            <a:p><a:r><a:t>Regular Text</a:t></a:r></a:p>
            <a:tbl>
                <a:tr>
                    <a:tc><a:txBody><a:p><a:r><a:t>Header1</a:t></a:r></a:p></a:txBody></a:tc>
                    <a:tc><a:txBody><a:p><a:r><a:t>Header2</a:t></a:r></a:p></a:txBody></a:tc>
                </a:tr>
                <a:tr>
                    <a:tc><a:txBody><a:p><a:r><a:t>Value1</a:t></a:r></a:p></a:txBody></a:tc>
                    <a:tc><a:txBody><a:p><a:r><a:t>Value2</a:t></a:r></a:p></a:txBody></a:tc>
                </a:tr>
            </a:tbl>
        "#;

        let result = extract_text_from_slide_xml(xml);
        assert!(result.contains("Regular Text"));
        assert!(result.contains("Row 1: Header1: Value1; Header2: Value2;"));
    }
}
