//! PDF → markdown.
//!
//! The pipeline has three stages, and the order matters:
//!
//! 1. **Geometry** (per page, parallel): classify each page and collect its
//!    words and lines with their boxes and font sizes.
//! 2. **Furniture** (whole document): decide which lines are running headers,
//!    footers, page numbers, or line-number gutters — from position, recurrence,
//!    size and alignment ([`pdf_furniture`]) — and exclude those regions.
//! 3. **Conversion** (per page, parallel): `pdf_oxide` turns the remaining text
//!    into markdown (headings, tables, reading order); ruling-line tables are
//!    recovered from vector paths; scanned pages go to OCR.
//!
//! After that the markdown is only *tidied* — page joins, block spacing, false
//! headings. No pass in this file rewrites words, numbers, or punctuation inside
//! prose. That rule exists because an earlier version did exactly that to pass a
//! benchmark, and corrupted drug doses, legal citations and form references in
//! every other PDF (see `docs/agent_context/AUDIT_REPORT.md`).

use crate::parsers::gobble::{Gobble, ParseOptions};
use crate::parsers::pdf_furniture::{self, PageGeom};
use anyhow::{Context, Result};
use pdf_oxide::PdfDocument;
use pdf_oxide::extractors::PageKind;
use pdf_oxide::geometry::Rect;
use pdf_oxide::layout::RectFilterMode;
use rayon::prelude::*;
use std::collections::{HashMap, HashSet};
use std::path::Path;

/// Detects a vertical two-column band on pages with mixed layouts (e.g. a
/// single-column top and bottom with a two-column section in between).
/// Returns `Some((y_top, y_bot, gutter_x))` in page coordinates.
///
/// Works on the page's words *after* furniture removal, so a line-number
/// gutter between the columns (patents) does not read as text crossing it.
/// Scan resolution follows the body font size rather than fixed points.
fn detect_two_column_bands(
    doc: &PdfDocument,
    page: usize,
    geom: &PageGeom,
    excluded: &[Rect],
    body: f32,
) -> Option<(f32, f32, f32)> {
    if geom.lines.iter().any(|l| l.text.contains('|')) {
        return None;
    }

    let words: Vec<&pdf_furniture::GWord> = geom
        .words
        .iter()
        .filter(|w| !pdf_furniture::excluded(excluded, w.x0, w.y0, w.x1, w.y1))
        .collect();
    if words.len() < 40 {
        return None;
    }

    let (bx0, by0, bx1, by1) = geom.bx;
    let width = bx1 - bx0;
    let height = by1 - by0;
    // One scan slice ≈ 1.5 text lines; margins ≈ 4.5% of the page.
    let slice = (body * 1.6).clamp(9.0, 30.0);
    let margin = 0.045 * height;
    let min_gap = (body * 0.5).max(4.0);

    let mut best_score = 0;
    let mut best_gx_min = 0.0f32;
    let mut best_gx_max = 0.0f32;
    let mut best_top = 0.0f32;
    let mut best_bot = 0.0f32;

    let mut gx = bx0 + 0.35 * width;
    while gx <= bx0 + 0.65 * width {
        let mut col_slices = Vec::new();
        let mut y = by1 - margin;
        while y >= by0 + margin {
            let sw: Vec<_> = words
                .iter()
                .filter(|w| w.y0 >= y - slice && w.y0 < y)
                .collect();
            let max_l = sw
                .iter()
                .filter(|w| w.x1 <= gx + 1.0)
                .map(|w| w.x1)
                .fold(0.0f32, f32::max);
            let min_r = sw
                .iter()
                .filter(|w| w.x0 >= gx - 1.0)
                .map(|w| w.x0)
                .fold(f32::INFINITY, f32::min);
            let has_mid = sw.iter().any(|w| w.x0 < gx - 5.0 && w.x1 > gx + 5.0);

            let is_2col =
                max_l > 0.0 && min_r < f32::INFINITY && !has_mid && (min_r - max_l) >= min_gap;
            col_slices.push((y, is_2col));
            y -= slice;
        }

        let mut max_window_len = 0;
        let mut window_top = 0.0f32;
        let mut window_bot = 0.0f32;

        for i in 0..col_slices.len() {
            if !col_slices[i].1 {
                continue;
            }
            let mut true_count = 0;
            for j in i..col_slices.len() {
                if col_slices[j].1 {
                    true_count += 1;
                }
                let len = j - i + 1;
                let ratio = (true_count as f32) / (len as f32);
                if len >= 8 && ratio >= 0.75 && true_count > max_window_len {
                    max_window_len = true_count;
                    window_top = col_slices[i].0;
                    window_bot = (col_slices[j].0 - slice).max(by0 + margin * 0.8);
                }
            }
        }

        if max_window_len >= 8 {
            if max_window_len > best_score {
                best_score = max_window_len;
                best_gx_min = gx;
                best_gx_max = gx;
                best_top = window_top;
                best_bot = window_bot;
            } else if max_window_len == best_score
                && (window_top - window_bot) >= (best_top - best_bot) - 5.0
            {
                best_gx_max = gx;
            }
        }

        gx += 2.0;
    }

    if best_score < 8 {
        return None;
    }
    // Pages with tables are left to the table extractor. Checked last: table
    // detection is expensive and most pages have no two-column band at all.
    if !doc.extract_tables(page).unwrap_or_default().is_empty() {
        return None;
    }
    let gx = (best_gx_min + best_gx_max) / 2.0;
    Some((best_top, best_bot, gx))
}

/// Converts one page's text to markdown, slicing a mixed two-column band into
/// top / left / right / bottom regions when one is detected.
fn convert_page_text(
    doc: &PdfDocument,
    page: usize,
    options: &pdf_oxide::converters::ConversionOptions,
    geom: Option<&PageGeom>,
    excluded: &[Rect],
    body: f32,
) -> Result<String, String> {
    if let Some(geom) = geom
        && let Some((mut top_cut, mut bot_cut, gx)) =
            detect_two_column_bands(doc, page, geom, excluded, body)
    {
        let (bx0, by0, bx1, by1) = geom.bx;
        let width = bx1 - bx0;
        let height = by1 - by0;
        let edge = 0.06 * height;
        let crosses = |w: &&&pdf_furniture::GWord| w.x0 < gx - 5.0 && w.x1 > gx + 5.0;
        let kept: Vec<&pdf_furniture::GWord> = geom
            .words
            .iter()
            .filter(|w| !pdf_furniture::excluded(excluded, w.x0, w.y0, w.x1, w.y1))
            .collect();
        let has_full_width_top = top_cut < by1 - edge
            && kept
                .iter()
                .filter(crosses)
                .any(|w| w.y0 >= top_cut + 8.0 && w.y0 < by1 - edge);
        if !has_full_width_top {
            top_cut = by1;
        }
        let has_full_width_bot = bot_cut > by0 + edge
            && kept
                .iter()
                .filter(crosses)
                .any(|w| w.y0 < bot_cut - 8.0 && w.y0 > by0 + 0.8 * edge);
        if !has_full_width_bot {
            bot_cut = by0;
        }

        let mut regions: Vec<Rect> = Vec::new();
        if has_full_width_top {
            regions.push(Rect::new(bx0, top_cut, width, by1 - top_cut));
        }
        regions.push(Rect::new(bx0, bot_cut, gx - bx0, top_cut - bot_cut));
        regions.push(Rect::new(gx, bot_cut, bx1 - gx, top_cut - bot_cut));
        if has_full_width_bot {
            regions.push(Rect::new(bx0, by0, width, bot_cut - by0));
        }

        let mut parts = Vec::new();
        for r in regions {
            let mut opts = options.clone();
            opts.reading_order_mode =
                pdf_oxide::converters::ReadingOrderMode::TopToBottomLeftToRight;
            opts.include_region = Some((r, RectFilterMode::MinOverlap(0.5)));
            if let Ok(md) = doc.to_markdown(page, &opts) {
                let t = md.trim();
                if !t.is_empty() {
                    parts.push(t.to_string());
                }
            }
        }
        if !parts.is_empty() {
            return Ok(parts.join("\n\n"));
        }
    }

    // Single-column pages read top-to-bottom; anything else goes to the
    // converter's column-aware (XY-cut) ordering.
    if let Some(geom) = geom {
        let (bx0, _, bx1, _) = geom.bx;
        let width = bx1 - bx0;
        let lines: Vec<_> = geom
            .lines
            .iter()
            .filter(|l| !pdf_furniture::excluded(excluded, l.x0, l.y0, l.x1, l.y1))
            .collect();
        let straddle = lines
            .iter()
            .filter(|l| {
                (l.x0 < bx0 + 0.45 * width && l.x1 > bx0 + 0.55 * width)
                    || (l.x1 - l.x0) > 0.50 * width
            })
            .count();
        if !lines.is_empty() && straddle * 100 / lines.len() >= 25 {
            let mut opts = options.clone();
            opts.reading_order_mode =
                pdf_oxide::converters::ReadingOrderMode::TopToBottomLeftToRight;
            return doc.to_markdown(page, &opts).map_err(|e| e.to_string());
        }
    }
    doc.to_markdown(page, options).map_err(|e| e.to_string())
}

/// Joins per-page markdown, healing a sentence that a page break cut in two:
/// the previous page ends mid-sentence and the next begins in lowercase.
/// Closes a code fence that `page` opens and never closes.
fn close_open_fence(page: &mut String) {
    let fences = page
        .lines()
        .filter(|l| l.trim_start().starts_with("```"))
        .count();
    if fences % 2 == 1 {
        if !page.ends_with('\n') {
            page.push('\n');
        }
        page.push_str("```");
    }
}

/// A footnote paragraph: starts with a footnote mark (`*`, `†`, `‡`, `§`,
/// a superscript digit, or a Markdown footnote definition), and is short.
fn is_footnote_para(p: &str) -> bool {
    let t = p.trim();
    let mut cs = t.chars();
    let starts = match cs.next() {
        Some('*') => cs.next().is_some_and(|c| c.is_alphanumeric()),
        Some('†' | '‡' | '§' | '¹' | '²' | '³' | '⁴' | '⁵' | '⁶' | '⁷' | '⁸' | '⁹') => {
            true
        }
        _ => t.starts_with("[^"),
    };
    starts && t.split_whitespace().count() <= 80
}

