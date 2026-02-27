use crate::parsers::gobble::Gobble;
use anyhow::{Context, Result};
use std::path::Path;
use calamine::{Reader, open_workbook_auto, Data};

pub struct SheetGobbler;

impl Gobble for SheetGobbler {
    fn gobble(&self, path: &Path) -> Result<String> {
        let extension = path.extension()
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
            .from_path(path)
            .context("Failed to open CSV file")?;

        let mut output = String::new();
        output.push_str(&format!("## Dataset: {}\n\n", path.file_name().unwrap_or_default().to_string_lossy()));

        let mut row_idx = 1;
        for result in rdr.records() {
            let record = result?;
            output.push_str(&format!("Row {}: ", row_idx));
            
            let mut col_parts = Vec::new();
            for (col_idx, field) in record.iter().enumerate() {
                if !field.trim().is_empty() {
                    let letter = column_index_to_letter(col_idx);
                    col_parts.push(format!("Col {}: {}", letter, field));
                }
            }

            output.push_str(&col_parts.join("; "));
            output.push('\n');
            row_idx += 1;
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
                
                let mut row_idx = 1;
                for row in range.rows() {
                    let mut col_parts = Vec::new();
                    let mut has_data = false;

                    for (col_idx, cell) in row.iter().enumerate() {
                        let val_str = match cell {
                            Data::String(s) => s.to_string(),
                            Data::Float(f) => f.to_string(),
                            Data::Int(i) => i.to_string(),
                            Data::Bool(b) => b.to_string(),
                            Data::DateTime(d) => d.as_f64().to_string(),
                            Data::DateTimeIso(d) => d.to_string(),
                            Data::DurationIso(d) => d.to_string(),
                            Data::Error(e) => format!("ERROR: {:?}", e),
                            Data::Empty => String::new(),
                        };

                        if !val_str.trim().is_empty() {
                            has_data = true;
                            let letter = column_index_to_letter(col_idx);
                            col_parts.push(format!("Col {}: {}", letter, val_str.trim()));
                        }
                    }

                    if has_data {
                        output.push_str(&format!("Row {}: ", row_idx));
                        output.push_str(&col_parts.join("; "));
                        output.push('\n');
                    }
                    row_idx += 1;
                }
                output.push('\n');
            }
        }

        if output.trim().is_empty() {
             return Ok("No data could be extracted from this workbook.".to_string());
        }

        Ok(output)
    }
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
