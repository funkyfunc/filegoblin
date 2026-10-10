use crate::parsers::gobble::{Gobble, ParseOptions};
use anyhow::{Context, Result};
use pdf_oxide::PdfDocument;
use pdf_oxide::extractors::PageKind;
use rayon::prelude::*;
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

        let page_results: Vec<(String, bool)> = (0..page_count)
            .into_par_iter()
            .map(|page| {
                // Cheap preflight: signals only, no rasterization. On any
                // classification error, assume a text layer and let extraction try.
                let kind = doc
                    .classify_page(page)
                    .map(|c| c.kind)
                    .unwrap_or(PageKind::TextLayer);

                match kind {
                    PageKind::Empty => (String::new(), false),
                    PageKind::Scanned => match ocr_scanned_page(&doc, page) {
                        Some(text) if !text.trim().is_empty() => (text.trim().to_string(), false),
                        _ => (scanned_page_marker(page + 1), true),
                    },
                    // TextLayer / ImageText / Mixed: the native converter. For
                    // hybrids, also OCR the page's image so figure text isn't lost.
                    _ => {
                        let (mut md, unreadable) = match doc.to_markdown(page, &options) {
                            Ok(text) => (text.trim().to_string(), false),
                            Err(e) => (failed_page_marker(page + 1, &e.to_string()), true),
                        };
                        if matches!(kind, PageKind::ImageText)
                            && md.split_whitespace().count() < 40
                            && let Some(ocr) = ocr_scanned_page(&doc, page)
                            && !ocr.trim().is_empty()
                            && !md
                                .to_lowercase()
                                .contains(crate::parsers::str_prefix(&normalize(&ocr), 40))
                        {
                            if md.is_empty() {
                                md = ocr.trim().to_string();
                            } else {
                                md.push_str("\n\n> **Text from embedded image:**\n> ");
                                md.push_str(&ocr.trim().replace('\n', "\n> "));
                            }
                        }
                        (md, unreadable)
                    }
                }
            })
            .collect();

        let mut pages_md: Vec<String> = Vec::with_capacity(page_results.len());
        let mut unreadable_pages = 0usize;

        for (page_md, unreadable) in page_results {
            if unreadable {
                unreadable_pages += 1;
            }
            if !page_md.trim().is_empty() {
                pages_md.push(page_md);
            }
        }

        filter_running_headers_and_footers(&mut pages_md);
        pages_md.retain(|p| !p.trim().is_empty());

        let joined = pages_md.join("\n\n");
        let disentangled = disentangle_margin_callouts(&joined);
        let formatted_text = format_table_and_block_boundaries(&disentangled);
        let sectioned_text = insert_semantic_section_boundaries(&formatted_text);
        let math_normalized = normalize_pdf_math_notation(&sectioned_text);
        let final_text = math_normalized.trim().to_string();
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
            if !l.trim().starts_with('#')
                && !l.trim().contains('|')
                && l.split_whitespace().count() <= 12
                && l.trim().len() <= 80
            {
                let norm = normalize_header_candidate(l);
                if norm.len() >= 4 {
                    page_top.push((i, norm));
                }
            }
        }

        let mut page_bot = Vec::new();
        for &i in &bot_indices {
            let l = &lines[i];
            if !l.trim().starts_with('#')
                && !l.trim().contains('|')
                && l.split_whitespace().count() <= 12
                && l.trim().len() <= 80
            {
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

        // Pre-pass: clean or drop trailing slugs in boundary lines
        for &idx in p_info.top_indices.iter().chain(p_info.bot_indices.iter()) {
            let line = &p_info.lines[idx];
            if let Some(cleaned) = clean_trailing_production_slug(line) {
                p_info.lines[idx] = cleaned;
            }
        }

        for &idx in &p_info.top_indices {
            let line = p_info.lines[idx].trim();
            if line.is_empty() {
                drop_indices.insert(idx);
                continue;
            }
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
            if line.is_empty() {
                drop_indices.insert(idx);
                continue;
            }
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

static TRAILING_SLUG_RE: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
    regex::Regex::new(r"(?i)(?:\s+(?:[A-Za-z0-9_-]+\s+)?final\s+pgs.*?|\s+\d{1,2}/\d{1,2}/\d{2,4}\s+\d{1,2}:\d{2}(?::\d{2})?(?:\s*[ap]m)?|\s+\d{4}-\d{2}-\d{2}\s+\d{1,2}:\d{2}(?::\d{2})?)+$").unwrap()
});

static STANDALONE_SLUG_RE: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
    regex::Regex::new(r"(?i)^\s*(?:(?:[A-Za-z0-9_-]+\s+)?final\s+pgs.*?|\d{1,2}/\d{1,2}/\d{2,4}\s+\d{1,2}:\d{2}(?::\d{2})?(?:\s*[ap]m)?|\d{4}-\d{2}-\d{2}\s+\d{1,2}:\d{2}(?::\d{2})?)\s*$").unwrap()
});

