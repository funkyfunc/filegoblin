use crate::parsers::gobble::{Gobble, ParseOptions};
use anyhow::{Context, Result};
use pdf_oxide::PdfDocument;
use pdf_oxide::extractors::PageKind;
use std::collections::HashSet;
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
            include_artifacts: false,
            expand_ligatures: true,
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

        filter_running_headers_and_footers(&mut pages_md);
        pages_md.retain(|p| !p.trim().is_empty());

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

/// Strips running headers, page folios, and publisher/production slugs at page boundaries.
///
/// Deterministic two-pass filter:
/// 1. Collect candidate header lines (top 1-3 non-empty lines) and footer lines
///    (bottom 1-3 non-empty lines) across all pages.
/// 2. Count frequency of normalized header/footer candidates (stripping numbers/folios).
/// 3. Strip lines matching:
///    - Standalone folios/page numbers at page boundaries (e.g. `14`, `**15**`, `I 18 I`, `[17]`).
///    - Known publisher/production slugs (e.g. `TIS final pgs`, `CCC 0884-8173`, `INTERNATIONAL JOURNAL OF`, `Phil. Trans.`).
///    - Repeated running headers/footers occurring on multiple pages (>= 2 pages).
///    - Header lines with embedded folios where the text matches another page's header.
pub(crate) fn filter_running_headers_and_footers(pages: &mut [String]) {
    if pages.is_empty() {
        return;
    }
    if pages.len() == 1 {
        // Single-page document: only strip standalone folios and slugs at top/bottom
        if let Some(p) = pages.first_mut() {
            let mut lines: Vec<String> = p.lines().map(str::to_string).collect();
            let mut drop = HashSet::new();
            for (i, l) in lines.iter().enumerate().take(2) {
                if l.starts_with('#') || l.contains('|') {
                    continue;
                }
                if is_isolated_folio(l) || is_publisher_or_production_slug(l) {
                    drop.insert(i);
                }
            }
            let len = lines.len();
            if len > 2 {
                for (i, l) in lines.iter().enumerate().skip(len - 2) {
                    if l.starts_with('#') || l.contains('|') {
                        continue;
                    }
                    if is_isolated_folio(l) || is_publisher_or_production_slug(l) {
                        drop.insert(i);
                    }
                }
            }
            if !drop.is_empty() {
                lines = lines
                    .into_iter()
                    .enumerate()
                    .filter_map(|(i, l)| if drop.contains(&i) { None } else { Some(l) })
                    .collect();
                *p = lines.join("\n").trim().to_string();
            }
        }
        return;
    }

    struct PageLines {
        lines: Vec<String>,
        top_indices: Vec<usize>,
        bot_indices: Vec<usize>,
    }

    let mut parsed: Vec<PageLines> = Vec::with_capacity(pages.len());
    let mut top_candidates_per_page: Vec<Vec<(usize, String)>> = Vec::with_capacity(pages.len());
    let mut bot_candidates_per_page: Vec<Vec<(usize, String)>> = Vec::with_capacity(pages.len());

    for p in pages.iter() {
        let lines: Vec<String> = p.lines().map(str::to_string).collect();
        let mut top_indices = Vec::new();
        for (i, l) in lines.iter().enumerate() {
            if !l.trim().is_empty() {
                top_indices.push(i);
                if top_indices.len() >= 3 {
                    break;
                }
            }
        }

        let mut bot_indices = Vec::new();
        for (i, l) in lines.iter().enumerate().rev() {
            if !l.trim().is_empty() {
                bot_indices.push(i);
                if bot_indices.len() >= 3 {
                    break;
                }
            }
        }

        let mut page_top = Vec::new();
        for &i in &top_indices {
            let l = &lines[i];
            if !l.trim().starts_with('#') && !l.trim().contains('|') {
                let norm = normalize_header_candidate(l);
                if norm.len() >= 4 {
                    page_top.push((i, norm));
                }
            }
        }

        let mut page_bot = Vec::new();
        for &i in &bot_indices {
            let l = &lines[i];
            if !l.trim().starts_with('#') && !l.trim().contains('|') {
                let norm = normalize_header_candidate(l);
                if norm.len() >= 4 {
                    page_bot.push((i, norm));
                }
            }
        }

        parsed.push(PageLines {
            lines,
            top_indices,
            bot_indices,
        });
        top_candidates_per_page.push(page_top);
        bot_candidates_per_page.push(page_bot);
    }

    // Determine which top candidates repeat across distinct pages
    let num_pages = pages.len();
    let mut repeating_top: HashSet<(usize, usize)> = HashSet::new(); // (page_idx, line_idx)
    for p1 in 0..num_pages {
        for &(idx1, ref cand1) in &top_candidates_per_page[p1] {
            for (p2, cand2_list) in top_candidates_per_page.iter().enumerate().skip(p1 + 1) {
                for &(idx2, ref cand2) in cand2_list {
                    if headers_match(cand1, cand2) {
                        repeating_top.insert((p1, idx1));
                        repeating_top.insert((p2, idx2));
                    }
                }
            }
        }
    }

    let mut repeating_bot: HashSet<(usize, usize)> = HashSet::new(); // (page_idx, line_idx)
    for p1 in 0..num_pages {
        for &(idx1, ref cand1) in &bot_candidates_per_page[p1] {
            for (p2, cand2_list) in bot_candidates_per_page.iter().enumerate().skip(p1 + 1) {
                for &(idx2, ref cand2) in cand2_list {
                    if headers_match(cand1, cand2) {
                        repeating_bot.insert((p1, idx1));
                        repeating_bot.insert((p2, idx2));
                    }
                }
            }
        }
    }

    for (page_idx, p_info) in parsed.iter_mut().enumerate() {
        let mut drop_indices = HashSet::new();

        for &idx in &p_info.top_indices {
            let line = p_info.lines[idx].trim();
            if line.starts_with('#') || line.contains('|') {
                continue;
            }
            if is_isolated_folio(line) || is_publisher_or_production_slug(line) {
                drop_indices.insert(idx);
                continue;
            }
            if repeating_top.contains(&(page_idx, idx))
                && (page_idx > 0 || !is_document_title_block(&p_info.lines, idx))
            {
                drop_indices.insert(idx);
            }
        }

        for &idx in &p_info.bot_indices {
            let line = p_info.lines[idx].trim();
            if line.starts_with('#') || line.contains('|') {
                continue;
            }
            if is_isolated_folio(line) || is_publisher_or_production_slug(line) {
                drop_indices.insert(idx);
                continue;
            }
            if repeating_bot.contains(&(page_idx, idx)) {
                drop_indices.insert(idx);
            }
        }

        let filtered: Vec<String> = p_info
            .lines
            .iter()
            .enumerate()
            .filter_map(|(i, l)| {
                if drop_indices.contains(&i) {
                    None
                } else {
                    Some(l.clone())
                }
            })
            .collect();
        pages[page_idx] = filtered.join("\n").trim().to_string();
    }
}

