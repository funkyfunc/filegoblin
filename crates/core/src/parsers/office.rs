use crate::parsers::gobble::{Gobble, ParseOptions};
use anyhow::{Context, Result};
use std::collections::HashMap;
use std::path::Path;

pub struct OfficeGobbler;

impl Gobble for OfficeGobbler {
    /// Converts .docx to structured markdown: heading styles become `#` levels,
    /// numbered/bulleted paragraphs become list items, bold/italic runs keep
    /// their emphasis, and hyperlinks keep their URLs. Tables use the same
    /// "Sequence of Records" dialect as the PDF and sheet parsers.
    fn gobble(&self, path: &Path, _opts: &ParseOptions) -> Result<String> {
        if !path.exists() {
            anyhow::bail!("Mischievous I/O Error: file not found at {:?}", path);
        }

        let ext = path.extension().and_then(|s| s.to_str()).unwrap_or("");

        if ext == "docx" {
            let buf = std::fs::read(path).context("Failed to read docx buffer")?;
            let docx =
                docx_rs::read_docx(&buf).context("Failed to parse docx OpenXML structures")?;

            // rid → URL, populated by the reader from document rels.
            let links: HashMap<&str, &str> = docx
                .hyperlinks
                .iter()
                .map(|(rid, target, _)| (rid.as_str(), target.as_str()))
                .collect();

            let mut blocks: Vec<(bool, String)> = Vec::new(); // (is_list_item, text)
            for child in &docx.document.children {
                match child {
                    docx_rs::DocumentChild::Paragraph(p) => {
                        let md = paragraph_to_md(p, &links);
                        if !md.trim().is_empty() {
                            let is_list = p.property.numbering_property.is_some();
                            blocks.push((is_list, md));
                        }
                    }
                    docx_rs::DocumentChild::Table(t) => {
                        blocks.push((false, format_table(t)));
                    }
                    _ => {}
                }
            }

            // Blank line between blocks, except between consecutive list items
            // (a tight markdown list stays one list).
            let mut extracted = String::new();
            let mut prev_was_list = false;
            for (is_list, block) in blocks {
                if !extracted.is_empty() {
                    extracted.push('\n');
                    if !(is_list && prev_was_list) {
                        extracted.push('\n');
                    }
                }
                extracted.push_str(block.trim_end());
                prev_was_list = is_list;
            }
            // A docx whose content lives entirely in node types this walker
            // drops (text boxes, SDT content controls, images) used to return
            // Ok("") — a real file, e.g. a flyer built from text boxes, quietly
            // converting to nothing at all.
            if extracted.trim().is_empty() {
                anyhow::bail!(
                    "No text could be extracted from this .docx (content may live in text \
                     boxes, content controls, or images rather than paragraphs)"
                );
            }
            Ok(extracted.trim().to_string())
        } else {
            // Spreadsheets are routed to SheetGobbler; OfficeGobbler only handles Word.
            anyhow::bail!(
                "OfficeGobbler only handles .docx; spreadsheets are routed to the sheet parser"
            )
        }
    }
}

/// One paragraph as a markdown block: heading, list item, or plain text.
fn paragraph_to_md(p: &docx_rs::Paragraph, links: &HashMap<&str, &str>) -> String {
    let text = children_to_md(&p.children, links);
    if text.trim().is_empty() {
        return String::new();
    }

    if let Some(style) = &p.property.style
        && let Some(level) = heading_level(&style.val)
    {
        return format!("{} {}", "#".repeat(level), text.trim());
    }

    if let Some(num) = &p.property.numbering_property {
        let level = num.level.as_ref().map(|l| l.val).unwrap_or(0);
        return format!("{}- {}", "  ".repeat(level), text.trim());
    }

    text
}

/// Maps Word style ids ("Heading1", "Heading 2", "Title") to markdown levels.
fn heading_level(style_val: &str) -> Option<usize> {
    let s = style_val.to_ascii_lowercase();
    if s == "title" {
        return Some(1);
    }
    s.strip_prefix("heading")
        .and_then(|n| n.trim().parse::<usize>().ok())
        .map(|n| n.clamp(1, 6))
}

fn children_to_md(children: &[docx_rs::ParagraphChild], links: &HashMap<&str, &str>) -> String {
    let mut out = String::new();
    for child in children {
        match child {
            docx_rs::ParagraphChild::Run(run) => out.push_str(&run_to_md(run)),
            docx_rs::ParagraphChild::Hyperlink(h) => {
                let inner = children_to_md(&h.children, links);
                let url = match &h.link {
                    docx_rs::HyperlinkData::External { rid, .. } => {
                        links.get(rid.as_str()).map(|u| u.to_string())
                    }
                    docx_rs::HyperlinkData::Anchor { anchor } => Some(format!("#{}", anchor)),
                };
                match url {
                    Some(url) if !inner.trim().is_empty() => {
                        out.push_str(&format!("[{}]({})", inner.trim(), url));
                    }
                    _ => out.push_str(&inner),
                }
            }
            _ => {}
        }
    }
    out
}

/// A run's text with `**bold**` / `*italic*` markers from its run properties.
fn run_to_md(run: &docx_rs::Run) -> String {
    let mut text = String::new();
    for child in &run.children {
        match child {
            docx_rs::RunChild::Text(t) => text.push_str(&t.text),
            docx_rs::RunChild::Tab(_) => text.push('\t'),
            docx_rs::RunChild::Break(_) => text.push('\n'),
            _ => {}
        }
    }
    let marker = match (
        run.run_property.bold.is_some(),
        run.run_property.italic.is_some(),
    ) {
        (true, true) => "***",
        (true, false) => "**",
        (false, true) => "*",
        (false, false) => return text,
    };
    emphasize(&text, marker)
}

