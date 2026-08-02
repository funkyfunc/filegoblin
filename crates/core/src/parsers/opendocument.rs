use crate::parsers::gobble::{Gobble, ParseOptions};
use anyhow::{Context, Result};
use std::io::Read;
use std::path::Path;

/// OpenDocument text (`.odt`) and presentation (`.odp`) → markdown. Both are
/// zip containers with a `content.xml`; the same walker handles headings
/// (`text:h` with outline level), paragraphs, nested lists, tables (as pipe
/// tables), and `draw:page` slide boundaries.
pub struct OpenDocGobbler;

impl Gobble for OpenDocGobbler {
    fn gobble(&self, path: &Path, _opts: &ParseOptions) -> Result<String> {
        let file = std::fs::File::open(path).context("Failed to open OpenDocument file")?;
        let mut archive = zip::ZipArchive::new(file)
            .context("Not a valid OpenDocument container (expected a zip)")?;

        let mut content = String::new();
        archive
            .by_name("content.xml")
            .context("Missing content.xml — not an OpenDocument file")?
            .read_to_string(&mut content)
            .context("Failed to read content.xml")?;

        let md = content_xml_to_md(&content);
        if md.trim().is_empty() {
            anyhow::bail!("No text could be extracted from this OpenDocument file");
        }
        Ok(md.trim().to_string())
    }
}

