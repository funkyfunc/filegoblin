use crate::parsers::gobble::{Gobble, ParseOptions};
use anyhow::{Context, Result};
use calamine::{Data, Reader, open_workbook_auto};
use std::path::Path;

/// CSV / spreadsheet parser emitting standard markdown pipe tables — the same
/// dialect every other parser (PDF, docx, pptx) produces. The first row is the
/// header row; an empty header cell falls back to its spreadsheet column letter.
pub struct SheetGobbler;

impl Gobble for SheetGobbler {
    fn gobble(&self, path: &std::path::Path, _opts: &ParseOptions) -> Result<String> {
        let extension = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_lowercase();

        match extension.as_str() {
            "csv" => self.parse_csv(path),
            "xlsx" | "xls" | "ods" => self.parse_workbook(path),
            _ => anyhow::bail!("Unsupported sheet extension: {}", extension),
        }
    }
}

impl SheetGobbler {
    fn parse_csv(&self, path: &Path) -> Result<String> {
        let mut rdr = csv::ReaderBuilder::new()
            .has_headers(false)
            .flexible(true)
            .from_path(path)
            .context("Failed to open CSV file")?;

        let mut output = String::new();
        output.push_str(&format!(
            "## Dataset: {}\n\n",
            path.file_name().unwrap_or_default().to_string_lossy()
        ));

        let mut headers: Option<Vec<String>> = None;
        let mut skipped = 0usize;
        for result in rdr.records() {
            // One malformed row must not abort the whole dataset.
            let Ok(record) = result else {
                skipped += 1;
                continue;
            };
            let fields: Vec<String> = record.iter().map(|f| f.trim().to_string()).collect();

            let Some(headers) = &headers else {
                let names = header_names(&fields);
                output.push_str(&table_header(&names));
                headers = Some(names);
                continue;
            };

            if let Some(line) = table_row(headers.len(), &fields) {
                output.push_str(&line);
            }
        }
        if skipped > 0 {
            output.push_str(&format!("\n({} malformed row(s) skipped)\n", skipped));
        }

        Ok(output)
    }

    fn parse_workbook(&self, path: &Path) -> Result<String> {
        let mut workbook = open_workbook_auto(path).context("Failed to open WorkBook")?;
        let sheet_names = workbook.sheet_names().to_owned();

        let mut output = String::new();

        let mut failed_sheets: Vec<String> = Vec::new();
        for sheet_name in sheet_names {
            match workbook.worksheet_range(&sheet_name) {
                Ok(range) => {
                    output.push_str(&format!("## Sheet: {}\n\n", sheet_name));

                    let mut headers: Option<Vec<String>> = None;
                    for row in range.rows() {
                        let fields: Vec<String> = row.iter().map(cell_to_string).collect();

                        let Some(headers) = &headers else {
                            if fields.iter().all(|f| f.is_empty()) {
                                continue; // leading blank rows before the header
                            }
                            let names = header_names(&fields);
                            output.push_str(&table_header(&names));
                            headers = Some(names);
                            continue;
                        };

                        if let Some(line) = table_row(headers.len(), &fields) {
                            output.push_str(&line);
                        }
                    }
                    output.push('\n');
                }
                // A sheet that fails to open used to disappear entirely, so a
                // five-sheet workbook could silently arrive with four.
                Err(e) => failed_sheets.push(format!("{}: {}", sheet_name, e)),
            }
        }

        if output.trim().is_empty() {
            if !failed_sheets.is_empty() {
                anyhow::bail!(
                    "No sheet in this workbook could be read:\n{}",
                    failed_sheets.join("\n")
                );
            }
            anyhow::bail!("No data could be extracted from this workbook");
        }
        if !failed_sheets.is_empty() {
            output.push_str(&format!(
                "\n> [{} sheet(s) could not be read: {}]\n",
                failed_sheets.len(),
                failed_sheets.join("; ")
            ));
        }

        Ok(output)
    }
}

