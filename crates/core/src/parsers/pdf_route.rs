//! Per-page routing for `--vision smart`: does this page need the vision
//! models, or does the fast geometric converter already read it well?
//!
//! The checks are cheap facts about the text layer, read before anything is
//! rendered. A page goes to the models when it has something the geometric
//! converter is known to get wrong; plain single-column prose does not.
//!
//! * **No usable text layer**: scans, image-only and empty pages need OCR.
//! * **Mathematics**: math fonts (Computer Modern math, AMS symbols, STIX,
//!   Cambria Math, …) or math symbols in the text. A text layer cannot spell
//!   a formula; only the image can.
//! * **Tables and forms**: several lines whose words are separated by
//!   column-sized gaps.
//! * **Columns**: most lines stay on one side of the page centre.
//! * **Pictures**: pages that mix text with images.
//! * **Short documents**: running heads and page numbers are recognised by
//!   recurrence across pages, which a one- or two-page document cannot show.
//! * **Formulas spelled as loose letters**: one word in twelve a single
//!   letter ("f ( x ) = x 2") is a page whose formulas the text layer could
//!   not spell. (Scans whose OCR layer is damaged are re-read from the image
//!   before routing; see `pdf_textq`.)
//!
//! Fillable forms (ten or more form-field widgets) stay on the fast path:
//! the layout models read a form's boxes as one page-wide table.

use super::pdf_furniture::PageGeom;
use pdf_oxide::PdfDocument;
use pdf_oxide::extractors::PageKind;

/// Why a page goes to the vision models.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Reason {
    NoTextLayer,
    Math,
    Table,
    Columns,
    Pictures,
    ShortDocument,
}

impl Reason {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Reason::NoTextLayer => "no text layer",
            Reason::Math => "math",
            Reason::Table => "table",
            Reason::Columns => "columns",
            Reason::Pictures => "pictures",
            Reason::ShortDocument => "short document",
        }
    }
}

/// Fonts that only typeset mathematics.
const MATH_FONTS: [&str; 17] = [
    "cmmi", "cmsy", "cmex", "cmbsy", "msam", "msbm", "eufm", "rsfs", "stix", "math", "mtmi",
    "mtsy", "txsy", "pxsy", "esint", "lmmath", "symbol",
];

pub(crate) fn needs_vision(
    doc: &PdfDocument,
    page: usize,
    kind: PageKind,
    geom: Option<&PageGeom>,
    page_count: usize,
) -> Option<Reason> {
    match kind {
        PageKind::Scanned | PageKind::Empty => return Some(Reason::NoTextLayer),
        PageKind::ImageText | PageKind::Mixed => return Some(Reason::Pictures),
        _ => {}
    }
    let Some(geom) = geom else {
        return Some(Reason::NoTextLayer);
    };
    // Fillable forms: the layout models read a form's boxes as one page-wide
    // table, while the geometric converter keeps the labels in reading
    // order. The fields themselves say it is a form.
    if is_form(doc, page) {
        return None;
    }
    if page_count <= 2 {
        return Some(Reason::ShortDocument);
    }
    if has_math(doc, page) {
        return Some(Reason::Math);
    }
    if has_table(geom) {
        return Some(Reason::Table);
    }
    if has_columns(geom) {
        return Some(Reason::Columns);
    }
    None
}

/// Ten or more form-field widgets on the page.
fn is_form(doc: &PdfDocument, page: usize) -> bool {
    use pdf_oxide::annotation_types::AnnotationSubtype;
    doc.get_annotations(page).is_ok_and(|annots| {
        annots
            .iter()
            .filter(|a| a.subtype_enum == AnnotationSubtype::Widget)
            .count()
            >= 10
    })
}

/// Formulas the text layer spells as loose letters ("f ( x ) = x 2"): a
/// scan rebuilt into fonts by its scanner, or a page whose formulas are
/// small pictures between text-layer words. One word in twelve or more is a
/// single letter other than "a" or "I", where ordinary prose has almost none.
pub(crate) fn spelled_math(q: &super::pdf_textq::TextQuality) -> bool {
    q.words >= 30 && q.single >= 0.08
}

/// Three or more characters set in a math font or drawn from the math
/// blocks of Unicode.
fn has_math(doc: &PdfDocument, page: usize) -> bool {
    let Ok(spans) = doc.extract_spans(page) else {
        return false;
    };
    let mut math = 0usize;
    for s in &spans {
        let font = s.font_name.to_lowercase();
        let math_font = MATH_FONTS.iter().any(|f| font.contains(f));
        for c in s.text.chars().filter(|c| !c.is_whitespace()) {
            if math_font || is_math_char(c) {
                math += 1;
                if math >= 3 {
                    return true;
                }
            }
        }
    }
    false
}

pub(crate) fn is_math_char(c: char) -> bool {
    matches!(c as u32,
        0x2200..=0x22FF       // mathematical operators
        | 0x27C0..=0x27EF     // misc math symbols A
        | 0x2980..=0x2AFF     // misc math symbols B, supplemental operators
        | 0x1D400..=0x1D7FF   // mathematical alphanumerics
        | 0x2032..=0x2034     // primes
    ) || matches!(
        c,
        '∑' | '∏' | '∫' | '√' | '∞' | '≤' | '≥' | '≠' | '≈' | '±' | '∓' | '÷' | '×'
    )
}

