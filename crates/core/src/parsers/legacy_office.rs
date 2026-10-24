//! Pre-2007 Office binaries — Word 97–2003 (`.doc`), PowerPoint 97–2003
//! (`.ppt`) and Excel 97–2003 (`.xls`) — read by `office_oxide`.
//!
//! All three are OLE2 compound files, so the extension is only a claim: the
//! container's own streams (`WordDocument`, `PowerPoint Document`, `Workbook`)
//! decide which reader runs. Word and PowerPoint come back as office_oxide's
//! format-neutral IR, rendered here in the markdown the `.docx` and `.pptx`
//! parsers write: ATX headings, `- ` lists, the one pipe-table dialect, and
//! `## Slide N` sections with `**Speaker notes:**`. Workbooks are rendered by
//! the spreadsheet parser's own sheet layout.

use crate::parsers::gobble::{Gobble, ParseOptions};
use anyhow::{Context, Result};
use office_oxide::DocumentFormat;
use office_oxide::ir::{Element, InlineContent, List, Note, Section, Table};
use std::path::Path;

/// The signature every compound file starts with ([MS-CFB] 2.2).
const OLE_MAGIC: [u8; 8] = [0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1];

/// Stack for the conversion thread: office_oxide parses on a 16 MB stack of
/// its own, and the IR walk below recurses once per nesting level.
const OFFICE_STACK: usize = 32 << 20;

/// Nesting deeper than this (a list in a table in a text box …) is not
/// walked into. office_oxide's readers bound their own depth; this keeps the
/// renderer from relying on it.
const MAX_DEPTH: usize = 32;

pub struct LegacyOfficeGobbler;

impl Gobble for LegacyOfficeGobbler {
    fn gobble(&self, path: &Path, opts: &ParseOptions) -> Result<String> {
        let header = crate::format::read_header(path, crate::format::SNIFF_BYTES);
        if !header.starts_with(&OLE_MAGIC) {
            return misnamed(path, &header, opts);
        }
        let owned = path.to_path_buf();
        let (text, truncated) = guarded(move || convert(&owned))?;
        if truncated && !opts.quiet {
            eprintln!(
                "⚠️  {}: part of the file could not be read (truncated or damaged?); the \
                 output may be incomplete",
                path.display()
            );
        }
        Ok(text)
    }
}

/// Whether the file starts with the compound-file signature.
pub(crate) fn is_compound_file(path: &Path) -> bool {
    crate::format::read_header(path, OLE_MAGIC.len()) == OLE_MAGIC
}

/// What a compound file holds, judged by the stream each application keeps
/// its document in ([MS-DOC] 2.1.1, [MS-XLS] 2.1.7.20, [MS-PPT] 2.1.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Contents {
    Word,
    Excel,
    PowerPoint,
    /// A password-protected `.docx`/`.xlsx`/`.pptx`: [MS-OFFCRYPTO] wraps the
    /// encrypted package in a compound file.
    EncryptedPackage,
    OutlookMessage,
    /// No stream could be found, or the reader had to work around a broken
    /// FAT or directory: most likely a truncated document.
    Damaged,
    Unknown,
}

fn contents(path: &Path) -> Result<Contents> {
    let file =
        std::fs::File::open(path).context("Mischievous I/O Error: could not open the file")?;
    let cfb = office_oxide::cfb::CfbReader::new(std::io::BufReader::new(file)).map_err(|e| {
        anyhow::anyhow!(
            "Mischievous Office Error: not a readable OLE2 compound file ({e}); truncated or corrupt?"
        )
    })?;
    Ok(if cfb.has_stream("WordDocument") {
        Contents::Word
    } else if cfb.has_stream("Workbook") || cfb.has_stream("Book") {
        Contents::Excel
    } else if cfb.has_stream("PowerPoint Document") || cfb.has_root_entry("PP97_DUALSTORAGE") {
        Contents::PowerPoint
    } else if cfb.has_stream("EncryptedPackage") {
        Contents::EncryptedPackage
    } else if cfb.has_stream("__properties_version1.0") {
        Contents::OutlookMessage
    } else if !cfb.warnings().is_empty()
        || !cfb
            .entries()
            .iter()
            .any(|e| e.entry_type == office_oxide::cfb::EntryType::Stream)
    {
        Contents::Damaged
    } else {
        Contents::Unknown
    })
}