fn cell_to_string(cell: &Data) -> String {
    match cell {
        Data::String(s) => s.trim().to_string(),
        Data::Float(f) => f.to_string(),
        Data::Int(i) => i.to_string(),
        Data::Bool(b) => b.to_string(),
        Data::DateTime(d) => d.as_f64().to_string(),
        Data::DateTimeIso(d) => d.to_string(),
        Data::DurationIso(d) => d.to_string(),
        Data::Error(e) => format!("ERROR: {:?}", e),
        Data::Empty => String::new(),
    }
}

/// Header names from the first row; empty cells fall back to column letters.
fn header_names(first_row: &[String]) -> Vec<String> {
    first_row
        .iter()
        .enumerate()
        .map(|(i, name)| {
            if name.is_empty() {
                format!("Col {}", column_index_to_letter(i))
            } else {
                name.clone()
            }
        })
        .collect()
}

/// The markdown table header + separator lines.
fn table_header(headers: &[String]) -> String {
    format!(
        "| {} |\n|{}\n",
        headers
            .iter()
            .map(|h| escape_cell(h))
            .collect::<Vec<_>>()
            .join(" | "),
        "---|".repeat(headers.len())
    )
}

/// One data row as a markdown table row, padded/truncated to the header width.
/// Returns `None` for entirely empty rows.
fn table_row(width: usize, fields: &[String]) -> Option<String> {
    if fields.iter().all(|f| f.is_empty()) {
        return None;
    }
    let cells: Vec<String> = (0..width.max(fields.len()))
        .map(|i| escape_cell(fields.get(i).map(String::as_str).unwrap_or("")))
        .collect();
    Some(format!("| {} |\n", cells.join(" | ")))
}

/// Pipe and newline characters inside a cell would break the table shape.
fn escape_cell(v: &str) -> String {
    v.replace('|', "\\|").replace('\n', " ")
}

fn column_index_to_letter(mut col: usize) -> String {
    let mut name = String::new();
    loop {
        let rem = col % 26;
        let c = (b'A' + rem as u8) as char;
        name.insert(0, c);
        if col < 26 {
            break;
        }
        col = (col / 26) - 1;
    }
    name
}

#[cfg(test)]
mod xlsx_tests {
    use super::*;
    use clap::Parser;
    use std::io::Write;