fn join_pages_healing_dangling(pages: &[String]) -> String {
    let mut out = String::new();
    for p in pages.iter() {
        let p_trimmed = p.trim();
        if p_trimmed.is_empty() {
            continue;
        }
        if out.is_empty() {
            out.push_str(p_trimmed);
            continue;
        }
        let last_line = out
            .lines()
            .rev()
            .find(|l| !l.trim().is_empty())
            .unwrap_or("");
        let first_line = p_trimmed
            .lines()
            .find(|l| !l.trim().is_empty())
            .unwrap_or("");
        let last_trimmed = last_line.trim();
        let first_trimmed = first_line.trim();
        let is_structural = |s: &str| {
            s.starts_with('#')
                || s.starts_with('|')
                || s.starts_with('>')
                || s.starts_with("```")
                || s.starts_with("$$")
                || s.starts_with("- ")
                || s.starts_with("* ")
        };
        let ends_dangling = !last_trimmed.is_empty()
            && !is_structural(last_trimmed)
            && last_trimmed
                .chars()
                .last()
                .is_some_and(|c| c.is_alphanumeric() || matches!(c, ',' | '-' | '–'));
        let starts_lower = !first_trimmed.is_empty()
            && !is_structural(first_trimmed)
            && first_trimmed
                .chars()
                .next()
                .is_some_and(|c| c.is_lowercase());

        // A footnote at the foot of the page sits between a sentence and its
        // continuation on the next page: join across it and move the
        // footnote after the joined paragraph.
        if starts_lower
            && let Some((head, foot)) = out.rsplit_once("\n\n")
            && is_footnote_para(foot)
            && head
                .lines()
                .rev()
                .find(|l| !l.trim().is_empty())
                .map(str::trim)
                .is_some_and(|l| {
                    !is_structural(l)
                        && l.chars()
                            .last()
                            .is_some_and(|c| c.is_alphanumeric() || matches!(c, ',' | '-' | '–'))
                })
        {
            let foot = foot.trim().to_string();
            let mut joined = head.trim_end().to_string();
            if joined.ends_with('-')
                && joined[..joined.len() - 1].ends_with(|c: char| c.is_alphabetic())
            {
                joined.pop();
            } else {
                joined.push(' ');
            }
            let (first_para, rest) = p_trimmed
                .split_once("\n\n")
                .map_or((p_trimmed, ""), |(a, b)| (a, b));
            joined.push_str(first_para);
            joined.push_str("\n\n");
            joined.push_str(&foot);
            if !rest.trim().is_empty() {
                joined.push_str("\n\n");
                joined.push_str(rest.trim());
            }
            out = joined;
            continue;
        }
        if ends_dangling && starts_lower {
            let trimmed_len = out.trim_end().len();
            out.truncate(trimmed_len);
            // A syllable hyphen at the page break joins the halves.
            if out.ends_with('-') && out[..out.len() - 1].ends_with(|c: char| c.is_alphabetic()) {
                out.pop();
            } else {
                out.push(' ');
            }
            out.push_str(p_trimmed);
        } else {
            out.push_str("\n\n");
            out.push_str(p_trimmed);
        }
    }
    out
}

/// PDF → markdown via `pdf_oxide`: native markdown conversion (headings, tables,
/// reading order) with per-page scanned/text classification. A page classified
/// as a scan is OCR'd from its embedded images — on macOS via Apple Vision with
/// no setup at all; elsewhere with `--features ocr`.
pub struct PdfGobbler;

