use crate::parsers::gobble::{Gobble, ParseOptions};
use anyhow::{Context, Result};
use oxidize_pdf::text::extraction::TextFragment;
use oxidize_pdf::text::table_detection::DetectedTable;
use std::path::Path;

pub struct PdfGobbler;

impl Gobble for PdfGobbler {
    fn gobble(&self, path: &Path, _opts: &ParseOptions) -> Result<String> {
        if !path.exists() {
            anyhow::bail!("Mischievous I/O Error: file not found at {:?}", path);
        }

        use oxidize_pdf::graphics::extraction::GraphicsExtractor;
        use oxidize_pdf::parser::{PdfDocument, PdfReader};
        use oxidize_pdf::text::extraction::{ExtractionOptions, TextExtractor};
        use oxidize_pdf::text::table_detection::TableDetector;

        let mut final_text = String::new();

        let reader =
            PdfReader::open(path).context("Mischievous PDF Error: failed to open document")?;
        let document = PdfDocument::new(reader);
        let page_count = document.page_count().context(
            "Mischievous PDF Error: could not read the page tree (encrypted or corrupt?)",
        )?;
        if page_count == 0 {
            anyhow::bail!("Mischievous PDF Error: the document has no pages");
        }

        let mut graphics_ext = GraphicsExtractor::default();
        let mut text_ext = TextExtractor::with_options(ExtractionOptions {
            preserve_layout: true,
            ..Default::default()
        });
        let detector = TableDetector::default();

        for i in 0..page_count {
            let mut page_text = String::new();
            let mut text_extracted = text_ext.extract_from_page(&document, i).ok();
            let graphics_extracted = graphics_ext.extract_from_page(&document, i as usize).ok();

            if let (Some(t_ext), Some(g_ext)) = (&mut text_extracted, &graphics_extracted) {
                if let Ok(tables) = detector.detect(g_ext, &t_ext.fragments) {
                    for table in tables {
                        let table_string = format_detected_table(&table, &t_ext.fragments);

                        t_ext.fragments.retain(|frag| {
                            let center_x = frag.x + frag.width / 2.0;
                            let center_y = frag.y + frag.height / 2.0;
                            !table.bbox.contains_point(center_x, center_y)
                        });

                        page_text.push_str(&table_string);
                        page_text.push('\n');
                    }
                }

                // Append remaining fragments as text
                t_ext.fragments.sort_by(|a, b| {
                    let y_diff = (b.y - a.y).abs();
                    if y_diff < 10.0 {
                        a.x.partial_cmp(&b.x).unwrap_or(std::cmp::Ordering::Equal)
                    } else {
                        b.y.partial_cmp(&a.y).unwrap_or(std::cmp::Ordering::Equal)
                    }
                });

                let mut last_y = f64::INFINITY;
                for frag in &t_ext.fragments {
                    if (last_y - frag.y).abs() > 10.0 && last_y != f64::INFINITY {
                        page_text.push('\n');
                    }
                    page_text.push_str(&frag.text);
                    page_text.push(' ');
                    last_y = frag.y;
                }
            } else if let Some(t_ext) = text_extracted {
                page_text.push_str(&t_ext.text);
            } else if let Some(p) = document
                .extract_text()
                .ok()
                .and_then(|pages| pages.into_iter().nth(i as usize))
            {
                page_text.push_str(&p.text);
            }
            final_text.push_str(&page_text);
            final_text.push('\n');
        }

        let final_text = final_text.trim().to_string();
        if final_text.is_empty() {
            anyhow::bail!(
                "No text could be extracted: the PDF may be scanned images (try --features ocr) or encrypted"
            );
        }
        Ok(final_text)
    }
}