    /// Builds a minimal but genuinely valid .xlsx (a zip of OOXML parts).
    ///
    /// `.xlsx` is a headline format with no fixture and no test of any kind —
    /// every sheet test used CSV, which does not touch calamine at all. A
    /// calamine upgrade that broke workbook reading would have been invisible.
    fn write_xlsx(rows: &[&[&str]]) -> tempfile::NamedTempFile {
        let mut sheet = String::from(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData>"#,
        );
        for (r, row) in rows.iter().enumerate() {
            sheet.push_str(&format!("<row r=\"{}\">", r + 1));
            for (c, cell) in row.iter().enumerate() {
                let col = super::column_index_to_letter(c);
                if cell.is_empty() {
                    continue; // a genuinely absent cell, as Excel writes it
                }
                // `t="inlineStr"` keeps the fixture free of sharedStrings.xml.
                sheet.push_str(&format!(
                    "<c r=\"{}{}\" t=\"inlineStr\"><is><t>{}</t></is></c>",
                    col,
                    r + 1,
                    cell
                ));
            }
            sheet.push_str("</row>");
        }
        sheet.push_str("</sheetData></worksheet>");

        let parts: Vec<(&str, String)> = vec![
            ("[Content_Types].xml", r#"<?xml version="1.0" encoding="UTF-8"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/><Override PartName="/xl/worksheets/sheet1.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/></Types>"#.to_string()),
            ("_rels/.rels", r#"<?xml version="1.0" encoding="UTF-8"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/></Relationships>"#.to_string()),
            ("xl/workbook.xml", r#"<?xml version="1.0" encoding="UTF-8"?>
<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets><sheet name="Ledger" sheetId="1" r:id="rId1"/></sheets></workbook>"#.to_string()),
            ("xl/_rels/workbook.xml.rels", r#"<?xml version="1.0" encoding="UTF-8"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/></Relationships>"#.to_string()),
            ("xl/worksheets/sheet1.xml", sheet),
        ];

        let mut file = tempfile::Builder::new().suffix(".xlsx").tempfile().unwrap();
        {
            let mut zip = zip::ZipWriter::new(file.as_file_mut());
            let opts = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Deflated);
            for (name, body) in parts {
                zip.start_file(name, opts).unwrap();
                zip.write_all(body.as_bytes()).unwrap();
            }
            zip.finish().unwrap();
        }
        file
    }

    #[test]
    fn xlsx_workbook_renders_a_pipe_table() {
        let file = write_xlsx(&[
            &["Item", "Qty", "Price"],
            &["Sword", "2", "150"],
            &["Shield", "1", "90"],
        ]);
        let args = crate::cli::Cli::parse_from(["filegoblin", "dummy"]);
        let out = SheetGobbler.gobble(file.path(), &(&args).into()).unwrap();

        assert!(out.contains("## Sheet: Ledger"), "got: {out}");
        assert!(out.contains("| Item | Qty | Price |"), "got: {out}");
        assert!(out.contains("|---|---|---|"), "got: {out}");
        assert!(out.contains("| Sword | 2 | 150 |"), "got: {out}");
        assert!(out.contains("| Shield | 1 | 90 |"), "got: {out}");
    }

    /// A gap in the middle of a row must not shift later values left.
    #[test]
    fn xlsx_missing_cells_keep_their_column() {
        let file = write_xlsx(&[&["A", "B", "C"], &["left", "", "right"]]);
        let args = crate::cli::Cli::parse_from(["filegoblin", "dummy"]);
        let out = SheetGobbler.gobble(file.path(), &(&args).into()).unwrap();
        assert!(out.contains("| left |  | right |"), "got: {out}");
    }

    /// An .xlsx that is not a zip at all must fail loudly.
    #[test]
    fn corrupt_xlsx_errors() {
        let mut file = tempfile::Builder::new().suffix(".xlsx").tempfile().unwrap();
        file.write_all(b"this is definitely not a workbook")
            .unwrap();
        file.flush().unwrap();
        let args = crate::cli::Cli::parse_from(["filegoblin", "dummy"]);
        assert!(SheetGobbler.gobble(file.path(), &(&args).into()).is_err());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;
    use std::io::Write;

    #[test]
    fn csv_uses_first_row_as_headers() {
        let mut file = tempfile::Builder::new().suffix(".csv").tempfile().unwrap();
        writeln!(file, "Date,Revenue,Growth").unwrap();
        writeln!(file, "2026-01-01,100,+5%").unwrap();
        writeln!(file, "2026-02-01,200,+10%").unwrap();
        file.flush().unwrap();

        let args = crate::cli::Cli::parse_from(["filegoblin", "dummy"]);
        let result = SheetGobbler.gobble(file.path(), &(&args).into()).unwrap();

        assert!(result.contains("| Date | Revenue | Growth |"));
        assert!(result.contains("|---|---|---|"));
        assert!(result.contains("| 2026-01-01 | 100 | +5% |"));
        assert!(result.contains("| 2026-02-01 | 200 | +10% |"));
    }

    #[test]
    fn empty_header_cells_fall_back_to_column_letters() {
        let mut file = tempfile::Builder::new().suffix(".csv").tempfile().unwrap();
        writeln!(file, "Name,,Score").unwrap();
        writeln!(file, "alice,x,10").unwrap();
        file.flush().unwrap();

        let args = crate::cli::Cli::parse_from(["filegoblin", "dummy"]);
        let result = SheetGobbler.gobble(file.path(), &(&args).into()).unwrap();

        assert!(result.contains("| Name | Col B | Score |"));
        assert!(result.contains("| alice | x | 10 |"));
    }

    #[test]
    fn column_letters_wrap_past_z() {
        assert_eq!(column_index_to_letter(0), "A");
        assert_eq!(column_index_to_letter(25), "Z");
        assert_eq!(column_index_to_letter(26), "AA");
    }
}