/// The markdown, and whether the reader reported part of the file unreadable.
fn convert(path: &Path) -> Result<(String, bool)> {
    match contents(path)? {
        Contents::Word => word_markdown(path),
        Contents::PowerPoint => slides_markdown(path),
        Contents::Excel => workbook_markdown(path),
        Contents::EncryptedPackage => Err(encrypted()),
        Contents::OutlookMessage => anyhow::bail!(
            "This is an Outlook .msg message, which filegoblin does not read. Save it as .eml \
             and gobble that instead."
        ),
        Contents::Damaged => anyhow::bail!(
            "Mischievous Office Error: this compound file's directory is unreadable (truncated \
             or damaged?), so there is no document to read"
        ),
        Contents::Unknown => anyhow::bail!(
            "This OLE2 compound file is not a Word, PowerPoint or Excel document (Visio, \
             Publisher, Works and installer packages share the container)."
        ),
    }
}

fn encrypted() -> anyhow::Error {
    anyhow::anyhow!(
        "This Office document is password-protected (encrypted). filegoblin cannot decrypt it: \
         remove the password in Office and save it again."
    )
}

/// Runs one conversion on a thread of its own with a generous stack. The
/// binary readers follow offsets taken from the file, so a panic in one is
/// this file's error rather than the end of a directory run, as on the PDF
/// path.
fn guarded<T: Send + 'static>(work: impl FnOnce() -> Result<T> + Send + 'static) -> Result<T> {
    std::thread::Builder::new()
        .name("filegoblin-office".into())
        .stack_size(OFFICE_STACK)
        .spawn(work)
        .context("Mischievous Office Error: could not start the conversion thread")?
        .join()
        .unwrap_or_else(|_| {
            anyhow::bail!(
                "Mischievous Office Error: the Office reader crashed on this file (malformed \
                 document)"
            )
        })
}

fn open(path: &Path, format: DocumentFormat) -> Result<office_oxide::Document> {
    use office_oxide::OfficeError;
    let file =
        std::fs::File::open(path).context("Mischievous I/O Error: could not open the file")?;
    office_oxide::Document::from_reader(file, format).map_err(|e| match e {
        OfficeError::Doc(office_oxide::doc::DocError::Encrypted)
        | OfficeError::Ppt(office_oxide::ppt::PptError::Encrypted) => encrypted(),
        OfficeError::Panic(_) => anyhow::anyhow!(
            "Mischievous Office Error: the Office reader crashed on this file (malformed document)"
        ),
        e => anyhow::anyhow!("Mischievous Office Error: {e}"),
    })
}

fn word_markdown(path: &Path) -> Result<(String, bool)> {
    let ir = open(path, DocumentFormat::Doc)?.to_ir();
    let text = render_document(&ir.sections);
    if text.trim().is_empty() {
        anyhow::bail!(
            "No text could be extracted from this Word document (images only, or text only \
             inside drawings)"
        );
    }
    Ok((text, ir.metadata.text_truncated))
}

fn slides_markdown(path: &Path) -> Result<(String, bool)> {
    let doc = open(path, DocumentFormat::Ppt)?;
    // office_oxide appends a "Slide Master" section of static master text
    // after the slides. The .pptx parser reads no masters, so neither does
    // this one: only the slides themselves are rendered.
    let slides = doc.as_ppt().map_or(0, |ppt| ppt.slides.len());
    let ir = doc.to_ir();
    let text = render_slides(&ir.sections[..slides.min(ir.sections.len())]);
    if text.trim().is_empty() {
        anyhow::bail!(
            "No text could be extracted from this presentation (image-only slides, or text \
             stored outside the slides)"
        );
    }
    Ok((text, ir.metadata.text_truncated))
}

/// Every worksheet in the spreadsheet parser's layout, cells as Excel
/// displays them (number formats applied, dates in ISO form).
fn workbook_markdown(path: &Path) -> Result<(String, bool)> {
    use office_oxide::xls::{CellValue, XlsDocument, XlsError};
    let file =
        std::fs::File::open(path).context("Mischievous I/O Error: could not open the file")?;
    let book = XlsDocument::from_reader(std::io::BufReader::new(file)).map_err(|e| match e {
        XlsError::Encrypted => encrypted(),
        e => anyhow::anyhow!("Mischievous Office Error: {e}"),
    })?;
    let mut output = String::new();
    // Chart and dialog sheets have no cells; macro sheets hold XLM code.
    for sheet in book
        .sheets
        .iter()
        .filter(|s| s.kind == office_oxide::ir::SheetKind::Worksheet)
    {
        // Columns start at the first one in use, as calamine's ranges do, so
        // a sheet laid out from column B reads the same from either reader.
        let first_col = sheet
            .rows
            .iter()
            .filter_map(|row| row.iter().position(|v| !matches!(v, CellValue::Empty)))
            .min()
            .unwrap_or(0);
        let rows = sheet.rows.iter().enumerate().map(|(r, row)| {
            (first_col..row.len())
                .map(|c| {
                    sheet
                        .display_text(r, c)
                        .map(|t| t.trim().to_string())
                        .unwrap_or_default()
                })
                .collect()
        });
        output.push_str(&crate::parsers::sheet::render_sheet(&sheet.name, rows));
    }
    let text = crate::parsers::sheet::finish_workbook(output, &[])?;
    Ok((text, book.truncated()))
}

