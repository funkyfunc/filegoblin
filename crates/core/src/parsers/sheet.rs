use crate::parsers::gobble::{Gobble, ParseOptions};
use anyhow::{Context, Result};
use calamine::{Data, Reader, open_workbook_auto};
use std::path::Path;

/// CSV / spreadsheet parser emitting standard markdown pipe tables — the same
/// dialect every other parser (PDF, docx, pptx) produces. The first row is the
/// header row; an empty header cell falls back to its spreadsheet column letter.
pub struct SheetGobbler;

impl Gobble for SheetGobbler {
    fn gobble(&self, path: &std::path::Path, opts: &ParseOptions) -> Result<String> {
        let extension = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_lowercase();

        match extension.as_str() {
            "csv" => self.parse_csv(path, None),
            // Tab-separated by definition: no sniffing.
            "tsv" => self.parse_csv(path, Some(b'\t')),
            // An Excel 97–2003 workbook (or a password-protected .xlsx) is a
            // compound file. calamine's reader for those panics on damaged
            // files, and a few kilobytes can make it allocate gigabytes
            // (ADR 2026-10-23); the legacy Office reader does neither.
            _ if crate::parsers::legacy_office::is_compound_file(path) => {
                crate::parsers::legacy_office::LegacyOfficeGobbler.gobble(path, opts)
            }
            // Every other workbook flavour (xlsx/xlsm/xltx/xltm/xlam, xlsb,
            // ods), plus renamed workbooks routed here by magic: calamine picks
            // the reader from the extension, or tries each one when it can't.
            _ => self.parse_workbook(path),
        }
    }
}

/// Delimiters a CSV is sniffed for, in tie-break order.
const CSV_DELIMITERS: [u8; 4] = *b",;\t|";
/// How many leading records the delimiter sniff looks at.
const CSV_SNIFF_RECORDS: usize = 20;

impl SheetGobbler {
    /// `delimiter: None` sniffs it. Bytes are decoded first (BOM, UTF-8,
    /// Windows-1252): the csv reader only yields UTF-8 records, so a Latin-1
    /// export used to lose every row with an accented letter.
    fn parse_csv(&self, path: &Path, delimiter: Option<u8>) -> Result<String> {
        let bytes = std::fs::read(path).context("Failed to open CSV file")?;
        let text = crate::parsers::text::decode_text(bytes).context("Failed to read CSV file")?;
        let delimiter = delimiter.unwrap_or_else(|| sniff_delimiter(&text));
        let records = csv::ReaderBuilder::new()
            .has_headers(false)
            .flexible(true)
            .delimiter(delimiter)
            .from_reader(text.as_bytes())
            .into_records();
        let title = path.file_name().unwrap_or_default().to_string_lossy();
        render_csv_records(&title, records)
    }

    fn parse_workbook(&self, path: &Path) -> Result<String> {
        let mut workbook = open_workbook_auto(path).context("Failed to open WorkBook")?;
        let sheet_names = workbook.sheet_names().to_owned();

        let mut output = String::new();

        let mut failed_sheets: Vec<String> = Vec::new();
        for sheet_name in sheet_names {
            match workbook.worksheet_range(&sheet_name) {
                Ok(range) => output.push_str(&render_sheet(
                    &sheet_name,
                    range
                        .rows()
                        .map(|row| row.iter().map(cell_to_string).collect()),
                )),
                // A sheet that fails to open used to disappear entirely, so a
                // five-sheet workbook could silently arrive with four.
                Err(e) => failed_sheets.push(format!("{}: {}", sheet_name, e)),
            }
        }
        finish_workbook(output, &failed_sheets)
    }
}

