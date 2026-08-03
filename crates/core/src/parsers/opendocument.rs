use crate::parsers::gobble::{Gobble, ParseOptions};
use anyhow::{Context, Result};
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

        let mut entry = archive
            .by_name("content.xml")
            .context("Missing content.xml — not an OpenDocument file")?;
        let content = crate::parsers::read_zip_entry_to_string(&mut entry, "content.xml")?;
        drop(entry);

        let md = content_xml_to_md(&content)?;
        if md.trim().is_empty() {
            anyhow::bail!("No text could be extracted from this OpenDocument file");
        }
        Ok(md.trim().to_string())
    }
}

/// A `table:number-columns-repeated` run is expanded, but bounded: ODS writers
/// pad every row out to the sheet width with a single repeated filler cell
/// (`table:number-columns-repeated="16384"`), which must not become 16k columns.
const MAX_CELL_REPEAT: usize = 512;

fn content_xml_to_md(xml: &str) -> Result<String> {
    use quick_xml::Reader;
    use quick_xml::events::Event;

    let mut reader = Reader::from_str(xml);
    let mut out = String::new();

    // Block state
    let mut line = String::new();
    let mut heading_level: Option<usize> = None;
    let mut list_depth = 0usize;
    let mut slide_count = 0usize;

    // Rows are buffered and rendered when the table closes so every row can be
    // padded to a single width — emitting rows as they closed meant a short row
    // silently shifted its values into the wrong columns.
    let mut in_table = false;
    let mut table = crate::parsers::TableBuilder::default();
    let mut cell_text = String::new();
    let mut cell_repeat = 1usize;

    // Footnotes and annotations nest a whole `<text:p>` inside the paragraph
    // that cites them. Treating that inner close as the end of the outer
    // paragraph flushed half a sentence and then spliced the note's text into
    // the remainder. Paragraph depth keeps the host paragraph intact; note
    // bodies are collected separately and emitted after it.
    let mut para_depth = 0usize;
    let mut note_depth = 0usize;
    let mut notes: Vec<String> = Vec::new();
    let mut note_text = String::new();

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
        // `Empty` is a self-closing element: it gets a Start *and* an End in
        // one event, so anything that only reacts to `End` must handle it here.
        let event = reader.read_event().map_err(|e| {
            anyhow::anyhow!(
                "Malformed OpenDocument XML at byte {}: {}",
                reader.buffer_position(),
                e
            )
        })?;
        let is_empty = matches!(event, Event::Empty(_));
        match event {
            Event::Start(ref e) | Event::Empty(ref e) => match e.name().as_ref() {
                // A self-closing cell — `<table:table-cell/>` is exactly what
                // LibreOffice writes for every blank cell — never reaches the
                // `End` arm, so it used to contribute no cell at all and every
                // value to its right slid one column left.
                b"table:table-cell" | b"table:covered-table-cell" if in_table => {
                    cell_repeat = repeat_count(e);
                    cell_text.clear();
                    if is_empty || e.name().as_ref() == b"table:covered-table-cell" {
                        table.push_cell("", cell_repeat);
                        cell_repeat = 1;
                    }
                }
                b"text:h" => {
                    let level = e
                        .attributes()
                        .flatten()
                        .find(|a| a.key.as_ref() == b"text:outline-level")
                        .and_then(|a| a.unescape_value().ok()?.parse::<usize>().ok())
                        .unwrap_or(1)
                        .clamp(1, 6);
                    heading_level = Some(level);
                    if !in_table && !is_empty {
                        para_depth += 1;
                    }
                }
                b"text:note" | b"office:annotation" => {
                    if !is_empty {
                        note_depth += 1;
                        note_text.clear();
                    }
                }
                b"text:p" if !in_table && !is_empty => para_depth += 1,
                b"text:list" => list_depth += 1,
                b"draw:page" => {
                    slide_count += 1;
                    out.push_str(&format!("## Slide {}\n\n", slide_count));
                }
                b"table:table" => in_table = true,
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
            Event::Text(ref e) => {
                if let Ok(text) = std::str::from_utf8(e.as_ref())
                    && let Ok(decoded) = quick_xml::escape::unescape(text)
                {
                    if note_depth > 0 {
                        note_text.push_str(&decoded);
                    } else if in_table {
                        cell_text.push_str(&decoded);
                    } else {
                        line.push_str(&decoded);
                    }
                }
            }
            Event::GeneralRef(ref e) => {
                if let Some(s) = crate::parsers::resolve_xml_entity(e) {
                    if note_depth > 0 {
                        note_text.push_str(&s);
                    } else if in_table {
                        cell_text.push_str(&s);
                    } else {
                        line.push_str(&s);
                    }
                }
            }
            Event::End(ref e) => match e.name().as_ref() {
                b"text:note" | b"office:annotation" => {
                    note_depth = note_depth.saturating_sub(1);
                    let text = std::mem::take(&mut note_text).trim().to_string();
                    if !text.is_empty() {
                        notes.push(text);
                    }
                }
                // Only the *outermost* paragraph ends a block: a footnote body
                // nests a whole `text:p` inside the citing paragraph.
                b"text:h" | b"text:p" if !in_table => {
                    para_depth = para_depth.saturating_sub(1);
                    if para_depth == 0 && note_depth == 0 {
                        flush_block(&mut out, &mut line, &mut heading_level, list_depth);
                        for note in notes.drain(..) {
                            out.push_str(&format!("> {}\n\n", note));
                        }
                    }
                }
                b"text:p" if in_table => cell_text.push(' '),
                b"text:list" => list_depth = list_depth.saturating_sub(1),
                b"table:covered-table-cell" if in_table => cell_text.clear(),
                b"table:table-cell" if in_table => {
                    table.push_cell(&cell_text, cell_repeat);
                    cell_repeat = 1;
                    cell_text.clear();
                }
                b"table:table-row" if in_table => table.end_row(),
                b"table:table" => {
                    in_table = false;
                    out.push_str(&table.render());
                    out.push('\n');
                }
                _ => {}
            },
            Event::Eof => break,
            _ => {}
        }
    }

    Ok(out)
}