impl Gobble for PdfGobbler {
    fn gobble(&self, path: &Path, opts: &ParseOptions) -> Result<String> {
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

        // Stage 1: classify pages and collect geometry for those with text.
        // Classification and geometry run back to back for each page, so the
        // page's parsed content is still in pdf_oxide's small per-page cache
        // when the geometry pass reads it.
        let staged: Vec<(PageKind, Option<PageGeom>, bool, bool)> = (0..page_count)
            .into_par_iter()
            .map(|page| {
                // Cheap preflight: signals only, no rasterization. On any
                // classification error, assume a text layer and let extraction try.
                // Classification reads the page's image resources, which,
                // like image extraction, are not safe to read for several
                // pages at once: a scan could be classified from another
                // page's images (one run in eight on a 5-page scan).
                // Serialized; it is cheap next to geometry and conversion.
                static CLASSIFY: std::sync::Mutex<()> = std::sync::Mutex::new(());
                let classified = {
                    let _guard = CLASSIFY.lock().unwrap_or_else(|e| e.into_inner());
                    doc.classify_page(page)
                };
                // A page-sized picture under the text: a scan carrying an
                // OCR text layer made by the scanner's software.
                // Or invisible text (render mode 3), which is what OCR
                // software lays over the picture it read — the picture's own
                // area is not always measurable (inline images, odd codecs).
                // Or Acrobat ClearScan's rebuilt fonts, which replace the
                // picture with synthesized type.
                let image_backed = classified.as_ref().is_ok_and(|c| {
                    c.signals.image_area_ratio >= 0.5 || c.signals.invisible_text_ratio >= 0.5
                }) || super::pdf_textq::clearscan_fonts(&doc, page);
                // The picture itself covers the page, so the platform OCR
                // engine can read it without rendering.
                let page_picture = classified
                    .as_ref()
                    .is_ok_and(|c| c.signals.image_area_ratio >= 0.5);
                if std::env::var_os("FILEGOBLIN_DEBUG").is_some()
                    && let Ok(c) = classified.as_ref()
                {
                    eprintln!(
                        "classify page {}: {:?}, image {:.2}, invisible {:.2}, glyphs {}, garbled {:.2}",
                        page + 1,
                        c.kind,
                        c.signals.image_area_ratio,
                        c.signals.invisible_text_ratio,
                        c.signals.text_glyph_count,
                        c.signals.garbled_ratio
                    );
                }
                let kind = match classified {
                    // A "scan" with no picture on it is a text page whose
                    // text tripped the quality gate (a table of contents'
                    // dot leaders read as repeated junk). There is nothing
                    // to OCR; its text layer is the content.
                    Ok(c)
                        if c.kind == PageKind::Scanned
                            && c.signals.image_area_ratio < 0.05
                            && c.signals.text_glyph_count > 0 =>
                    {
                        PageKind::TextLayer
                    }
                    Ok(c) => c.kind,
                    Err(_) => PageKind::TextLayer,
                };
                let geom = match kind {
                    PageKind::Empty => None,
                    _ => pdf_furniture::collect_page_geom(&doc, page),
                };
                (kind, geom, image_backed, page_picture)
            })
            .collect();
        let mut kinds: Vec<PageKind> = Vec::with_capacity(page_count);
        let mut geoms: Vec<Option<PageGeom>> = Vec::with_capacity(page_count);
        let mut image_backed: Vec<bool> = Vec::with_capacity(page_count);
        let mut page_picture: Vec<bool> = Vec::with_capacity(page_count);
        for (k, g, b, p) in staged {
            kinds.push(k);
            geoms.push(g);
            image_backed.push(b);
            page_picture.push(p);
        }

        // Stage 2: furniture, decided across the whole document.
        let pdf_furniture::Furniture {
            rects: furniture,
            gutters,
        } = pdf_furniture::detect_furniture(&geoms);

        let body = pdf_furniture::body_font_size(&geoms);

        let base_options = pdf_oxide::converters::ConversionOptions {
            detect_headings: true,
            extract_tables: true,
            include_images: false,
            // Keep pdf_oxide's own artifact heuristics out of it: for untagged
            // PDFs they drop any top/bottom-band line whose digit-stripped text
            // recurs — which deletes real headings like "Section 3: Overview"
            // on every page after the first. Furniture is decided above,
            // geometrically, instead.
            include_artifacts: true,
            expand_ligatures: true,
            ..Default::default()
        };

        #[cfg(not(feature = "vision"))]
        let _ = opts.vision;
        // Layout models, when built in and their weights are present: render
        // pages and let the model place each text-layer word.
        #[cfg(feature = "vision")]
        let mode = opts.vision;
        #[cfg(feature = "vision")]
        let renderer = if mode != crate::cli::VisionMode::Off && crate::vision::enabled() {
            std::fs::read(path)
                .ok()
                .and_then(|bytes| crate::vision::render::Renderer::open(std::sync::Arc::new(bytes)))
        } else {
            if mode != crate::cli::VisionMode::Off
                && !opts.quiet
                && std::env::var_os("FILEGOBLIN_VISION").is_none()
            {
                static HINT: std::sync::Once = std::sync::Once::new();
                HINT.call_once(|| {
                    eprintln!(
                        "filegoblin: PDF vision models not found — using the fast converter. \
                         For math, tables, columns and scans install them (`just get-vision-models`, \
                         or unpack the release's filegoblin-vision-models.tar.gz into ~/.filegoblin/models). \
                         `--vision off` silences this."
                    );
                });
            }
            None
        };

        #[cfg(feature = "vision")]
        let vision_pages = std::sync::atomic::AtomicUsize::new(0);

        // Stage 3: convert each page with its furniture excluded.
        // (markdown, unreadable, came_from_ocr)
        let page_results: Vec<(String, bool, bool)> = (0..page_count)
            .into_par_iter()
            .map(|page| {
                let excluded = &furniture[page];
                let geom = geoms[page].as_ref();
                let mut options = base_options.clone();
                options.exclude_regions = excluded.clone();
                options.exclude_regions_mode = RectFilterMode::MinOverlap(0.5);
                let kind = kinds[page];
                // Scans: the bundled layout + OCR models when available, else
                // the platform OCR engine on the page's embedded images.
                let scanned_page_text = |doc: &PdfDocument, page: usize| -> Option<String> {
                    #[cfg(feature = "vision")]
                    if let Some(md) = renderer
                        .as_ref()
                        .and_then(|r| super::pdf_vision::convert_scanned_page(r, page))
                        .filter(|md| !md.trim().is_empty())
                    {
                        vision_pages.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        return Some(md);
                    }
                    ocr_scanned_page(doc, page)
                };

                match kind {
                    // "Empty" to the text layer can still be a picture the
                    // classifier could not decode (JBIG2, odd masks); the
                    // renderer and layout model get the final say.
                    PageKind::Empty => {
                        #[cfg(feature = "vision")]
                        if let Some(md) = renderer
                            .as_ref()
                            .and_then(|r| super::pdf_vision::convert_scanned_page(r, page))
                            .filter(|md| !md.trim().is_empty())
                        {
                            vision_pages.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                            return (md, false, true);
                        }
                        (String::new(), false, false)
                    }
                    PageKind::Scanned => match scanned_page_text(&doc, page) {
                        Some(text) if !text.trim().is_empty() => {
                            (text.trim().to_string(), false, true)
                        }
                        _ => {
                            let (mut md, unreadable) =
                                match convert_page_text(&doc, page, &options, geom, excluded, body)
                                {
                                    Ok(text) if !text.trim().is_empty() => {
                                        (text.trim().to_string(), false)
                                    }
                                    _ => (scanned_page_marker(page + 1), true),
                                };
                            if !unreadable && !md.is_empty() {
                                md = recover_page_vector_tables(&doc, page, md, geom, excluded);
                            }
                            (md, unreadable, false)
                        }
                    },
                    // TextLayer / ImageText / Mixed: the native converter. For
                    // hybrids, also OCR the page's image so figure text isn't lost.
                    _ => {
                        // The fast converter's reading comes first: it is the
                        // fallback for every route below, and its words are
                        // what the text layer's quality is judged on (the
                        // raw word list of a layer without space characters
                        // is one long token per line).
                        let (mut md, unreadable) =
                            match convert_page_text(&doc, page, &options, geom, excluded, body) {
                                Ok(text) => (
                                    rejoin_letter_spacing(
                                        &pdf_furniture::remove_gutter_numbers(
                                            text.trim(),
                                            &gutters[page],
                                        ),
                                        geom,
                                    ),
                                    false,
                                ),
                                Err(e) => (failed_page_marker(page + 1, &e), true),
                            };
                        let quality = super::pdf_textq::measure_words(md.split_whitespace());
                        let poor_layer = image_backed[page] && quality.is_poor();
                        if std::env::var_os("FILEGOBLIN_DEBUG").is_some() {
                            eprintln!(
                                "text layer page {}: {} words, single {:.3}, garbage {:.3}, unmapped {:.3}{}{}",
                                page + 1,
                                quality.words,
                                quality.single,
                                quality.garbage,
                                quality.unmapped,
                                if image_backed[page] { ", over a page image" } else { "" },
                                if poor_layer { " -> re-read from image" } else { "" }
                            );
                        }
                        // A scan whose OCR layer is too damaged to trust
                        // (formulas spelled as loose letters, made-up words,
                        // unmappable glyphs) is read again from its image,
                        // like a scan with no text at all: rendered, by the
                        // layout and OCR models; without them, by the
                        // platform OCR engine, but only when one picture is
                        // the whole page (a ClearScan page's small formula
                        // images would replace its text with a few symbols).
                        #[cfg(feature = "vision")]
                        if poor_layer
                            && let Some(md) = renderer
                                .as_ref()
                                .and_then(|r| super::pdf_vision::convert_scanned_page(r, page))
                                .filter(|md| !md.trim().is_empty())
                        {
                            vision_pages.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                            return (md.trim().to_string(), false, true);
                        }
                        if poor_layer
                            && page_picture[page]
                            && let Some(text) = ocr_scanned_page(&doc, page)
                            && !text.trim().is_empty()
                        {
                            return (text.trim().to_string(), false, true);
                        }
                        // `--vision smart` keeps plain prose on the fast path.
                        #[cfg(feature = "vision")]
                        let wanted = match mode {
                            crate::cli::VisionMode::All => true,
                            crate::cli::VisionMode::Off => false,
                            crate::cli::VisionMode::Smart => {
                                let why = super::pdf_route::needs_vision(
                                    &doc, page, kind, geom, page_count,
                                )
                                .or(super::pdf_route::spelled_math(&quality)
                                    .then_some(super::pdf_route::Reason::Math));
                                if std::env::var_os("FILEGOBLIN_DEBUG").is_some() {
                                    eprintln!(
                                        "route page {}: {}",
                                        page + 1,
                                        why.map_or("fast path", super::pdf_route::Reason::as_str)
                                    );
                                }
                                why.is_some()
                            }
                        };
                        #[cfg(feature = "vision")]
                        if wanted
                            && let (Some(r), Some(g)) = (renderer.as_ref(), geom)
                            && let Some(md) = super::pdf_vision::convert_page(
                                r,
                                &doc,
                                page,
                                g,
                                excluded,
                                &base_options,
                            )
                        {
                            vision_pages.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                            let md = rejoin_letter_spacing(
                                &pdf_furniture::remove_gutter_numbers(md.trim(), &gutters[page]),
                                geom,
                            );
                            return (md, false, false);
                        }
                        if !unreadable && !md.is_empty() {
                            md = recover_page_vector_tables(&doc, page, md, geom, excluded);
                        }
                        let mut from_ocr = false;
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
                                from_ocr = true;
                            } else {
                                md.push_str("\n\n> **Text from embedded image:**\n> ");
                                md.push_str(&ocr.trim().replace('\n', "\n> "));
                            }
                        }
                        (md, unreadable, from_ocr)
                    }
                }
            })
            .collect();

        #[cfg(feature = "vision")]
        if renderer.is_some() && !opts.quiet && page_count > 1 {
            eprintln!(
                "filegoblin: {} of {page_count} pages read with the vision models (--vision {})",
                vision_pages.load(std::sync::atomic::Ordering::Relaxed),
                match mode {
                    crate::cli::VisionMode::Smart => "smart",
                    crate::cli::VisionMode::All => "all",
                    crate::cli::VisionMode::Off => "off",
                }
            );
        }

        let mut pages_md: Vec<String> = Vec::with_capacity(page_results.len());
        let mut ocr_flags: Vec<bool> = Vec::with_capacity(page_results.len());
        let mut unreadable_pages = 0usize;
        for (page_md, unreadable, from_ocr) in page_results {
            if unreadable {
                unreadable_pages += 1;
            }
            if !page_md.trim().is_empty() {
                pages_md.push(page_md);
                ocr_flags.push(from_ocr);
            }
        }

        // OCR'd pages carry no geometry here, so their furniture is found from
        // the text alone (outermost lines, recurrence, page-number sequence).
        pdf_furniture::strip_text_furniture(&mut pages_md, &ocr_flags);

        pages_md.retain(|p| !p.trim().is_empty());

        let hyphen_breaks: HashSet<(String, String)> = geoms
            .iter()
            .zip(furniture.iter())
            .filter_map(|(g, f)| g.as_ref().map(|g| pdf_furniture::hyphen_breaks(g, f)))
            .flatten()
            .collect();
        let glued: HashMap<String, String> = geoms
            .iter()
            .flatten()
            .flat_map(|g| g.glued.iter().cloned())
            .collect();
        // A page that leaves a code fence open would turn every page after
        // it into one code block once the pages are joined.
        for p in pages_md.iter_mut() {
            close_open_fence(p);
        }
        let joined = split_glued_tokens(&join_pages_healing_dangling(&pages_md), &glued);
        let tidy = tidy_markdown(&joined, &hyphen_breaks);
        if tidy.is_empty() {
            if unreadable_pages > 0 {
                anyhow::bail!(
                    "All {} page(s) are scanned images and OCR is unavailable in this build \
                     (macOS has Vision built in; elsewhere recompile with --features ocr)",
                    page_count
                );
            }
            anyhow::bail!("No text could be extracted from this PDF (corrupt or encrypted?)");
        }
        Ok(tidy)
    }
}

/// Repairs word spacing from the page geometry: rejoins letter-spaced
/// headings (`S U M M A R Y` → `SUMMARY`) and splits words the converter ran
/// together across a visible gap (`theUnionPacific` → `the Union Pacific`).
fn rejoin_letter_spacing(md: &str, geom: Option<&PageGeom>) -> String {
    let Some(geom) = geom else {
        return md.to_string();
    };
    let mut out = md.to_string();
    let mut runs = pdf_furniture::letter_spaced_runs(geom);
    runs.sort_by_key(|r| std::cmp::Reverse(r.0.len()));
    for (spaced, joined) in runs {
        out = out.replace(&spaced, &joined);
    }
    out
}

/// Splits tokens the converter ran together across visible word gaps
/// (`theUnionPacificRailroad` → `the Union Pacific Railroad`). A token is
/// split only when it equals a gap-separated run of words on its page
/// (`glued`) *and* every split point is also visible in the characters: a
/// lowercase→uppercase change, a letter↔digit change, punctuation followed by
/// a letter or digit, or an acronym running into a lowercase word. Word boxes
/// alone are not trusted — some fonts (ligatures, bad width tables) give the
/// extractor gaps inside "feedback" or "coffee".
fn split_glued_tokens(md: &str, glued: &HashMap<String, String>) -> String {
    if glued.is_empty() {
        return md.to_string();
    }
    fn boundary_visible(left: &str, right: &str) -> bool {
        let (Some(a), Some(b)) = (left.chars().last(), right.chars().next()) else {
            return false;
        };
        let acronym = left.chars().filter(|c| c.is_alphabetic()).count() >= 2
            && left
                .chars()
                .filter(|c| c.is_alphabetic())
                .all(|c| c.is_uppercase());
        (a.is_lowercase() && b.is_uppercase())
            || (a.is_alphabetic() && b.is_ascii_digit())
            || (a.is_ascii_digit() && b.is_alphabetic())
            || (matches!(a, '.' | ',' | ';' | ':' | ')' | '”') && b.is_alphanumeric())
            || (acronym && b.is_lowercase())
    }
    static TOKEN_RE: std::sync::LazyLock<regex::Regex> =
        std::sync::LazyLock::new(|| regex::Regex::new(r"[^\s|*_`()\[\]]{6,}").unwrap());
    TOKEN_RE
        .replace_all(md, |c: &regex::Captures| {
            let tok = &c[0];
            let Some(spaced) = glued.get(tok) else {
                return tok.to_string();
            };
            let parts: Vec<&str> = spaced.split(' ').collect();
            let ok = parts
                .iter()
                .all(|p| p.chars().any(|ch| ch.is_alphanumeric()))
                && parts.windows(2).all(|w| boundary_visible(w[0], w[1]));
            if ok { spaced.clone() } else { tok.to_string() }
        })
        .into_owned()
}

