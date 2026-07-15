use crate::parsers::gobble::Gobble;
use anyhow::{Context, Result};
use std::path::Path;

pub struct PdfGobbler;

impl Gobble for PdfGobbler {
    fn gobble(&self, path: &Path, _flags: &crate::cli::Cli) -> Result<String> {
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
        let page_count = document.page_count().unwrap_or(0);

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
                        let mut table_string = String::new();
                        let mut headers = Vec::new();
                        for col in 0..table.column_count() {
                            if let Some(cell) = table.get_cell(0, col) {
                                let h = cell.text.trim();
                                headers.push(if h.is_empty() {
                                    format!("Col {}", col)
                                } else {
                                    h.to_string()
                                });
                            } else {
                                headers.push(format!("Col {}", col));
                            }
                        }

                        for row in 1..table.row_count() {
                            table_string.push_str(&format!("Row {}: ", row));
                            for col in 0..table.column_count() {
                                if let Some(cell) = table.get_cell(row, col) {
                                    let default_header = format!("Col {}", col);
                                    let header = headers.get(col).unwrap_or(&default_header);
                                    let val = cell.text.trim();
                                    table_string.push_str(&format!("{}: {}; ", header, val));
                                }
                            }
                            table_string.push('\n');
                        }

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

        Ok(final_text.trim().to_string())
    }
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
        let result = gobbler.gobble(&p, &args).unwrap();

        println!("Extracted PDF:\n{}", result);

        assert!(result.contains("Row 1: Date: 2026-01-01; Revenue: $10,000,000; Expenses: $8,000,000; Net ProfitGrowth: $2,000,000; Col 4: +5%;"));
        assert!(result.contains("Row 2: Date: 2026-02-01; Revenue: $12,500,000; Expenses: $9,000,000; Net ProfitGrowth: $3,500,000; Col 4: +20%;"));
    }
}