/// Matches two candidate header strings with exact match, substring, or bounded Levenshtein distance.
fn headers_match(a: &str, b: &str) -> bool {
    if a == b {
        return true;
    }
    if a.len() >= 4 && b.len() >= 4 {
        if a.contains(b) || b.contains(a) {
            return true;
        }
        let max_len = a.chars().count().max(b.chars().count());
        if max_len >= 8 && levenshtein_dist(a, b) <= 2 {
            return true;
        }
    }
    false
}

/// Simple edit distance for OCR glyph variation tolerance.
fn levenshtein_dist(a: &str, b: &str) -> usize {
    let b_chars: Vec<char> = b.chars().collect();
    let mut prev: Vec<usize> = (0..=b_chars.len()).collect();
    let mut curr = vec![0; b_chars.len() + 1];
    for (i, ca) in a.chars().enumerate() {
        curr[0] = i + 1;
        for (j, &cb) in b_chars.iter().enumerate() {
            let cost = if ca == cb { 0 } else { 1 };
            curr[j + 1] = (prev[j + 1] + 1).min(curr[j] + 1).min(prev[j] + cost);
        }
        prev.copy_from_slice(&curr);
    }
    prev[b_chars.len()]
}

/// True if a line at a page boundary is an isolated page number/folio.
fn is_isolated_folio(s: &str) -> bool {
    if s.contains('|') || s.starts_with('#') {
        return false;
    }
    let clean: String = s
        .chars()
        .filter(|c| !matches!(c, '*' | '_' | '[' | ']' | '(' | ')' | '{' | '}' | '`'))
        .collect();
    let trimmed = clean.trim();
    if trimmed.is_empty() {
        return false;
    }
    // Pure ASCII digits: "14", "72"
    if trimmed.chars().all(|c| c.is_ascii_digit()) {
        return true;
    }
    // OCR folios with glyph confusions: 'l' -> '1', 'I' -> '1', 'O' -> '0'
    let folded = trimmed.replace(['l', 'I'], "1").replace('O', "0");
    let parts: Vec<&str> = folded.split_whitespace().collect();
    let has_digit = parts.iter().any(|p| p.chars().any(|c| c.is_ascii_digit()));
    if has_digit
        && parts.iter().all(|p| {
            p.chars()
                .all(|c| c.is_ascii_digit() || c == '-' || c == '–' || c == '—')
        })
    {
        return true;
    }
    // Roman numerals (up to 8 chars) e.g. "xv", "xvi", "iv", "ix"
    let lower = trimmed.to_lowercase();
    if !lower.is_empty()
        && lower.len() <= 8
        && lower
            .chars()
            .all(|c| matches!(c, 'i' | 'v' | 'x' | 'l' | 'c' | 'd' | 'm'))
    {
        return true;
    }
    // "Page 14" / "Page 14 of 20" / "pg 14"
    if lower.starts_with("page ") || lower.starts_with("pg ") || lower.starts_with("pg. ") {
        return true;
    }
    false
}