/// Formats a detected table as "Sequence of Records" (`Row 1: Header: val; …`),
/// assigning text to cells from fragment geometry ourselves.
///
/// We do our own assignment because oxidize-pdf's `preserve_layout` merges
/// fragments whose gap it underestimates (its glyph-width estimate runs short on
/// bold text), producing single fragments like "Net ProfitGrowth" that straddle
/// a column boundary — the next cell then reads as empty. Any fragment spanning
/// multiple columns is split at the boundary-proportional character position,
/// snapped to a space or case seam when one is nearby.
fn format_detected_table(table: &DetectedTable, fragments: &[TextFragment]) -> String {
    let rows = table.row_count();
    let cols = table.column_count();
    if rows == 0 || cols == 0 {
        return String::new();
    }

    // Column x-ranges from row 0, row y-ranges from column 0 (the detector
    // materialises a full grid, so these are always present in practice).
    let col_bounds: Vec<(f64, f64)> = (0..cols)
        .filter_map(|c| table.get_cell(0, c))
        .map(|cell| (cell.bbox.x, cell.bbox.x + cell.bbox.width))
        .collect();
    let row_bounds: Vec<(f64, f64)> = (0..rows)
        .filter_map(|r| table.get_cell(r, 0))
        .map(|cell| (cell.bbox.y, cell.bbox.y + cell.bbox.height))
        .collect();
    if col_bounds.len() != cols || row_bounds.len() != rows {
        return format_table_from_cells(table);
    }

    // grid[row][col] = x-ordered text pieces
    let mut grid: Vec<Vec<Vec<(f64, String)>>> = vec![vec![Vec::new(); cols]; rows];
    for frag in fragments {
        let center_y = frag.y + frag.height / 2.0;
        let Some(row) = row_bounds
            .iter()
            .position(|(lo, hi)| center_y >= *lo && center_y < *hi)
        else {
            continue;
        };
        for (col, x, piece) in split_fragment_by_columns(frag, &col_bounds) {
            let piece = piece.trim();
            if !piece.is_empty() {
                grid[row][col].push((x, piece.to_string()));
            }
        }
    }

    let cell_text = |pieces: &mut Vec<(f64, String)>| -> String {
        pieces.sort_by(|a, b| a.0.total_cmp(&b.0));
        pieces
            .iter()
            .map(|(_, t)| t.as_str())
            .collect::<Vec<_>>()
            .join(" ")
    };

    let headers: Vec<String> = (0..cols)
        .map(|c| {
            let h = cell_text(&mut grid[0][c]);
            if h.is_empty() {
                format!("Col {}", c)
            } else {
                h
            }
        })
        .collect();

    let mut out = String::new();
    for (row, row_cells) in grid.iter_mut().enumerate().skip(1) {
        let parts: Vec<String> = row_cells
            .iter_mut()
            .zip(&headers)
            .filter_map(|(pieces, header)| {
                let val = cell_text(pieces);
                (!val.is_empty()).then(|| format!("{}: {}", header, val))
            })
            .collect();
        if !parts.is_empty() {
            out.push_str(&format!("Row {}: {};\n", row, parts.join("; ")));
        }
    }
    out
}

/// Pieces of a fragment, one per column it overlaps: `(column, x, text)`.
/// Single-column fragments come back whole.
fn split_fragment_by_columns(
    frag: &TextFragment,
    col_bounds: &[(f64, f64)],
) -> Vec<(usize, f64, String)> {
    let start = frag.x;
    let end = frag.x + frag.width;
    let spanned: Vec<usize> = col_bounds
        .iter()
        .enumerate()
        .filter(|(_, (lo, hi))| start < *hi && end > *lo)
        .map(|(i, _)| i)
        .collect();

    match spanned.as_slice() {
        [] => {
            // Outside the grid horizontally; bucket by center as a last resort.
            let center = frag.x + frag.width / 2.0;
            col_bounds
                .iter()
                .position(|(lo, hi)| center >= *lo && center < *hi)
                .map(|c| vec![(c, frag.x, frag.text.clone())])
                .unwrap_or_default()
        }
        [only] => vec![(*only, frag.x, frag.text.clone())],
        spanned => {
            let chars: Vec<char> = frag.text.chars().collect();
            if chars.is_empty() || frag.width <= 0.0 {
                return vec![(spanned[0], frag.x, frag.text.clone())];
            }
            let mut cuts = vec![0usize];
            for pair in spanned.windows(2) {
                let boundary_x = col_bounds[pair[1]].0;
                let ideal =
                    ((boundary_x - start) / frag.width * chars.len() as f64).round() as usize;
                cuts.push(snap_cut(&chars, ideal.min(chars.len())));
            }
            cuts.push(chars.len());
            spanned
                .iter()
                .zip(cuts.windows(2))
                .filter(|(_, w)| w[1] > w[0])
                .map(|(col, w)| {
                    let piece: String = chars[w[0]..w[1]].iter().collect();
                    let x = start + (w[0] as f64 / chars.len() as f64) * frag.width;
                    (*col, x, piece)
                })
                .collect()
        }
    }
}