/// `table:number-columns-repeated` on a cell, clamped to [`MAX_CELL_REPEAT`].
fn repeat_count(e: &quick_xml::events::BytesStart) -> usize {
    e.attributes()
        .flatten()
        .find(|a| a.key.as_ref() == b"table:number-columns-repeated")
        .and_then(|a| a.unescape_value().ok()?.parse::<usize>().ok())
        .unwrap_or(1)
        .clamp(1, MAX_CELL_REPEAT)
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

    /// Regression: a self-closing `<table:table-cell/>` (what LibreOffice
    /// writes for every blank cell) contributed no cell at all, so every value
    /// to its right moved one column left — silent data corruption in a table
    /// an LLM then reads as fact.
    #[test]
    fn empty_cells_keep_their_column() {
        let xml = r#"<office:document-content><office:body><office:text>
            <table:table>
                <table:table-row>
                    <table:table-cell><text:p>Name</text:p></table:table-cell>
                    <table:table-cell><text:p>Middle</text:p></table:table-cell>
                    <table:table-cell><text:p>Value</text:p></table:table-cell>
                </table:table-row>
                <table:table-row>
                    <table:table-cell><text:p>Alpha</text:p></table:table-cell>
                    <table:table-cell/>
                    <table:table-cell><text:p>42</text:p></table:table-cell>
                </table:table-row>
            </table:table>
        </office:text></office:body></office:document-content>"#;
        let file = make_od(xml, "odt");
        let result = OpenDocGobbler
            .gobble(file.path(), &ParseOptions::default())
            .unwrap();
        assert!(
            result.contains("| Alpha |  | 42 |"),
            "42 must stay in column 3, got: {result}"
        );
    }

    /// A merged-away cell still occupies a column position.
    #[test]
    fn covered_cells_hold_their_place() {
        let xml = r#"<office:document-content><office:body><office:text>
            <table:table>
                <table:table-row>
                    <table:table-cell><text:p>A</text:p></table:table-cell>
                    <table:table-cell><text:p>B</text:p></table:table-cell>
                    <table:table-cell><text:p>C</text:p></table:table-cell>
                </table:table-row>
                <table:table-row>
                    <table:table-cell><text:p>span</text:p></table:table-cell>
                    <table:covered-table-cell/>
                    <table:table-cell><text:p>tail</text:p></table:table-cell>
                </table:table-row>
            </table:table>
        </office:text></office:body></office:document-content>"#;
        let file = make_od(xml, "odt");
        let result = OpenDocGobbler
            .gobble(file.path(), &ParseOptions::default())
            .unwrap();
        assert!(result.contains("| span |  | tail |"), "got: {result}");
    }

    /// A repeated-cell run expands, and the huge trailing filler run that ODS
    /// writers emit is trimmed rather than becoming thousands of columns.
    #[test]
    fn repeated_cells_expand_but_filler_is_trimmed() {
        let xml = r#"<office:document-content><office:body><office:text>
            <table:table>
                <table:table-row>
                    <table:table-cell><text:p>x</text:p></table:table-cell>
                    <table:table-cell table:number-columns-repeated="3"><text:p>d</text:p></table:table-cell>
                    <table:table-cell table:number-columns-repeated="16384"/>
                </table:table-row>
            </table:table>
        </office:text></office:body></office:document-content>"#;
        let file = make_od(xml, "odt");
        let result = OpenDocGobbler
            .gobble(file.path(), &ParseOptions::default())
            .unwrap();
        assert!(result.contains("| x | d | d | d |"), "got: {result}");
        assert!(
            result.lines().next().unwrap().len() < 100,
            "filler run must not widen the table, got: {result}"
        );
    }

    /// Rows shorter than the header are padded, never left ragged.
    #[test]
    fn short_rows_are_padded_to_the_table_width() {
        let xml = r#"<office:document-content><office:body><office:text>
            <table:table>
                <table:table-row>
                    <table:table-cell><text:p>A</text:p></table:table-cell>
                    <table:table-cell><text:p>B</text:p></table:table-cell>
                    <table:table-cell><text:p>C</text:p></table:table-cell>
                </table:table-row>
                <table:table-row>
                    <table:table-cell><text:p>only</text:p></table:table-cell>
                </table:table-row>
            </table:table>
        </office:text></office:body></office:document-content>"#;
        let file = make_od(xml, "odt");
        let result = OpenDocGobbler
            .gobble(file.path(), &ParseOptions::default())
            .unwrap();
        assert!(result.contains("| only |  |  |"), "got: {result}");
    }

    /// Regression: a footnote nests a whole `text:p` inside the citing
    /// paragraph, and treating that inner close as the paragraph's end split
    /// the sentence in half and spliced the note's text into the remainder.
    #[test]
    fn footnotes_do_not_split_the_citing_paragraph() {
        let xml = r#"<office:document-content><office:body><office:text>
            <text:p>The goblin hoards documents<text:note text:note-class="footnote">
                <text:note-citation>1</text:note-citation>
                <text:note-body><text:p>Especially the crunchy ones.</text:p></text:note-body>
            </text:note> and refuses to share them.</text:p>
        </office:text></office:body></office:document-content>"#;
        let file = make_od(xml, "odt");
        let result = OpenDocGobbler
            .gobble(file.path(), &ParseOptions::default())
            .unwrap();

        assert!(
            result.contains("The goblin hoards documents and refuses to share them."),
            "the citing paragraph must stay whole, got: {result}"
        );
        assert!(
            result.contains("Especially the crunchy ones."),
            "the note text must still appear, got: {result}"
        );
    }

    /// Malformed XML is an error, not a half-document returned as success.
    #[test]
    fn malformed_xml_fails_loudly() {
        let xml = r#"<office:document-content><office:body><office:text>
            <text:p>Good paragraph</text:p>
            <text:p>Unterminated <<<"#;
        let file = make_od(xml, "odt");
        let err = OpenDocGobbler
            .gobble(file.path(), &ParseOptions::default())
            .unwrap_err();
        assert!(
            err.to_string().contains("Malformed OpenDocument XML"),
            "got: {err}"
        );
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
