//! Page "furniture" detection for PDFs: running headers and footers, page
//! numbers (folios), and line-number gutters (legal pleading paper, patents).
//!
//! Furniture is decided **geometrically, before markdown exists**, and the
//! result is a list of rectangles per page that the converter excludes. That is
//! the whole point of this module: an earlier design tried to recognise
//! furniture in the finished markdown string with regexes and keyword lists,
//! which meant every heuristic was one bad match away from deleting real text
//! ("take 5 mg" losing its "5" because 5 is a multiple of five, a real section
//! heading deleted because it looked like a running header).
//!
//! The signals used here are layout facts that hold across documents:
//!
//! * **Position** — furniture lives in the outermost lines of a page, in a band
//!   near the top or bottom edge of the page box.
//! * **Recurrence** — a running header repeats on many pages (with its page
//!   number masked), either across the whole document or in a run of nearby
//!   pages (chapter titles on alternating pages).
//! * **Size** — running heads are set at or below body size; a line set larger
//!   than body text is a heading and is never furniture.
//! * **Numbering** — a folio is a lone number whose value advances with the
//!   page index; a stray "2023" on one cover page does not.
//! * **Alignment** — line numbers form a narrow vertical column of isolated
//!   integers that increase down the page in a constant step.

use pdf_oxide::PdfDocument;
use pdf_oxide::geometry::Rect;
use std::collections::HashMap;

/// A physical text line with the geometry the detectors need.
/// Coordinates are PDF user space (origin bottom-left).
#[derive(Debug, Clone)]
pub(crate) struct GLine {
    pub text: String,
    pub x0: f32,
    pub y0: f32,
    pub x1: f32,
    pub y1: f32,
    /// Largest word font size on the line.
    pub size: f32,
    /// The line's words, left to right.
    pub words: Vec<GWord>,
}

#[derive(Debug, Clone)]
pub(crate) struct GWord {
    pub text: String,
    pub x0: f32,
    pub y0: f32,
    pub x1: f32,
    pub y1: f32,
    pub size: f32,
}

/// Everything furniture detection needs to know about one page.
#[derive(Debug, Clone)]
pub(crate) struct PageGeom {
    /// Page box (llx, lly, urx, ury). Uses the MediaBox with its real origin.
    pub bx: (f32, f32, f32, f32),
    pub lines: Vec<GLine>,
    pub words: Vec<GWord>,
    /// Words the converter ran together although the page shows a visible gap
    /// between them: (`theUnionPacific`, `the Union Pacific`).
    pub glued: Vec<(String, String)>,
    /// Boxes (x0, y0, x1, y1) of tables the converter detects on this page;
    /// only computed when the page has enough numbers to hold a gutter.
    pub table_boxes: Vec<(f32, f32, f32, f32)>,
}

impl PageGeom {
    pub fn height(&self) -> f32 {
        (self.bx.3 - self.bx.1).max(1.0)
    }
}

/// Page box as (llx, lly, urx, ury), normalised so that ll < ur. Falls back to
/// the extent of the page's own text (padded) when the box is unreadable, which
/// is better than assuming US Letter for an A4 or slide-sized page.
pub(crate) fn page_box(doc: &PdfDocument, page: usize, words: &[GWord]) -> (f32, f32, f32, f32) {
    if let Ok((a, b, c, d)) = doc.get_page_media_box(page) {
        let (x0, x1) = if a <= c { (a, c) } else { (c, a) };
        let (y0, y1) = if b <= d { (b, d) } else { (d, b) };
        if x1 - x0 > 10.0 && y1 - y0 > 10.0 {
            return (x0, y0, x1, y1);
        }
    }
    let x0 = words.iter().map(|w| w.x0).fold(f32::INFINITY, f32::min);
    let y0 = words.iter().map(|w| w.y0).fold(f32::INFINITY, f32::min);
    let x1 = words.iter().map(|w| w.x1).fold(0.0f32, f32::max);
    let y1 = words.iter().map(|w| w.y1).fold(0.0f32, f32::max);
    if x0.is_finite() && y0.is_finite() && x1 > x0 && y1 > y0 {
        (x0 - 36.0, y0 - 36.0, x1 + 36.0, y1 + 36.0)
    } else {
        (0.0, 0.0, 612.0, 792.0)
    }
}

/// The part of the media box a viewer shows: its intersection with the
/// CropBox. A printer's PDF draws crop marks, colour bars and job slugs
/// ("TIS final pgs 6 … 5/2/09 10:37:35") outside the crop, where no reader
/// sees them. A CropBox that would hide most of the page is taken for a
/// mistake and ignored.
fn visible_part(
    doc: &PdfDocument,
    page: usize,
    media: (f32, f32, f32, f32),
) -> (f32, f32, f32, f32) {
    let Some([a, b, c, d]) = crop_box(doc, page) else {
        return media;
    };
    let (cx0, cx1) = (a.min(c), a.max(c));
    let (cy0, cy1) = (b.min(d), b.max(d));
    let v = (
        media.0.max(cx0),
        media.1.max(cy0),
        media.2.min(cx1),
        media.3.min(cy1),
    );
    let area = |b: (f32, f32, f32, f32)| (b.2 - b.0).max(0.0) * (b.3 - b.1).max(0.0);
    if area(v) >= 0.25 * area(media) {
        v
    } else {
        media
    }
}

/// The page's `/CropBox` (inherited from its page tree when set there).
fn crop_box(doc: &PdfDocument, page: usize) -> Option<[f32; 4]> {
    let page = doc.get_page(page).ok()?;
    let raw = page.as_dict()?.get("CropBox")?;
    let arr = doc.resolve_object(raw).ok()?;
    let arr = arr.as_array()?;
    if arr.len() < 4 {
        return None;
    }
    let mut v = [0f32; 4];
    for (slot, o) in v.iter_mut().zip(arr) {
        let o = doc.resolve_object(o).ok()?;
        *slot = o.as_real().or_else(|| o.as_integer().map(|i| i as f64))? as f32;
    }
    Some(v)
}

/// The parts of the media box outside what a viewer shows, as rectangles to
/// leave out of the conversion (see [`visible_part`]).
pub(crate) fn outside_visible(doc: &PdfDocument, page: usize) -> Vec<Rect> {
    let Ok((a, b, c, d)) = doc.get_page_media_box(page) else {
        return Vec::new();
    };
    if rotated(doc, page) {
        return Vec::new();
    }
    let media = (a.min(c), b.min(d), a.max(c), b.max(d));
    let (x0, y0, x1, y1) = visible_part(doc, page, media);
    let (mx0, my0, mx1, my1) = media;
    [
        (mx0, my0, mx1, y0), // below
        (mx0, y1, mx1, my1), // above
        (mx0, y0, x0, y1),   // left
        (x1, y0, mx1, y1),   // right
    ]
    .into_iter()
    .filter(|&(l, b, r, t)| r - l > 0.5 && t - b > 0.5)
    .map(|(l, b, r, t)| Rect::new(l, b, r - l, t - b))
    .collect()
}