/// One sheet as `## Sheet: <name>` and a pipe table. Leading blank rows are
/// skipped; the first row with content is the header.
pub(crate) fn render_sheet(name: &str, rows: impl IntoIterator<Item = Vec<String>>) -> String {
    let mut output = format!("## Sheet: {}\n\n", name);
    let mut headers: Option<Vec<String>> = None;
    for fields in rows {
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
    output
}

/// The rendered sheets of a workbook, with any sheet that could not be read
/// named in-band. A workbook that yields nothing is an error.
pub(crate) fn finish_workbook(mut output: String, failed_sheets: &[String]) -> Result<String> {
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

/// Picks the CSV delimiter from the first [`CSV_SNIFF_RECORDS`] records.
///
/// Excel in most of Europe writes `;` (the comma is the decimal separator), so
/// assuming `,` turned `name;city;amount` into a one-column table. Each
/// candidate is scored by how many sampled records share its most common field
/// count, then by that count; a candidate that never splits a row (count 1)
/// cannot win. Parsing is quote-aware, so a delimiter inside a quoted field
/// does not count. With no winner the file is single-column, read with `,`.
fn sniff_delimiter(text: &str) -> u8 {
    let mut best: Option<(u8, (usize, usize))> = None;
    for delimiter in CSV_DELIMITERS {
        let counts: Vec<usize> = csv::ReaderBuilder::new()
            .has_headers(false)
            .flexible(true)
            .delimiter(delimiter)
            .from_reader(text.as_bytes())
            .into_records()
            .filter_map(Result::ok)
            .take(CSV_SNIFF_RECORDS)
            .map(|record| record.len())
            .collect();
        let Some(fields) = modal_value(&counts) else {
            continue;
        };
        if fields < 2 {
            continue;
        }
        let agreeing = counts.iter().filter(|&&c| c == fields).count();
        // Agreement as a fraction, kept in integers: per-mille is plenty.
        let score = (agreeing * 1000 / counts.len(), fields);
        if best.is_none_or(|(_, top)| score > top) {
            best = Some((delimiter, score));
        }
    }
    best.map_or(b',', |(delimiter, _)| delimiter)
}

/// The most common value (the smallest one on a tie), or `None` when empty.
fn modal_value(values: &[usize]) -> Option<usize> {
    let mut freq: std::collections::BTreeMap<usize, usize> = std::collections::BTreeMap::new();
    for v in values {
        *freq.entry(*v).or_default() += 1;
    }
    freq.into_iter()
        .max_by(|(va, fa), (vb, fb)| fa.cmp(fb).then(vb.cmp(va)))
        .map(|(v, _)| v)
}

/// Renders CSV records as a pipe table titled `## Dataset: <title>`; row 1 is
/// the header.
///
/// A malformed row is skipped (one bad line must not abort the dataset) and
/// counted in-band, but a CSV with no rows, or whose every data row was
/// malformed, is an error: printing just the header and exiting 0 was silent
/// data loss.
fn render_csv_records(
    title: &str,
    records: impl Iterator<Item = csv::Result<csv::StringRecord>>,
) -> Result<String> {
    let mut output = format!("## Dataset: {}\n\n", title);
    let mut headers: Option<Vec<String>> = None;
    let mut skipped = 0usize;
    let mut data_rows = 0usize;
    for result in records {
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
            data_rows += 1;
        }
    }

    match (headers.is_some(), data_rows, skipped) {
        (false, _, 0) => anyhow::bail!("This CSV has no rows"),
        (false, _, skipped) => {
            anyhow::bail!("Every row of this CSV was malformed ({skipped} skipped)")
        }
        (true, 0, skipped) if skipped > 0 => anyhow::bail!(
            "All {skipped} data row(s) of this CSV were malformed; only the header row \
             could be read"
        ),
        _ => {}
    }
    if skipped > 0 {
        output.push_str(&format!("\n({} malformed row(s) skipped)\n", skipped));
    }
    Ok(output)
}

fn cell_to_string(cell: &Data) -> String {
    match cell {
        Data::String(s) => s.trim().to_string(),
        Data::Float(f) => format_float(*f),
        Data::Int(i) => i.to_string(),
        Data::Bool(b) => b.to_string(),
        // A date is stored as a serial number; `as_f64().to_string()` printed
        // `45292.5` where the sheet showed a date, which is unreadable to a
        // human and actively misleading to a model.
        Data::DateTime(d) => excel_serial_to_string(d.as_f64()),
        Data::DateTimeIso(d) => d.to_string(),
        Data::DurationIso(d) => d.to_string(),
        Data::Error(e) => format!("ERROR: {:?}", e),
        Data::Empty => String::new(),
    }
}

/// Renders an Excel date serial as `YYYY-MM-DD`, adding `HH:MM:SS` when the
/// cell carries a time component.
///
/// Done by hand rather than pulling in chrono (calamine only exposes the
/// conversion behind its `dates` feature). Serials count days from 1899-12-30;
/// serials at or below 59 predate Excel's phantom 1900-02-29 and need one day
/// back. A serial that doesn't convert sensibly falls back to the raw number
/// rather than inventing a date.
fn excel_serial_to_string(serial: f64) -> String {
    if !serial.is_finite() || serial <= 0.0 || serial > 2_958_465.0 {
        return format_float(serial);
    }
    let days_from_1899 = serial.trunc() as i64;
    // Days between 1899-12-30 and the 1970-01-01 civil epoch.
    let unix_days = if days_from_1899 <= 59 {
        days_from_1899 - 25_568
    } else {
        days_from_1899 - 25_569
    };
    let (y, m, d) = civil_from_days(unix_days);

    let frac = serial.fract();
    if frac <= f64::EPSILON {
        return format!("{:04}-{:02}-{:02}", y, m, d);
    }
    // Round to the nearest second: a serial is a binary fraction of a day, so
    // 12:00 often arrives as .49999999.
    let total_seconds = (frac * 86_400.0).round() as i64;
    let (h, min, s) = (
        total_seconds / 3600,
        (total_seconds % 3600) / 60,
        total_seconds % 60,
    );
    if h >= 24 {
        // Rounded past midnight; report the date alone rather than 24:00:00.
        return format!("{:04}-{:02}-{:02}", y, m, d);
    }
    format!("{:04}-{:02}-{:02} {:02}:{:02}:{:02}", y, m, d, h, min, s)
}

/// Days since 1970-01-01 → (year, month, day). Howard Hinnant's `civil_from_days`.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365; // [0, 399]
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32; // [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32; // [1, 12]
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// Formats a spreadsheet float without binary-representation noise.
///
/// `f64::to_string` faithfully prints `0.30000000000000004` for a cell the
/// sheet displays as `0.3`. Excel itself only guarantees 15 significant digits,
/// so rounding there and trimming trailing zeros reproduces what the user saw.
fn format_float(f: f64) -> String {
    if !f.is_finite() {
        return f.to_string();
    }
    if f == f.trunc() && f.abs() < 1e15 {
        return format!("{}", f as i64);
    }
    // 15 *significant* digits, via scientific notation, then back to a plain
    // number. Excel itself only guarantees 15, so this reproduces the displayed
    // value rather than the stored binary approximation.
    match format!("{:.14e}", f).parse::<f64>() {
        Ok(rounded) => rounded.to_string(),
        Err(_) => f.to_string(),
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
        write_workbook(".xlsx", rows)
    }

    /// [`write_xlsx`] under any file suffix (`.xlsm`, `.xltx`, none, …).
    fn write_workbook(suffix: &str, rows: &[&[&str]]) -> tempfile::NamedTempFile {
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

        let mut file = tempfile::Builder::new().suffix(suffix).tempfile().unwrap();
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

    /// Regression: macro-enabled and template workbooks were extracted as an
    /// archive of raw XML; a renamed workbook was "Unsupported sheet extension".
    #[test]
    fn workbook_variants_and_renamed_workbooks_render() {
        let rows: &[&[&str]] = &[&["Item", "Qty"], &["Sword", "2"]];
        let args = crate::cli::Cli::parse_from(["filegoblin", "dummy"]);
        for suffix in [".xlsm", ".xltx", ".xltm", ".xlam", ".XLSX", ".bin", ""] {
            let file = write_workbook(suffix, rows);
            let out = SheetGobbler
                .gobble(file.path(), &(&args).into())
                .unwrap_or_else(|e| panic!("{suffix:?}: {e:#}"));
            assert!(out.contains("| Sword | 2 |"), "{suffix:?}: {out}");
        }
    }

    /// End to end through format detection: a renamed workbook routes to the
    /// sheet parser by its zip entries.
    #[test]
    fn renamed_workbook_routes_by_content() {
        let file = write_workbook(".bin", &[&["Item", "Qty"], &["Sword", "2"]]);
        let args = crate::cli::Cli::parse_from(["filegoblin", "dummy"]);
        let (format, out) = crate::route_and_gobble(&file.path().to_string_lossy(), &args);
        assert_eq!(format, crate::format::Format::Spreadsheet);
        assert!(out.unwrap().contains("| Sword | 2 |"));
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

    fn write_bytes(suffix: &str, bytes: &[u8]) -> tempfile::NamedTempFile {
        let mut file = tempfile::Builder::new().suffix(suffix).tempfile().unwrap();
        file.write_all(bytes).unwrap();
        file.flush().unwrap();
        file
    }

    fn gobble_file(file: &tempfile::NamedTempFile) -> Result<String> {
        let args = crate::cli::Cli::parse_from(["filegoblin", "dummy"]);
        SheetGobbler.gobble(file.path(), &(&args).into())
    }

    /// Regression: a Latin-1, semicolon-delimited export (Excel in most of
    /// Europe) printed only its header, "(2 malformed row(s) skipped)", and
    /// exited 0.
    #[test]
    fn latin1_semicolon_csv_keeps_every_row() {
        let file = write_bytes(
            ".csv",
            b"name;city;amount\nJos\xe9;M\xfcnchen;1,50\nAnna;K\xf6ln;2,75\n",
        );
        let out = gobble_file(&file).unwrap();
        assert!(out.contains("| name | city | amount |"), "{out}");
        assert!(out.contains("|---|---|---|"), "{out}");
        assert!(out.contains("| José | München | 1,50 |"), "{out}");
        assert!(out.contains("| Anna | Köln | 2,75 |"), "{out}");
        assert!(!out.contains("malformed"), "{out}");
    }

    /// Regression: a UTF-8 semicolon CSV came out as a single column.
    #[test]
    fn utf8_semicolon_csv_is_split_into_columns() {
        let file = write_bytes(".csv", "name;city\nJosé;Berlin\n".as_bytes());
        let out = gobble_file(&file).unwrap();
        assert!(out.contains("| name | city |"), "{out}");
        assert!(out.contains("| José | Berlin |"), "{out}");
    }

    #[test]
    fn tsv_is_tab_delimited() {
        let file = write_bytes(".tsv", b"a\tb, with comma\n1\t2;3\n");
        let out = gobble_file(&file).unwrap();
        assert!(out.contains("| a | b, with comma |"), "{out}");
        assert!(out.contains("| 1 | 2;3 |"), "{out}");
    }

    #[test]
    fn delimiter_sniffing() {
        assert_eq!(sniff_delimiter("a,b,c\n1,2,3\n"), b',');
        assert_eq!(sniff_delimiter("a;b\n1,5;2,5\n3,0;4,0\n"), b';');
        assert_eq!(sniff_delimiter("a\tb\tc\n1\t2\t3\n"), b'\t');
        assert_eq!(sniff_delimiter("a|b\n1|2\n"), b'|');
        // A delimiter inside a quoted field doesn't count.
        assert_eq!(
            sniff_delimiter("name,note\nA,\"x;y;z\"\nB,\"p;q;r\"\n"),
            b','
        );
        // Single-column data has no winner and reads with the default.
        assert_eq!(sniff_delimiter("word\nother\n"), b',');
        assert_eq!(sniff_delimiter(""), b',');
    }

    /// A UTF-8 BOM (Excel's "CSV UTF-8") must not end up in the first header.
    #[test]
    fn utf8_bom_is_not_part_of_the_header() {
        let file = write_bytes(".csv", b"\xef\xbb\xbfid,name\n1,x\n");
        let out = gobble_file(&file).unwrap();
        assert!(out.contains("| id | name |"), "{out}");
    }

    /// Printing the header alone and exiting 0 was silent data loss.
    #[test]
    fn all_data_rows_malformed_is_an_error() {
        // Raw, undecoded invalid UTF-8 makes the csv reader reject each row.
        let records = csv::ReaderBuilder::new()
            .has_headers(false)
            .flexible(true)
            .from_reader(&b"name,city\n\xff,\xfe\n\xfd,\xfc\n"[..])
            .into_records();
        let err = render_csv_records("t.csv", records)
            .unwrap_err()
            .to_string();
        assert!(err.contains("All 2 data row(s)"), "{err}");

        // Some rows surviving is a partial success, reported in-band.
        let records = csv::ReaderBuilder::new()
            .has_headers(false)
            .flexible(true)
            .from_reader(&b"name,city\n\xff,\xfe\nok,fine\n"[..])
            .into_records();
        let out = render_csv_records("t.csv", records).unwrap();
        assert!(out.contains("| ok | fine |"), "{out}");
        assert!(out.contains("(1 malformed row(s) skipped)"), "{out}");
    }

    #[test]
    fn empty_csv_is_an_error() {
        let file = write_bytes(".csv", b"");
        assert!(gobble_file(&file).is_err());
    }

    /// Binary data with a .csv name is refused rather than decoded as text.
    #[test]
    fn binary_csv_is_refused() {
        let file = write_bytes(".csv", b"\x00\x9f\x92\x00PK\x03\x04\xff\x00");
        assert!(gobble_file(&file).is_err());
    }
}

#[cfg(test)]
mod value_format_tests {
    use super::*;

    /// `f64::to_string` printed `0.30000000000000004` for a cell showing `0.3`.
    #[test]
    fn floats_lose_binary_noise_but_stay_exact() {
        assert_eq!(format_float(0.1 + 0.2), "0.3");
        assert_eq!(format_float(1.0), "1");
        assert_eq!(format_float(42.5), "42.5");
        assert_eq!(format_float(-0.75), "-0.75");
        assert_eq!(format_float(1e20), "100000000000000000000");
        // Rounding is to 15 significant digits — what the sheet displays — so a
        // repeating value keeps its digits without the 16th-place artifact.
        assert!(
            format_float(1.0 / 3.0).starts_with("0.333333333333333"),
            "got: {}",
            format_float(1.0 / 3.0)
        );
    }

    /// A date used to render as its raw serial (`45292.5`).
    #[test]
    fn date_serials_render_as_dates() {
        // 45292 = 2024-01-01; the .5 is midday.
        assert_eq!(excel_serial_to_string(45292.0), "2024-01-01");
        assert_eq!(excel_serial_to_string(45292.5), "2024-01-01 12:00:00");
        // 25569 = the unix epoch.
        assert_eq!(excel_serial_to_string(25569.0), "1970-01-01");
        // 1 = 1900-01-01, before Excel's phantom leap day.
        assert_eq!(excel_serial_to_string(1.0), "1900-01-01");
        // 61 = 1900-03-01, the first serial after it.
        assert_eq!(excel_serial_to_string(61.0), "1900-03-01");
        // Nonsense falls back to the number rather than inventing a date.
        assert_eq!(excel_serial_to_string(-5.0), "-5");
    }

    #[test]
    fn civil_conversion_handles_leap_years() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(59), (1970, 3, 1));
        // 2000 was a leap year; 1900 was not. 36526 = 2000-01-01, so
        // 2000-02-29 is 36585 and 2000-03-01 is 36586.
        assert_eq!(excel_serial_to_string(36526.0), "2000-01-01");
        assert_eq!(excel_serial_to_string(36585.0), "2000-02-29");
        assert_eq!(excel_serial_to_string(36586.0), "2000-03-01");
    }
}