/// Picks the actual character to cut at: a space or lowercase→uppercase seam
/// within 3 chars of the proportional position wins, because glyph-width
/// estimates are too coarse to cut on blindly.
fn snap_cut(chars: &[char], ideal: usize) -> usize {
    let lo = ideal.saturating_sub(3);
    let hi = (ideal + 3).min(chars.len());
    let mut best: Option<(usize, usize)> = None; // (distance, index)
    for i in lo..=hi {
        if i == 0 || i >= chars.len() {
            continue;
        }
        let (prev, cur) = (chars[i - 1], chars[i]);
        let is_seam = prev == ' ' || cur == ' ' || (prev.is_lowercase() && cur.is_uppercase());
        if is_seam {
            let d = ideal.abs_diff(i);
            if best.is_none_or(|(bd, _)| d < bd) {
                best = Some((d, i));
            }
        }
    }
    best.map(|(_, i)| i).unwrap_or(ideal)
}

/// Fallback when the grid geometry is unavailable: trust the detector's own
/// per-cell text (no boundary splitting).
fn format_table_from_cells(table: &DetectedTable) -> String {
    let cols = table.column_count();
    let headers: Vec<String> = (0..cols)
        .map(|col| {
            table
                .get_cell(0, col)
                .map(|c| c.text.trim().to_string())
                .filter(|t| !t.is_empty())
                .unwrap_or_else(|| format!("Col {}", col))
        })
        .collect();

    let mut out = String::new();
    for row in 1..table.row_count() {
        let mut parts = Vec::new();
        for (col, header) in headers.iter().enumerate() {
            if let Some(cell) = table.get_cell(row, col) {
                let val = cell.text.trim();
                if !val.is_empty() {
                    parts.push(format!("{}: {}", header, val));
                }
            }
        }
        if !parts.is_empty() {
            out.push_str(&format!("Row {}: {};\n", row, parts.join("; ")));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[test]
    fn test_pdf_sequence_of_records() {
        let gobbler = PdfGobbler;
        let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
        let p = Path::new(&manifest_dir).join("../../test_assets/financial_report.pdf");

        let args = crate::cli::Cli::parse_from(["filegoblin", "dummy"]);
        let result = gobbler.gobble(&p, &(&args).into()).unwrap();

        println!("Extracted PDF:\n{}", result);

        // The source table's headers are Date/Revenue/Expenses/Net Profit/Growth.
        // "Net Profit" and "Growth" arrive merged from the extractor
        // ("Net ProfitGrowth"); the column-boundary splitter must pull them apart.
        assert!(result.contains(
            "Row 1: Date: 2026-01-01; Revenue: $10,000,000; Expenses: $8,000,000; Net Profit: $2,000,000; Growth: +5%;"
        ));
        assert!(result.contains(
            "Row 2: Date: 2026-02-01; Revenue: $12,500,000; Expenses: $9,000,000; Net Profit: $3,500,000; Growth: +20%;"
        ));
    }

    #[test]
    fn snap_cut_prefers_case_seam() {
        let chars: Vec<char> = "Net ProfitGrowth".chars().collect();
        // Proportional cut lands at 9; the t→G seam at 10 should win.
        assert_eq!(snap_cut(&chars, 9), 10);
    }

    #[test]
    fn snap_cut_prefers_space() {
        let chars: Vec<char> = "alpha beta".chars().collect();
        assert_eq!(snap_cut(&chars, 5), 5);
        assert_eq!(snap_cut(&chars, 7), 6);
    }
}