/// The page has a `/Rotate` of 90, 180 or 270 degrees.
pub(crate) fn rotated(doc: &PdfDocument, page: usize) -> bool {
    doc.get_page_rotation(page)
        .is_ok_and(|r| r.rem_euclid(360) != 0)
}

/// Collects line and word geometry for one page.
pub(crate) fn collect_page_geom(doc: &PdfDocument, page: usize) -> Option<PageGeom> {
    let to_word = |w: &pdf_oxide::layout::Word| GWord {
        text: w.text.trim().to_string(),
        x0: w.bbox.x,
        y0: w.bbox.y,
        x1: w.bbox.x + w.bbox.width,
        y1: w.bbox.y + w.bbox.height,
        size: w.avg_font_size,
    };
    let words: Vec<GWord> = doc
        .extract_words(page)
        .ok()?
        .iter()
        .filter(|w| !w.text.trim().is_empty())
        .map(to_word)
        .collect();
    let lines: Vec<GLine> = doc
        .extract_text_lines(page)
        .ok()?
        .iter()
        .filter(|l| !l.text.trim().is_empty())
        .map(|l| GLine {
            text: l.text.trim().to_string(),
            x0: l.bbox.x,
            y0: l.bbox.y,
            x1: l.bbox.x + l.bbox.width,
            y1: l.bbox.y + l.bbox.height,
            size: l
                .words
                .iter()
                .map(|w| w.avg_font_size)
                .fold(0.0f32, f32::max),
            words: l
                .words
                .iter()
                .filter(|w| !w.text.trim().is_empty())
                .map(to_word)
                .collect(),
        })
        .collect();
    let bx = page_box(doc, page, &words);
    // Only what a viewer shows is the page's text. (On a page with /Rotate,
    // pdf_oxide reports rotated runs in the displayed frame, which the crop
    // box is not in; those pages keep all their words.)
    let seen = if rotated(doc, page) {
        (f32::MIN, f32::MIN, f32::MAX, f32::MAX)
    } else {
        visible_part(doc, page, bx)
    };
    let shown = |x0: f32, y0: f32, x1: f32, y1: f32| {
        let (cx, cy) = ((x0 + x1) / 2.0, (y0 + y1) / 2.0);
        cx >= seen.0 && cx <= seen.2 && cy >= seen.1 && cy <= seen.3
    };
    let mut words = words;
    words.retain(|w| shown(w.x0, w.y0, w.x1, w.y1));
    let mut lines = lines;
    for l in lines.iter_mut() {
        let n = l.words.len();
        l.words.retain(|w| shown(w.x0, w.y0, w.x1, w.y1));
        if l.words.len() < n && !l.words.is_empty() {
            l.text = l
                .words
                .iter()
                .map(|w| w.text.as_str())
                .collect::<Vec<_>>()
                .join(" ");
            l.x0 = l.words.iter().map(|w| w.x0).fold(f32::MAX, f32::min);
            l.x1 = l.words.iter().map(|w| w.x1).fold(f32::MIN, f32::max);
        }
    }
    lines.retain(|l| !l.words.is_empty() && shown(l.x0, l.y0, l.x1, l.y1));
    let glued = glued_runs(&lines);
    let mut geom = PageGeom {
        bx,
        lines,
        words,
        glued,
        table_boxes: Vec::new(),
    };
    // Table boxes only matter to protect a numeric table column from being
    // read as a line-number gutter, so detect tables only when a gutter
    // candidate exists. The permissive size limit (largest font on the page)
    // makes this a superset of what the document-level pass will consider.
    let largest = geom.words.iter().map(|w| w.size).fold(0.0f32, f32::max);
    if !line_number_gutters(&geom, largest.max(1.0)).0.is_empty() {
        geom.table_boxes = doc
            .extract_tables(page)
            .unwrap_or_default()
            .iter()
            .filter_map(|t| t.bbox)
            .map(|b| (b.x, b.y, b.x + b.width, b.y + b.height))
            .collect();
    }
    Some(geom)
}

/// The first word of the line that continues a line spanning `x0..x1` with
/// box `(y0, y1)`: among words below it (within two line heights) that fall
/// inside its horizontal extent, the leftmost word of the highest row — the
/// next line of the same column. Works on words rather than the converter's
/// line objects, which can merge two physical lines when a text layer is
/// skewed.
fn next_line_start<'a>(
    words: &[&'a GWord],
    x0: f32,
    x1: f32,
    y0: f32,
    y1: f32,
) -> Option<&'a GWord> {
    let height = (y1 - y0).max(1.0);
    let below: Vec<&GWord> = words
        .iter()
        .filter(|w| {
            w.y1 <= y0 + 0.3 * height
                && y0 - w.y1 < 2.0 * height
                && w.x0 >= x0 - 2.0 * height
                && w.x1 <= x1 + 2.0 * height
        })
        .copied()
        .collect();
    let top = below.iter().map(|w| w.y1).fold(f32::NEG_INFINITY, f32::max);
    below
        .into_iter()
        .filter(|w| (w.y1 - top).abs() <= 0.4 * height)
        .min_by(|a, b| a.x0.total_cmp(&b.x0))
}

/// Pairs of word halves split by a hyphen at the end of a line: the line
/// (ignoring furniture such as a line number after it) ends in `letters-` or
/// `letters -`, and the line directly below it, overlapping it horizontally,
/// starts with a lowercase word. Lowercase pairs like (`infor`, `mation`).
pub(crate) fn hyphen_breaks(p: &PageGeom, furniture: &[Rect]) -> Vec<(String, String)> {
    let kept: Vec<Vec<&GWord>> = p
        .lines
        .iter()
        .map(|l| {
            l.words
                .iter()
                .filter(|w| !excluded(furniture, w.x0, w.y0, w.x1, w.y1))
                .collect()
        })
        .collect();
    let mut out = Vec::new();
    for (i, l) in p.lines.iter().enumerate() {
        let ws = &kept[i];
        if ws.is_empty() {
            continue;
        }

        let Some(last) = ws.last() else { continue };
        let stem = if last.text == "-" {
            ws.len()
                .checked_sub(2)
                .map(|k| ws[k].text.as_str())
                .unwrap_or("")
        } else if let Some(stem) = last.text.strip_suffix('-') {
            stem
        } else {
            continue;
        };
        let left: String = stem
            .rsplit('-')
            .next()
            .unwrap_or(stem)
            .chars()
            .filter(|c| c.is_alphabetic())
            .collect();
        if left.chars().count() < 2 {
            continue;
        }
        let all: Vec<&GWord> = kept.iter().flatten().copied().collect();
        if let Some(first) = next_line_start(&all, ws[0].x0, last.x1, l.y0, l.y1).map(|w| &w.text) {
            let right: String = first.chars().take_while(|c| c.is_alphabetic()).collect();
            if right.chars().next().is_some_and(|c| c.is_lowercase()) {
                out.push((left.to_lowercase(), right.to_lowercase()));
            }
        }
    }
    out
}