/// A `.doc`/`.ppt` without the compound-file signature. Word and WordPad save
/// RTF under `.doc`, web apps export HTML as `.doc`, and a renamed
/// `.docx`/`.pptx` is a zip: each goes to its own parser. Word 1.x/2.0 wrote
/// flat files that office_oxide reads directly.
fn misnamed(path: &Path, header: &[u8], opts: &ParseOptions) -> Result<String> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .unwrap_or_default();
    let slides = matches!(ext.as_str(), "ppt" | "pps" | "pot");
    let lead: Vec<u8> = header
        .iter()
        .skip_while(|b| b.is_ascii_whitespace() || matches!(b, 0xEF | 0xBB | 0xBF))
        .take(16)
        .map(u8::to_ascii_lowercase)
        .collect();

    if header.starts_with(b"{\\rtf") {
        return crate::parsers::rtf::RtfGobbler.gobble(path, opts);
    }
    if header.starts_with(b"PK\x03\x04") {
        return if slides {
            crate::parsers::powerpoint::PptxGobbler.gobble(path, opts)
        } else {
            crate::parsers::office::OfficeGobbler.gobble(path, opts)
        };
    }
    if lead.starts_with(b"<!doctype html") || lead.starts_with(b"<html") {
        return crate::parsers::html::HtmlGobbler {
            extract_full: opts.full,
            ..Default::default()
        }
        .gobble(path, opts);
    }
    let word2 = header
        .get(..2)
        .is_some_and(|w| matches!(u16::from_le_bytes([w[0], w[1]]), 0xA59B | 0xA5DB));
    if word2 && !slides {
        let owned = path.to_path_buf();
        return guarded(move || word_markdown(&owned)).map(|(text, _)| text);
    }
    anyhow::bail!(
        "Mischievous Office Error: this .{ext} file is not a Word or PowerPoint binary (no \
         OLE2 signature): truncated, or not really a .{ext}"
    )
}

// --- IR → markdown -----------------------------------------------------------

/// A Word document: blocks separated by blank lines, footnotes and endnotes
/// after the body.
fn render_document(sections: &[Section]) -> String {
    let mut blocks = Vec::new();
    let mut notes = Vec::new();
    for section in sections {
        document_blocks(&section.elements, 0, &mut blocks, &mut notes);
    }
    blocks.extend(notes);
    blocks.join("\n\n")
}

fn document_blocks(
    elements: &[Element],
    depth: usize,
    blocks: &mut Vec<String>,
    notes: &mut Vec<String>,
) {
    for element in elements {
        let block = match element {
            Element::Heading(h) => {
                let text = inline(&h.content, Marks::Links);
                let level = usize::from(h.clamped_level()).clamp(1, 6);
                if text.is_empty() {
                    text
                } else {
                    format!("{} {}", "#".repeat(level), text)
                }
            }
            Element::Paragraph(p) => inline(&p.content, Marks::Rich),
            Element::Table(t) => table(t, depth),
            Element::List(l) => {
                let mut lines = Vec::new();
                list_lines(l, usize::from(l.level), depth, &mut lines);
                lines.join("\n")
            }
            Element::CodeBlock(c) => format!(
                "```{}\n{}\n```",
                c.language.as_deref().unwrap_or(""),
                c.content.trim_end()
            ),
            Element::TextBox(b) if depth < MAX_DEPTH => {
                document_blocks(&b.content, depth + 1, blocks, notes);
                continue;
            }
            Element::Footnote(n) | Element::Endnote(n) => {
                notes.extend(note(n, depth));
                continue;
            }
            // Pictures, rules, page and column breaks and drawn shapes carry
            // no text; `Element` is non-exhaustive.
            _ => continue,
        };
        if !block.trim().is_empty() {
            blocks.push(block);
        }
    }
}

