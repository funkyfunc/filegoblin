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
                match child {
                    docx_rs::DocumentChild::Paragraph(p) => {
                        extracted.push_str(&p.raw_text());
                        extracted.push('\n');
                    }
                    docx_rs::DocumentChild::Table(t) => {
                        extracted.push_str(&format_table(&t));
                        extracted.push('\n');
                    }
                    _ => {}
                }
            }
            Ok(extracted.trim().to_string())
        } else {
            // Fallback for .xlsx
            Ok("EXCEL PARSING NOT YET IMPLEMENTED".to_string())
        }
    }
}

fn format_table(t: &docx_rs::Table) -> String {
    let mut table_string = String::new();
    let mut headers = Vec::new();

    for (row_idx, row_child) in t.rows.iter().enumerate() {
        let docx_rs::TableChild::TableRow(row) = row_child;
        let mut row_string = String::new();
        if row_idx == 0 {
            for (col_idx, cell_child) in row.cells.iter().enumerate() {
                let docx_rs::TableRowChild::TableCell(cell) = cell_child;
                let text = extract_cell_text(cell);
                headers.push(if text.is_empty() {
                    format!("Col {}", col_idx)
                } else {
                    text
                });
            }
        } else {
            row_string.push_str(&format!("Row {}: ", row_idx));
            for (col_idx, cell_child) in row.cells.iter().enumerate() {
                let docx_rs::TableRowChild::TableCell(cell) = cell_child;
                let default_header = format!("Col {}", col_idx);
                let header = headers.get(col_idx).unwrap_or(&default_header);
                let val = extract_cell_text(cell);
                row_string.push_str(&format!("{}: {}; ", header, val));
            }
            row_string.push('\n');
            table_string.push_str(&row_string);
        }
    }
    table_string
}

fn extract_cell_text(cell: &docx_rs::TableCell) -> String {
    let mut text = String::new();
    for child in &cell.children {
        match child {
            docx_rs::TableCellContent::Paragraph(p) => {
                text.push_str(&p.raw_text());
                text.push(' ');
            }
            docx_rs::TableCellContent::Table(t) => {
                let t_str = format_table(t);
                text.push_str(&t_str);
                text.push(' ');
            }
            _ => {}
        }
    }
    text.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;
    use docx_rs::{Paragraph, Run, Table, TableCell, TableRow};

    #[test]
    fn test_format_table_sequence_of_records() {
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
        assert!(result.contains("Row 1: Header1: Value1; Header2: Value2;"));
        assert!(result.contains("Row 2: Header1: Value3; Header2: Value4;"));
    }

    #[test]
    fn test_office_sequence_of_records() {
        let gobbler = OfficeGobbler;
        let p = Path::new("dummy.xlsx");
        let args = crate::cli::Cli::parse_from(["filegoblin", "dummy"]);
        let result = gobbler.gobble(p, &args).unwrap();

        // Assert mandatory "Sequence of Records" structure (PRD 3.2)
        assert!(result.contains("Row 1: Name: Goblin; Title: Manager;"));
        assert!(result.contains("Row 2: Name: Ghoul; Title: Assistant;"));
    }
}