/// Letter-spaced ("tracked") text such as `F I N A N C I A L  S U M M A R Y`
/// reaches the converter as one-letter words. On each line, a run of three or
/// more single letters is rejoined, splitting it into words where the gap
/// between letters is clearly wider than the run's letter spacing. Returns
/// (spaced form as the converter writes it, rejoined form).
pub(crate) fn letter_spaced_runs(p: &PageGeom) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for l in &p.lines {
        let ws = &l.words;
        let single =
            |w: &GWord| w.text.chars().count() == 1 && w.text.chars().all(char::is_alphabetic);
        let mut i = 0;
        while i < ws.len() {
            if !single(&ws[i]) {
                i += 1;
                continue;
            }
            let mut j = i;
            while j + 1 < ws.len() && single(&ws[j + 1]) {
                j += 1;
            }
            let run = &ws[i..=j];
            i = j + 1;
            if run.len() < 3 {
                continue;
            }
            let gaps: Vec<f32> = run.windows(2).map(|w| w[1].x0 - w[0].x1).collect();
            let mut sorted = gaps.clone();
            sorted.sort_by(f32::total_cmp);
            let base = sorted[sorted.len() / 3].max(0.1);
            let size = run[0].size.max(4.0);
            let mut joined = run[0].text.clone();
            for (k, g) in gaps.iter().enumerate() {
                if *g > 1.8 * base && *g > 0.25 * size {
                    joined.push(' ');
                }
                joined.push_str(&run[k + 1].text);
            }
            let spaced = run
                .iter()
                .map(|w| w.text.as_str())
                .collect::<Vec<_>>()
                .join(" ");
            // Only a run that actually forms words (not "a b c" list labels).
            if joined.split(' ').any(|w| w.chars().count() >= 3) {
                out.push((spaced, joined));
            }
        }
    }
    out
}

/// Runs of consecutive words on a line, separated by clear word gaps
/// (≥ 0.15 em), keyed by their concatenation: (`theUnionPacific`,
/// `the Union Pacific`). When the converter loses the spaces of text set
/// without space characters, its output contains exactly such a
/// concatenation, which can then be split where the page shows the gaps.
fn glued_runs(lines: &[GLine]) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for l in lines {
        let ws = &l.words;
        for i in 0..ws.len() {
            let mut glued = ws[i].text.clone();
            let mut spaced = ws[i].text.clone();
            for j in i + 1..ws.len().min(i + 12) {
                let gap = ws[j].x0 - ws[j - 1].x1;
                if gap < 0.15 * ws[j].size.max(4.0) {
                    break;
                }
                glued.push_str(&ws[j].text);
                spaced.push(' ');
                spaced.push_str(&ws[j].text);
                if glued.chars().count() >= 6 {
                    out.push((glued.clone(), spaced.clone()));
                }
            }
        }
    }
    out
}

/// Character-weighted median font size: the size of running text.
pub(crate) fn body_font_size(pages: &[Option<PageGeom>]) -> f32 {
    let mut sizes: Vec<(f32, usize)> = pages
        .iter()
        .flatten()
        .flat_map(|p| p.words.iter())
        .filter(|w| w.size > 0.0)
        .map(|w| (w.size, w.text.chars().count()))
        .collect();
    if sizes.is_empty() {
        return 10.0;
    }
    sizes.sort_by(|a, b| a.0.total_cmp(&b.0));
    let total: usize = sizes.iter().map(|s| s.1).sum();
    let mut acc = 0;
    for (size, n) in &sizes {
        acc += n;
        if acc * 2 >= total {
            return *size;
        }
    }
    sizes.last().map(|s| s.0).unwrap_or(10.0)
}

/// How far into the page (as a fraction of its height) the top and bottom
/// furniture bands reach.
const BAND: f32 = 0.12;
/// Only this many outermost lines per edge are furniture candidates.
const EDGE_LINES: usize = 3;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
enum Edge {
    Top,
    Bottom,
}

/// Lines eligible to be furniture on one page: among the outermost lines at
/// each edge and inside that edge's band. Returns (edge, line index).
/// The document's text frame: the median top of the highest text line and
/// median bottom of the lowest one across pages. Pages printed inside a
/// smaller trim (a 6×9 slip opinion on letter paper) have running heads far
/// below the media box's top edge; the frame finds them, while one sparse
/// page (a cover with a lone year mid-page) cannot move its own bands.
fn text_frame(pages: &[Option<PageGeom>]) -> Option<(f32, f32)> {
    let mut tops: Vec<f32> = Vec::new();
    let mut bottoms: Vec<f32> = Vec::new();
    for p in pages.iter().flatten() {
        if p.lines.len() < 3 {
            continue;
        }
        tops.push(p.lines.iter().map(|l| l.y1).fold(f32::MIN, f32::max));
        bottoms.push(p.lines.iter().map(|l| l.y0).fold(f32::MAX, f32::min));
    }
    if tops.len() < 3 {
        return None;
    }
    tops.sort_by(f32::total_cmp);
    bottoms.sort_by(f32::total_cmp);
    Some((tops[tops.len() / 2], bottoms[bottoms.len() / 2]))
}