/// A footnote or endnote as `**Footnote:** text`. The IR has no reference
/// marks for `.doc` notes, so markdown `[^n]` definitions would dangle.
/// Comments are reviewers' remarks rather than the document, and the `.docx`
/// parser leaves them out too.
fn note(n: &Note, depth: usize) -> Option<String> {
    let label = match n.marker.as_deref() {
        Some("footnote") => "Footnote",
        Some("endnote") => "Endnote",
        _ => return None,
    };
    let body = flat_text(&n.content, depth + 1, Marks::Links);
    (!body.is_empty()).then(|| format!("**{label}:** {body}"))
}

/// A list as `- item` lines, two spaces per nesting level: the `.docx`
/// parser's form, numbered lists included. office_oxide's `ordered` flag is
/// not used: 0.1.15 reports every level whose number format is not "none"
/// as numbered, and Word stores a bullet as a number format of its own
/// (`nfc` 0x17), so every bullet list arrives marked as numbered.
fn list_lines(list: &List, indent: usize, depth: usize, lines: &mut Vec<String>) {
    for item in &list.items {
        let text = item
            .content
            .iter()
            .map(|e| match e {
                Element::Paragraph(p) => inline(&p.content, Marks::Rich),
                Element::Heading(h) => inline(&h.content, Marks::Links),
                other => flat_text(std::slice::from_ref(other), depth + 1, Marks::Links),
            })
            .filter(|t| !t.is_empty())
            .collect::<Vec<_>>()
            .join(" ");
        lines.push(format!(
            "{}- {}",
            "  ".repeat(indent),
            text.replace('\n', " ")
        ));
        if let Some(nested) = &item.nested
            && depth < MAX_DEPTH
        {
            list_lines(nested, indent + 1, depth + 1, lines);
        }
    }
}

/// A pipe table through the shared builder. Cells are text with their
/// links; a cell spanning columns repeats in each, as the `.docx` parser
/// writes it.
fn table(t: &Table, depth: usize) -> String {
    let mut builder = crate::parsers::TableBuilder::default();
    for row in &t.rows {
        for cell in &row.cells {
            let span = usize::try_from(cell.col_span).unwrap_or(1).clamp(1, 64);
            builder.push_cell(&flat_text(&cell.content, depth + 1, Marks::Links), span);
        }
        builder.end_row();
    }
    builder.render().trim_end().to_string()
}

/// Block elements flattened to one line of text (table cells, notes).
fn flat_text(elements: &[Element], depth: usize, marks: Marks) -> String {
    if depth > MAX_DEPTH {
        return String::new();
    }
    let mut parts: Vec<String> = Vec::new();
    for element in elements {
        let text = match element {
            Element::Heading(h) => inline(&h.content, marks),
            Element::Paragraph(p) => inline(&p.content, marks),
            Element::List(l) => l
                .items
                .iter()
                .map(|item| flat_text(&item.content, depth + 1, marks))
                .collect::<Vec<_>>()
                .join(" "),
            Element::Table(t) => t
                .rows
                .iter()
                .flat_map(|row| &row.cells)
                .map(|cell| flat_text(&cell.content, depth + 1, marks))
                .collect::<Vec<_>>()
                .join(" "),
            Element::TextBox(b) => flat_text(&b.content, depth + 1, marks),
            Element::CodeBlock(c) => c.content.clone(),
            _ => String::new(),
        };
        let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
        if !text.is_empty() {
            parts.push(text);
        }
    }
    parts.join(" ")
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Marks {
    /// Text only: slides, as the `.pptx` parser writes them.
    Plain,
    /// `[links](url)` without emphasis: headings, table cells, notes.
    Links,
    /// `**bold**`, `*italic*` and `[links](url)`, as the `.docx` parser writes.
    Rich,
}

/// Inline content as markdown. Neighbouring runs with the same bold, italic
/// and link are merged before they are marked, so a word split across runs
/// never comes out as `**a****b**` or as two links.
fn inline(content: &[InlineContent], marks: Marks) -> String {
    type Style<'a> = (bool, bool, Option<&'a str>);
    let mut out = String::new();
    let mut run = String::new();
    let mut style: Style = (false, false, None);
    for item in content {
        match item {
            InlineContent::Text(span) => {
                let next: Style = match marks {
                    Marks::Rich => (span.bold, span.italic, span.hyperlink.as_deref()),
                    Marks::Links => (false, false, span.hyperlink.as_deref()),
                    Marks::Plain => (false, false, None),
                };
                if next != style {
                    out.push_str(&styled(&run, style));
                    run.clear();
                    style = next;
                }
                run.push_str(&span.text);
            }
            InlineContent::LineBreak => run.push('\n'),
            // Note references and comment anchors are positions, not text.
            _ => {}
        }
    }
    out.push_str(&styled(&run, style));
    out.trim().to_string()
}

