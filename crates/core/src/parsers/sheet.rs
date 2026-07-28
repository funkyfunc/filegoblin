use crate::parsers::gobble::{Gobble, ParseOptions};
use anyhow::{Context, Result};
use calamine::{Data, Reader, open_workbook_auto};
use std::path::Path;

/// CSV / spreadsheet parser emitting the same "Sequence of Records" dialect as
/// the PDF and docx table paths: the first row is treated as the header row and
/// its names label every value (`Row 1: Date: …; Revenue: …;`). A header cell
/// that is empty falls back to its spreadsheet column letter.
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
        let mut data_row = 0usize;
        let mut skipped = 0usize;
        for result in rdr.records() {
            // One malformed row must not abort the whole dataset.
            let Ok(record) = result else {
                skipped += 1;
                continue;
            };
            let fields: Vec<String> = record.iter().map(|f| f.trim().to_string()).collect();

            let Some(headers) = &headers else {
                headers = Some(header_names(&fields));
                output.push_str(&format!(
                    "Headers: {}\n",
                    headers.as_ref().unwrap().join("; ")
                ));
                continue;
            };

            data_row += 1;
            if let Some(line) = record_line(data_row, headers, &fields) {
                output.push_str(&line);
            }
        }
        if skipped > 0 {
            output.push_str(&format!("({} malformed row(s) skipped)\n", skipped));
        }

        Ok(output)
    }

    fn parse_workbook(&self, path: &Path) -> Result<String> {
        let mut workbook = open_workbook_auto(path).context("Failed to open WorkBook")?;
        let sheet_names = workbook.sheet_names().to_owned();

        let mut output = String::new();

        for sheet_name in sheet_names {
            if let Ok(range) = workbook.worksheet_range(&sheet_name) {
                output.push_str(&format!("## Sheet: {}\n\n", sheet_name));

                let mut headers: Option<Vec<String>> = None;
                let mut data_row = 0usize;
                for row in range.rows() {
                    let fields: Vec<String> = row.iter().map(cell_to_string).collect();

                    let Some(headers) = &headers else {
                        if fields.iter().all(|f| f.is_empty()) {
                            continue; // leading blank rows before the header
                        }
                        headers = Some(header_names(&fields));
                        output.push_str(&format!(
                            "Headers: {}\n",
                            headers.as_ref().unwrap().join("; ")
                        ));
                        continue;
                    };

                    data_row += 1;
                    if let Some(line) = record_line(data_row, headers, &fields) {
                        output.push_str(&line);
                    }
                }
                output.push('\n');
            }
        }

        if output.trim().is_empty() {
            anyhow::bail!("No data could be extracted from this workbook");
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

/// One data row as `Row N: Header: val; …` (empty cells omitted). Returns
/// `None` for entirely empty rows.
fn record_line(row_idx: usize, headers: &[String], fields: &[String]) -> Option<String> {
    let parts: Vec<String> = fields
        .iter()
        .enumerate()
        .filter(|(_, v)| !v.is_empty())
        .map(|(i, v)| {
            let fallback = format!("Col {}", column_index_to_letter(i));
            let header = headers.get(i).filter(|h| !h.is_empty());
            format!("{}: {}", header.unwrap_or(&fallback), v)
        })
        .collect();
    if parts.is_empty() {
        return None;
    }
    Some(format!("Row {}: {}\n", row_idx, parts.join("; ")))
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

        assert!(result.contains("Headers: Date; Revenue; Growth"));
        assert!(result.contains("Row 1: Date: 2026-01-01; Revenue: 100; Growth: +5%"));
        assert!(result.contains("Row 2: Date: 2026-02-01; Revenue: 200; Growth: +10%"));
    }

    #[test]
    fn empty_header_cells_fall_back_to_column_letters() {
        let mut file = tempfile::Builder::new().suffix(".csv").tempfile().unwrap();
        writeln!(file, "Name,,Score").unwrap();
        writeln!(file, "alice,x,10").unwrap();
        file.flush().unwrap();

        let args = crate::cli::Cli::parse_from(["filegoblin", "dummy"]);
        let result = SheetGobbler.gobble(file.path(), &(&args).into()).unwrap();

        assert!(result.contains("Row 1: Name: alice; Col B: x; Score: 10"));
    }

    #[test]
    fn column_letters_wrap_past_z() {
        assert_eq!(column_index_to_letter(0), "A");
        assert_eq!(column_index_to_letter(25), "Z");
        assert_eq!(column_index_to_letter(26), "AA");
    }
}