fn edge_candidates(p: &PageGeom, frame: Option<(f32, f32)>) -> Vec<(Edge, usize)> {
    let h = p.height();
    // Group lines into physical rows (a footer is often several line objects
    // side by side: folio, title, date), then take the outermost rows.
    let mut order: Vec<usize> = (0..p.lines.len()).collect();
    order.sort_by(|&a, &b| p.lines[b].y1.total_cmp(&p.lines[a].y1));
    let mut rows: Vec<Vec<usize>> = Vec::new();
    for i in order {
        let l = &p.lines[i];
        if let Some(row) = rows.last_mut() {
            let r = &p.lines[row[0]];
            let ov = overlap(l.y0, l.y1, r.y0, r.y1);
            if ov > 0.5 * (l.y1 - l.y0).min(r.y1 - r.y0).max(0.1) {
                row.push(i);
                continue;
            }
        }
        rows.push(vec![i]);
    }
    // Bands hang from the paper's edges. A row the paper bands miss but the
    // document's text frame reaches (pages printed inside a smaller trim: a
    // 6×9 slip opinion on letter paper puts its running head 110 pt below
    // the sheet's top) counts only when it carries a page number, the mark
    // of a running head; ordinary first and last lines of text never do.
    let frame_top = frame.map(|(t, _)| t.min(p.bx.3) + 2.0);
    let frame_bottom = frame.map(|(_, b)| b.max(p.bx.1) - 2.0);
    let has_folio = |row: &[usize]| {
        row.iter().any(|&i| {
            let t = &p.lines[i].text;
            folio_number(t).is_some()
                || t.split_whitespace()
                    .last()
                    .is_some_and(|w| w.len() <= 4 && w.chars().all(|c| c.is_ascii_digit()))
        })
    };
    let mut out = Vec::new();
    for row in rows.iter().take(EDGE_LINES) {
        let by_frame = frame_top
            .is_some_and(|t| row.iter().all(|&i| p.lines[i].y0 >= t - BAND * h))
            && has_folio(row);
        for &i in row {
            if p.lines[i].y0 >= p.bx.3 - BAND * h || by_frame {
                out.push((Edge::Top, i));
            }
        }
    }
    for row in rows.iter().rev().take(EDGE_LINES) {
        let by_frame = frame_bottom
            .is_some_and(|b| row.iter().all(|&i| p.lines[i].y1 <= b + BAND * h))
            && has_folio(row);
        for &i in row {
            if (p.lines[i].y1 <= p.bx.1 + BAND * h || by_frame) && !out.iter().any(|&(_, j)| j == i)
            {
                out.push((Edge::Bottom, i));
            }
        }
    }
    out
}

fn is_roman(tok: &str) -> bool {
    let t = tok.to_ascii_lowercase();
    !t.is_empty()
        && t.len() <= 7
        && t.chars()
            .all(|c| matches!(c, 'i' | 'v' | 'x' | 'l' | 'c' | 'd' | 'm'))
        && roman_value(&t).is_some()
}

fn roman_value(t: &str) -> Option<u32> {
    let val = |c| match c {
        'i' => 1,
        'v' => 5,
        'x' => 10,
        'l' => 50,
        'c' => 100,
        'd' => 500,
        'm' => 1000,
        _ => 0,
    };
    let chars: Vec<u32> = t.chars().map(val).collect();
    if chars.contains(&0) {
        return None;
    }
    let mut total = 0i64;
    for i in 0..chars.len() {
        let v = chars[i] as i64;
        if i + 1 < chars.len() && (chars[i + 1] as i64) > v {
            total -= v;
        } else {
            total += v;
        }
    }
    // Reject non-canonical spellings like "iiii" or "vx" by round-tripping.
    (total > 0 && to_roman(total as u32) == t).then_some(total as u32)
}

fn to_roman(mut n: u32) -> String {
    const TABLE: [(u32, &str); 13] = [
        (1000, "m"),
        (900, "cm"),
        (500, "d"),
        (400, "cd"),
        (100, "c"),
        (90, "xc"),
        (50, "l"),
        (40, "xl"),
        (10, "x"),
        (9, "ix"),
        (5, "v"),
        (4, "iv"),
        (1, "i"),
    ];
    let mut s = String::new();
    for (v, r) in TABLE {
        while n >= v {
            s.push_str(r);
            n -= v;
        }
    }
    s
}