fn styled(text: &str, (bold, italic, link): (bool, bool, Option<&str>)) -> String {
    let marker = match (bold, italic) {
        (true, true) => "***",
        (true, false) => "**",
        (false, true) => "*",
        (false, false) => "",
    };
    let text = wrap_core(text, marker, marker);
    match link {
        Some(url) => wrap_core(&text, "[", &format!("]({url})")),
        None => text,
    }
}

/// Wraps the text between its leading and trailing whitespace: `** word**`
/// is not emphasis in markdown, and a link should not swallow the space
/// before the next word.
fn wrap_core(text: &str, open: &str, close: &str) -> String {
    let core = text.trim();
    if core.is_empty() || (open.is_empty() && close.is_empty()) {
        return text.to_string();
    }
    let lead = &text[..text.len() - text.trim_start().len()];
    let trail = &text[text.trim_end().len()..];
    format!("{lead}{open}{core}{close}{trail}")
}

/// Slides in the `.pptx` parser's shape: `## Slide N`, every paragraph a
/// `- ` line, tables as pipe tables, then `**Speaker notes:**`. Slides are
/// numbered by their place in the deck; one with neither text nor notes is
/// left out.
fn render_slides(sections: &[Section]) -> String {
    let mut out = String::new();
    for (i, slide) in sections.iter().enumerate() {
        let mut body = SlideBody::default();
        body.add(&slide.elements, 0);
        let notes: Vec<String> = slide
            .speaker_notes
            .iter()
            .flatten()
            .map(|e| flat_text(std::slice::from_ref(e), 1, Marks::Plain))
            // Notes pages carry a slide-number placeholder; digit-only lines
            // are that artifact, not the speaker's words.
            .filter(|t| !t.is_empty() && !t.chars().all(|c| c.is_ascii_digit()))
            .map(|t| format!("- {t}"))
            .collect();
        if body.blocks.is_empty() && notes.is_empty() {
            continue;
        }
        // A slide with notes but no text (a picture, say) still gets its
        // notes, without an empty body in between.
        let mut parts = body.blocks;
        if !notes.is_empty() {
            parts.push(format!("**Speaker notes:**\n{}", notes.join("\n")));
        }
        out.push_str(&format!(
            "## Slide {}\n\n{}\n\n---\n\n",
            i + 1,
            parts.join("\n\n")
        ));
    }
    out
}

/// A slide's blocks: runs of `- ` lines, and tables between them.
#[derive(Default)]
struct SlideBody {
    blocks: Vec<String>,
    in_list: bool,
}

impl SlideBody {
    fn add(&mut self, elements: &[Element], depth: usize) {
        for element in elements {
            match element {
                Element::Heading(h) => self.line(&inline(&h.content, Marks::Plain)),
                Element::Paragraph(p) => self.line(&inline(&p.content, Marks::Plain)),
                Element::List(l) => l
                    .items
                    .iter()
                    .for_each(|item| self.line(&flat_text(&item.content, depth + 1, Marks::Plain))),
                Element::Table(t) => {
                    let md = table(t, depth);
                    if !md.is_empty() {
                        self.blocks.push(md);
                        self.in_list = false;
                    }
                }
                Element::TextBox(b) if depth < MAX_DEPTH => self.add(&b.content, depth + 1),
                _ => {}
            }
        }
    }