/// Structure-only clean-up of the joined markdown. Each pass leaves the words
/// of the document untouched.
pub(crate) fn tidy_markdown(md: &str, hyphen_breaks: &HashSet<(String, String)>) -> String {
    let headings_fixed = suppress_false_headings(md);
    let bold_merged =
        super::headings::promote_headings(&merge_adjacent_bold_spans(&headings_fixed));
    let unwrapped = unwrap_false_prose_code_blocks(&bold_merged);
    let dehyphenated = split_glued_capitals(&dehyphenate_line_breaks(&unwrapped, hyphen_breaks));
    let formatted = format_table_and_block_boundaries(&dehyphenated);
    let sectioned = insert_semantic_section_boundaries(&formatted);
    prune_isolated_letter_lines(&sectioned).trim().to_string()
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

/// Demotes markdown headings that cannot be headings. `pdf_oxide` promotes text
/// to a heading from font size alone, so a large formula, a big KPI number, or a
/// paragraph set in a display face can arrive as `#`.
pub(crate) fn suppress_false_headings(md: &str) -> String {
    let mut out = Vec::new();
    let mut in_code_fence = false;

    for line in md.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("```") {
            in_code_fence = !in_code_fence;
            out.push(line.to_string());
            continue;
        }
        if in_code_fence || !trimmed.starts_with('#') {
            out.push(line.to_string());
            continue;
        }

        let level = trimmed.chars().take_while(|&c| c == '#').count();
        let body = trimmed[level..].trim();
        if is_false_heading(body) {
            out.push(body.to_string());
        } else {
            out.push(line.to_string());
        }
    }
    out.join("\n")
}

fn is_false_heading(body: &str) -> bool {
    let plain = body.trim_matches(|c| matches!(c, '*' | '_' | '`')).trim();
    if plain.is_empty() {
        return true;
    }
    // No letters at all: a number, date, or symbol run ("52,000", "2023").
    if !plain.chars().any(|c| c.is_alphabetic()) {
        return true;
    }
    // A heading does not start mid-sentence…
    if plain.chars().next().is_some_and(|c| c.is_lowercase()) {
        return true;
    }
    // …or stop on a dangling operator / open bracket / comma.
    if plain.ends_with(['(', '[', '{', '+', '=', '\\', ',']) {
        return true;
    }
    // Equations and formulas.
    if plain.contains('=') || plain.contains('\\') {
        return true;
    }
    // A whole paragraph set in a large face.
    plain.split_whitespace().count() > 25
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
    // pdf_oxide's image extraction is not safe to run for several pages at
    // once: under parallel calls a page could come back with the *next*
    // page's images, so a scan's text shifted one page and the last page
    // came back empty (seen in about one run in four on a 5-page scan).
    // Extraction is serialized; the OCR itself stays parallel.
    static EXTRACT_IMAGES: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let mut images = {
        let _guard = EXTRACT_IMAGES.lock().unwrap_or_else(|e| e.into_inner());
        doc.extract_images(page).ok()?
    };
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

/// Merges adjacent bold spans separated only by horizontal whitespace on the same line,
/// e.g. `**Standard**  **deduction for—**` -> `**Standard deduction for—**`.
pub(crate) fn merge_adjacent_bold_spans(md: &str) -> String {
    static ADJACENT_BOLD_RE: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
        regex::Regex::new(r"\*\*([^\*\n]+)\*\*[ \t]+\*\*([^\*\n]+)\*\*").unwrap()
    });
    let mut result = md.to_string();
    while ADJACENT_BOLD_RE.is_match(&result) {
        result = ADJACENT_BOLD_RE
            .replace_all(&result, "**$1 $2**")
            .to_string();
    }
    result
}

/// Joins words split by a hyphen at a line break (`infor-` / `mation`).
///
/// Which hyphens were line breaks is known from the page geometry: a line
/// ending in `word-` followed by a line starting with a lowercase word
/// (`hyphen_breaks`, collected per page). In the markdown those pairs may
/// already be glued back together by the converter (`infor-mation`) or still
/// sit across a newline. A hyphen that was *not* at a line end is a real
/// compound and is never touched; across a markdown newline every pair is a
/// candidate.
///
/// To tell a syllable break from a compound broken at its hyphen, the
/// document's own vocabulary decides:
/// * the joined form occurs elsewhere in the document → join (`information`);
/// * the hyphenated form occurs elsewhere, both halves occur as words of their
///   own, or the left part is already a compound (`state-of-the-`) → keep the
///   hyphen (`well-known`, `state-of-the-art`);
/// * otherwise → join, since syllable breaks are far more common.
pub(crate) fn dehyphenate_line_breaks(
    md: &str,
    hyphen_breaks: &HashSet<(String, String)>,
) -> String {
    static BREAK_RE: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
        regex::Regex::new(r"([\p{L}][\p{L}-]*)([ \t]?)-([ \t]*\n?[ \t]*)(\p{Ll}[\p{L}]*)").unwrap()
    });
    static WORD_RE: std::sync::LazyLock<regex::Regex> =
        std::sync::LazyLock::new(|| regex::Regex::new(r"[\p{L}]+(?:-[\p{L}]+)*").unwrap());
    // Vocabulary of the document, counting each hyphenated token whole and
    // by its parts — except the tokens that are line-break candidates.
    // Words split across a line break are not evidence for themselves:
    // "incre-⏎mentally" must not count "incre" and "mentally" as words.
    static LINE_BREAK_PAIR: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
        regex::Regex::new(r"[\p{L}]+-[ \t]*\n[ \t]*\p{Ll}[\p{L}]*").unwrap()
    });
    let vocab_text = LINE_BREAK_PAIR.replace_all(md, " ");
    let mut vocab: HashMap<String, usize> = HashMap::new();
    for line in vocab_text.lines() {
        for m in WORD_RE.find_iter(line) {
            let w = m.as_str().to_lowercase();
            if let Some((l, r)) = w.rsplit_once('-')
                && hyphen_breaks
                    .contains(&(l.rsplit('-').next().unwrap_or(l).to_string(), r.to_string()))
            {
                continue;
            }
            for part in w.split('-') {
                if part.chars().count() > 1 {
                    *vocab.entry(part.to_string()).or_default() += 1;
                }
            }
            *vocab.entry(w).or_default() += 1;
        }
    }
    let count = |w: &str| vocab.get(w).copied().unwrap_or(0);
    let seen = |w: &str| count(w) > 0;
    // A right half that ends three or more other words of the document is a
    // suffix fragment ("tions": applications, operations, …), not a word in
    // its own right, even when OCR has left it standing alone somewhere.
    let suffix_fragment = |right: &str| {
        right.chars().count() >= 3
            && vocab
                .keys()
                .filter(|w| w.len() >= right.len() + 3 && w.ends_with(right) && !w.contains('-'))
                .take(3)
                .count()
                >= 3
    };

    let mut out = String::with_capacity(md.len());
    let mut in_code = false;
    // Process paragraph-sized chunks so a match can span a single newline but
    // never a code fence or table row.
    let mut buf = String::new();
    let flush = |buf: &mut String, out: &mut String| {
        if buf.is_empty() {
            return;
        }
        let fixed = BREAK_RE.replace_all(buf, |caps: &regex::Captures| {
            let left = &caps[1];
            let pre = &caps[2];
            let space = &caps[3];
            let right = &caps[4];
            let last_part = left.rsplit('-').next().unwrap_or(left);
            let key = (last_part.to_lowercase(), right.to_lowercase());
            let geometric = hyphen_breaks.contains(&key);
            // A spaced hyphen ("a - b") is a dash unless geometry says it was
            // a line break; an in-line hyphen is a compound unless geometry
            // says so too.
            if !geometric && (!pre.is_empty() || !space.contains('\n')) {
                return caps[0].to_string();
            }
            let joined = format!("{}{}", key.0, key.1);
            let hyphenated = format!("{}-{}", key.0, key.1);
            let keep_hyphen = !seen(&joined)
                && (seen(&hyphenated)
                    || left.contains('-')
                    || (key.0.chars().count() >= 3
                        && key.1.chars().count() >= 3
                        && seen(&key.0)
                        && seen(&key.1)
                        && !suffix_fragment(&key.1)));
            if keep_hyphen {
                format!("{}-{}", left, right)
            } else {
                format!("{}{}", left, right)
            }
        });
        out.push_str(&join_split_words(&fixed, hyphen_breaks, &count));
        buf.clear();
    };
    for line in md.split_inclusive('\n') {
        let t = line.trim();
        if t.starts_with("```") {
            flush(&mut buf, &mut out);
            in_code = !in_code;
            out.push_str(line);
            continue;
        }
        if in_code || t.starts_with('|') || t.is_empty() {
            flush(&mut buf, &mut out);
            out.push_str(line);
            continue;
        }
        buf.push_str(line);
    }
    flush(&mut buf, &mut out);
    out
}

/// Splits capitals the text layer ran together ("CONSOLIDATEDBALANCE
/// SHEETS", "OFINDEPENDENTREGISTEREDPUBLICACCOUNTINGFIRM"): letter-spaced
/// headings are often positioned glyph by glyph with no space characters.
///
/// The document's own vocabulary decides, never a word list: a run of 12 or
/// more capitals that is not itself a word of the document is split only
/// when it divides completely into words that occur elsewhere in the
/// document, using as few words as possible, none shorter than two letters,
/// and into three or more words (or two, for runs of 18+ letters: shorter
/// two-piece runs may be one real word, "UNDER|STANDING").
pub(crate) fn split_glued_capitals(md: &str) -> String {
    static GLUED_RE: std::sync::LazyLock<regex::Regex> =
        std::sync::LazyLock::new(|| regex::Regex::new(r"\b[A-Z][A-Z’']{11,}\b").unwrap());
    static WORD_RE: std::sync::LazyLock<regex::Regex> =
        std::sync::LazyLock::new(|| regex::Regex::new(r"[\p{L}’']+").unwrap());
    if !GLUED_RE.is_match(md) {
        return md.to_string();
    }
    let mut vocab: HashSet<String> = HashSet::new();
    for m in WORD_RE.find_iter(md) {
        let w = m.as_str();
        // Glued runs are not evidence for themselves.
        if w.chars().count() >= 12 && w.chars().all(|c| c.is_uppercase() || c == '’' || c == '\'')
        {
            continue;
        }
        if w.chars().count() >= 2 {
            vocab.insert(w.to_lowercase());
        }
    }
    GLUED_RE
        .replace_all(md, |c: &regex::Captures| {
            let run = &c[0];
            let lower = run.to_lowercase();
            if vocab.contains(&lower) {
                return run.to_string();
            }
            segment(&lower, &vocab)
                .map(|cuts| {
                    let chars: Vec<char> = run.chars().collect();
                    let mut out = String::new();
                    let mut start = 0;
                    for end in cuts {
                        if start > 0 {
                            out.push(' ');
                        }
                        out.extend(&chars[start..end]);
                        start = end;
                    }
                    out
                })
                .unwrap_or_else(|| run.to_string())
        })
        .into_owned()
}

