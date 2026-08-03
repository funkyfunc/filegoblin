use crate::parsers::gobble::{Gobble, ParseOptions};
use anyhow::{Context, Result};
use pdf_oxide::PdfDocument;
use pdf_oxide::extractors::PageKind;
use std::path::Path;

/// PDF → markdown via `pdf_oxide`: native markdown conversion (headings, tables,
/// reading order, hyphenation) with per-page scanned/text classification. A page
/// classified as a scan is OCR'd from its largest embedded image — on macOS via
/// Apple Vision with no setup at all; elsewhere with `--features ocr`.
pub struct PdfGobbler;

impl Gobble for PdfGobbler {
    fn gobble(&self, path: &Path, _opts: &ParseOptions) -> Result<String> {
        if !path.exists() {
            anyhow::bail!("Mischievous I/O Error: file not found at {:?}", path);
        }

        let doc = PdfDocument::open(path)
            .context("Mischievous PDF Error: failed to open document (corrupt or encrypted?)")?;
        let page_count = doc
            .page_count()
            .context("Mischievous PDF Error: could not read the page tree")?;
        if page_count == 0 {
            anyhow::bail!("Mischievous PDF Error: the document has no pages");
        }

        let options = pdf_oxide::converters::ConversionOptions {
            detect_headings: true,
            extract_tables: true,
            include_images: false,
            ..Default::default()
        };

        let mut pages_md: Vec<String> = Vec::new();
        let mut unreadable_pages = 0usize;

        for page in 0..page_count {
            // Cheap preflight: signals only, no rasterization. On any
            // classification error, assume a text layer and let extraction try.
            let kind = doc
                .classify_page(page)
                .map(|c| c.kind)
                .unwrap_or(PageKind::TextLayer);

            let page_md = match kind {
                PageKind::Empty => String::new(),
                PageKind::Scanned => match ocr_scanned_page(&doc, page) {
                    Some(text) if !text.trim().is_empty() => text.trim().to_string(),
                    _ => {
                        unreadable_pages += 1;
                        scanned_page_marker(page + 1)
                    }
                },
                // TextLayer / ImageText / Mixed: the native converter. For
                // hybrids, also OCR the page's image so figure text isn't lost.
                _ => {
                    // An extraction *error* is not an empty page. Emitting
                    // `unwrap_or_default()` here silently dropped page N from
                    // an otherwise complete document — the reader has no way to
                    // tell a 9-page render of a 10-page file from the real
                    // thing. Mark it, exactly like the scanned branch does.
                    let mut md = match doc.to_markdown(page, &options) {
                        Ok(text) => text.trim().to_string(),
                        Err(e) => {
                            unreadable_pages += 1;
                            failed_page_marker(page + 1, &e.to_string())
                        }
                    };
                    if matches!(kind, PageKind::ImageText)
                        && let Some(ocr) = ocr_scanned_page(&doc, page)
                        && !ocr.trim().is_empty()
                        && !md
                            .to_lowercase()
                            .contains(crate::parsers::str_prefix(&normalize(&ocr), 40))
                    {
                        md.push_str("\n\n> **Text from embedded image:**\n> ");
                        md.push_str(&ocr.trim().replace('\n', "\n> "));
                    }
                    md
                }
            };

            if !page_md.trim().is_empty() {
                pages_md.push(page_md);
            }
        }

        let final_text = pages_md.join("\n\n").trim().to_string();
        if final_text.is_empty() {
            if unreadable_pages > 0 {
                anyhow::bail!(
                    "All {} page(s) are scanned images and OCR is unavailable in this build \
                     (macOS has Vision built in; elsewhere recompile with --features ocr)",
                    page_count
                );
            }
            anyhow::bail!("No text could be extracted from this PDF (corrupt or encrypted?)");
        }
        Ok(final_text)
    }
}

/// Lowercased, whitespace-collapsed prefix for cheap "already present" checks.
fn normalize(s: &str) -> String {
    s.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

/// A visible, honest marker instead of silently emitting half a document.
fn scanned_page_marker(display_page: usize) -> String {
    format!(
        "> [Page {}: scanned image with no text layer — OCR unavailable in this build]",
        display_page
    )
}

/// Same idea for a page the extractor refused outright.
fn failed_page_marker(display_page: usize, err: &str) -> String {
    format!(
        "> [Page {}: text extraction failed — {}]",
        display_page, err
    )
}

/// OCRs a scanned page by pulling its **largest embedded image** (the scan
/// itself) rather than rasterizing — faster and full fidelity. Returns `None`
/// when OCR isn't available in this build or the page has no usable image.
fn ocr_scanned_page(doc: &PdfDocument, page: usize) -> Option<String> {
    if !crate::parsers::ocr::ocr_available() {
        return None;
    }
    let images = doc.extract_images(page).ok()?;
    let largest = images
        .into_iter()
        .max_by_key(|img| u64::from(img.width()) * u64::from(img.height()))?;
    let dynamic = largest.to_dynamic_image().ok()?;

    let mut png: Vec<u8> = Vec::new();
    dynamic
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .ok()?;
    crate::parsers::ocr::ocr_image_bytes(&png).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[test]
    fn test_pdf_extraction_with_table() {
        let gobbler = PdfGobbler;
        let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
        let p = Path::new(&manifest_dir).join("../../test_assets/financial_report.pdf");

        let args = crate::cli::Cli::parse_from(["filegoblin", "dummy"]);
        let result = gobbler.gobble(&p, &(&args).into()).unwrap();

        println!("Extracted PDF:\n{}", result);

        // All five headers and the first data row must survive, un-merged.
        for needle in ["Date", "Revenue", "Expenses", "Net Profit", "Growth"] {
            assert!(result.contains(needle), "missing header {:?}", needle);
        }
        for needle in [
            "2026-01-01",
            "$10,000,000",
            "+5%",
            "2026-04-01",
            "$14,000,000",
        ] {
            assert!(result.contains(needle), "missing cell {:?}", needle);
        }
    }

    /// macOS-only: the fixture is an image-only PDF (the financial report
    /// rasterized), so extraction must route through Vision OCR automatically.
    /// Loose assertions — OCR output can vary slightly across OS versions.
    #[test]
    #[cfg(target_os = "macos")]
    fn scanned_pdf_falls_back_to_ocr() {
        let gobbler = PdfGobbler;
        let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
        let p = Path::new(&manifest_dir).join("../../test_assets/scanned_report.pdf");

        let args = crate::cli::Cli::parse_from(["filegoblin", "dummy"]);
        let result = gobbler.gobble(&p, &(&args).into()).unwrap();

        assert!(result.contains("Revenue"), "OCR should read the header row");
        assert!(result.contains("2026-01-01"), "OCR should read data cells");
    }

    #[test]
    fn missing_file_errors() {
        let gobbler = PdfGobbler;
        let args = crate::cli::Cli::parse_from(["filegoblin", "dummy"]);
        assert!(
            gobbler
                .gobble(Path::new("does_not_exist.pdf"), &(&args).into())
                .is_err()
        );
    }
}