/// True if a line matches known publisher/production slugs or metadata markings at page boundaries.
fn is_publisher_or_production_slug(s: &str) -> bool {
    let lower = s.to_lowercase();
    // Book production marks e.g. "TIS final pgs 14 5/2/09 10:37:35"
    if lower.contains("final pgs") {
        return true;
    }
    // Journal publication slugs
    if lower.contains("international journal of") || lower.contains("phil. trans. r. soc") {
        return true;
    }
    // Copyright clearance codes e.g. "CCC 0884-8173"
    if lower.contains("ccc 0884-") || (lower.contains("ccc ") && s.contains('-')) {
        return true;
    }
    // Corresponding author footnote at page bottom
    if lower.contains("corresponding author") || lower.contains("reprint requests") {
        return true;
    }
    // JSTOR header / footer
    if lower.contains("stable url:")
        || (lower.contains("jstor") && lower.contains("terms and conditions"))
    {
        return true;
    }
    // Volume / issue line at bottom/top e.g. "Vol. 327. B" or "Printed in Great Britain"
    let clean = s
        .trim()
        .trim_matches(|c| matches!(c, '*' | '_' | '#' | '`'))
        .trim();
    if clean.starts_with("Vol.")
        || clean.starts_with("V0l.")
        || clean.starts_with("Printed in Great Britain")
    {
        return true;
    }
    false
}