/// Fewest-word split of `s` into words of `vocab` (char end offsets).
fn segment(s: &str, vocab: &HashSet<String>) -> Option<Vec<usize>> {
    let chars: Vec<char> = s.chars().collect();
    let n = chars.len();
    // best[i]: fewest words covering chars[..i], and the previous cut.
    let mut best: Vec<Option<(usize, usize)>> = vec![None; n + 1];
    best[0] = Some((0, 0));
    for end in 2..=n {
        for start in 0..=end - 2 {
            let Some((count, _)) = best[start] else {
                continue;
            };
            let piece: String = chars[start..end].iter().collect();
            if vocab.contains(&piece) && best[end].is_none_or(|(c, _)| count + 1 < c) {
                best[end] = Some((count + 1, start));
            }
        }
    }
    best[n]?;
    let mut cuts = Vec::new();
    let mut at = n;
    while at > 0 {
        cuts.push(at);
        at = best[at]?.1;
    }
    cuts.reverse();
    // Two pieces could be one real word ("UNDER|STANDING"); English words
    // of 18+ letters are rare, so two pieces need that length.
    (cuts.len() >= 3 || (cuts.len() == 2 && n >= 18)).then_some(cuts)
}

/// Rejoins words that reached the text split in two (`limita tions`,
/// `represen tation`) — converters and OCR text layers drop line-end hyphens
/// or break words around a gutter. Two kinds of evidence, never a word list:
///
/// * **geometry**: the pair was recorded as a hyphenated line break; or
/// * **the document's own vocabulary**: the joined word occurs elsewhere in
///   the document more often than either fragment does on its own — which a
///   real two-word phrase ("in to", "every one", "the rapist") never shows,
///   because its parts are common words.
fn join_split_words(
    text: &str,
    hyphen_breaks: &HashSet<(String, String)>,
    count: &dyn Fn(&str) -> usize,
) -> String {
    static PAIR_RE: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
        regex::Regex::new(r"(\p{L}+)([ \t]+\n?[ \t]*|\n[ \t]*)(\p{Ll}\p{L}*)").unwrap()
    });
    let mut out = String::with_capacity(text.len());
    let mut pos = 0;
    while let Some(c) = PAIR_RE.captures_at(text, pos) {
        let whole = c.get(0).unwrap();
        let (l, r) = (&c[1], &c[3]);
        let key = (l.to_lowercase(), r.to_lowercase());
        let joined = format!("{}{}", key.0, key.1);
        // The pair set is document-wide, so geometry only vouches for a
        // split that also *looks* like a dropped line break: a newline or a
        // widened gap. "to the" with one space is just two words.
        let gap = &c[2];
        let break_shaped =
            gap.contains('\n') || gap.chars().filter(|c| *c == ' ' || *c == '\t').count() >= 2;
        let geometric = break_shaped
            && hyphen_breaks.contains(&key)
            && count(&format!("{}-{}", key.0, key.1)) == 0;
        let lexical = key.0.chars().count() >= 2
            && key.1.chars().count() >= 2
            && joined.chars().count() >= 6
            && count(&joined) >= 2
            && count(&key.0) < count(&joined)
            && count(&key.1) < count(&joined);
        if geometric || lexical {
            out.push_str(&text[pos..whole.start()]);
            out.push_str(l);
            out.push_str(r);
            pos = whole.end();
        } else {
            // Advance past the left word only, so "a b c" also tries "b c".
            let next = c.get(2).unwrap().start();
            out.push_str(&text[pos..next]);
            pos = next;
        }
    }
    out.push_str(&text[pos..]);
    out
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

        out.push(squeeze_spaces(raw_line));
    }

    if in_table {
        flush_table(&mut out, &table_rows);
    }

    collapse_blank_lines(&out)
}

/// Emits a table in the project's single pipe-table dialect (`| a | b |`,
/// `|---|`) without changing any cell text: cell padding is normalised, the
/// delimiter row matches the column count, and rows or columns with no
/// content at all are dropped. A block without a delimiter row passes through.
fn flush_table(out: &mut Vec<String>, rows: &[String]) {
    if rows.len() < 2 || !is_table_separator(&rows[1]) {
        out.extend(rows.iter().cloned());
        return;
    }
    let header = split_cells(&rows[0]);
    let width = header.len();
    if width == 0 {
        out.extend(rows.iter().cloned());
        return;
    }
    let delim: Vec<String> = split_cells(&rows[1])
        .into_iter()
        .map(|c| {
            let (l, r) = (c.starts_with(':'), c.ends_with(':') && c.len() > 1);
            format!(
                "{}---{}",
                if l { ":" } else { "" },
                if r { ":" } else { "" }
            )
        })
        .collect();
    let mut body: Vec<Vec<String>> = rows[2..]
        .iter()
        .map(|r| split_cells(r))
        .filter(|cells| cells.iter().any(|c| !c.is_empty()))
        .collect();
    for r in body.iter_mut() {
        r.resize(width.max(r.len()), String::new());
    }
    let width = body.iter().map(|r| r.len()).max().unwrap_or(0).max(width);
    // Columns that are empty in every row, header included.
    let keep: Vec<bool> = (0..width)
        .map(|i| {
            header.get(i).is_some_and(|c| !c.is_empty())
                || body.iter().any(|r| r.get(i).is_some_and(|c| !c.is_empty()))
        })
        .collect();
    if !keep.iter().any(|k| *k) {
        return;
    }
    let render = |cells: &[String]| {
        let kept: Vec<&str> = (0..width)
            .filter(|i| keep[*i])
            .map(|i| cells.get(i).map(String::as_str).unwrap_or(""))
            .collect();
        format!("| {} |", kept.join(" | "))
    };
    let mut head = header.clone();
    head.resize(width, String::new());
    out.push(render(&head));
    let mut d = delim;
    d.resize(width, "---".to_string());
    let d_kept: Vec<&str> = (0..width)
        .filter(|i| keep[*i])
        .map(|i| d[i].as_str())
        .collect();
    out.push(format!("|{}|", d_kept.join("|")));
    for r in &body {
        out.push(render(r));
    }
}

/// Splits a pipe-table row into trimmed cells, honouring `\|` escapes.
fn split_cells(row: &str) -> Vec<String> {
    let t = row.trim();
    let inner = t.strip_prefix('|').unwrap_or(t);
    let inner = inner.strip_suffix('|').unwrap_or(inner);
    let mut cells = Vec::new();
    let mut cur = String::new();
    let mut escaped = false;
    for c in inner.chars() {
        if escaped {
            cur.push(c);
            escaped = false;
        } else if c == '\\' {
            cur.push(c);
            escaped = true;
        } else if c == '|' {
            cells.push(cur.trim().to_string());
            cur.clear();
        } else {
            cur.push(c);
        }
    }
    cells.push(cur.trim().to_string());
    cells
}