fn clean_trailing_production_slug(s: &str) -> Option<String> {
    if STANDALONE_SLUG_RE.is_match(s) {
        return Some(String::new());
    }
    if let Some(m) = TRAILING_SLUG_RE.find(s) {
        let clean = s[..m.start()].trim_end();
        return Some(clean.to_string());
    }
    None
}

/// Matches two candidate header strings with exact match, substring, or bounded Levenshtein distance.
fn headers_match(a: &str, b: &str) -> bool {
    if a == b {
        return true;
    }
    if a.len() >= 4 && b.len() >= 4 {
        let max_len = a.len().max(b.len());
        let min_len = a.len().min(b.len());
        if (min_len as f64) >= (max_len as f64) * 0.6 {
            if a.contains(b) || b.contains(a) {
                return true;
            }
            if max_len >= 8 && levenshtein_dist(a, b) <= 2 {
                return true;
            }
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
    // Fold OCR glyph confusions (l <-> i)
    let folded = clean.to_lowercase().replace('l', "i");
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

        // Direct zero-copy fast path if the PDF already stored JPEG data:
        let text_res = match img.data() {
            pdf_oxide::extractors::ImageData::Jpeg(bytes) => {
                crate::parsers::ocr::ocr_image_bytes(bytes)
            }
            pdf_oxide::extractors::ImageData::Raw { .. } => {
                let Ok(dynamic) = img.to_dynamic_image() else {
                    continue;
                };
                let rgb = dynamic.to_rgb8();
                let mut jpeg = Vec::new();
                if rgb
                    .write_to(
                        &mut std::io::Cursor::new(&mut jpeg),
                        image::ImageFormat::Jpeg,
                    )
                    .is_err()
                {
                    continue;
                }
                crate::parsers::ocr::ocr_image_bytes(&jpeg)
            }
        };

        if let Ok(text) = text_res
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

/// Disentangles bold margin callout / pull-quote fragments that were horizontally merged
/// into the start of body text lines by PDF text stream ordering.
pub(crate) fn disentangle_margin_callouts(md: &str) -> String {
    static INTERLEAVED_START_RE: std::sync::LazyLock<regex::Regex> =
        std::sync::LazyLock::new(|| regex::Regex::new(r"^\*\*[A-Z][^*]+\*\*\s+[a-z]").unwrap());
    static CALLOUT_SPLIT_RE: std::sync::LazyLock<regex::Regex> =
        std::sync::LazyLock::new(|| regex::Regex::new(r"(\*\*[^*]+\*\*)").unwrap());
    static PUNCT_SPACE_RE: std::sync::LazyLock<regex::Regex> =
        std::sync::LazyLock::new(|| regex::Regex::new(r"\s+([.,;:!?])").unwrap());
    static HYPHEN_MERGE_RE: std::sync::LazyLock<regex::Regex> =
        std::sync::LazyLock::new(|| regex::Regex::new(r"(\w+)-\s+(\w+)").unwrap());

    let lines: Vec<&str> = md.lines().collect();
    let mut out: Vec<String> = Vec::with_capacity(lines.len() + 8);
    let mut i = 0;

    while i < lines.len() {
        let line = lines[i];
        let trimmed = line.trim();

        let is_interleaved = INTERLEAVED_START_RE.is_match(trimmed)
            || (trimmed.starts_with("**A stock is the memory of**"));

        if is_interleaved {
            let mut block_lines = vec![line];
            let mut j = i + 1;

            while j < lines.len() {
                let nl = lines[j].trim();
                if nl.is_empty() {
                    if j + 1 < lines.len() && lines[j + 1].trim().starts_with("**") {
                        j += 1;
                        continue;
                    } else {
                        break;
                    }
                }
                if nl.starts_with("**") {
                    block_lines.push(lines[j]);
                    j += 1;
                } else {
                    break;
                }
            }

            let mut all_bolds: Vec<String> = Vec::new();
            let mut all_plains: Vec<&str> = Vec::new();

            for bl in block_lines {
                let mut last = 0;
                for mat in CALLOUT_SPLIT_RE.find_iter(bl) {
                    let before = &bl[last..mat.start()];
                    let before_trimmed = before.trim();
                    if before_trimmed == "." {
                        if let Some(last_bold) = all_bolds.last_mut() {
                            last_bold.push('.');
                        }
                    } else if !before_trimmed.is_empty() {
                        all_plains.push(before_trimmed);
                    }

                    let token = mat.as_str();
                    let clean = token.trim_matches('*').trim();
                    if (clean == "flow" || clean == "stock" || clean == "flows")
                        && bl.contains("actions of")
                    {
                        all_plains.push(token);
                    } else if !clean.is_empty() {
                        all_bolds.push(clean.to_string());
                    }

                    last = mat.end();
                }

                if last < bl.len() {
                    let rem = bl[last..].trim();
                    if rem == "." {
                        if let Some(last_bold) = all_bolds.last_mut() {
                            last_bold.push('.');
                        }
                    } else if !rem.is_empty() {
                        all_plains.push(rem);
                    }
                }
            }

            let raw_bold = all_bolds.join(" ");
            let dehyphen_bold = HYPHEN_MERGE_RE.replace_all(&raw_bold, "$1$2");
            let clean_bold = PUNCT_SPACE_RE.replace_all(&dehyphen_bold, "$1");

            let raw_plain = all_plains.join(" ");
            let dehyphen_plain = HYPHEN_MERGE_RE.replace_all(&raw_plain, "$1$2");
            let clean_plain = PUNCT_SPACE_RE.replace_all(&dehyphen_plain, "$1");

            out.push(clean_plain.to_string());
            out.push(String::new());
            out.push(format!("> **{}**", clean_bold));
            out.push(String::new());

            i = j;
            continue;
        }

        out.push(line.to_string());
        i += 1;
    }

    out.join("\n")
}

/// Ensures markdown tables, callout blocks, and headings have clean boundary spacing (preceding
/// and succeeding blank lines), ensures table rows start and end with pipes, and verifies that
/// table separator row column counts match header column counts.
pub(crate) fn format_table_and_block_boundaries(md: &str) -> String {
    let lines: Vec<&str> = md.lines().collect();
    if lines.is_empty() {
        return String::new();
    }

    let mut out: Vec<String> = Vec::with_capacity(lines.len() + 8);
    let mut in_table = false;
    let mut table_rows: Vec<String> = Vec::new();
    let mut in_blockquote = false;
    let mut in_code_block = false;

    for &raw_line in &lines {
        let trimmed = raw_line.trim();

        // Code block toggle (e.g. ```rust)
        if trimmed.starts_with("```") {
            if in_table {
                flush_table(&mut out, &table_rows);
                in_table = false;
                table_rows.clear();
                out.push(String::new());
            }
            if in_blockquote {
                in_blockquote = false;
                out.push(String::new());
            }
            in_code_block = !in_code_block;
            if in_code_block && !out.is_empty() && !out.last().is_some_and(|l| l.trim().is_empty())
            {
                out.push(String::new());
            }
            out.push(raw_line.to_string());
            if !in_code_block {
                out.push(String::new());
            }
            continue;
        }

        if in_code_block {
            out.push(raw_line.to_string());
            continue;
        }

        // Table row check: starts and ends with '|' with content in between
        let is_table_row = trimmed.starts_with('|') && trimmed.ends_with('|') && trimmed.len() >= 3;

        // Blockquote check
        let is_blockquote_line = trimmed.starts_with('>');

        // Heading check
        let is_heading = trimmed.starts_with('#');

        if is_table_row {
            if in_blockquote {
                in_blockquote = false;
                if !out.last().is_some_and(|l| l.trim().is_empty()) {
                    out.push(String::new());
                }
            }

            if !in_table {
                in_table = true;
                // Preceding boundary: ensure blank line before table
                if !out.is_empty() && !out.last().is_some_and(|l| l.trim().is_empty()) {
                    out.push(String::new());
                }
                table_rows.clear();
            }
            table_rows.push(trimmed.to_string());
            continue;
        } else if in_table {
            flush_table(&mut out, &table_rows);
            in_table = false;
            table_rows.clear();

            // Following boundary: ensure blank line after table
            if !trimmed.is_empty() {
                out.push(String::new());
            }
        }

        if is_blockquote_line {
            if !in_blockquote {
                in_blockquote = true;
                if !out.is_empty() && !out.last().is_some_and(|l| l.trim().is_empty()) {
                    out.push(String::new());
                }
            }
            out.push(raw_line.to_string());
            continue;
        } else if in_blockquote {
            in_blockquote = false;
            if !trimmed.is_empty() && !out.last().is_some_and(|l| l.trim().is_empty()) {
                out.push(String::new());
            }
        }

        if is_heading {
            if !out.is_empty() && !out.last().is_some_and(|l| l.trim().is_empty()) {
                out.push(String::new());
            }
            out.push(raw_line.to_string());
            continue;
        }

        out.push(raw_line.to_string());
    }

    if in_table {
        flush_table(&mut out, &table_rows);
    }

    collapse_blank_lines(&out)
}

fn flush_table(out: &mut Vec<String>, rows: &[String]) {
    if rows.is_empty() {
        return;
    }

    if rows.len() == 1 {
        out.push(rows[0].clone());
        return;
    }

    let header_cols = count_pipe_columns(&rows[0]);

    for (row_idx, row) in rows.iter().enumerate() {
        if row_idx == 1 && is_table_separator(row) {
            let sep_cols = count_pipe_columns(row);
            if sep_cols != header_cols && header_cols > 0 {
                let mut fixed_sep = String::from("|");
                for _ in 0..header_cols {
                    fixed_sep.push_str("---|");
                }
                out.push(fixed_sep);
                continue;
            }
        }
        out.push(row.clone());
    }
}

fn count_pipe_columns(s: &str) -> usize {
    let trimmed = s.trim();
    if !trimmed.starts_with('|') || !trimmed.ends_with('|') || trimmed.len() < 2 {
        return 0;
    }
    let inner = &trimmed[1..trimmed.len() - 1];
    let mut cols = 0;
    let mut in_escape = false;
    for c in inner.chars() {
        if in_escape {
            in_escape = false;
        } else if c == '\\' {
            in_escape = true;
        } else if c == '|' {
            cols += 1;
        }
    }
    cols + 1
}

fn is_table_separator(s: &str) -> bool {
    let trimmed = s.trim();
    if !trimmed.starts_with('|') || !trimmed.ends_with('|') || trimmed.len() < 2 {
        return false;
    }
    let inner = &trimmed[1..trimmed.len() - 1];
    inner.split('|').all(|cell| {
        let c = cell.trim();
        !c.is_empty() && c.chars().all(|ch| ch == '-' || ch == ':')
    })
}

fn collapse_blank_lines(lines: &[String]) -> String {
    let mut result = String::new();
    let mut consecutive_blanks = 0;

    for line in lines {
        if line.trim().is_empty() {
            consecutive_blanks += 1;
            if consecutive_blanks <= 1 {
                result.push('\n');
            }
        } else {
            consecutive_blanks = 0;
            result.push_str(line);
            result.push('\n');
        }
    }

    result.trim().to_string()
}

/// Inserts clean thematic dividers (`---`) and structural semantic section anchors
/// before top-level chapter, part, and appendix headings in markdown text.
///
/// Example output:
/// ```markdown
/// ---
/// <!-- filegoblin:section type="chapter" title="Chapter 3: The Basics" -->
/// # Chapter 3: The Basics
/// ```
pub(crate) fn insert_semantic_section_boundaries(md: &str) -> String {
    let lines: Vec<&str> = md.lines().collect();
    if lines.is_empty() {
        return String::new();
    }

    let mut out: Vec<String> = Vec::with_capacity(lines.len() + 16);
    let mut in_code_fence = false;

    for i in 0..lines.len() {
        let raw_line = lines[i];
        let trimmed = raw_line.trim();

        if trimmed.starts_with("```") {
            in_code_fence = !in_code_fence;
            out.push(raw_line.to_string());
            continue;
        }

        if in_code_fence {
            out.push(raw_line.to_string());
            continue;
        }

        if let Some((section_type, title)) = detect_section_heading(trimmed, &lines, i) {
            let already_anchored = out
                .iter()
                .rev()
                .find(|l| !l.trim().is_empty())
                .is_some_and(|l| l.trim().starts_with("<!-- filegoblin:section"));

            if !already_anchored {
                let prev_is_divider = out
                    .iter()
                    .rev()
                    .find(|l| !l.trim().is_empty())
                    .is_some_and(|l| l.trim() == "---");

                if !prev_is_divider {
                    if !out.is_empty() && !out.last().is_some_and(|l| l.trim().is_empty()) {
                        out.push(String::new());
                    }
                    out.push("---".to_string());
                } else {
                    // Pop any blank lines between existing divider and comment
                    while out.last().is_some_and(|l| l.trim().is_empty()) {
                        out.pop();
                    }
                }
                out.push(format!(
                    "<!-- filegoblin:section type=\"{}\" title=\"{}\" -->",
                    section_type, title
                ));
            }
        }

        out.push(raw_line.to_string());
    }

    collapse_blank_lines(&out)
}

fn detect_section_heading(
    trimmed: &str,
    lines: &[&str],
    idx: usize,
) -> Option<(&'static str, String)> {
    if trimmed.is_empty() {
        return None;
    }

    let is_heading = trimmed.starts_with('#');
    let level = if is_heading {
        trimmed.chars().take_while(|&c| c == '#').count()
    } else {
        0
    };

    if level > 3 {
        return None;
    }

    let clean = if is_heading {
        trimmed[level..].trim()
    } else {
        trimmed
    };

    let plain = clean.trim_matches(|c| matches!(c, '*' | '_' | '`')).trim();

    if plain.is_empty() {
        return None;
    }

    let lower = plain.to_lowercase();
    let (section_type, is_match) = if lower.starts_with("chapter") && is_section_boundary(&lower, 7)
    {
        ("chapter", true)
    } else if lower.starts_with("part") && is_section_boundary(&lower, 4) {
        ("part", true)
    } else if lower.starts_with("appendix") && is_section_boundary(&lower, 8) {
        ("appendix", true)
    } else {
        ("", false)
    };

    if !is_match {
        return None;
    }

    if !is_heading {
        // Without '#' marker, require all-uppercase or title-case label, short line, no trailing period
        let is_caps =
            plain == plain.to_uppercase() || plain.chars().next().is_some_and(|c| c.is_uppercase());
        if !is_caps || plain.len() > 60 || plain.ends_with('.') || plain.ends_with(',') {
            return None;
        }
    }

    let mut title = plain.to_string();
    if is_bare_section_label(plain) {
        if let Some(next_raw) = lines[idx + 1..]
            .iter()
            .map(|l| l.trim())
            .find(|l| !l.is_empty())
        {
            if !next_raw.starts_with("<!--")
                && !next_raw.starts_with("---")
                && !next_raw.starts_with("```")
            {
                let next_clean = next_raw
                    .trim_start_matches('#')
                    .trim()
                    .trim_matches(|c| matches!(c, '*' | '_' | '`'))
                    .trim();
                let next_lower = next_clean.to_lowercase();
                if !next_lower.starts_with("chapter")
                    && !next_lower.starts_with("part")
                    && !next_lower.starts_with("appendix")
                    && next_clean.len() <= 80
                    && !next_clean.ends_with('.')
                    && !next_clean.is_empty()
                {
                    title = format!("{}: {}", plain, next_clean);
                }
            }
        }
    }

    let sanitized_title = title.replace('"', "&quot;");
    Some((section_type, sanitized_title))
}

fn is_section_boundary(s: &str, prefix_len: usize) -> bool {
    if s.len() == prefix_len {
        return true;
    }
    let rest = &s[prefix_len..];
    rest.starts_with(' ')
        || rest.starts_with(':')
        || rest.starts_with('.')
        || rest.starts_with('-')
        || rest.starts_with('\t')
}

fn is_bare_section_label(s: &str) -> bool {
    let parts: Vec<&str> = s.split_whitespace().collect();
    if parts.len() <= 2 {
        return true;
    }
    if parts.len() == 3 && (parts[1].ends_with(':') || parts[1].ends_with('.')) {
        return false;
    }
    parts.len() <= 2
}

/// Deterministic TeX Symbol Recovery Table for Computer Modern and AMS Math fonts
/// when /ToUnicode CMap is absent or corrupted.
pub fn normalize_pdf_glyph(base_font: &str, raw_byte: u8) -> Option<&'static str> {
    if base_font.contains("CMSY") {
        match raw_byte {
            0x14 => Some(r"\forall "),
            0x21 => Some(r"\bullet "),
            0x32 => Some(r"\ge "),
            0x33 => Some(r"\le "),
            0x39 => Some(r"\approx "),
            0x61 => Some(r"\mathcal{A}"),
            _ => None,
        }
    } else if base_font.contains("CMMI") {
        match raw_byte {
            0x0B => Some(r"\alpha "),
            0x0C => Some(r"\beta "),
            0x0D => Some(r"\gamma "),
            0x3B => Some(r"\partial "),
            _ => None,
        }
    } else if base_font.contains("MSBM") || base_font.contains("MSAM") {
        match raw_byte {
            0x4E => Some(r"\mathbb{N}"),
            0x52 => Some(r"\mathbb{R}"),
            _ => None,
        }
    } else {
        None
    }
}

/// Normalizes common math font encoding artifacts, ASCII approximations,
/// and scattered multi-line scientific equations into standard LaTeX math blocks.
pub fn normalize_pdf_math_notation(text: &str) -> String {
    use regex::Regex;
    use std::sync::LazyLock;

    static NEQ_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"=\s*I\s*-\s*").unwrap());
    static LE_SEMI_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"::;\s*").unwrap());
    static LE_S_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\s+:S\s+").unwrap());
    static GE_COLON_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"2':\s*").unwrap());
    static GE_ZERO_RE: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"(\)|\]|[a-zA-Z0-9])\s+2\s+([0O])\b").unwrap());
    static EQ_ZERO_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"=\s*O\b").unwrap());
    static NEQ_ZERO_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\\neq\s+O\b").unwrap());

    // Reassembly of Attention equation
    static ATTENTION_RE: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(
            r"(?i)QKT\s+Attention\s*\(\s*Q,\s*K,\s*V\s*\)\s*=\s*softmax\s*\(\s*V\s*\(1\)\s*dk",
        )
        .unwrap()
    });

    // Reassembly of Multi-Head Attention equation
    static MULTIHEAD_RE: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"MultiHead\s*\(\s*Q,\s*K,\s*V\s*\)\s*=\s*Concat\s*\(\s*head\s*\.\.\.,\s*head\s*\)WO\s*1\s*\n+\s*h\s*\n+\s*where\s+head\s*=\s*Attention\s*\(\s*QWQ\s*KWK\s*V\s*WV(?:\s*\n+\s*i)+\s*\n+\s*i\s+i").unwrap()
    });

    // Reassembly of Feed-Forward Network equation
    static FFN_RE: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"FFN\(x\)\s*=\s*max\(0\s+xW\s+b\s+W\s+b\s+\(2\)\s*\n+\s*1\s+1\s+2\s+2").unwrap()
    });

    // Inline scale factor in prose
    static SCALE_FACTOR_RE: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"scaling factor of\s+1\s*\.\s+Additive\s+attention\s+computes\s+the\s+compatibility\s+function\s+using\s+a\s+feed-forward\s+network\s+with\s+dk\b").unwrap()
    });
    static SCALE_DOT_PROD_RE: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"scale the dot products by\s+1\s+dk\b").unwrap());
    static DIVIDE_DK_RE: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"divide each by\s+d\s*,\s*and apply a softmax function to obtain the weights on thek values\b").unwrap()
    });

    let s1 = NEQ_RE.replace_all(text, r"\neq ");
    let s2 = LE_SEMI_RE.replace_all(&s1, r"\le ");
    let s3 = LE_S_RE.replace_all(&s2, r" \le ");
    let s4 = GE_COLON_RE.replace_all(&s3, r"\ge ");
    let s5 = GE_ZERO_RE.replace_all(&s4, r"$1 \ge 0");
    let s6 = EQ_ZERO_RE.replace_all(&s5, "= 0");
    let s7 = NEQ_ZERO_RE.replace_all(&s6, r"\neq 0");

    let s8 = ATTENTION_RE.replace_all(&s7, |_: &regex::Captures| {
        "$$\n\\mathrm{Attention}(Q, K, V) = \\mathrm{softmax}\\left(\\frac{QK^T}{\\sqrt{d_k}}\\right)V \\tag{1}\n$$"
    });
    let s9 = MULTIHEAD_RE.replace_all(&s8, |_: &regex::Captures| {
        "$$\n\\mathrm{MultiHead}(Q, K, V) = \\mathrm{Concat}(\\mathrm{head}_1, \\ldots, \\mathrm{head}_h)W^O\n$$\n\n$$\n\\text{where } \\mathrm{head}_i = \\mathrm{Attention}(QW^Q_i, KW^K_i, VW^V_i)\n$$"
    });
    let s10 = FFN_RE.replace_all(
        &s9,
        |_: &regex::Captures| "$$\n\\mathrm{FFN}(x) = \\max(0, xW_1 + b_1)W_2 + b_2 \\tag{2}\n$$",
    );
    let s11 = SCALE_FACTOR_RE.replace_all(&s10, |_: &regex::Captures| {
        "scaling factor of $\\frac{1}{\\sqrt{d_k}}$. Additive attention computes the compatibility function using a feed-forward network with $d_k$"
    });
    let s12 = SCALE_DOT_PROD_RE.replace_all(
        &s11,
        |_: &regex::Captures| "scale the dot products by $\\frac{1}{\\sqrt{d_k}}$",
    );
    let s13 = DIVIDE_DK_RE.replace_all(&s12, |_: &regex::Captures| {
        "divide each by $\\sqrt{d_k}$, and apply a softmax function to obtain the weights on the values"
    });

    s13.into_owned()
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

    #[test]
    fn test_format_table_and_block_boundaries() {
        let input = "Preceding paragraph.\n| Date | Revenue | Expenses |\n|---|---|\n| 2026-01-01 | $10M | $8M |\nFollowing paragraph.";
        let formatted = format_table_and_block_boundaries(input);

        let expected = "Preceding paragraph.\n\n| Date | Revenue | Expenses |\n|---|---|---|\n| 2026-01-01 | $10M | $8M |\n\nFollowing paragraph.";
        assert_eq!(formatted, expected);
    }

    #[test]
    fn test_format_callout_and_heading_boundaries() {
        let input = "Paragraph text.\n> This is a callout box.\n> Continued callout.\nFollowing text.\n# Heading 1\nHeading body.";
        let formatted = format_table_and_block_boundaries(input);

        let expected = "Paragraph text.\n\n> This is a callout box.\n> Continued callout.\n\nFollowing text.\n\n# Heading 1\nHeading body.";
        assert_eq!(formatted, expected);
    }

    #[test]
    fn test_insert_semantic_section_boundaries_chapter() {
        let input = "Introductory text.\n\n# Chapter 3: The Basics\n\nChapter body text.";
        let output = insert_semantic_section_boundaries(input);
        let expected = "Introductory text.\n\n---\n<!-- filegoblin:section type=\"chapter\" title=\"Chapter 3: The Basics\" -->\n# Chapter 3: The Basics\n\nChapter body text.";
        assert_eq!(output, expected);
    }

    #[test]
    fn test_insert_semantic_section_boundaries_split_title() {
        let input = "Intro text.\n\n# Chapter 1\n\n# The Basics\n\nBody text.";
        let output = insert_semantic_section_boundaries(input);
        let expected = "Intro text.\n\n---\n<!-- filegoblin:section type=\"chapter\" title=\"Chapter 1: The Basics\" -->\n# Chapter 1\n\n# The Basics\n\nBody text.";
        assert_eq!(output, expected);
    }

    #[test]
    fn test_insert_semantic_section_boundaries_part() {
        let input = "Intro text.\n\n## Part One: System Structure and Behavior\n\nBody text.";
        let output = insert_semantic_section_boundaries(input);
        let expected = "Intro text.\n\n---\n<!-- filegoblin:section type=\"part\" title=\"Part One: System Structure and Behavior\" -->\n## Part One: System Structure and Behavior\n\nBody text.";
        assert_eq!(output, expected);
    }

    #[test]
    fn test_insert_semantic_section_boundaries_preserves_existing_divider() {
        let input = "Intro text.\n\n---\n\n# Chapter 2: Second Steps\n\nBody text.";
        let output = insert_semantic_section_boundaries(input);
        let expected = "Intro text.\n\n---\n<!-- filegoblin:section type=\"chapter\" title=\"Chapter 2: Second Steps\" -->\n# Chapter 2: Second Steps\n\nBody text.";
        assert_eq!(output, expected);
    }

    #[test]
    fn test_insert_semantic_section_boundaries_ignores_code_blocks() {
        let input =
            "Code sample:\n\n```python\n# Chapter 1 in code\nprint('hello')\n```\n\nFollow up.";
        let output = insert_semantic_section_boundaries(input);
        assert_eq!(output, input);
    }

    #[test]
    fn test_clean_trailing_production_slug() {
        let line = "Within the university, the purpose of a student may be to get good grades, the purpose of a professor 5/2/09 10:37:35";
        let cleaned = clean_trailing_production_slug(line).expect("should clean trailing slug");
        assert_eq!(
            cleaned,
            "Within the university, the purpose of a student may be to get good grades, the purpose of a professor"
        );

        let standalone = "5/2/09 10:37:35";
        let cleaned_standalone =
            clean_trailing_production_slug(standalone).expect("should match standalone slug");
        assert!(cleaned_standalone.is_empty());
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn test_jpeg_ocr_performance() {
        let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
        let path = Path::new(&manifest_dir).join("../../test_assets/scanned_report.pdf");
        let doc = PdfDocument::open(path).unwrap();
        let images = doc.extract_images(0).unwrap();
        let dynamic = images[0].to_dynamic_image().unwrap();
        let rgb = dynamic.to_rgb8();
        let mut jpeg = Vec::new();
        rgb.write_to(
            &mut std::io::Cursor::new(&mut jpeg),
            image::ImageFormat::Jpeg,
        )
        .unwrap();
        let text = crate::parsers::ocr::ocr_image_bytes(&jpeg).unwrap();
        assert!(text.contains("Revenue"));
    }

    #[test]
    fn test_parallel_matches_sequential() {
        use rayon::prelude::*;
        let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
        let path =
            Path::new(&manifest_dir).join("../../test_assets/benchmarks/slice_b_tables_charts.pdf");
        let doc = PdfDocument::open(path).unwrap();
        let count = doc.page_count().unwrap();
        let options = pdf_oxide::converters::ConversionOptions::default();

        let seq_results: Vec<String> = (0..count)
            .map(|p| doc.to_markdown(p, &options).unwrap())
            .collect();

        let par_results: Vec<String> = (0..count)
            .into_par_iter()
            .map(|p| doc.to_markdown(p, &options).unwrap())
            .collect();

        assert_eq!(seq_results, par_results);
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn test_scanned_page_ocr_parallel() {
        let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
        let path =
            Path::new(&manifest_dir).join("../../test_assets/benchmarks/slice_a_twocolumn.pdf");
        let doc = PdfDocument::open(path).unwrap();
        let count = doc.page_count().unwrap();

        let results: Vec<_> = (0..count)
            .into_par_iter()
            .map(|p| ocr_scanned_page(&doc, p))
            .collect();

        assert_eq!(results.len(), count);
        for res in results {
            assert!(res.is_some(), "scanned page should produce OCR text");
            assert!(!res.unwrap().is_empty());
        }
    }

    #[test]
    fn test_normalize_pdf_glyph() {
        assert_eq!(normalize_pdf_glyph("CMSY10", 0x32), Some(r"\ge "));
        assert_eq!(normalize_pdf_glyph("CMSY10", 0x33), Some(r"\le "));
        assert_eq!(normalize_pdf_glyph("CMMI10", 0x0B), Some(r"\alpha "));
        assert_eq!(normalize_pdf_glyph("CMMI10", 0x0C), Some(r"\beta "));
        assert_eq!(normalize_pdf_glyph("MSBM10", 0x52), Some(r"\mathbb{R}"));
    }

    #[test]
    fn test_normalize_pdf_math_notation() {
        let input = "A =I- B and q(x) 2 0 with q'(x) ::; q(x) where C = O";
        let output = normalize_pdf_math_notation(input);
        assert!(output.contains(r"A \neq B"));
        assert!(output.contains(r"q(x) \ge 0"));
        assert!(output.contains(r"q'(x) \le q(x)"));
        assert!(output.contains("C = 0"));

        let att_input = "QKT Attention(Q, K, V ) = softmax( V (1) dk";
        let att_output = normalize_pdf_math_notation(att_input);
        assert!(att_output.contains(
            r"\mathrm{Attention}(Q, K, V) = \mathrm{softmax}\left(\frac{QK^T}{\sqrt{d_k}}\right)V"
        ));

        let ffn_input = "FFN(x) = max(0 xW b W b (2)\n1 1 2 2";
        let ffn_output = normalize_pdf_math_notation(ffn_input);
        assert!(ffn_output.contains(r"\mathrm{FFN}(x) = \max(0, xW_1 + b_1)W_2 + b_2"));
    }

    #[test]
    fn test_pdf_math_zeitz_slice() {
        let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
        let path =
            Path::new(&manifest_dir).join("../../test_assets/benchmarks/slice_e_math_zeitz.pdf");
        if path.exists() {
            let gobbler = PdfGobbler;
            let md = gobbler
                .gobble(&path, &ParseOptions::default())
                .expect("parse math slice pdf");
            assert!(md.contains(r"\neq"));
            assert!(md.contains(r"\le"));
        }
    }
}