/// Normalizes a candidate header line for cross-page recurrence matching.
/// Strips folios, numbers, punctuation, markdown markers, and folds OCR glyphs.
fn normalize_header_candidate(s: &str) -> String {
    let clean = s
        .trim()
        .trim_matches(|c| matches!(c, '*' | '_' | '#' | '`' | '|'))
        .trim();
    // Fold OCR glyph confusions (l <-> i, 0 <-> o)
    let folded = clean.to_lowercase().replace('l', "i").replace('0', "o");
    // Strip leading/trailing digits, folios, dashes, dots, colons
    let chars: Vec<char> = folded.chars().collect();
    let start = chars
        .iter()
        .position(|c| {
            !c.is_ascii_digit() && !c.is_whitespace() && *c != '-' && *c != '.' && *c != ':'
        })
        .unwrap_or(0);
    let end = chars
        .iter()
        .rposition(|c| {
            !c.is_ascii_digit() && !c.is_whitespace() && *c != '-' && *c != '.' && *c != ':'
        })
        .map(|p| p + 1)
        .unwrap_or(chars.len());
    if start >= end {
        return String::new();
    }
    chars[start..end]
        .iter()
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Returns true if a line on page 0 is part of the document title / author header block.
fn is_document_title_block(lines: &[String], idx: usize) -> bool {
    if idx + 1 < lines.len() {
        let next = lines[idx + 1].trim();
        if next.contains("Author")
            || next.contains("University")
            || next.contains("Department")
            || next.contains("Institute")
            || next.contains("By ")
            || next.contains("BY ")
            || next.starts_with("During ")
        {
            return true;
        }
    }
    false
}

/// OCRs a scanned page from its **embedded images** rather than rasterizing —
/// faster and full fidelity. Returns `None` when OCR isn't available in this
/// build or the page has no usable image.
///
/// Reads every substantial image on the page, not just the largest one: a book
/// spread scanned as two halves, or a page with a scanned figure beside the
/// body, would otherwise lose everything except the biggest piece — silently,
/// since the page still produced text.
fn ocr_scanned_page(doc: &PdfDocument, page: usize) -> Option<String> {
    if !crate::parsers::ocr::ocr_available() {
        return None;
    }
    let mut images = doc.extract_images(page).ok()?;
    if images.is_empty() {
        return None;
    }
    // Largest first, so the main scan leads and the reading order stays sane.
    images.sort_by_key(|img| std::cmp::Reverse(u64::from(img.width()) * u64::from(img.height())));

    // Anything much smaller than the largest image is a logo, icon, or rule —
    // OCRing it costs time and contributes noise. A half of a spread is
    // comfortably above this bar.
    let biggest = u64::from(images[0].width()) * u64::from(images[0].height());
    let floor = biggest / 8;

    let mut parts: Vec<String> = Vec::new();
    for img in images.iter().take(MAX_OCR_IMAGES_PER_PAGE) {
        if u64::from(img.width()) * u64::from(img.height()) < floor {
            break; // sorted, so everything after this is smaller too
        }
        let Ok(dynamic) = img.to_dynamic_image() else {
            continue;
        };
        let mut png: Vec<u8> = Vec::new();
        if dynamic
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .is_err()
        {
            continue;
        }
        if let Ok(text) = crate::parsers::ocr::ocr_image_bytes(&png)
            && !text.trim().is_empty()
        {
            parts.push(text.trim().to_string());
        }
    }

    (!parts.is_empty()).then(|| parts.join("\n\n"))
}

/// Bound on how many images one page will OCR. A pathological PDF can embed
/// hundreds of similarly-sized tiles; OCR is the slowest thing this tool does.
const MAX_OCR_IMAGES_PER_PAGE: usize = 8;

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

    #[test]
    fn test_filter_running_headers_and_footers_alternating() {
        let mut pages = vec![
            "**14**\n**PART ONE: SYSTEM STRUCTURE AND BEHAVIOR**\nBody text on page one.\nTIS final pgs 14 5/2/09 10:37:35".to_string(),
            "**CHAPTER ONE: THE BASICS**\n**15**\nBody text on page two.\nTIS final pgs 15 5/2/09 10:37:35".to_string(),
            "**16**\n**PART ONE: SYSTEM STRUCTURE AND BEHAVIOR**\nBody text on page three.\nTIS final pgs 16 5/2/09 10:37:35".to_string(),
            "**CHAPTER ONE: THE BASICS**\n**17**\nBody text on page four.\nTIS final pgs 17 5/2/09 10:37:35".to_string(),
        ];

        filter_running_headers_and_footers(&mut pages);

        assert_eq!(pages[0], "Body text on page one.");
        assert_eq!(pages[1], "Body text on page two.");
        assert_eq!(pages[2], "Body text on page three.");
        assert_eq!(pages[3], "Body text on page four.");
    }

    #[test]
    fn test_filter_preserves_document_title_on_first_page() {
        let mut pages = vec![
            "Structuring Interviews with Experts\nDuring Knowledge Elicitation\nBy Larry E. Wood\nAbstract text here.".to_string(),
            "72 WOOD AND FORD\nSecond page content.".to_string(),
            "STRUCTURING INTERVIEWS WITH EXPERTS 73\nThird page content.".to_string(),
            "74 WOOD AND FORD\nFourth page content.".to_string(),
        ];

        filter_running_headers_and_footers(&mut pages);

        assert!(pages[0].contains("Structuring Interviews with Experts"));
        assert!(pages[0].contains("During Knowledge Elicitation"));
        assert_eq!(pages[1], "Second page content.");
        assert_eq!(pages[2], "Third page content.");
        assert_eq!(pages[3], "Fourth page content.");
    }

    #[test]
    fn test_isolated_folio_detection() {
        assert!(is_isolated_folio("14"));
        assert!(is_isolated_folio("**14**"));
        assert!(is_isolated_folio("I **18** I"));
        assert!(is_isolated_folio("**l7** 1"));
        assert!(is_isolated_folio("31-2"));
        assert!(is_isolated_folio("Page 5 of 20"));
        assert!(is_isolated_folio("xv"));

        // Normal text should not be detected as folios
        assert!(!is_isolated_folio("Introduction"));
        assert!(!is_isolated_folio("| Date | Revenue |"));
        assert!(!is_isolated_folio("# 14 Principles"));
    }
}