    fn line(&mut self, text: &str) {
        let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
        if text.is_empty() {
            return;
        }
        match self.blocks.last_mut() {
            Some(list) if self.in_list => {
                list.push_str("\n- ");
                list.push_str(&text);
            }
            _ => {
                self.blocks.push(format!("- {text}"));
                self.in_list = true;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use office_oxide::ir::{Heading, ListItem, Paragraph, TableCell, TableRow, TextBox, TextSpan};
    use std::path::PathBuf;

    fn text(s: &str) -> InlineContent {
        InlineContent::Text(TextSpan::plain(s))
    }

    fn styled_span(s: &str, bold: bool, italic: bool, link: Option<&str>) -> InlineContent {
        InlineContent::Text(TextSpan {
            text: s.into(),
            bold,
            italic,
            hyperlink: link.map(Into::into),
            ..Default::default()
        })
    }

    fn para(content: Vec<InlineContent>) -> Element {
        Element::Paragraph(Paragraph {
            content,
            ..Default::default()
        })
    }

    fn heading(level: u8, content: Vec<InlineContent>) -> Element {
        Element::Heading(Heading {
            level,
            content,
            ..Default::default()
        })
    }

    fn section(elements: Vec<Element>) -> Section {
        Section {
            elements,
            ..Default::default()
        }
    }

    fn grid(rows: &[&[(&str, u32)]]) -> Element {
        Element::Table(Table {
            rows: rows
                .iter()
                .map(|cells| TableRow {
                    cells: cells
                        .iter()
                        .map(|&(s, span)| TableCell {
                            content: vec![para(vec![styled_span(s, true, false, None)])],
                            col_span: span,
                            ..Default::default()
                        })
                        .collect(),
                    ..Default::default()
                })
                .collect(),
            ..Default::default()
        })
    }

    #[test]
    fn heading_levels_come_from_the_ir_without_emphasis() {
        let md = render_document(&[section(vec![
            heading(1, vec![styled_span("Field Guide", true, false, None)]),
            heading(
                2,
                vec![
                    text("See "),
                    styled_span("the cave", true, false, Some("https://example.com/cave")),
                ],
            ),
            heading(9, vec![text("Too deep")]),
            heading(0, vec![text("Unset")]),
            heading(3, vec![text("  ")]),
        ])]);
        assert_eq!(
            md,
            "# Field Guide\n\n## See [the cave](https://example.com/cave)\n\n###### Too deep\n\n# Unset"
        );
    }

    /// Word splits a run wherever formatting state changes (spell-check,
    /// revision marks), so one bold phrase often arrives as several spans.
    #[test]
    fn runs_sharing_a_style_are_marked_once() {
        let md = inline(
            &[
                text("The hoard grew by "),
                styled_span("212 ", true, false, None),
                styled_span("items", true, false, None),
                text(", an "),
                styled_span("alarming", false, true, None),
                text(" rate."),
            ],
            Marks::Rich,
        );
        assert_eq!(md, "The hoard grew by **212 items**, an *alarming* rate.");
    }

    #[test]
    fn links_keep_their_url_and_the_spaces_around_them() {
        let url = Some("https://example.com/hoard");
        let md = inline(
            &[
                text("See the"),
                styled_span(" full ", false, false, url),
                styled_span("ledger", false, false, url),
                text("."),
            ],
            Marks::Rich,
        );
        assert_eq!(md, "See the [full ledger](https://example.com/hoard).");
        // Plain mode (headings, cells, slides) keeps the words only.
        let plain = inline(&[styled_span("ledger", true, false, url)], Marks::Plain);
        assert_eq!(plain, "ledger");
    }

    /// Numbered lists too: office_oxide marks Word's bullet lists as
    /// numbered (see `list_lines`), and the `.docx` parser writes `- ` for both.
    #[test]
    fn lists_nest_by_two_spaces_as_in_docx() {
        let item = |s: &str, nested: Option<List>| ListItem {
            content: vec![para(vec![text(s)])],
            nested,
        };
        let inner = List {
            items: vec![item("unpaired", None)],
            level: 1,
            ..Default::default()
        };
        let md = render_document(&[section(vec![
            Element::List(List {
                items: vec![item("Socks", Some(inner)), item("Spoons", None)],
                ..Default::default()
            }),
            Element::List(List {
                ordered: true,
                start_number: Some(3),
                items: vec![item("Count the spoons", None), item("Hide them", None)],
                ..Default::default()
            }),
        ])]);
        assert_eq!(
            md,
            "- Socks\n  - unpaired\n- Spoons\n\n- Count the spoons\n- Hide them"
        );
    }

    #[test]
    fn tables_use_the_shared_pipe_dialect() {
        let md = render_document(&[section(vec![grid(&[
            &[("Cave", 1), ("Items", 1), ("Shiny?", 1)],
            &[("Bog | Hollow", 1), ("", 1), ("No", 1)],
            &[("Total", 2), ("1,204", 1)],
        ])])]);
        assert_eq!(
            md,
            "| Cave | Items | Shiny? |\n|---|---|---|\n| Bog \\| Hollow |  | No |\n\
             | Total | Total | 1,204 |"
        );
    }

    #[test]
    fn notes_follow_the_body_and_comments_are_left_out() {
        let note = |id, marker: &str, s: &str| Note {
            id,
            marker: Some(marker.into()),
            content: vec![para(vec![text(s)])],
            ..Default::default()
        };
        let md = render_document(&[section(vec![
            para(vec![text("Body.")]),
            Element::Footnote(note(0, "footnote", "Counted twice.")),
            Element::Endnote(note(1, "comment", "Is this right?")),
            Element::Endnote(note(2, "endnote", "See appendix.")),
            Element::TextBox(TextBox {
                content: vec![para(vec![text("Boxed aside")])],
                ..Default::default()
            }),
            Element::PageBreak,
        ])]);
        assert_eq!(
            md,
            "Body.\n\nBoxed aside\n\n**Footnote:** Counted twice.\n\n**Endnote:** See appendix."
        );
    }

    #[test]
    fn slides_follow_the_pptx_layout() {
        let notes = |lines: &[&str]| Some(lines.iter().map(|l| para(vec![text(l)])).collect());
        let slides = [
            Section {
                elements: vec![
                    heading(1, vec![text("Hoard Status")]),
                    para(vec![text("Spoons: 412")]),
                ],
                // The notes page's slide-number placeholder is not a note.
                speaker_notes: notes(&["Do not mention the crowns.", "2"]),
                ..Default::default()
            },
            // Nothing on it: left out, but the numbering keeps its place.
            Section::default(),
            Section {
                elements: vec![
                    para(vec![text("Totals")]),
                    grid(&[&[("A", 1), ("B", 1)], &[("1", 1), ("2", 1)]]),
                    para(vec![text("After the table")]),
                ],
                ..Default::default()
            },
            // A picture slide: its notes, and no empty body before them.
            Section {
                speaker_notes: notes(&["Point at the map."]),
                ..Default::default()
            },
        ];
        assert_eq!(
            render_slides(&slides),
            "## Slide 1\n\n- Hoard Status\n- Spoons: 412\n\n**Speaker notes:**\n\
             - Do not mention the crowns.\n\n---\n\n\
             ## Slide 3\n\n- Totals\n\n| A | B |\n|---|---|\n| 1 | 2 |\n\n- After the table\
             \n\n---\n\n\
             ## Slide 4\n\n**Speaker notes:**\n- Point at the map.\n\n---\n\n"
        );
    }

    // --- Real files ----------------------------------------------------------

    fn asset(name: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../test_assets")
            .join(name)
    }

    fn gobble(path: &Path) -> Result<String> {
        let opts = ParseOptions {
            quiet: true,
            ..Default::default()
        };
        LegacyOfficeGobbler.gobble(path, &opts)
    }

    /// Writes `bytes` to `name` in a fresh temporary directory.
    fn staged(name: &str, bytes: &[u8]) -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(name);
        std::fs::write(&path, bytes).unwrap();
        (dir, path)
    }

    #[test]
    fn the_container_decides_not_the_extension() {
        for (fixture, expected) in [
            ("legacy_report.doc", "| Cave | Items | Shiny? |"),
            ("legacy_deck.ppt", "**Speaker notes:**"),
            ("legacy_ledger.xls", "## Sheet: Hoard"),
        ] {
            let bytes = std::fs::read(asset(fixture)).unwrap();
            for name in ["no_extension", "renamed.doc", "renamed.ppt"] {
                let (_dir, path) = staged(name, &bytes);
                let md = gobble(&path).unwrap_or_else(|e| panic!("{fixture} as {name}: {e:#}"));
                assert!(md.contains(expected), "{fixture} as {name}:\n{md}");
            }
        }
    }

    /// Truncated or overwritten files must come back as an error (or what
    /// text survived) for this file alone, never as a crash of the run.
    #[test]
    fn damaged_files_fail_on_their_own() {
        for fixture in ["legacy_report.doc", "legacy_deck.ppt", "legacy_ledger.xls"] {
            let bytes = std::fs::read(asset(fixture)).unwrap();
            let name = format!("damaged.{}", fixture.rsplit('.').next().unwrap());
            for len in [0, 8, 100, 511, 512, 1024, bytes.len() / 2, bytes.len() - 1] {
                let (_dir, path) = staged(&name, &bytes[..len]);
                let result = gobble(&path);
                if len < 1024 {
                    assert!(result.is_err(), "{fixture} cut to {len} bytes converted");
                }
            }
            // Garbage over each sector in turn: FAT, directory, streams.
            for sector in 1..bytes.len() / 512 {
                let mut damaged = bytes.clone();
                for (i, b) in damaged[sector * 512..(sector + 1) * 512]
                    .iter_mut()
                    .enumerate()
                {
                    *b ^= (i as u8).wrapping_mul(37) | 1;
                }
                let (_dir, path) = staged(&name, &damaged);
                let _ = gobble(&path);
            }
        }
    }

    fn assert_refused(bytes: &[u8], name: &str, expected: &str) {
        let (_dir, path) = staged(name, bytes);
        let err = gobble(&path).expect_err("must be refused");
        assert!(format!("{err:#}").contains(expected), "{name}: {err:#}");
    }

    #[test]
    fn encrypted_documents_are_named_as_such() {
        // Word: fEncrypted, bit 8 of the FIB flags at 0x0A ([MS-DOC] 2.5.2).
        let mut doc = std::fs::read(asset("legacy_report.doc")).unwrap();
        let fib = (0..doc.len())
            .step_by(512)
            .find(|&at| doc[at..at + 2] == [0xEC, 0xA5])
            .expect("FIB of the WordDocument stream");
        doc[fib + 0x0B] |= 0x01;
        assert_refused(&doc, "locked.doc", "password-protected");

        // PowerPoint: the encrypted CurrentUserAtom token ([MS-PPT] 2.3.2).
        let mut ppt = std::fs::read(asset("legacy_deck.ppt")).unwrap();
        let token = 0xE391_C05Fu32.to_le_bytes();
        let at = ppt
            .windows(4)
            .position(|w| w == token)
            .expect("header token");
        ppt[at..at + 4].copy_from_slice(&0xF3D1_C4DFu32.to_le_bytes());
        assert_refused(&ppt, "locked.ppt", "password-protected");
    }

    /// The ledger's only stream renamed: what a compound file holds is read
    /// from its stream names, so each name must get its own answer.
    #[test]
    fn other_compound_files_get_a_clear_answer() {
        let xls = std::fs::read(asset("legacy_ledger.xls")).unwrap();
        let old: Vec<u8> = "Workbook\0"
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect();
        let entry = xls.windows(old.len()).position(|w| w == old).unwrap();
        let renamed = |name: &str| {
            let mut bytes = xls.clone();
            let encoded: Vec<u8> = format!("{name}\0")
                .encode_utf16()
                .flat_map(u16::to_le_bytes)
                .collect();
            bytes[entry..entry + 64].fill(0);
            bytes[entry..entry + encoded.len()].copy_from_slice(&encoded);
            bytes[entry + 0x40..entry + 0x42]
                .copy_from_slice(&(encoded.len() as u16).to_le_bytes());
            bytes
        };
        assert_refused(
            &renamed("EncryptedPackage"),
            "locked.docx.doc",
            "password-protected",
        );
        // An encrypted .xlsx reaches the spreadsheet parser by its extension.
        let (_dir, xlsx) = staged("locked.xlsx", &renamed("EncryptedPackage"));
        let err = crate::parsers::sheet::SheetGobbler
            .gobble(&xlsx, &ParseOptions::default())
            .unwrap_err();
        assert!(format!("{err:#}").contains("password-protected"), "{err:#}");
        assert_refused(&renamed("__properties_version1.0"), "mail", "Outlook .msg");
        assert_refused(
            &renamed("VisioDocument"),
            "drawing",
            "not a Word, PowerPoint or Excel",
        );
    }

    #[test]
    fn misnamed_files_go_to_their_own_parser() {
        let (_dir, rtf) = staged(
            "letter.doc",
            b"{\\rtf1\\ansi Dear goblin,\\par Bring spoons.}",
        );
        assert!(gobble(&rtf).unwrap().contains("Bring spoons."));
        let (_dir, html) = staged(
            "export.doc",
            b"<html><body><h1>Export</h1><p>Spoons counted: 412.</p></body></html>",
        );
        assert!(gobble(&html).unwrap().contains("Spoons counted: 412."));
        let (_dir, junk) = staged("junk.ppt", b"this is not a presentation at all");
        let err = gobble(&junk).unwrap_err();
        assert!(
            format!("{err:#}").contains("not a Word or PowerPoint binary"),
            "{err:#}"
        );
    }
}