/// Three or more lines broken into at least three pieces by gaps wider than
/// one and a half em: columns of a table, fields of a form.
fn has_table(geom: &PageGeom) -> bool {
    let tabular = geom
        .lines
        .iter()
        .filter(|l| {
            let em = l.size.max(1.0);
            l.words
                .windows(2)
                .filter(|w| w[1].x0 - w[0].x1 > 1.5 * em)
                .count()
                >= 2
        })
        .count();
    tabular >= 3
}

/// At least eight line segments, of which fewer than a quarter cross the
/// middle of the text block or span more than half its width. Lines are cut
/// at gaps wider than 1.2 em first: text extraction often merges the two
/// columns' lines that share a baseline.
fn has_columns(geom: &PageGeom) -> bool {
    let mut segs: Vec<(f32, f32)> = Vec::new();
    for l in &geom.lines {
        let em = l.size.max(1.0);
        let mut start: Option<(f32, f32, usize)> = None; // (x0, x1, words)
        for w in &l.words {
            match start {
                Some((x0, x1, n)) if w.x0 - x1 <= 1.2 * em => start = Some((x0, w.x1, n + 1)),
                Some((x0, x1, n)) => {
                    if n >= 3 {
                        segs.push((x0, x1));
                    }
                    start = Some((w.x0, w.x1, 1));
                }
                None => start = Some((w.x0, w.x1, 1)),
            }
        }
        if let Some((x0, x1, n)) = start
            && n >= 3
        {
            segs.push((x0, x1));
        }
    }
    if segs.len() < 8 {
        return false;
    }
    let x0 = segs.iter().map(|s| s.0).fold(f32::MAX, f32::min);
    let x1 = segs.iter().map(|s| s.1).fold(f32::MIN, f32::max);
    let width = (x1 - x0).max(1.0);
    let mid = (x0 + x1) / 2.0;
    let straddle = segs
        .iter()
        .filter(|s| {
            (s.0 < mid - 0.05 * width && s.1 > mid + 0.05 * width) || s.1 - s.0 > 0.5 * width
        })
        .count();
    straddle * 4 < segs.len()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parsers::pdf_furniture::{GLine, GWord};

    fn line(y: f32, spans: &[(f32, f32)]) -> GLine {
        let words: Vec<GWord> = spans
            .iter()
            .map(|&(x0, x1)| GWord {
                text: "w".into(),
                x0,
                y0: y,
                x1,
                y1: y + 10.0,
                size: 10.0,
            })
            .collect();
        GLine {
            text: "w".into(),
            x0: words.first().map_or(0.0, |w| w.x0),
            y0: y,
            x1: words.last().map_or(0.0, |w| w.x1),
            y1: y + 10.0,
            size: 10.0,
            words,
        }
    }

    fn geom(lines: Vec<GLine>) -> PageGeom {
        PageGeom {
            bx: (0.0, 0.0, 612.0, 792.0),
            lines,
            words: vec![],
            glued: vec![],
            table_boxes: vec![],
        }
    }

    #[test]
    fn prose_is_neither_table_nor_columns() {
        let g = geom(
            (0..20)
                .map(|i| {
                    line(
                        700.0 - 14.0 * i as f32,
                        &[(72.0, 120.0), (123.0, 300.0), (303.0, 540.0)],
                    )
                })
                .collect(),
        );
        assert!(!has_table(&g));
        assert!(!has_columns(&g));
    }

    #[test]
    fn two_columns_are_detected() {
        let mut lines = Vec::new();
        for i in 0..12 {
            let y = 700.0 - 14.0 * i as f32;
            lines.push(line(y, &[(72.0, 120.0), (123.0, 200.0), (203.0, 290.0)]));
            lines.push(line(y, &[(322.0, 400.0), (403.0, 480.0), (483.0, 540.0)]));
        }
        assert!(has_columns(&geom(lines)));
    }

    #[test]
    fn merged_two_column_lines_are_still_columns() {
        let g = geom(
            (0..12)
                .map(|i| {
                    line(
                        700.0 - 14.0 * i as f32,
                        &[
                            (72.0, 120.0),
                            (123.0, 200.0),
                            (203.0, 290.0),
                            (322.0, 400.0),
                            (403.0, 480.0),
                            (483.0, 540.0),
                        ],
                    )
                })
                .collect(),
        );
        assert!(has_columns(&g));
    }

    #[test]
    fn column_gaps_make_a_table() {
        let g = geom(
            (0..5)
                .map(|i| {
                    line(
                        700.0 - 14.0 * i as f32,
                        &[(72.0, 120.0), (200.0, 240.0), (320.0, 360.0)],
                    )
                })
                .collect(),
        );
        assert!(has_table(&g));
    }

    #[test]
    fn math_symbols_are_recognised() {
        assert!(is_math_char('≤') && is_math_char('∑') && is_math_char('𝑥'));
        assert!(!is_math_char('a') && !is_math_char('é'));
    }
}