fn content_xml_to_md(xml: &str) -> String {
    use quick_xml::Reader;
    use quick_xml::events::Event;

    let mut reader = Reader::from_str(xml);
    let mut out = String::new();

    // Block state
    let mut line = String::new();
    let mut heading_level: Option<usize> = None;
    let mut list_depth = 0usize;
    let mut slide_count = 0usize;

    // Table state (rendered as markdown pipe tables, row 0 = header)
    let mut in_table = false;
    let mut table_row_idx = 0usize;
    let mut row_cells: Vec<String> = Vec::new();
    let mut cell_text = String::new();

    let flush_block = |out: &mut String,
                       line: &mut String,
                       heading_level: &mut Option<usize>,
                       list_depth: usize| {
        let text = line.trim().to_string();
        line.clear();
        if text.is_empty() {
            *heading_level = None;
            return;
        }
        match heading_level.take() {
            Some(level) => out.push_str(&format!("{} {}\n\n", "#".repeat(level), text)),
            None if list_depth > 0 => {
                out.push_str(&format!("{}- {}\n", "  ".repeat(list_depth - 1), text))
            }
            None => out.push_str(&format!("{}\n\n", text)),
        }
    };

    loop {
        match reader.read_event() {
            Ok(Event::Start(ref e)) | Ok(Event::Empty(ref e)) => match e.name().as_ref() {
                b"text:h" => {
                    let level = e
                        .attributes()
                        .flatten()
                        .find(|a| a.key.as_ref() == b"text:outline-level")
                        .and_then(|a| a.unescape_value().ok()?.parse::<usize>().ok())
                        .unwrap_or(1)
                        .clamp(1, 6);
                    heading_level = Some(level);
                }
                b"text:list" => list_depth += 1,
                b"draw:page" => {
                    slide_count += 1;
                    out.push_str(&format!("## Slide {}\n\n", slide_count));
                }
                b"table:table" => {
                    in_table = true;
                    table_row_idx = 0;
                }
                b"table:table-row" if in_table => row_cells.clear(),
                b"table:table-cell" if in_table => cell_text.clear(),
                b"text:s" => {
                    if in_table {
                        cell_text.push(' ')
                    } else {
                        line.push(' ')
                    }
                }
                b"text:tab" => {
                    if in_table {
                        cell_text.push('\t')
                    } else {
                        line.push('\t')
                    }
                }
                b"text:line-break" => {
                    if in_table {
                        cell_text.push(' ')
                    } else {
                        line.push(' ')
                    }
                }
                _ => {}
            },
            Ok(Event::Text(e)) => {
                if let Ok(text) = std::str::from_utf8(e.as_ref())
                    && let Ok(decoded) = quick_xml::escape::unescape(text)
                {
                    if in_table {
                        cell_text.push_str(&decoded);
                    } else {
                        line.push_str(&decoded);
                    }
                }
            }
            Ok(Event::GeneralRef(e)) => {
                if let Some(s) = crate::parsers::resolve_xml_entity(&e) {
                    if in_table {
                        cell_text.push_str(&s);
                    } else {
                        line.push_str(&s);
                    }
                }
            }
            Ok(Event::End(ref e)) => match e.name().as_ref() {
                b"text:h" | b"text:p" if !in_table => {
                    flush_block(&mut out, &mut line, &mut heading_level, list_depth);
                }
                b"text:p" if in_table => cell_text.push(' '),
                b"text:list" => list_depth = list_depth.saturating_sub(1),
                b"table:table-cell" if in_table => {
                    row_cells.push(
                        cell_text
                            .split_whitespace()
                            .collect::<Vec<_>>()
                            .join(" ")
                            .replace('|', "\\|"),
                    );
                    cell_text.clear();
                }
                b"table:table-row" if in_table => {
                    out.push_str(&format!("| {} |\n", row_cells.join(" | ")));
                    if table_row_idx == 0 {
                        out.push_str(&format!("|{}\n", "---|".repeat(row_cells.len())));
                    }
                    table_row_idx += 1;
                }
                b"table:table" => {
                    in_table = false;
                    out.push('\n');
                }
                _ => {}
            },
            Ok(Event::Eof) => break,
            Err(_) => break, // tolerate malformed chunks; emit what we have
            _ => {}
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn make_od(content_xml: &str, ext: &str) -> tempfile::NamedTempFile {
        let mut file = tempfile::Builder::new()
            .suffix(&format!(".{ext}"))
            .tempfile()
            .unwrap();
        {
            let mut zip = zip::ZipWriter::new(file.as_file_mut());
            let options = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Stored);
            zip.start_file("content.xml", options).unwrap();
            zip.write_all(content_xml.as_bytes()).unwrap();
            zip.finish().unwrap();
        }
        file
    }

    #[test]
    fn odt_headings_lists_and_tables() {
        let xml = r#"<office:document-content>
            <office:body><office:text>
            <text:h text:outline-level="1">Report Title</text:h>
            <text:p>An opening paragraph.</text:p>
            <text:h text:outline-level="2">Findings</text:h>
            <text:list>
                <text:list-item><text:p>First finding</text:p></text:list-item>
                <text:list-item><text:p>Second finding</text:p></text:list-item>
            </text:list>
            <table:table>
                <table:table-row>
                    <table:table-cell><text:p>Name</text:p></table:table-cell>
                    <table:table-cell><text:p>Value</text:p></table:table-cell>
                </table:table-row>
                <table:table-row>
                    <table:table-cell><text:p>Alpha</text:p></table:table-cell>
                    <table:table-cell><text:p>42</text:p></table:table-cell>
                </table:table-row>
            </table:table>
            </office:text></office:body>
        </office:document-content>"#;
        let file = make_od(xml, "odt");
        let result = OpenDocGobbler
            .gobble(file.path(), &ParseOptions::default())
            .unwrap();

        assert!(result.contains("# Report Title"));
        assert!(result.contains("## Findings"));
        assert!(result.contains("- First finding"));
        assert!(result.contains("| Name | Value |"));
        assert!(result.contains("|---|---|"));
        assert!(result.contains("| Alpha | 42 |"));
    }

    #[test]
    fn entities_in_paragraphs_survive() {
        let xml = r#"<office:document-content><office:body><office:text>
            <text:p>Ben &amp; Jerry&apos;s &lt;notes&gt;</text:p>
        </office:text></office:body></office:document-content>"#;
        let file = make_od(xml, "odt");
        let result = OpenDocGobbler
            .gobble(file.path(), &ParseOptions::default())
            .unwrap();
        assert!(result.contains("Ben & Jerry's <notes>"), "got: {result}");
    }

    #[test]
    fn odp_slides_get_headings() {
        let xml = r#"<office:document-content>
            <office:body><office:presentation>
            <draw:page draw:name="page1">
                <draw:frame><draw:text-box><text:p>Welcome to the deck</text:p></draw:text-box></draw:frame>
            </draw:page>
            <draw:page draw:name="page2">
                <draw:frame><draw:text-box><text:p>Second slide body</text:p></draw:text-box></draw:frame>
            </draw:page>
            </office:presentation></office:body>
        </office:document-content>"#;
        let file = make_od(xml, "odp");
        let result = OpenDocGobbler
            .gobble(file.path(), &ParseOptions::default())
            .unwrap();

        assert!(result.contains("## Slide 1"));
        assert!(result.contains("Welcome to the deck"));
        assert!(result.contains("## Slide 2"));
        assert!(result.contains("Second slide body"));
    }
}