/// Recurrence signature of a line: lowercase words with every number
/// (arabic or roman) replaced by `#` and punctuation dropped.
fn signature(text: &str) -> String {
    text.split_whitespace()
        .map(|tok| {
            let core: String = tok.chars().filter(|c| c.is_alphanumeric()).collect();
            if core.is_empty() {
                String::new()
            } else if core.chars().all(|c| c.is_ascii_digit()) || is_roman(&core) {
                "#".to_string()
            } else if core.chars().any(|c| c.is_ascii_digit())
                && core.chars().any(|c| c.is_alphabetic())
            {
                // "p.12", "A-4": keep the letters, mask the digits.
                core.chars()
                    .map(|c| if c.is_ascii_digit() { '#' } else { c })
                    .collect::<String>()
                    .to_lowercase()
            } else {
                core.to_lowercase()
            }
        })
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

/// The page number a folio-looking line states, if it is nothing but a page
/// number: "12", "- 12 -", "[xii]", "Page 12", "Page 12 of 40", "12 | 40".
fn folio_number(text: &str) -> Option<u32> {
    let toks: Vec<String> = text
        .split(|c: char| {
            c.is_whitespace()
                || matches!(c, '|' | '/' | '-' | '–' | '—' | '[' | ']' | '(' | ')' | '.')
        })
        .map(|t| t.trim_matches('*').to_string())
        .filter(|t| !t.is_empty())
        .collect();
    if toks.is_empty() || toks.len() > 4 {
        return None;
    }
    // Arabic numerals win over roman ones: OCR often reads the rules around
    // a folio as "I", giving "I 18 I".
    let mut arabic = None;
    let mut roman = None;
    let mut numbers = 0;
    let mut of = false;
    for t in &toks {
        let lower = t.to_lowercase();
        if matches!(lower.as_str(), "page" | "pg" | "p" | "of") {
            of |= lower == "of";
            continue;
        }
        if let Ok(n) = t.parse::<u32>() {
            arabic = arabic.or(Some(n));
            numbers += 1;
            // Two numbers are a folio only as "Page 3 of 6"; otherwise they
            // are a range ("243–56)" closing a citation), a date, a value.
            if numbers > 1 && !of {
                return None;
            }
            continue;
        }
        if is_roman(t) {
            roman = roman.or(roman_value(&lower));
            continue;
        }
        return None;
    }
    arabic.or(roman)
}

/// A line number found in a gutter, with the words beside it on its line, so
/// it can be removed from converted text even when the converter merged it
/// into a span with its neighbours (exclusion works on whole spans).
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct GutterHit {
    pub left: Option<String>,
    pub number: String,
    pub right: Option<String>,
    /// First word of the next line on the left side: where the converter's
    /// text continues when it joined a column's lines around the number.
    pub next: Option<String>,
}

/// Everything furniture detection found, per page.
pub(crate) struct Furniture {
    /// Regions to exclude from conversion.
    pub rects: Vec<Vec<Rect>>,
    /// Line numbers, for contextual removal from the converted text.
    pub gutters: Vec<Vec<GutterHit>>,
    /// The line-number columns among `rects`, per page.
    pub gutter_rects: Vec<Vec<Rect>>,
}

/// One occurrence of a candidate running line: (page, line index, distance
/// from the page edge in points).
type Hit = (usize, usize, f32);

/// Detects running headers/footers, folios and line-number gutters.
pub(crate) fn detect_furniture(pages: &[Option<PageGeom>]) -> Furniture {
    let n_pages = pages.len();
    let mut out: Vec<Vec<Rect>> = vec![Vec::new(); n_pages];
    let body = body_font_size(pages);

    // ---- Running headers / footers: recurrence of masked signatures -------
    // (edge, signature) -> list of (page, line index, relative y)
    let mut occ: HashMap<(Edge, String), Vec<Hit>> = HashMap::new();
    let mut folio_cands: Vec<(usize, usize, u32)> = Vec::new();
    let frame = text_frame(pages);
    for (pi, p) in pages.iter().enumerate() {
        let Some(p) = p else { continue };
        for (edge, li) in edge_candidates(p, frame) {
            let line = &p.lines[li];
            if let Some(n) = folio_number(&line.text) {
                folio_cands.push((pi, li, n));
                continue;
            }
            let sig = signature(&line.text);
            if sig.chars().filter(|c| c.is_alphabetic()).count() < 3 {
                continue;
            }
            // Distance from the page edge the line sits on, in points: stable
            // across portrait and landscape pages of one document.
            let edge_dist = match edge {
                Edge::Top => p.bx.3 - line.y1,
                Edge::Bottom => line.y0 - p.bx.1,
            };
            occ.entry((edge, sig))
                .or_default()
                .push((pi, li, edge_dist));
        }
    }

    let text_pages = pages.iter().filter(|p| p.is_some()).count().max(1);
    for ((_edge, _sig), hits) in occ.iter() {
        // One hit per page; keep hits at a consistent vertical position.
        let mut by_page: Vec<Hit> = Vec::new();
        for &h in hits {
            if !by_page.iter().any(|b| b.0 == h.0) {
                by_page.push(h);
            }
        }
        if by_page.len() < 2 {
            continue;
        }
        let mut ys: Vec<f32> = by_page.iter().map(|h| h.2).collect();
        ys.sort_by(f32::total_cmp);
        let median_y = ys[ys.len() / 2];
        by_page.retain(|h| (h.2 - median_y).abs() <= 8.0);
        let count = by_page.len();
        if count < 2 {
            continue;
        }
        by_page.sort_by_key(|h| h.0);

        let global = count >= 3 && (count as f32) >= 0.2 * text_pages as f32;
        // A chapter running head recurs only within its chapter, often on
        // alternate pages: three or more hits with gaps of at most two pages.
        let mut best_run = 1;
        let mut run = 1;
        for w in by_page.windows(2) {
            if w[1].0 - w[0].0 <= 2 {
                run += 1;
                best_run = best_run.max(run);
            } else {
                run = 1;
            }
        }
        let local = best_run >= 3;
        // Tiny documents: on every page counts.
        let all_pages = text_pages <= 4 && count == text_pages && count >= 2;
        if !(global || local || all_pages) {
            continue;
        }
        for &(pi, li, _) in &by_page {
            let Some(p) = &pages[pi] else { continue };
            let line = &p.lines[li];
            // A line set larger than body text is a heading, not a running head.
            if line.size > body * 1.15 {
                continue;
            }
            out[pi].push(line_rect(line));
        }
    }

    // ---- Folios: lone numbers that advance with the page index ------------
    let mut offsets: HashMap<i64, usize> = HashMap::new();
    for &(pi, _, n) in &folio_cands {
        *offsets.entry(n as i64 - pi as i64).or_default() += 1;
    }
    for &(pi, li, n) in &folio_cands {
        let off = n as i64 - pi as i64;
        let consistent = offsets.get(&off).copied().unwrap_or(0) >= 2;
        // A one-page document's "1" (or "Page 1") at an edge is its folio.
        let single = n_pages == 1 && n == 1;
        if consistent || single {
            if let Some(p) = &pages[pi] {
                let line = &p.lines[li];
                if line.size <= body * 1.15 {
                    out[pi].push(line_rect(line));
                }
            }
        }
    }

    // ---- Line-number gutters ----------------------------------------------
    let mut gutters: Vec<Vec<GutterHit>> = vec![Vec::new(); n_pages];
    let mut gutter_rects: Vec<Vec<Rect>> = vec![Vec::new(); n_pages];
    for (pi, p) in pages.iter().enumerate() {
        if let Some(p) = p {
            let (rects, hits) = line_number_gutters(p, body);
            out[pi].extend(rects.iter().cloned());
            gutter_rects[pi] = rects;
            gutters[pi] = hits;
        }
    }
    if std::env::var_os("FILEGOBLIN_DEBUG_FURNITURE").is_some() {
        for (pi, (rects, p)) in out.iter().zip(pages).enumerate() {
            let Some(p) = p else { continue };
            for r in rects {
                let text: Vec<&str> = p
                    .lines
                    .iter()
                    .filter(|l| excluded(std::slice::from_ref(r), l.x0, l.y0, l.x1, l.y1))
                    .map(|l| l.text.as_str())
                    .collect();
                eprintln!("furniture page {}: {:?}", pi + 1, text);
            }
        }
    }
    Furniture {
        rects: out,
        gutters,
        gutter_rects,
    }
}

/// Furniture removal for pages that have text but no geometry (OCR output):
/// the same recurrence and page-number rules, applied to each page's first
/// and last two non-empty lines. Only pages flagged in `eligible` are edited.
pub(crate) fn strip_text_furniture(pages: &mut [String], eligible: &[bool]) {
    let n = pages.len();
    let idx: Vec<usize> = (0..n)
        .filter(|&i| eligible.get(i).copied().unwrap_or(false))
        .collect();
    if idx.len() < 3 {
        return;
    }
    // (page, line index, edge) candidates
    let mut cands: Vec<(usize, usize, Edge)> = Vec::new();
    for &pi in &idx {
        let lines: Vec<&str> = pages[pi].lines().collect();
        let nonempty: Vec<usize> = (0..lines.len())
            .filter(|&i| !lines[i].trim().is_empty())
            .collect();
        for &li in nonempty.iter().take(2) {
            cands.push((pi, li, Edge::Top));
        }
        for &li in nonempty.iter().rev().take(2) {
            if !cands.iter().any(|c| c.0 == pi && c.1 == li) {
                cands.push((pi, li, Edge::Bottom));
            }
        }
    }
    let line_of = |pages: &[String], pi: usize, li: usize| -> String {
        pages[pi].lines().nth(li).unwrap_or("").trim().to_string()
    };

    let mut drop: Vec<(usize, usize)> = Vec::new();
    let mut occ: HashMap<(Edge, String), Vec<(usize, usize)>> = HashMap::new();
    let mut folios: Vec<(usize, usize, u32)> = Vec::new();
    for &(pi, li, edge) in &cands {
        let text = line_of(pages, pi, li);
        if text.starts_with('#') || text.starts_with('|') || text.split_whitespace().count() > 12 {
            continue;
        }
        if let Some(num) = folio_number(&text) {
            folios.push((pi, li, num));
            continue;
        }
        let sig = signature(&text);
        if sig.chars().filter(|c| c.is_alphabetic()).count() >= 3 {
            occ.entry((edge, sig)).or_default().push((pi, li));
        }
    }
    let rank: HashMap<usize, usize> = idx.iter().enumerate().map(|(r, &p)| (p, r)).collect();
    for hits in occ.values() {
        let mut pages_hit: Vec<usize> = hits.iter().map(|h| rank[&h.0]).collect();
        pages_hit.sort_unstable();
        pages_hit.dedup();
        let count = pages_hit.len();
        let global = count >= 3 && (count as f32) >= 0.2 * idx.len() as f32;
        let local = pages_hit
            .windows(2)
            .fold((1, 1), |(best, run), w| {
                let run = if w[1] - w[0] <= 2 { run + 1 } else { 1 };
                (best.max(run), run)
            })
            .0
            >= 3;
        if global || local {
            drop.extend(hits.iter().copied());
        }
    }
    let mut offsets: HashMap<i64, usize> = HashMap::new();
    for &(pi, _, num) in &folios {
        *offsets.entry(num as i64 - pi as i64).or_default() += 1;
    }
    for &(pi, li, num) in &folios {
        if offsets.get(&(num as i64 - pi as i64)).copied().unwrap_or(0) >= 2 {
            drop.push((pi, li));
        }
    }
    for &pi in &idx {
        let kill: Vec<usize> = drop.iter().filter(|d| d.0 == pi).map(|d| d.1).collect();
        if kill.is_empty() {
            continue;
        }
        pages[pi] = pages[pi]
            .lines()
            .enumerate()
            .filter(|(i, _)| !kill.contains(i))
            .map(|(_, l)| l)
            .collect::<Vec<_>>()
            .join("\n")
            .trim()
            .to_string();
    }
}

fn line_rect(l: &GLine) -> Rect {
    Rect::new(
        l.x0 - 0.5,
        l.y0 - 0.5,
        (l.x1 - l.x0) + 1.0,
        (l.y1 - l.y0) + 1.0,
    )
}

fn word_rect(w: &GWord) -> Rect {
    Rect::new(
        w.x0 - 0.5,
        w.y0 - 0.5,
        (w.x1 - w.x0) + 1.0,
        (w.y1 - w.y0) + 1.0,
    )
}

/// Finds columns of line numbers (legal pleading paper, patents, transcripts):
///
/// * integers stacked in a narrow vertical column,
/// * increasing down the page by a constant step (1, 2, 5 or 10),
/// * at a constant vertical pitch per step (they track the text lines),
/// * standing in an otherwise empty vertical channel, and
/// * outside the text flow: either in a margin (no text on one side of the
///   channel) or in the gutter between two columns (text on both sides).
///
/// The last condition is what separates a gutter from a list such as
/// "Item 5 / Item 10 / Item 15", whose numbers end their lines.
pub(crate) fn line_number_gutters(p: &PageGeom, body: f32) -> (Vec<Rect>, Vec<GutterHit>) {
    let ints: Vec<(usize, u32)> = p
        .words
        .iter()
        .enumerate()
        .filter_map(|(i, w)| {
            let n: u32 = w.text.parse().ok()?;
            (n > 0 && n <= 999 && w.size <= body * 1.4).then_some((i, n))
        })
        .collect();
    if ints.len() < 4 {
        return (Vec::new(), Vec::new());
    }

    // Cluster by horizontal centre.
    let mut clusters: Vec<Vec<(usize, u32)>> = Vec::new();
    for &(i, n) in &ints {
        let w = &p.words[i];
        let cx = (w.x0 + w.x1) / 2.0;
        if let Some(c) = clusters.iter_mut().find(|c| {
            let f = &p.words[c[0].0];
            ((f.x0 + f.x1) / 2.0 - cx).abs() <= 0.6 * f.size.max(4.0)
        }) {
            c.push((i, n));
        } else {
            clusters.push(vec![(i, n)]);
        }
    }

    let mut rects = Vec::new();
    let mut hits = Vec::new();
    for mut c in clusters {
        if c.len() < 4 {
            continue;
        }
        // Top to bottom.
        c.sort_by(|a, b| p.words[b.0].y0.total_cmp(&p.words[a.0].y0));
        let diffs: Vec<i64> = c
            .windows(2)
            .map(|w| w[1].1 as i64 - w[0].1 as i64)
            .collect();
        if diffs.iter().any(|&d| d <= 0) {
            continue;
        }
        let step = diffs.iter().copied().min().unwrap_or(1);
        if !matches!(step, 1 | 2 | 5 | 10) || diffs.iter().any(|d| d % step != 0) {
            continue;
        }
        let pitches: Vec<f32> = c
            .windows(2)
            .zip(diffs.iter())
            .map(|(w, &d)| (p.words[w[0].0].y0 - p.words[w[1].0].y0) / (d / step) as f32)
            .collect();
        let mut sorted = pitches.clone();
        sorted.sort_by(f32::total_cmp);
        let med = sorted[sorted.len() / 2];
        if med <= 0.0 || pitches.iter().any(|&pt| (pt - med).abs() > 0.25 * med) {
            continue;
        }

        let members: Vec<usize> = c.iter().map(|m| m.0).collect();
        let sx0 = members
            .iter()
            .map(|&i| p.words[i].x0)
            .fold(f32::INFINITY, f32::min);
        let sx1 = members
            .iter()
            .map(|&i| p.words[i].x1)
            .fold(0.0f32, f32::max);
        let sy0 = members
            .iter()
            .map(|&i| p.words[i].y0)
            .fold(f32::INFINITY, f32::min);
        let sy1 = members
            .iter()
            .map(|&i| p.words[i].y1)
            .fold(0.0f32, f32::max);
        let others = p
            .words
            .iter()
            .enumerate()
            .filter(|(j, w)| !members.contains(j) && w.y1 >= sy0 && w.y0 <= sy1);

        // The channel must be empty of other text.
        let in_channel = others
            .clone()
            .filter(|(_, w)| overlap(w.x0, w.x1, sx0, sx1) > 0.5 * (sx1 - sx0))
            .count();
        if in_channel > (c.len() / 5).max(2) {
            continue;
        }
        let any_left = others.clone().any(|(_, w)| w.x1 <= sx0);
        let any_right = others.clone().any(|(_, w)| w.x0 >= sx1);
        // Text on both sides of each number's own line → a column gutter.
        let both_sides = members
            .iter()
            .filter(|&&i| {
                let m = &p.words[i];
                let same_line = |w: &GWord| overlap(w.y0, w.y1, m.y0, m.y1) > 0.3 * (m.y1 - m.y0);
                p.words.iter().any(|w| same_line(w) && w.x1 <= sx0)
                    && p.words.iter().any(|w| same_line(w) && w.x0 >= sx1)
            })
            .count();
        // In a margin the numbers stand well clear of the text block (a
        // list like "Item 5 / Item 10" has its numbers a word-space away).
        let clear_gap = |side_left: bool| {
            let clear = members
                .iter()
                .filter(|&&i| {
                    let m = &p.words[i];
                    let em = m.size.max(4.0);
                    let same_line =
                        |w: &&GWord| overlap(w.y0, w.y1, m.y0, m.y1) > 0.3 * (m.y1 - m.y0);
                    let nearest = if side_left {
                        p.words
                            .iter()
                            .filter(same_line)
                            .filter(|w| w.x0 >= sx1)
                            .map(|w| w.x0 - sx1)
                            .fold(f32::INFINITY, f32::min)
                    } else {
                        p.words
                            .iter()
                            .filter(same_line)
                            .filter(|w| w.x1 <= sx0)
                            .map(|w| sx0 - w.x1)
                            .fold(f32::INFINITY, f32::min)
                    };
                    nearest >= 1.5 * em
                })
                .count();
            clear * 10 >= members.len() * 8
        };
        let margin = (!any_left && clear_gap(true)) || (!any_right && clear_gap(false));
        let gutter = both_sides * 10 >= members.len() * 6;
        if !(margin || gutter) {
            continue;
        }
        // A numeric column inside a detected table is data, not a gutter.
        if p.table_boxes
            .iter()
            .any(|t| sx0 >= t.0 - 1.0 && sx1 <= t.2 + 1.0 && sy0 >= t.1 - 1.0 && sy1 <= t.3 + 1.0)
        {
            continue;
        }
        for i in members {
            let m = &p.words[i];
            let em = m.size.max(4.0);
            let same_line = |w: &&GWord| overlap(w.y0, w.y1, m.y0, m.y1) > 0.3 * (m.y1 - m.y0);
            let left = p
                .words
                .iter()
                .filter(same_line)
                .filter(|w| w.x1 <= m.x0 + 0.5 && m.x0 - w.x1 < 12.0 * em)
                .max_by(|a, b| a.x1.total_cmp(&b.x1))
                .map(|w| w.text.clone());
            let right = p
                .words
                .iter()
                .filter(same_line)
                .filter(|w| w.x0 >= m.x1 - 0.5 && w.x0 - m.x1 < 12.0 * em)
                .min_by(|a, b| a.x0.total_cmp(&b.x0))
                .map(|w| w.text.clone());
            // Where the left column's text continues: the first word of the
            // next line, aligned with the start of the number's own line.
            let others: Vec<&GWord> = p
                .words
                .iter()
                .filter(|w| !(w.x0 >= sx0 - 0.5 && w.x1 <= sx1 + 0.5))
                .collect();
            let line_start = others
                .iter()
                .filter(|w| same_line(w) && w.x1 <= m.x0 + 0.5)
                .min_by(|a, b| a.x0.total_cmp(&b.x0));
            let next = line_start
                .and_then(|s| next_line_start(&others, s.x0, m.x0, m.y0, m.y1))
                .map(|w| w.text.clone());
            hits.push(GutterHit {
                left,
                number: m.text.clone(),
                right,
                next,
            });
            rects.push(word_rect(m));
        }
    }
    (rects, hits)
}

/// Removes detected line numbers from converted page text. Each number is
/// removed once, and only where it appears between the same neighbouring
/// words it had on the page, so a "15" elsewhere in the text is never touched.
pub(crate) fn remove_gutter_numbers(md: &str, hits: &[GutterHit]) -> String {
    let mut out = md.to_string();
    for h in hits {
        let n = regex::escape(&h.number);
        let mut patterns = Vec::new();
        if let Some(l) = &h.left {
            for follow in [&h.right, &h.next].into_iter().flatten() {
                patterns.push(format!(
                    r"(?P<l>{})(?P<s1>\s+){}(?P<s2>\s+)(?P<r>{})",
                    regex::escape(l),
                    n,
                    regex::escape(follow)
                ));
            }
            if h.right.is_none() {
                patterns.push(format!(
                    r"(?m)(?P<l>{})(?P<s1>[ \t]+){}(?P<s2>[ \t]*)$",
                    regex::escape(l),
                    n
                ));
            }
            // Last resort: the left neighbour, the number, and a lowercase
            // word (a sentence continuing past a gutter number). Requires a
            // real word on the left so punctuation alone never anchors it.
            if l.chars().filter(|c| c.is_alphabetic()).count() >= 2 {
                patterns.push(format!(
                    r"(?P<l>\b{})(?P<s1>[ \t]+){}(?P<s2>[ \t]+)(?P<r>\p{{Ll}})",
                    regex::escape(l),
                    n
                ));
            }
        } else if let Some(r) = &h.right {
            patterns.push(format!(
                r"(?m)(?P<s1>^[ \t]*){}(?P<s2>\s+)(?P<r>{})",
                n,
                regex::escape(r)
            ));
        }
        for pattern in patterns {
            let Ok(re) = regex::Regex::new(&pattern) else {
                continue;
            };
            if !re.is_match(&out) {
                continue;
            }
            out = re
                .replacen(&out, 1, |c: &regex::Captures| {
                    let l = c.name("l").map_or("", |m| m.as_str());
                    let r = c.name("r").map_or("", |m| m.as_str());
                    let gap = format!(
                        "{}{}",
                        c.name("s1").map_or("", |m| m.as_str()),
                        c.name("s2").map_or("", |m| m.as_str())
                    );
                    let sep = if gap.contains('\n') {
                        "\n"
                    } else if l.is_empty() || r.is_empty() {
                        ""
                    } else {
                        " "
                    };
                    format!("{l}{sep}{r}")
                })
                .into_owned();
            break;
        }
    }
    out
}

fn overlap(a0: f32, a1: f32, b0: f32, b1: f32) -> f32 {
    (a1.min(b1) - a0.max(b0)).max(0.0)
}

/// True when the rectangle `r` covers most of the word box.
pub(crate) fn excluded(rects: &[Rect], x0: f32, y0: f32, x1: f32, y1: f32) -> bool {
    let area = ((x1 - x0) * (y1 - y0)).max(0.01);
    rects.iter().any(|r| {
        let ox = overlap(r.x, r.x + r.width, x0, x1);
        let oy = overlap(r.y, r.y + r.height, y0, y1);
        ox * oy >= 0.5 * area
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(text: &str, y: f32, size: f32) -> GLine {
        GLine {
            text: text.into(),
            x0: 72.0,
            y0: y,
            x1: 400.0,
            y1: y + size,
            size,
            words: vec![],
        }
    }

    fn page(lines: Vec<GLine>) -> Option<PageGeom> {
        let words = lines
            .iter()
            .map(|l| GWord {
                text: l.text.clone(),
                x0: l.x0,
                y0: l.y0,
                x1: l.x1,
                y1: l.y1,
                size: l.size,
            })
            .collect();
        Some(PageGeom {
            bx: (0.0, 0.0, 612.0, 792.0),
            lines,
            words,
            glued: vec![],
            table_boxes: vec![],
        })
    }

    #[test]
    fn signature_masks_numbers_and_punctuation() {
        assert_eq!(
            signature("Chapter 3 | The Basics — 45"),
            "chapter # the basics #"
        );
        assert_eq!(signature("Page xii"), "page #");
        assert_eq!(signature("Civil Procedure"), "civil procedure");
    }

    #[test]
    fn folio_numbers() {
        assert_eq!(folio_number("12"), Some(12));
        assert_eq!(folio_number("- 12 -"), Some(12));
        assert_eq!(folio_number("Page 3 of 6"), Some(3));
        assert_eq!(folio_number("xiv"), Some(14));
        assert_eq!(folio_number("Mild"), None);
        assert_eq!(folio_number("Revenue 2023"), None);
        assert_eq!(folio_number("243–56)"), None);
        assert_eq!(folio_number("12 / 2023"), None);
    }

    #[test]
    fn running_header_and_folio_removed_but_heading_kept() {
        let mut pages = Vec::new();
        for i in 0..6 {
            pages.push(page(vec![
                line("Northwind Holdings | Internal Memo", 760.0, 8.0),
                line(
                    &format!("Section {}: Overview of Operations", i + 1),
                    700.0,
                    15.0,
                ),
                line(
                    "Body text that is long enough to be ordinary prose here.",
                    650.0,
                    10.0,
                ),
                line(
                    "More body text follows on this page in the same size.",
                    630.0,
                    10.0,
                ),
                line(&format!("{}", i + 1), 30.0, 8.0),
            ]));
        }
        let rects = detect_furniture(&pages).rects;
        for (i, r) in rects.iter().enumerate() {
            // header + folio, never the 15pt heading
            assert_eq!(r.len(), 2, "page {i}: {r:?}");
            assert!(r.iter().all(|r| r.y > 600.0 || r.y < 100.0));
        }
    }

    #[test]
    fn lone_year_on_cover_is_not_a_folio() {
        let mut pages = vec![page(vec![
            line("Annual Report", 700.0, 24.0),
            line(
                "Body text that is long enough to be ordinary prose here.",
                650.0,
                10.0,
            ),
            line("2023", 40.0, 10.0),
        ])];
        for i in 1..4 {
            pages.push(page(vec![
                line(
                    "Body text that is long enough to be ordinary prose here.",
                    650.0,
                    10.0,
                ),
                line(
                    "Another ordinary paragraph line of body text for page.",
                    630.0,
                    10.0,
                ),
                line(&format!("{}", 10 + i), 30.0, 10.0),
            ]));
        }
        let rects = detect_furniture(&pages).rects;
        assert!(rects[0].is_empty(), "{:?}", rects[0]);
        assert_eq!(rects[1].len(), 1);
    }

    #[test]
    fn pleading_line_numbers_detected() {
        let mut words = Vec::new();
        for i in 1..=10u32 {
            let y = 700.0 - (i as f32) * 24.0;
            words.push(GWord {
                text: i.to_string(),
                x0: 40.0,
                y0: y,
                x1: 46.0,
                y1: y + 10.0,
                size: 10.0,
            });
            words.push(GWord {
                text: "Plaintiff".into(),
                x0: 80.0,
                y0: y,
                x1: 130.0,
                y1: y + 12.0,
                size: 12.0,
            });
        }
        let p = PageGeom {
            bx: (0.0, 0.0, 612.0, 792.0),
            lines: vec![],
            words,
            glued: vec![],
            table_boxes: vec![],
        };
        let (rects, hits) = line_number_gutters(&p, 12.0);
        assert_eq!(rects.len(), 10);
        assert_eq!(hits[0].right.as_deref(), Some("Plaintiff"));
    }

    #[test]
    fn table_cells_next_to_labels_are_not_line_numbers() {
        let mut words = Vec::new();
        for i in 1..=6u32 {
            let y = 700.0 - (i as f32) * 20.0;
            words.push(GWord {
                text: "Item".into(),
                x0: 72.0,
                y0: y,
                x1: 95.0,
                y1: y + 10.0,
                size: 10.0,
            });
            words.push(GWord {
                text: (i * 5).to_string(),
                x0: 98.0,
                y0: y,
                x1: 108.0,
                y1: y + 10.0,
                size: 10.0,
            });
        }
        let p = PageGeom {
            bx: (0.0, 0.0, 612.0, 792.0),
            lines: vec![],
            words,
            glued: vec![],
            table_boxes: vec![],
        };
        assert!(line_number_gutters(&p, 10.0).0.is_empty());
    }

    #[test]
    fn ocr_text_furniture() {
        let bodies = [
            "Interviews were recorded and transcribed.",
            "Experts disagreed about the second task.",
            "The protocol changed after the pilot.",
            "Coding took two weeks for each analyst.",
            "Results are summarised in the next section.",
        ];
        let mut pages: Vec<String> = bodies
            .iter()
            .enumerate()
            .map(|(i, b)| {
                format!(
                    "{} WOOD AND FORD\n{b}\nA second paragraph of ordinary text.\nThird line.",
                    70 + i
                )
            })
            .collect();
        pages.insert(
            0,
            "Structuring Interviews\nAbstract body text here.".to_string(),
        );
        let flags = vec![true; pages.len()];
        strip_text_furniture(&mut pages, &flags);
        assert!(pages[0].contains("Structuring Interviews"));
        for (p, b) in pages[1..].iter().zip(bodies) {
            assert!(!p.contains("WOOD AND FORD"), "{p}");
            assert!(p.starts_with(b), "{p}");
        }
    }

    #[test]
    fn gutter_numbers_removed_only_in_context() {
        let hits = vec![
            GutterHit {
                left: Some("limita".into()),
                number: "5".into(),
                right: Some("tions".into()),
                next: None,
            },
            GutterHit {
                left: Some("be".into()),
                number: "10".into(),
                right: Some("the".into()),
                next: Some("implemented".into()),
            },
        ];
        let md = "Take 5 mg daily. Not construed as limita 5  tions and may also be 10 implemented in 10 ways.";
        assert_eq!(
            remove_gutter_numbers(md, &hits),
            "Take 5 mg daily. Not construed as limita tions and may also be implemented in 10 ways."
        );
    }
}