/// Wraps only the non-whitespace core in emphasis markers — `**text **` (with
/// an inner space) is not valid markdown emphasis.
fn emphasize(text: &str, marker: &str) -> String {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return text.to_string();
    }
    let start = text.find(trimmed).unwrap_or(0);
    let (lead, rest) = text.split_at(start);
    let (core, trail) = rest.split_at(trimmed.len());
    format!("{}{}{}{}{}", lead, marker, core, marker, trail)
}

/// A Word table as a standard markdown pipe table (row 0 is the header row).
fn format_table(t: &docx_rs::Table) -> String {
    let mut builder = crate::parsers::TableBuilder::default();
    for row_child in t.rows.iter() {
        let docx_rs::TableChild::TableRow(row) = row_child;
        for cell_child in row.cells.iter() {
            let docx_rs::TableRowChild::TableCell(cell) = cell_child;
            builder.push_cell(&extract_cell_text(cell), 1);
        }
        builder.end_row();
    }
    builder.render()
}

fn extract_cell_text(cell: &docx_rs::TableCell) -> String {
    let mut text = String::new();
    for child in &cell.children {
        match child {
            docx_rs::TableCellContent::Paragraph(p) => {
                text.push_str(&p.raw_text());
                text.push(' ');
            }
            // Markdown has no nested tables, and rendering one as a pipe table
            // inside a cell produced `| a | b |---|---| c |` garbage once the
            // parent flattened its newlines. Flatten to readable text instead.
            docx_rs::TableCellContent::Table(t) => {
                text.push_str(&flatten_nested_table(t));
                text.push(' ');
            }
            _ => {}
        }
    }
    text.trim().to_string()
}

/// A table inside a cell, as `a: b; c: d / e: f` — cells joined by `; `, rows
/// by ` / `. Lossy by necessity, but readable and never malformed.
fn flatten_nested_table(t: &docx_rs::Table) -> String {
    let rows: Vec<String> = t
        .rows
        .iter()
        .map(|row_child| {
            let docx_rs::TableChild::TableRow(row) = row_child;
            row.cells
                .iter()
                .map(|cell_child| {
                    let docx_rs::TableRowChild::TableCell(cell) = cell_child;
                    extract_cell_text(cell)
                })
                .filter(|c| !c.trim().is_empty())
                .collect::<Vec<_>>()
                .join("; ")
        })
        .filter(|r| !r.trim().is_empty())
        .collect();
    rows.join(" / ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;
    use docx_rs::{Paragraph, Run, Table, TableCell, TableRow};

    #[test]
    fn test_format_table_markdown_pipe() {
        // Construct a table manually
        let t = Table::new(vec![
            TableRow::new(vec![
                TableCell::new()
                    .add_paragraph(Paragraph::new().add_run(Run::new().add_text("Header1"))),
                TableCell::new()
                    .add_paragraph(Paragraph::new().add_run(Run::new().add_text("Header2"))),
            ]),
            TableRow::new(vec![
                TableCell::new()
                    .add_paragraph(Paragraph::new().add_run(Run::new().add_text("Value1"))),
                TableCell::new()
                    .add_paragraph(Paragraph::new().add_run(Run::new().add_text("Value2"))),
            ]),
            TableRow::new(vec![
                TableCell::new()
                    .add_paragraph(Paragraph::new().add_run(Run::new().add_text("Value3"))),
                TableCell::new()
                    .add_paragraph(Paragraph::new().add_run(Run::new().add_text("Value4"))),
            ]),
        ]);

        let result = format_table(&t);
        println!("Table:\n{}", result);
        assert!(result.contains("| Header1 | Header2 |"));
        assert!(result.contains("|---|---|"));
        assert!(result.contains("| Value1 | Value2 |"));
        assert!(result.contains("| Value3 | Value4 |"));
    }

    #[test]
    fn test_office_missing_file_errors() {
        let gobbler = OfficeGobbler;
        let p = Path::new("does_not_exist.docx");
        let args = crate::cli::Cli::parse_from(["filegoblin", "dummy"]);
        assert!(gobbler.gobble(p, &(&args).into()).is_err());
    }

    #[test]
    fn heading_styles_map_to_markdown_levels() {
        assert_eq!(heading_level("Heading1"), Some(1));
        assert_eq!(heading_level("Heading 3"), Some(3));
        assert_eq!(heading_level("Title"), Some(1));
        assert_eq!(heading_level("Heading9"), Some(6));
        assert_eq!(heading_level("Normal"), None);
    }

    #[test]
    fn paragraph_styles_render_as_markdown() {
        let links = HashMap::new();
        let heading = Paragraph::new()
            .add_run(Run::new().add_text("Chapter One"))
            .style("Heading2");
        assert_eq!(paragraph_to_md(&heading, &links), "## Chapter One");

        let bold = Paragraph::new().add_run(Run::new().add_text("important").bold());
        assert_eq!(paragraph_to_md(&bold, &links), "**important**");

        let listed = Paragraph::new()
            .add_run(Run::new().add_text("first item"))
            .numbering(docx_rs::NumberingId::new(1), docx_rs::IndentLevel::new(0));
        assert_eq!(paragraph_to_md(&listed, &links), "- first item");
    }

    #[test]
    fn emphasize_keeps_surrounding_whitespace_outside_markers() {
        assert_eq!(emphasize("word ", "**"), "**word** ");
        assert_eq!(emphasize(" word", "*"), " *word*");
    }
}