/// Collapses runs of spaces inside a prose line to one space (the converter
/// often emits two), keeping leading indentation and dropping trailing blanks.
fn squeeze_spaces(line: &str) -> String {
    let indent = line.len() - line.trim_start().len();
    let mut out = String::with_capacity(line.len());
    out.push_str(&line[..indent]);
    let mut prev_space = false;
    for c in line[indent..].trim_end().chars() {
        if c == ' ' || c == '\t' {
            if !prev_space {
                out.push(' ');
            }
            prev_space = true;
        } else {
            out.push(c);
            prev_space = false;
        }
    }
    out
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

/// Unwraps fenced blocks with no language tag whose content reads as prose.
/// `pdf_oxide` fences text it sees as preformatted (monospace or deeply
/// indented), which also catches indented legal and claim paragraphs.
fn unwrap_false_prose_code_blocks(md: &str) -> String {
    let mut out: Vec<String> = Vec::new();
    let lines: Vec<&str> = md.lines().collect();
    // When most of the text is fenced, monospace is simply the document's
    // font (a typewritten page, a patent's OCR text layer): any block
    // without code syntax is text.
    let (mut fenced, mut total, mut inside) = (0usize, 0usize, false);
    for l in &lines {
        if l.trim_start().starts_with("```") {
            inside = !inside;
            continue;
        }
        let n = l.trim().len();
        total += n;
        if inside {
            fenced += n;
        }
    }
    let monospace_document = total > 0 && fenced * 2 >= total;
    let mut i = 0;
    while i < lines.len() {
        let trimmed = lines[i].trim();
        if let Some(rest) = trimmed.strip_prefix("```") {
            let has_lang = !rest.trim().is_empty();
            let mut j = i + 1;
            while j < lines.len() && !lines[j].trim().starts_with("```") {
                j += 1;
            }
            if j < lines.len() && !has_lang {
                let block_lines = &lines[i + 1..j];
                if looks_like_prose(block_lines)
                    || (monospace_document && !has_code_syntax(block_lines))
                    || is_short_plain_text(block_lines)
                {
                    for &bl in block_lines {
                        out.push(bl.trim().to_string());
                    }
                    i = j + 1;
                    continue;
                }
            }
            // A block that stays code is copied whole, so its closing fence
            // is never read as the opening of the next block (which paired
            // one block's close with the next one's open, unwrapped the text
            // between them and left a fence unmatched to the end).
            if j < lines.len() {
                out.extend(lines[i..=j].iter().map(|l| l.to_string()));
                i = j + 1;
                continue;
            }
        }
        out.push(lines[i].to_string());
        i += 1;
    }
    out.join("\n")
}

/// Prose, not code: mostly dictionary-shaped words, very few code symbols, and
/// sentences that end in punctuation.
/// Symbols code is made of, at more than 1% of the characters, or indented
/// lines (code structure).
fn has_code_syntax(lines: &[&str]) -> bool {
    let text = lines.join(" ");
    let chars = text.chars().filter(|c| !c.is_whitespace()).count().max(1);
    let syms = text
        .chars()
        .filter(|c| {
            matches!(
                c,
                '{' | '}' | ';' | '=' | '<' | '>' | '[' | ']' | '_' | '$' | '#' | '&' | '(' | ')'
            )
        })
        .count();
    let indented = lines
        .iter()
        .any(|l| l.starts_with("  ") || l.starts_with('\t'));
    syms * 100 > chars * 3 || indented && syms > 0
}

/// A few words and figures with no code symbols at all ("12 United States
/// Patent", "65"): a fence adds nothing.
fn is_short_plain_text(lines: &[&str]) -> bool {
    let text = lines.join(" ");
    let words = text.split_whitespace().count();
    words > 0
        && words < 10
        && lines.len() <= 3
        && !lines
            .iter()
            .any(|l| l.starts_with(' ') || l.starts_with('\t'))
        && !text.chars().any(|c| {
            matches!(
                c,
                '{' | '}'
                    | ';'
                    | '='
                    | '<'
                    | '>'
                    | '['
                    | ']'
                    | '_'
                    | '$'
                    | '#'
                    | '&'
                    | '('
                    | ')'
                    | '/'
                    | '\\'
                    | '"'
                    | '`'
            )
        })
}

fn looks_like_prose(lines: &[&str]) -> bool {
    let text = lines.join(" ");
    let words: Vec<&str> = text.split_whitespace().collect();
    if words.len() < 10 {
        return false;
    }
    let wordlike = words
        .iter()
        .filter(|w| {
            let core = w.trim_matches(|c: char| !c.is_alphanumeric());
            !core.is_empty()
                && core
                    .chars()
                    .all(|c| c.is_alphabetic() || c == '\'' || c == '-')
        })
        .count();
    let chars = text.chars().filter(|c| !c.is_whitespace()).count().max(1);
    let code_syms = text
        .chars()
        .filter(|c| {
            matches!(
                c,
                '{' | '}' | ';' | '=' | '<' | '>' | '[' | ']' | '_' | '$' | '#' | '&'
            )
        })
        .count();
    let has_sentence_end = text.contains(". ") || text.trim_end().ends_with('.');
    wordlike * 100 >= words.len() * 75 && code_syms * 100 <= chars && has_sentence_end
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
        // Without a '#' marker only an all-caps short label qualifies; a
        // title-case line is too often the start of a wrapped sentence
        // ("Part of the problem is…").
        let is_caps = plain == plain.to_uppercase();
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

/// "Chapter 3" or "PART TWO": a label with no title of its own.
fn is_bare_section_label(s: &str) -> bool {
    s.split_whitespace().count() <= 2
}

/// Drops a lone lowercase letter on its own line next to a blank line — the
/// debris of subscripts and superscripts that the converter set apart from
/// their formula (`k`, `v`, `i`). Code and display math are left alone, and no
/// digits or words are ever removed.
pub fn prune_isolated_letter_lines(text: &str) -> String {
    let mut result: Vec<&str> = Vec::new();
    let mut in_code_fence = false;
    let mut in_math_block = false;

    let lines: Vec<&str> = text.lines().collect();
    for (i, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        if trimmed.starts_with("```") {
            in_code_fence = !in_code_fence;
            result.push(line);
            continue;
        }
        if trimmed.starts_with("$$") {
            if trimmed == "$$" || (trimmed.ends_with("$$") && trimmed.len() > 2) {
                in_math_block = !in_math_block;
            }
            result.push(line);
            continue;
        }
        if !in_code_fence
            && !in_math_block
            && trimmed.len() == 1
            && trimmed.chars().all(|c| c.is_ascii_lowercase())
        {
            let prev_blank = i == 0 || lines[i - 1].trim().is_empty();
            let next_blank = i + 1 >= lines.len() || lines[i + 1].trim().is_empty();
            if prev_blank || next_blank {
                continue;
            }
        }
        result.push(line);
    }
    result.join("\n")
}

/// Largest char boundary at or below `i`.
fn floor_boundary(s: &str, mut i: usize) -> usize {
    i = i.min(s.len());
    while i > 0 && !s.is_char_boundary(i) {
        i -= 1;
    }
    i
}

/// Recovers GFM tables from vector ruling lines and spatial word clusters
/// (the ported pdfplumber algorithm) and splices them into the page markdown
/// in place of the converter's rendering of the same table.
fn recover_page_vector_tables(
    doc: &PdfDocument,
    page: usize,
    mut page_md: String,
    geom: Option<&PageGeom>,
    excluded: &[Rect],
) -> String {
    let Ok(paths) = doc.extract_paths(page) else {
        return page_md;
    };
    let Some(geom) = geom else {
        return page_md;
    };
    if paths.is_empty() || geom.words.is_empty() {
        return page_md;
    }

    // pdfplumber works top-down from the page's top edge.
    let page_top = geom.bx.3;
    let mut edges = Vec::new();
    for p in &paths {
        let top = page_top - (p.bbox.y + p.bbox.height);
        let bottom = page_top - p.bbox.y;
        if p.is_horizontal_line(3.0) || (p.bbox.height <= 2.5 && p.bbox.width >= 10.0) {
            edges.push(crate::parsers::pdf_table::Edge::horizontal(
                p.bbox.x,
                p.bbox.x + p.bbox.width,
                top,
            ));
        } else if p.is_vertical_line(3.0) || (p.bbox.width <= 2.5 && p.bbox.height >= 10.0) {
            edges.push(crate::parsers::pdf_table::Edge::vertical(
                p.bbox.x, top, bottom,
            ));
        } else if p.is_rectangle() && p.bbox.width >= 15.0 && p.bbox.height >= 10.0 {
            edges.push(crate::parsers::pdf_table::Edge::horizontal(
                p.bbox.x,
                p.bbox.x + p.bbox.width,
                top,
            ));
            edges.push(crate::parsers::pdf_table::Edge::horizontal(
                p.bbox.x,
                p.bbox.x + p.bbox.width,
                bottom,
            ));
            edges.push(crate::parsers::pdf_table::Edge::vertical(
                p.bbox.x, top, bottom,
            ));
            edges.push(crate::parsers::pdf_table::Edge::vertical(
                p.bbox.x + p.bbox.width,
                top,
                bottom,
            ));
        }
    }

    if edges.is_empty() {
        return page_md;
    }

    let pdf_words: Vec<crate::parsers::pdf_table::PdfWord> = geom
        .words
        .iter()
        .filter(|w| !pdf_furniture::excluded(excluded, w.x0, w.y0, w.x1, w.y1))
        .map(|w| {
            crate::parsers::pdf_table::PdfWord::new(
                w.text.clone(),
                w.x0,
                page_top - w.y1,
                w.x1,
                page_top - w.y0,
            )
        })
        .collect();

    let settings = crate::parsers::pdf_table::TableSettings::default();
    let tables = crate::parsers::pdf_table::extract_tables_from_edges_and_words(
        &edges, &pdf_words, &settings,
    );

    let word_gap = median_word_gap(geom);
    for table in tables {
        if table.rows.len() < 2 || table.rows.iter().all(|r| r.len() < 2) {
            continue;
        }
        if !table_columns_are_real(&table, &edges, &pdf_words, word_gap) {
            continue;
        }
        if table.rows.first().map(|r| r.len()).unwrap_or(0) > 12 {
            continue;
        }
        let non_empty_cells: usize = table
            .rows
            .iter()
            .flat_map(|r| r.iter())
            .filter(|c| !c.text.trim().is_empty())
            .count();
        if non_empty_cells < 2 {
            continue;
        }
        let table_md = table.to_markdown().trim().to_string();
        if table_md.is_empty() || page_md.contains(&table_md) {
            continue;
        }

        let sample_words: Vec<&str> = table
            .rows
            .iter()
            .flat_map(|r| r.iter())
            .map(|c| c.text.trim())
            .filter(|s| s.len() >= 3)
            .collect();

        let table_tokens: HashSet<String> = table
            .rows
            .iter()
            .flat_map(|r| r.iter())
            .flat_map(|c| c.text.split_whitespace())
            .map(|w| {
                w.trim_matches(|ch: char| !ch.is_alphanumeric())
                    .to_lowercase()
            })
            .filter(|s| !s.is_empty())
            .collect();

        // Case 1: the converter already emitted a pipe table for this region —
        // replace it with the ruling-line version.
        if let Some((start, end)) = find_pipe_table_span(&page_md, &sample_words) {
            let existing_table = &page_md[start..end];
            if existing_table.trim() != table_md.trim() {
                let mut new_md = String::with_capacity(page_md.len() + table_md.len());
                new_md.push_str(&page_md[..start]);
                new_md.push_str(&table_md);
                new_md.push('\n');
                new_md.push_str(&page_md[end..]);
                page_md = new_md;
            }
            continue;
        }

        // Case 2: the table's text came out as loose lines — find the span
        // from its first header cell to its last cell and replace it, keeping
        // any headings and genuine prose lines found inside that span.
        let first_header = table.rows[0].first().map(|c| c.text.trim()).unwrap_or("");
        let last_cell = table
            .rows
            .last()
            .and_then(|r| r.last())
            .map(|c| c.text.trim())
            .unwrap_or("");

        let max_table_char_span = table.rows.len() * 250 + 500;
        let mut replaced = false;

        if !first_header.is_empty() && !last_cell.is_empty() {
            // The first occurrence of the header that is followed by the last
            // cell within the table's plausible span.
            let span = page_md
                .match_indices(first_header)
                .find_map(|(start_idx, _)| {
                    let window_end =
                        floor_boundary(&page_md, start_idx + max_table_char_span).max(start_idx);
                    page_md[start_idx..window_end]
                        .rfind(last_cell)
                        .map(|rel_end| (start_idx, start_idx + rel_end + last_cell.len()))
                });
            if let Some((start_idx, end_idx)) = span {
                let line_start = page_md[..start_idx].rfind('\n').map(|i| i + 1).unwrap_or(0);
                let line_end = page_md[end_idx..]
                    .find('\n')
                    .map(|i| end_idx + i)
                    .unwrap_or(page_md.len());

                let existing_chunk = &page_md[line_start..line_end];
                let line_count = existing_chunk.lines().count();

                if line_count <= table.rows.len() * 3 + 4 {
                    let mut headings: Vec<&str> = Vec::new();
                    let mut prose_paragraphs: Vec<String> = Vec::new();
                    let mut current_p = Vec::new();
                    for l in existing_chunk.lines() {
                        let trimmed = l.trim();
                        if trimmed.starts_with('#') {
                            headings.push(trimmed);
                            continue;
                        }
                        if trimmed.is_empty() {
                            if !current_p.is_empty() {
                                prose_paragraphs.push(current_p.join(" "));
                                current_p.clear();
                            }
                            continue;
                        }
                        let words: Vec<&str> = trimmed.split_whitespace().collect();
                        let non_table = words
                            .iter()
                            .filter(|w| {
                                let clean = w
                                    .trim_matches(|c: char| !c.is_alphanumeric())
                                    .to_lowercase();
                                !clean.is_empty() && !table_tokens.contains(&clean)
                            })
                            .count();
                        if non_table * 2 > words.len() {
                            current_p.push(trimmed);
                        }
                    }
                    if !current_p.is_empty() {
                        prose_paragraphs.push(current_p.join(" "));
                    }

                    let mut new_md = String::with_capacity(page_md.len() + table_md.len());
                    new_md.push_str(&page_md[..line_start]);
                    for h in &headings {
                        new_md.push_str(h);
                        new_md.push_str("\n\n");
                    }
                    new_md.push_str(&table_md);
                    for p in &prose_paragraphs {
                        new_md.push_str("\n\n");
                        new_md.push_str(p);
                    }
                    new_md.push_str(&page_md[line_end..]);
                    page_md = new_md;
                    replaced = true;
                }
            }
        }

        if !replaced {
            // Only append if the table's words are not already predominantly present on the page
            let matching_words = sample_words.iter().filter(|w| page_md.contains(*w)).count();
            if matching_words < sample_words.len() / 2 {
                if !page_md.is_empty() {
                    page_md.push_str("\n\n");
                }
                page_md.push_str(&table_md);
            }
        }
    }

    page_md
}

/// Median horizontal gap between consecutive words on a line: the page's
/// word spacing.
fn median_word_gap(geom: &PageGeom) -> f32 {
    let mut gaps: Vec<f32> = geom
        .lines
        .iter()
        .flat_map(|l| l.words.windows(2).map(|w| w[1].x0 - w[0].x1))
        .filter(|g| *g > 0.0)
        .collect();
    if gaps.is_empty() {
        return 2.5;
    }
    gaps.sort_by(f32::total_cmp);
    gaps[gaps.len() / 2]
}

/// A table recovered from page geometry is kept only if its columns are
/// real: every interior column boundary is either drawn (a vertical ruling
/// line spanning most of the table) or a gap clearly wider than the page's
/// word spacing. Otherwise the "columns" are just the spaces between words of
/// ordinary lines — e.g. bullets on a slide whose background rectangles made
/// the extractor look for a grid.
fn table_columns_are_real(
    table: &crate::parsers::pdf_table::ExtractedTable,
    edges: &[crate::parsers::pdf_table::Edge],
    words: &[crate::parsers::pdf_table::PdfWord],
    word_gap: f32,
) -> bool {
    use crate::parsers::pdf_table::EdgeOrientation;
    let t = &table.bbox;
    let ruled = |x: f32| {
        edges.iter().any(|e| {
            e.orientation == EdgeOrientation::Vertical
                && (e.x0 - x).abs() <= 3.0
                && (e.bottom.min(t.bottom) - e.top.max(t.top)) >= 0.6 * (t.bottom - t.top)
        })
    };
    let mut unruled_gaps: Vec<f32> = Vec::new();
    for row in &table.rows {
        let filled: Vec<&crate::parsers::pdf_table::TableCell> =
            row.iter().filter(|c| !c.text.trim().is_empty()).collect();
        for pair in filled.windows(2) {
            let (a, b) = (&pair[0].bbox, &pair[1].bbox);
            if ruled(a.x1) || ruled(b.x0) {
                continue;
            }
            let inside = |w: &&crate::parsers::pdf_table::PdfWord,
                          c: &crate::parsers::pdf_table::CellBbox| {
                let (mx, my) = w.midpoint();
                mx >= c.x0 && mx <= c.x1 && my >= c.top && my <= c.bottom
            };
            let left_end = words
                .iter()
                .filter(|w| inside(w, a))
                .map(|w| w.x1)
                .fold(f32::NEG_INFINITY, f32::max);
            let right_start = words
                .iter()
                .filter(|w| inside(w, b))
                .map(|w| w.x0)
                .fold(f32::INFINITY, f32::min);
            if left_end.is_finite() && right_start.is_finite() {
                unruled_gaps.push(right_start - left_end);
            }
        }
    }
    if unruled_gaps.is_empty() {
        return true;
    }
    unruled_gaps.sort_by(f32::total_cmp);
    unruled_gaps[unruled_gaps.len() / 2] >= 2.5 * word_gap.max(1.0)
}

/// Locates the byte span of a pipe table block in markdown containing matching words.
fn find_pipe_table_span(text: &str, target_words: &[&str]) -> Option<(usize, usize)> {
    let mut in_table = false;
    let mut block_start = 0;
    let mut block_end = 0;
    let mut best_span = None;
    let mut best_matches = 0;

    let mut current_offset = 0;
    for line in text.split_inclusive('\n') {
        let line_len = line.len();
        let trimmed = line.trim();
        let is_pipe_row = trimmed.starts_with('|') && trimmed.ends_with('|');

        if is_pipe_row {
            if !in_table {
                in_table = true;
                block_start = current_offset;
            }
            block_end = current_offset + line_len;
        } else if in_table && !trimmed.is_empty() {
            in_table = false;
            let block = &text[block_start..block_end];
            let matches = target_words
                .iter()
                .filter(|&&w| !w.is_empty() && block.contains(w))
                .count();
            if matches >= 2 && matches > best_matches {
                best_matches = matches;
                best_span = Some((block_start, block_end));
            }
        }
        current_offset += line_len;
    }

    if in_table {
        let block = &text[block_start..block_end];
        let matches = target_words
            .iter()
            .filter(|&&w| !w.is_empty() && block.contains(w))
            .count();
        if matches >= 2 && matches > best_matches {
            best_span = Some((block_start, block_end));
        }
    }

    best_span
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_sentence_continues_across_a_page_footnote() {
        let pages = vec![
            "Body text that runs on to the\n\n*Corresponding author. e-mail: a@b.edu".to_string(),
            "next page and ends here.\n\nNew paragraph.".to_string(),
        ];
        assert_eq!(
            join_pages_healing_dangling(&pages),
            "Body text that runs on to the next page and ends here.\n\n*Corresponding author. e-mail: a@b.edu\n\nNew paragraph."
        );
    }
    use clap::Parser;

    #[test]
    fn test_extract_paths_and_words_on_adversarial() {
        let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
        let p = Path::new(&manifest_dir)
            .join("../../test_assets/gauntlet/tables/adversarial_ruling_table.pdf");
        let doc = PdfDocument::open(&p).unwrap();
        let paths = doc.extract_paths(0).unwrap();
        let words = doc.extract_words(0).unwrap();
        let page_height = doc.get_page_media_box(0).map(|m| m.3).unwrap_or(792.0);
        let mut edges = Vec::new();
        for p in &paths {
            let top = page_height - (p.bbox.y + p.bbox.height);
            let bottom = page_height - p.bbox.y;
            if p.is_horizontal_line(3.0) || (p.bbox.height <= 2.5 && p.bbox.width >= 10.0) {
                edges.push(crate::parsers::pdf_table::Edge::horizontal(
                    p.bbox.x,
                    p.bbox.x + p.bbox.width,
                    top,
                ));
            } else if p.is_vertical_line(3.0) || (p.bbox.width <= 2.5 && p.bbox.height >= 10.0) {
                edges.push(crate::parsers::pdf_table::Edge::vertical(
                    p.bbox.x, top, bottom,
                ));
            }
        }
        let pdf_words: Vec<crate::parsers::pdf_table::PdfWord> = words
            .iter()
            .map(|w| {
                let top = page_height - (w.bbox.y + w.bbox.height);
                let bottom = page_height - w.bbox.y;
                crate::parsers::pdf_table::PdfWord::new(
                    w.text.clone(),
                    w.bbox.x,
                    top,
                    w.bbox.x + w.bbox.width,
                    bottom,
                )
            })
            .collect();
        let settings = crate::parsers::pdf_table::TableSettings::default();
        let tables = crate::parsers::pdf_table::extract_tables_from_edges_and_words(
            &edges, &pdf_words, &settings,
        );
        assert_eq!(tables.len(), 1);
        let md = tables[0].to_markdown();
        assert!(md.contains("Vanguard Total Stock Market Index Fund Institutional"));
        assert!(!md.contains("|  | Institutional |"));
        assert!(!md.contains("|  | Liquidity Fund |"));
    }

    #[test]
    fn test_extract_paths_and_words_on_financial_report() {
        let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
        let p = Path::new(&manifest_dir).join("../../test_assets/financial_report.pdf");
        let doc = PdfDocument::open(&p).unwrap();
        let paths = doc.extract_paths(0).unwrap();
        let words = doc.extract_words(0).unwrap();
        assert!(!paths.is_empty());
        assert!(!words.is_empty());

        let page_height = doc.get_page_media_box(0).map(|m| m.3).unwrap_or(792.0);
        let mut edges = Vec::new();
        for p in &paths {
            let top = page_height - (p.bbox.y + p.bbox.height);
            let bottom = page_height - p.bbox.y;
            if p.is_horizontal_line(3.0) || (p.bbox.height <= 2.5 && p.bbox.width >= 10.0) {
                edges.push(crate::parsers::pdf_table::Edge::horizontal(
                    p.bbox.x,
                    p.bbox.x + p.bbox.width,
                    top,
                ));
            } else if p.is_vertical_line(3.0) || (p.bbox.width <= 2.5 && p.bbox.height >= 10.0) {
                edges.push(crate::parsers::pdf_table::Edge::vertical(
                    p.bbox.x, top, bottom,
                ));
            }
        }
        let pdf_words: Vec<crate::parsers::pdf_table::PdfWord> = words
            .iter()
            .map(|w| {
                let top = page_height - (w.bbox.y + w.bbox.height);
                let bottom = page_height - w.bbox.y;
                crate::parsers::pdf_table::PdfWord::new(
                    w.text.clone(),
                    w.bbox.x,
                    top,
                    w.bbox.x + w.bbox.width,
                    bottom,
                )
            })
            .collect();
        let settings = crate::parsers::pdf_table::TableSettings::default();
        let tables = crate::parsers::pdf_table::extract_tables_from_edges_and_words(
            &edges, &pdf_words, &settings,
        );
        assert_eq!(tables.len(), 1);
        let md = tables[0].to_markdown();
        assert!(md.contains("| Date | Revenue | Expenses | Net Profit | Growth |"));
        assert!(md.contains("| 2026-01-01 | $10,000,000 | $8,000,000 | $2,000,000 | +5% |"));
    }

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
    fn test_prune_isolated_letter_lines() {
        let input = "Paragraph one.\n\nk\n\nParagraph two.\n\nv\n\nParagraph three.";
        let cleaned = prune_isolated_letter_lines(input);
        assert!(!cleaned.contains("\nk\n"));
        assert!(!cleaned.contains("\nv\n"));
        assert!(cleaned.contains("Paragraph one."));
        assert!(cleaned.contains("Paragraph two."));
        assert!(cleaned.contains("Paragraph three."));

        let code = "```python\nx\ny\n```";
        assert_eq!(prune_isolated_letter_lines(code), code);

        let math = "$$\nz\n$$";
        assert_eq!(prune_isolated_letter_lines(math), math);

        let inline = "This is a test of a single model.";
        assert_eq!(prune_isolated_letter_lines(inline), inline);
    }

    #[test]
    fn test_unwrap_false_prose_code_blocks() {
        let input = "Intro text.\n\n```\n1. A system comprising:\none or more computers;\nand one or more storage devices storing instructions that, when executed by the one or more computers, cause the one or more computers to perform operations.\n```\n\nOutro text.";
        let unwrapped = unwrap_false_prose_code_blocks(input);
        assert!(!unwrapped.contains("```"));
        assert!(unwrapped.contains("A system comprising:"));

        // Real code must NOT be unwrapped
        let real_code = "```rust\npub struct Node {\n    id: u32,\n}\n```";
        assert_eq!(unwrap_false_prose_code_blocks(real_code), real_code);
    }

    #[test]
    fn false_headings_are_demoted_but_real_ones_kept() {
        let md = "# 52,000\n# x = y + 1\n# continued from the previous page\n# FAQ\n# Item 7 - Management's Discussion\n# C++ Interop\n# Chapter 2";
        let out = suppress_false_headings(md);
        let lines: Vec<&str> = out.lines().collect();
        assert_eq!(lines[0], "52,000");
        assert_eq!(lines[1], "x = y + 1");
        assert_eq!(lines[2], "continued from the previous page");
        assert_eq!(lines[3], "# FAQ");
        assert_eq!(lines[4], "# Item 7 - Management's Discussion");
        assert_eq!(lines[5], "# C++ Interop");
        assert_eq!(lines[6], "# Chapter 2");
    }

    #[test]
    fn a_suffix_fragment_is_rejoined_even_when_ocr_left_it_alone() {
        // OCR split words elsewhere ("limita tions", "applica tions"), so both
        // halves look like words; "tions" ends many words, so it is a
        // fragment and the hyphen at the line end is a syllable break.
        let md = "limita tions apply. applications operations relations stations.\n\
                  these should not be construed as limita-\ntions on the scope";
        let out = dehyphenate_line_breaks(md, &HashSet::new());
        assert!(out.contains("as limitations on"), "{out}");
        // A real compound keeps its hyphen.
        let md2 = "a well known fact and the known world; it is well.\nthe well-\nknown result";
        let out2 = dehyphenate_line_breaks(md2, &HashSet::new());
        assert!(out2.contains("well-known"), "{out2}");
    }

    #[test]
    fn monospace_documents_and_short_plain_blocks_are_not_code() {
        let ocr = "```\n12 United States Patent\n```\n\n```\n54 ATTENTION BASED SEQUENCE TRANSDUCTION 71 Applicant Google LLC 72 Inventors 10 452 978\n```\n\nx";
        let out = unwrap_false_prose_code_blocks(ocr);
        assert!(!out.contains("```"), "{out}");
        // Real code stays fenced, also in a mostly-fenced document.
        let code = "```\nfn main() {\n    let x = vec![1, 2];\n}\n```";
        assert_eq!(unwrap_false_prose_code_blocks(code), code);
    }

    #[test]
    fn kept_code_blocks_do_not_pair_their_close_with_the_next_open() {
        let md = "```\nlet a = [0];\n```\n\nas one patent Oct on 2019 Sheet 2 of 3 and the text goes on here as prose.\n\n```\nlet b = {1};\n```\n\nmore";
        let out = unwrap_false_prose_code_blocks(md);
        assert_eq!(
            out.lines().filter(|l| l.starts_with("```")).count(),
            4,
            "{out}"
        );
        assert!(out.contains("```\nlet b = {1};\n```"), "{out}");
    }

    #[test]
    fn a_page_cannot_leave_a_code_fence_open() {
        let mut p = String::from("FIG. 2\n\n```\n230\n2001");
        close_open_fence(&mut p);
        assert!(p.ends_with("2001\n```"), "{p}");
        let mut q = String::from("```\nx\n```");
        close_open_fence(&mut q);
        assert_eq!(q, "```\nx\n```");
    }

    #[test]
    fn glued_capitals_split_on_the_documents_own_words() {
        let md = "CONSOLIDATEDBALANCE SHEETS\n\nThe consolidated balance shows sheets of equity. \
                  Report of the independent registered public accounting firm.\n\n\
                  REPORTOFINDEPENDENTREGISTEREDPUBLICACCOUNTINGFIRM\n\nCONTRAINDICATIONS apply.";
        let out = split_glued_capitals(md);
        assert!(out.starts_with("CONSOLIDATED BALANCE SHEETS"), "{out}");
        assert!(
            out.contains("REPORT OF INDEPENDENT REGISTERED PUBLIC ACCOUNTING FIRM"),
            "{out}"
        );
        // A real long word the document does not split elsewhere stays.
        assert!(out.contains("CONTRAINDICATIONS apply"), "{out}");
        let w = split_glued_capitals("UNDERSTANDING the under side and standing room");
        assert!(w.starts_with("UNDERSTANDING "), "{w}");
    }

    #[test]
    fn dehyphenation_uses_document_vocabulary() {
        let md = "Better infor-\nmation retrieval. Some information is free.\nA well-\nknown fact is well known.\nA state-of-the-\nart model.\nWe re-\nported it.";
        let out = dehyphenate_line_breaks(md, &HashSet::new());
        assert!(out.contains("Better information retrieval"), "{out}");
        assert!(out.contains("A well-known fact"), "{out}");
        assert!(out.contains("state-of-the-art model"), "{out}");
        assert!(out.contains("We reported it"), "{out}");
        // Code is never touched.
        let code = "```\nfoo-\nbar\n```";
        assert_eq!(dehyphenate_line_breaks(code, &HashSet::new()), code);
        // Glued by the converter: only geometric line-break pairs are joined.
        let glued = "The infor-mation age and a well-known co-author.";
        let breaks: HashSet<(String, String)> = [("infor".to_string(), "mation".to_string())]
            .into_iter()
            .collect();
        assert_eq!(
            dehyphenate_line_breaks(glued, &breaks),
            "The information age and a well-known co-author."
        );
    }

    #[test]
    fn page_join_heals_only_true_continuations() {
        let pages = vec![
            "The results were consistent with".to_string(),
            "the earlier study.".to_string(),
            "Figure 2: Results".to_string(),
            "**3 Method**".to_string(),
        ];
        let out = join_pages_healing_dangling(&pages);
        assert!(out.contains("consistent with the earlier study."));
        assert!(out.contains("Figure 2: Results\n\n**3 Method**"));
    }

    #[test]
    fn prose_is_never_rewritten() {
        // Every one of these was corrupted by an earlier regex pass.
        let md = "Take 5 mg orally for 30 days. Submit Form 1040 (2025). \
                  See 304 U. S. 64, 78 (1938). Revenue grew 12.5. 2023 was good. \
                  Call localStorage.getItem and parseInt. Table A 2 lists x 2 cases.";
        assert_eq!(tidy_markdown(md, &HashSet::new()), md);
    }

    #[test]
    fn real_code_block_is_not_unwrapped() {
        let real_code =
            "```\nfn main() {\n    let x = vec![1, 2, 3];\n    println!(\"{:?}\", x);\n}\n```";
        assert_eq!(unwrap_false_prose_code_blocks(real_code), real_code);
    }

    /// The synthetic survival corpus: numbers, headings and order survive while
    /// headers, footers and line-number gutters are removed.
    fn survival(name: &str) -> String {
        let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
        let p = Path::new(&manifest_dir)
            .join("../../test_assets/pdf_survival")
            .join(name);
        let args = crate::cli::Cli::parse_from(["filegoblin", "dummy"]);
        PdfGobbler.gobble(&p, &(&args).into()).unwrap()
    }

    fn flat(s: &str) -> String {
        s.replace("**", "")
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
    }

    #[test]
    fn survival_memo_keeps_headings_and_numbers_drops_furniture() {
        let out = survival("memo_a4_running_headers.pdf");
        let f = flat(&out);
        for n in 1..=6 {
            let h = format!("Section {n}: Overview of Operations");
            assert!(
                out.lines().any(|l| l.starts_with('#') && l.contains(&h)),
                "missing heading {h}"
            );
        }
        assert!(f.contains("take 5 mg orally twice daily for 30 days"));
        assert!(f.contains("Submit Form 1040 (2025) by April 15 and subtract line 10 from line 9"));
        assert!(f.contains("304 U.S. 64, 78 (1938)"));
        assert!(!f.contains("Page 3 of 6"));
        assert!(!f.contains("Confidential Draft"));
    }

    #[test]
    fn survival_line_number_gutters_removed() {
        let f = flat(&survival("pleading_line_numbers.pdf"));
        assert!(
            f.contains("deliver 25 units within 30 days of the order"),
            "{f}"
        );
        let f = flat(&survival("patent_two_column_gutter.pdf"));
        assert!(f.contains("a batch of 25 sequences in parallel"), "{f}");
        assert!(f.contains("Training proceeds for 15 epochs"), "{f}");
    }

    #[test]
    fn tables_are_compacted_without_changing_cells() {
        let md = "| Metric |  | 2023 | Note |\n|:---|---|---:|---|\n| Revenue |  | 1,200 | a \\| b |\n|  |  |  |  |\n| Margin |  | (3.4) |  |";
        let out = format_table_and_block_boundaries(md);
        assert_eq!(
            out,
            "| Metric | 2023 | Note |\n|:---|---:|---|\n| Revenue | 1,200 | a \\| b |\n| Margin | (3.4) |  |"
        );
        assert_eq!(squeeze_spaces("  a  b   c  "), "  a b c");
    }
}
