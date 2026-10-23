//! Layout-guided page assembly for PDFs (the `vision` feature).
//!
//! The page is rendered and a layout model finds its regions and their
//! reading order. Text still comes from the PDF's own text layer — the model
//! never invents words — but the model decides *where each word belongs*:
//! which paragraph, heading, caption or table it is part of, which words are
//! running heads or page numbers, which are labels inside a figure, and which
//! are mathematics. Formulas (display and inline) are re-read from the image
//! as LaTeX, because a PDF text layer cannot express them.
//!
//! Words the model did not cover are never dropped: they are grouped into
//! blocks and slotted into the reading order next to the region above them.

use super::pdf_furniture::{self, PageGeom};
use crate::vision::UserBox;
use crate::vision::formula;
use crate::vision::layout::{self, Label, Region};
use crate::vision::render::{DisplayFrame, PageImage, Renderer};
use pdf_oxide::PdfDocument;
use pdf_oxide::geometry::Rect;
use pdf_oxide::layout::RectFilterMode;

/// Long side, in pixels, of the page raster the models see.
const RENDER_LONG_SIDE: f32 = 1700.0;
const MAX_SCALE: f32 = 3.0;
/// Scans are read by OCR, which wants more pixels per character.
const SCAN_LONG_SIDE: f32 = 2400.0;
const SCAN_MAX_SCALE: f32 = 4.0;

/// One word of the text layer, or one recognized inline formula.
#[derive(Debug, Clone)]
struct Tok {
    text: String,
    b: UserBox,
    size: f32,
    /// The text layer writes this word flush against the previous word of
    /// the same line (no space between them in the line's text).
    glued: bool,
    /// The text layer sets the word in a math font (or as math symbols).
    math: bool,
}

/// Converts one text-layer page with the layout model. `None` when the page
/// cannot be rendered or the model is unavailable — the caller then falls
/// back to the geometric converter.
///
/// With the markdown comes whether a column the furniture pass took for
/// line numbers lies inside a table the model found: those numbers are then
/// the table's, and the caller leaves them in the text.
pub(crate) fn convert_page(
    renderer: &Renderer,
    doc: &PdfDocument,
    page: usize,
    geom: &PageGeom,
    excluded: &[Rect],
    gutters: &[Rect],
    base_options: &pdf_oxide::converters::ConversionOptions,
) -> Option<(String, bool)> {
    crate::vision::PageGate::global()
        .run(|| convert_page_gated(renderer, doc, page, geom, excluded, gutters, base_options))
}

fn convert_page_gated(
    renderer: &Renderer,
    doc: &PdfDocument,
    page: usize,
    geom: &PageGeom,
    excluded: &[Rect],
    gutters: &[Rect],
    base_options: &pdf_oxide::converters::ConversionOptions,
) -> Option<(String, bool)> {
    let t = std::time::Instant::now();
    // A page with /Rotate is assembled as it is displayed: the layout model
    // reads the turned page, and pdf_oxide reports rotated text that way.
    let frame = display_frame(doc, page);
    let render = |long: f32, scale: f32| {
        let img = renderer.render(page, long, scale)?;
        match frame {
            Some(f) => img.in_display_frame(f),
            None => Some(img),
        }
    };
    let img = render(RENDER_LONG_SIDE, MAX_SCALE)?;
    let page_box = frame.map_or(geom.bx, |f| f.page_box());
    let regions = layout::detect(&img)?;
    debug_regions(page, &regions);
    let t_layout = t.elapsed();
    // A "line-number gutter" inside a table region is the table's number
    // column (row IDs, sample numbers): its words stay.
    let in_table = |g: &&Rect| {
        frame.is_none()
            && regions.iter().any(|r| {
                r.label == Label::Table
                    && r.bbox
                        .contains_point(g.x + g.width / 2.0, g.y + g.height / 2.0)
            })
    };
    let table_numbers = gutters.iter().any(|g| in_table(&g));
    let kept_excluded: Vec<Rect>;
    let excluded: &[Rect] = if table_numbers {
        kept_excluded = excluded
            .iter()
            .filter(|e| {
                !gutters.iter().filter(in_table).any(|g| {
                    (g.x - e.x).abs() < 0.01
                        && (g.y - e.y).abs() < 0.01
                        && (g.width - e.width).abs() < 0.01
                        && (g.height - e.height).abs() < 0.01
                })
            })
            .cloned()
            .collect();
        &kept_excluded
    } else {
        excluded
    };
    // Glyph positions are raw page space, and a rotated glyph reports no
    // advance; rotated pages take their words from the spans.
    let mut words = match frame {
        Some(_) => span_words(doc, page, excluded, frame),
        None => {
            glyph_words(doc, page, geom, excluded).or_else(|| span_words(doc, page, excluded, None))
        }
    }
    .unwrap_or_default();
    // Regions the model sees as text but the text layer leaves empty are
    // pictures of text (a scanned page behind a thin text layer, a pasted
    // image of a paragraph or table): read those with OCR.
    let empty_text_regions: Vec<&Region> = regions
        .iter()
        .filter(|r| {
            !r.label.is_furniture()
                && !r.label.is_graphic()
                && r.label != Label::InlineFormula
                && r.label != Label::DisplayFormula
                && r.bbox.area() > 2000.0
                && !words.iter().any(|w| {
                    r.bbox
                        .contains_point((w.b.x0 + w.b.x1) / 2.0, (w.b.y0 + w.b.y1) / 2.0)
                })
        })
        .collect();
    if !empty_text_regions.is_empty() && crate::vision::ocr::available() {
        // OCR wants more pixels per character than layout does.
        let hi = render(SCAN_LONG_SIDE, SCAN_MAX_SCALE);
        let ocr = ocr_words(hi.as_ref().unwrap_or(&img), false);
        // Only inside the regions the text layer leaves empty: elsewhere an
        // OCR word's estimated box can miss the text-layer word it repeats.
        let filled: Vec<Tok> = ocr
            .into_iter()
            .filter(|o| {
                let (cx, cy) = ((o.b.x0 + o.b.x1) / 2.0, (o.b.y0 + o.b.y1) / 2.0);
                !words
                    .iter()
                    .any(|w| w.b.intersection(&o.b) > 0.3 * o.b.area().min(w.b.area()))
                    && empty_text_regions
                        .iter()
                        .any(|r| r.bbox.contains_point(cx, cy))
            })
            .collect();
        words.extend(filled);
    }
    let table_fallback = |b: &UserBox| table_markdown(doc, page, b, excluded, base_options);
    // Ruling lines are read once, and only for a page that has a table.
    let paths: std::cell::OnceCell<Vec<pdf_oxide::elements::PathContent>> =
        std::cell::OnceCell::new();
    // (Ruling lines are raw page space; a rotated page's tables are left to
    // the model.)
    let ruled = |b: &UserBox, toks: &[Tok]| {
        if frame.is_some() {
            return None;
        }
        let paths = paths.get_or_init(|| doc.extract_paths(page).unwrap_or_default());
        ruled_structure(paths, geom.bx.3, b, toks)
    };
    let md = assemble(&img, &regions, words, page_box, &ruled, &table_fallback);
    debug_timing(page, "text layer", t_layout, t.elapsed(), regions.len());
    Some((md, table_numbers))
}

/// Converts a page that has no text layer: the layout model places regions
/// and PP-OCR reads the words. `None` when OCR or layout is unavailable.
pub(crate) fn convert_scanned_page(
    renderer: &Renderer,
    page: usize,
    frame: Option<DisplayFrame>,
) -> Option<String> {
    if !crate::vision::ocr::available() {
        return None;
    }
    crate::vision::PageGate::global().run(|| convert_scanned_page_gated(renderer, page, frame))
}

/// The displayed frame of a page with `/Rotate` (`None` for an upright
/// page).
pub(crate) fn display_frame(doc: &PdfDocument, page: usize) -> Option<DisplayFrame> {
    let rot = doc.get_page_rotation(page).ok()?.rem_euclid(360);
    let (a, b, c, d) = doc.get_page_media_box(page).ok()?;
    matches!(rot, 90 | 180 | 270).then_some(DisplayFrame {
        rot,
        media: (a.min(c), b.min(d), a.max(c), b.max(d)),
    })
}

/// Nothing on the page stands out from its background beyond a few specks.
/// Any pixel more than a tenth of the range away from the background counts
/// as marked, light or dark: small print blurs to grey in a thumbnail but
/// stays well off the paper's tone.
fn is_blank(img: &image::RgbImage) -> bool {
    let luma =
        |p: &image::Rgb<u8>| (u32::from(p[0]) * 3 + u32::from(p[1]) * 6 + u32::from(p[2])) / 10;
    let mut hist = [0u32; 256];
    for p in img.pixels() {
        hist[luma(p) as usize] += 1;
    }
    let total: u32 = hist.iter().sum();
    let (mut seen, mut background) = (0u32, 255usize);
    for (v, &n) in hist.iter().enumerate() {
        seen += n;
        if seen * 2 >= total {
            background = v;
            break;
        }
    }
    let marked: u32 = hist
        .iter()
        .enumerate()
        .filter(|(v, _)| v.abs_diff(background) > 24)
        .map(|(_, &n)| n)
        .sum();
    marked * 2000 <= total
}

fn convert_scanned_page_gated(
    renderer: &Renderer,
    page: usize,
    frame: Option<DisplayFrame>,
) -> Option<String> {
    // A blank page (a book's empty verso, a separator sheet) has nothing for
    // the models to find, and a thumbnail shows that for a fraction of the
    // cost of reading it.
    if renderer
        .render(page, 400.0, 1.0)
        .is_some_and(|thumb| is_blank(&thumb.rgb))
    {
        return Some(String::new());
    }
    let t = std::time::Instant::now();
    // In the displayed frame, so the OCR's lines run left to right.
    let img = renderer.render(page, SCAN_LONG_SIDE, SCAN_MAX_SCALE)?;
    let img = match frame {
        Some(f) => img.in_display_frame(f)?,
        None => img,
    };
    let regions = layout::detect(&img)?;
    let t_layout = t.elapsed();
    let words = ocr_words(&img, true);
    let (w, h) = (img.rgb.width() as f32, img.rgb.height() as f32);
    let pb = img.to_user(0.0, 0.0, w, h);
    let md = assemble(
        &img,
        &regions,
        words,
        (pb.x0, pb.y0, pb.x1, pb.y1),
        &|_, _| None,
        &|_| None,
    );
    // OCR writes the inline formulas the layout model did not box as prose.
    let md = super::pdf_math::wrap_math_runs(&md);
    debug_timing(page, "scan", t_layout, t.elapsed(), regions.len());
    Some(md)
}

/// Words of the text layer from glyph positions ([`super::pdf_words`]): word
/// breaks where the text layer has a space character or where the pen moves
/// clearly further than between the line's letters. `None` when the page's
/// glyphs cannot be read or are mostly rotated.
fn glyph_words(
    doc: &PdfDocument,
    page: usize,
    geom: &PageGeom,
    excluded: &[Rect],
) -> Option<Vec<Tok>> {
    let chars = super::pdf_words::on_page(doc.extract_chars(page).ok()?, geom.bx);
    let words = super::pdf_words::glyph_words(&chars)?;
    let theirs: Vec<&str> = geom.words.iter().map(|w| w.text.as_str()).collect();
    if !super::pdf_words::agrees(&words, &theirs) {
        return None;
    }
    let toks = words
        .into_iter()
        .filter(|w| !pdf_furniture::excluded(excluded, w.x0, w.y0, w.x1, w.y1))
        .map(|w| Tok {
            text: expand_ligatures(&w.text),
            b: UserBox {
                x0: w.x0,
                y0: w.y0,
                x1: w.x1,
                y1: w.y1,
            },
            size: w.size,
            glued: w.glued,
            math: w.math,
        })
        .collect();
    Some(drop_doubled(toks))
}

/// Words of the text layer, taken from the converter's own text spans so
/// spacing follows the PDF's space characters and the converter's word-gap
/// rules (word boxes alone misjudge both tight italics and OCR text layers
/// whose glyph widths are wrong). A word that starts a span which the
/// previous span runs into without a space is marked `glued`.
///
/// On a rotated page (`frame`), pdf_oxide reports rotated runs — and on a
/// 180° page all runs — in the displayed frame; the rest are mapped there
/// here, so every word is in one frame. Character offsets stay in raw space,
/// so words inside a mapped run are placed proportionally.
fn span_words(
    doc: &PdfDocument,
    page: usize,
    excluded: &[Rect],
    frame: Option<DisplayFrame>,
) -> Option<Vec<Tok>> {
    let spans = doc.extract_spans(page).ok()?;
    let mut words = Vec::new();
    let mut prev: Option<&pdf_oxide::layout::TextSpan> = None;
    for s in &spans {
        let chars: Vec<char> = s.text.chars().collect();
        let n = chars.len();
        if n == 0 {
            continue;
        }
        let displayed = frame.is_some_and(|f| f.rot == 180 || s.rotation_degrees != 0.0);
        let to_frame = |b: UserBox| match frame {
            Some(f) if !displayed => f.map(&b),
            _ => b,
        };
        let exact = !displayed && s.char_x_offsets.len() == n && s.char_widths.len() == n;
        let x_at = |i: usize| {
            if exact {
                s.char_x_offsets[i]
            } else {
                s.bbox.x + s.bbox.width * i as f32 / n as f32
            }
        };
        let x_end = |j: usize| {
            if exact {
                s.char_x_offsets[j - 1] + s.char_widths[j - 1]
            } else {
                s.bbox.x + s.bbox.width * j as f32 / n as f32
            }
        };
        // Runs on from the previous span: same line, just to its right, and
        // no space character on either side of the seam.
        let runs_on = prev.is_some_and(|p| {
            let same_line = (p.bbox.y + p.bbox.height / 2.0 - (s.bbox.y + s.bbox.height / 2.0))
                .abs()
                < 0.5 * p.bbox.height.max(s.bbox.height);
            let gap = s.bbox.x - (p.bbox.x + p.bbox.width);
            // Broken width tables make spans overlap their successors, so
            // only the start positions are trusted for the "to the right" test.
            same_line
                && s.bbox.x > p.bbox.x
                && gap < 0.1 * s.font_size.min(p.font_size)
                && !p.text.ends_with(char::is_whitespace)
                && !s.text.starts_with(char::is_whitespace)
        });
        // Some text layers carry no space characters at all; their word gaps
        // are only visible in glyph positions. In a long span with no space
        // in it, a gap well above the span's typical letter gap is a word
        // break. Spans that do contain spaces are trusted as written: their
        // odd gaps come from wrong glyph widths, not missing spaces.
        let spaceless = n > 15 && !chars.iter().any(|c| c.is_whitespace());
        let breaks: Vec<bool> = if exact && spaceless {
            let gaps: Vec<f32> = (0..n - 1)
                .map(|k| s.char_x_offsets[k + 1] - (s.char_x_offsets[k] + s.char_widths[k]))
                .collect();
            let mut sorted: Vec<f32> = gaps
                .iter()
                .zip(chars.windows(2))
                .filter(|(_, w)| !w[0].is_whitespace() && !w[1].is_whitespace())
                .map(|(g, _)| *g)
                .collect();
            sorted.sort_by(f32::total_cmp);
            let typical = sorted.get(sorted.len() / 2).copied().unwrap_or(0.0);
            let thr = (0.15 * s.font_size).max(typical + 0.15 * s.font_size);
            gaps.iter().map(|&g| g > thr).collect()
        } else {
            vec![false; n.saturating_sub(1)]
        };
        let mut i = 0;
        while i < n {
            while i < n && chars[i].is_whitespace() {
                i += 1;
            }
            let mut j = i;
            while j < n && !chars[j].is_whitespace() {
                j += 1;
                if j < n && breaks[j - 1] {
                    break;
                }
            }
            if j > i {
                let (x0, x1) = (x_at(i), x_end(j).max(x_at(i) + 0.1));
                let (y0, y1) = (s.bbox.y, s.bbox.y + s.bbox.height);
                if !pdf_furniture::excluded(excluded, x0, y0, x1, y1) {
                    let text: String = chars[i..j].iter().collect();
                    words.push(Tok {
                        text: expand_ligatures(&text),
                        b: to_frame(UserBox { x0, y0, x1, y1 }),
                        size: s.font_size,
                        glued: i == 0 && runs_on,
                        math: false,
                    });
                }
            }
            i = j;
        }
        prev = Some(s);
    }
    Some(drop_doubled(words))
}

/// Removes words the page draws twice in the same place (fake bold, drop
/// shadows, overprinted layers): same text, boxes overlapping by more than
/// half. The reader sees one word; so should the output.
fn drop_doubled(words: Vec<Tok>) -> Vec<Tok> {
    let mut kept: Vec<Tok> = Vec::with_capacity(words.len());
    'next: for w in words {
        for k in kept.iter().rev().take(64) {
            if k.text == w.text && k.b.iou(&w.b) > 0.5 {
                continue 'next;
            }
        }
        kept.push(w);
    }
    kept
}

/// OCR words of a rendered page in user space. Each recognized line is split
/// into words, each given a share of the line's width proportional to its
/// length.
///
/// With `drop_folios`, a line that is nothing but a page number ("31",
/// "-3-", "2.", "Page 12") in the top or bottom 8% of the page is left out:
/// on a scan there is no second page to recognise it by recurrence, and the
/// layout model does not always box it as a page number.
fn ocr_words(img: &PageImage, drop_folios: bool) -> Vec<Tok> {
    static FOLIO: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
        regex::Regex::new(r"(?i)^[-–—(\[]?\s*(?:page\s*|p\.\s*)?\d{1,4}\s*[-–—.)\]]?$")
            .expect("valid regex")
    });
    let ih = img.rgb.height() as f32;
    let mut words: Vec<Tok> = Vec::new();
    for l in crate::vision::ocr::read(&img.rgb).unwrap_or_default() {
        if drop_folios && (l.y1 < 0.08 * ih || l.y0 > 0.92 * ih) && FOLIO.is_match(l.text.trim()) {
            continue;
        }
        let total: usize = l.text.chars().count().max(1);
        let mut pos = 0usize;
        for (i, w) in l.text.split(' ').enumerate() {
            if i > 0 {
                pos += 1;
            }
            let n = w.chars().count();
            if n > 0 {
                let fx0 = l.x0 + (l.x1 - l.x0) * pos as f32 / total as f32;
                let fx1 = l.x0 + (l.x1 - l.x0) * (pos + n) as f32 / total as f32;
                let b = img.to_user(fx0, l.y0, fx1, l.y1);
                words.push(Tok {
                    text: w.to_string(),
                    size: b.height(),
                    b,
                    glued: false,
                    math: false,
                });
            }
            pos += n;
        }
    }
    words
}

fn debug_regions(page: usize, regions: &[Region]) {
    if std::env::var_os("FILEGOBLIN_DEBUG_REGIONS").is_some() {
        for r in regions {
            eprintln!(
                "region page {} #{} {:?} {:.2} ({:.1}, {:.1}, {:.1}, {:.1})",
                page + 1,
                r.order,
                r.label,
                r.score,
                r.bbox.x0,
                r.bbox.y0,
                r.bbox.x1,
                r.bbox.y1
            );
        }
    }
}

fn debug_timing(
    page: usize,
    kind: &str,
    layout: std::time::Duration,
    total: std::time::Duration,
    n: usize,
) {
    if std::env::var_os("FILEGOBLIN_DEBUG").is_some() {
        eprintln!(
            "vision page {} ({kind}): layout {layout:?}, total {total:?}, {n} regions",
            page + 1
        );
    }
}

fn assemble(
    img: &PageImage,
    regions: &[Region],
    words: Vec<Tok>,
    bx: (f32, f32, f32, f32),
    ruled: &dyn Fn(&UserBox, &[Tok]) -> Option<crate::vision::table::Structure>,
    table_fallback: &dyn Fn(&UserBox) -> Option<String>,
) -> String {
    let (_, by0, _, by1) = bx;
    let ph = (by1 - by0).max(1.0);

    // Furniture regions only count near the top or bottom edge: a model that
    // calls a mid-page line a "header" is wrong more often than not.
    let mut furniture: Vec<&Region> = regions
        .iter()
        .filter(|r| r.label.is_furniture())
        .filter(|r| {
            let cy = (r.bbox.y0 + r.bbox.y1) / 2.0;
            cy > by1 - 0.2 * ph || cy < by0 + 0.2 * ph
        })
        .collect();
    // A journal's imprint at the foot or head of an article's first page
    // ("INTERNATIONAL JOURNAL OF …, VOL. 8, 71–90 (1993) © 1993 John Wiley &
    // Sons, Inc. CCC 0884-8173/93/010071-20") is publication furniture that
    // appears once, so recurrence cannot find it and the layout model
    // usually calls it text.
    let imprint: Vec<&Region> = regions
        .iter()
        .filter(|r| !r.label.is_furniture() && !r.label.is_graphic())
        .filter(|r| {
            let cy = (r.bbox.y0 + r.bbox.y1) / 2.0;
            cy > by1 - 0.18 * ph || cy < by0 + 0.18 * ph
        })
        .filter(|r| {
            let text: Vec<&str> = words
                .iter()
                .filter(|w| {
                    r.bbox
                        .contains_point((w.b.x0 + w.b.x1) / 2.0, (w.b.y0 + w.b.y1) / 2.0)
                })
                .map(|w| w.text.as_str())
                .collect();
            text.len() <= 40 && is_imprint(&text.join(" "))
        })
        .collect();
    furniture.extend(imprint.iter().copied());
    let content: Vec<&Region> = regions
        .iter()
        .filter(|r| {
            !r.label.is_furniture()
                && r.label != Label::InlineFormula
                && !imprint.iter().any(|f| std::ptr::eq(*f, *r))
        })
        .collect();

    // Recognize formulas: display formulas replace their region's text,
    // inline formulas replace the words under them.
    // Inside a table, a value like "48.96 ± 2.84" reads better — and is
    // searched for — as the text it is than as LaTeX; the text layer (or
    // OCR) already has it.
    let tables: Vec<&Region> = regions.iter().filter(|r| r.label == Label::Table).collect();
    let mut inline: Vec<Region> = regions
        .iter()
        .filter(|r| r.label == Label::InlineFormula)
        .filter(|r| {
            let (cx, cy) = ((r.bbox.x0 + r.bbox.x1) / 2.0, (r.bbox.y0 + r.bbox.y1) / 2.0);
            !tables.iter().any(|t| t.bbox.contains_point(cx, cy))
        })
        .cloned()
        .collect();
    complete_inline_formulas(&mut inline, &words, regions);
    let display_idx: Vec<usize> = content
        .iter()
        .enumerate()
        .filter(|(_, r)| r.label == Label::DisplayFormula)
        .map(|(i, _)| i)
        .collect();
    let mut latex_display: Vec<Option<String>> = vec![None; content.len()];
    let mut latex_inline: Vec<Option<String>> = vec![None; inline.len()];
    if formula::available() && (!display_idx.is_empty() || !inline.is_empty()) {
        let crops_d: Vec<_> = display_idx
            .iter()
            .map(|&i| crop(img, &content[i].bbox))
            .collect();
        for (k, l) in formula::recognize(&crops_d, 400).into_iter().enumerate() {
            latex_display[display_idx[k]] = l;
        }
        let crops_i: Vec<_> = inline.iter().map(|r| crop(img, &r.bbox)).collect();
        latex_inline = formula::recognize(&crops_i, 160);
    }

    // Tokens: words minus furniture and words under a recognized inline
    // formula; plus one token per recognized inline formula.
    let mut toks: Vec<Tok> = Vec::new();
    let page_has_math =
        latex_inline.iter().any(Option::is_some) || latex_display.iter().any(Option::is_some);
    for w in &words {
        let (cx, cy) = ((w.b.x0 + w.b.x1) / 2.0, (w.b.y0 + w.b.y1) / 2.0);
        if furniture.iter().any(|r| r.bbox.contains_point(cx, cy)) {
            continue;
        }
        let under_formula = inline
            .iter()
            .zip(latex_inline.iter())
            .any(|(r, l)| l.is_some() && covers(&r.bbox, &w.b));
        if !under_formula {
            let mut w = w.clone();
            // On a page that carries LaTeX, a literal dollar sign in the
            // prose would open a formula for every math-aware reader.
            if page_has_math && w.text.contains('$') {
                w.text = w.text.replace('$', "\\$");
            }
            toks.push(w);
        }
    }
    for (r, l) in inline.iter().zip(latex_inline.iter()) {
        let Some(l) = l else { continue };
        // The detector's box often takes in the sentence punctuation after a
        // formula; that punctuation belongs to the prose, outside the `$`.
        let body = l.trim_end_matches([',', '.', ';', ':', ' ']);
        if body.is_empty() {
            continue;
        }
        let mid = (r.bbox.y0 + r.bbox.y1) / 2.0;
        let trail = words
            .iter()
            .filter(|w| {
                covers(&r.bbox, &w.b) && (w.b.y0 + w.b.y1) / 2.0 <= mid + 0.25 * r.bbox.height()
            })
            .max_by(|a, b| a.b.x1.total_cmp(&b.b.x1))
            .and_then(|w| w.text.chars().last())
            .filter(|c| matches!(c, ',' | '.' | ';' | ':'));
        let mut text = format!("${body}$");
        if let Some(c) = trail {
            text.push(c);
        }
        toks.push(Tok {
            text,
            b: r.bbox,
            size: r.bbox.height(),
            glued: false,
            math: false,
        });
    }

    // Assign every token to the smallest content region holding its centre.
    let mut members: Vec<Vec<usize>> = vec![Vec::new(); content.len()];
    let mut orphans: Vec<usize> = Vec::new();
    // Detected boxes are drawn tight and sometimes clip a paragraph's first
    // glyph (a raised or dropped initial, a first letter set in its own
    // font). A token just outside a box, within half an em, still belongs to
    // it when no box holds it outright.
    for (ti, t) in toks.iter().enumerate() {
        let (cx, cy) = ((t.b.x0 + t.b.x1) / 2.0, (t.b.y0 + t.b.y1) / 2.0);
        let holding = |margin: f32| {
            content
                .iter()
                .enumerate()
                .filter(|(_, r)| {
                    let b = r.bbox;
                    cx >= b.x0 - margin
                        && cx <= b.x1 + margin
                        && cy >= b.y0 - margin
                        && cy <= b.y1 + margin
                })
                .min_by(|a, b| a.1.bbox.area().total_cmp(&b.1.bbox.area()))
                .map(|(i, _)| i)
        };
        let lettered = t.text.chars().any(char::is_alphabetic) && !t.text.starts_with('$');
        let best = holding(0.0).or_else(|| {
            lettered
                .then(|| holding(0.5 * t.size.max(t.b.height())))
                .flatten()
        });
        match best {
            Some(i) => members[i].push(ti),
            None => orphans.push(ti),
        }
    }

    // Blocks in reading order: (order key, markdown).
    let mut blocks: Vec<(f32, String)> = Vec::new();
    for (i, r) in content.iter().enumerate() {
        let key = r.order as f32;
        let text = || lines_to_text(&toks, &members[i]);
        let md = match r.label {
            l if l.is_graphic() => String::new(),
            Label::DisplayFormula => match &latex_display[i] {
                Some(l) => format!("$$\n{l}\n$$"),
                None => text(),
            },
            Label::DocTitle => heading(1, &text()),
            // A caption is one line of text however it wraps on the page, so
            // a table's title can be recognised as its heading.
            Label::FigureTitle => text().split_whitespace().collect::<Vec<_>>().join(" "),
            Label::ParagraphTitle => {
                let t = text();
                heading(heading_level(&t), &t)
            }
            Label::Table => ruled(&r.bbox, &toks)
                .and_then(|st| grid_markdown(&st, &toks, &members[i]))
                .or_else(|| {
                    crate::vision::table::recognize(img, &r.bbox)
                        .and_then(|st| grid_markdown(&st, &toks, &members[i]))
                })
                .or_else(|| table_fallback(&r.bbox))
                .unwrap_or_else(text),
            _ => text(),
        };
        if !md.trim().is_empty() {
            blocks.push((key, md));
        }
    }

    // Words no region claimed: group them into blocks and slot each block in
    // after the region just above it in the same column.
    for blk in group_orphans(&toks, &orphans) {
        let bb = blk
            .iter()
            .map(|&i| toks[i].b)
            .reduce(|a, b| a.union(&b))
            .expect("non-empty block");
        let key = orphan_key(&bb, &content);
        let md = lines_to_text(&toks, &blk);
        let cy = (bb.y0 + bb.y1) / 2.0;
        if (cy > by1 - 0.18 * ph || cy < by0 + 0.18 * ph) && blk.len() <= 40 && is_imprint(&md) {
            continue;
        }
        // A lone glyph no region claimed (a QED box, a stray bracket from an
        // equation number) is noise, not content.
        if md.chars().filter(|c| c.is_alphanumeric()).count() <= 1 {
            continue;
        }
        if !md.trim().is_empty() {
            blocks.push((key, md));
        }
    }

    blocks.sort_by(|a, b| a.0.total_cmp(&b.0));
    let blocks: Vec<(f32, String)> = blocks
        .into_iter()
        .map(|(k, md)| (k, merge_split_inline_formulas(&md)))
        .collect();
    let mut out = String::new();
    for (_, md) in blocks {
        let md = md.trim();
        if !out.is_empty() {
            // A paragraph cut by a column or region break continues: the
            // previous block stops mid-sentence and this one starts in
            // lowercase. Joining with a single newline keeps it one paragraph
            // (and lets line-break hyphenation heal across the cut).
            out.push_str(if continues(&out, md) { "\n" } else { "\n\n" });
        }
        out.push_str(md);
    }
    if page_has_math {
        super::pdf_math::join_split_inline(&super::pdf_math::absorb_operator_names(&out))
    } else {
        out
    }
}

/// Inline formulas completed from the text layer, which knows which words
/// are set in math fonts. The layout model boxes inline formulas tightly and
/// misses some: a box is grown over math words that touch it on its line
/// (`T ΔE` boxed as `ΔE`), and a run of two or more math words, or one word
/// of several math glyphs, that no formula box holds becomes a formula of
/// its own. Words inside tables, figures, display formulas and page
/// furniture are left alone.
fn complete_inline_formulas(inline: &mut Vec<Region>, words: &[Tok], regions: &[Region]) {
    let centre = |b: &UserBox| ((b.x0 + b.x1) / 2.0, (b.y0 + b.y1) / 2.0);
    let elsewhere = |t: &Tok| {
        let (cx, cy) = centre(&t.b);
        regions.iter().any(|r| {
            (r.label == Label::Table
                || r.label == Label::DisplayFormula
                || r.label.is_graphic()
                || r.label.is_furniture())
                && r.bbox.contains_point(cx, cy)
        })
    };
    let math: Vec<&Tok> = words.iter().filter(|t| t.math && !elsewhere(t)).collect();
    if math.is_empty() {
        return;
    }
    let same_line = |a: &UserBox, b: &UserBox| {
        let overlap = a.y1.min(b.y1) - a.y0.max(b.y0);
        overlap >= 0.5 * a.height().min(b.height()).max(0.1)
    };
    let touches = |a: &UserBox, t: &Tok| {
        let gap = (t.b.x0 - a.x1).max(a.x0 - t.b.x1);
        same_line(a, &t.b) && gap <= 0.3 * t.size.max(1.0)
    };
    for r in inline.iter_mut() {
        loop {
            let grow = math.iter().find(|t| {
                let (cx, cy) = centre(&t.b);
                !r.bbox.contains_point(cx, cy) && touches(&r.bbox, t)
            });
            match grow {
                Some(t) => r.bbox = r.bbox.union(&t.b),
                None => break,
            }
        }
    }
    // Runs of math words no formula holds, left to right on their lines.
    let mut loose: Vec<&Tok> = math
        .into_iter()
        .filter(|t| {
            let (cx, cy) = centre(&t.b);
            !inline.iter().any(|r| r.bbox.contains_point(cx, cy))
        })
        .collect();
    // Lines top to bottom (a word joins the line whose first word it
    // overlaps), then left to right: a total order, as sorting needs.
    loose.sort_by(|a, b| centre(&b.b).1.total_cmp(&centre(&a.b).1));
    let mut line_of: Vec<usize> = Vec::with_capacity(loose.len());
    let mut firsts: Vec<UserBox> = Vec::new();
    for t in &loose {
        match firsts.iter().rposition(|f| same_line(f, &t.b)) {
            Some(l) => line_of.push(l),
            None => {
                firsts.push(t.b);
                line_of.push(firsts.len() - 1);
            }
        }
    }
    let mut order: Vec<usize> = (0..loose.len()).collect();
    order.sort_by(|&a, &b| {
        line_of[a]
            .cmp(&line_of[b])
            .then(loose[a].b.x0.total_cmp(&loose[b].b.x0))
    });
    let loose: Vec<&Tok> = order.into_iter().map(|i| loose[i]).collect();
    let mut runs: Vec<(UserBox, usize, usize)> = Vec::new(); // (box, words, glyphs)
    for t in loose {
        match runs.last_mut() {
            Some((b, n, g)) if touches(b, t) && t.b.x0 >= b.x0 => {
                *b = b.union(&t.b);
                *n += 1;
                *g += t.text.chars().count();
            }
            _ => runs.push((t.b, 1, t.text.chars().count())),
        }
    }
    for (bbox, n, glyphs) in runs {
        if n >= 2 || glyphs >= 3 {
            inline.push(Region {
                label: Label::InlineFormula,
                score: 0.5,
                bbox,
                order: 0,
            });
        }
    }
}

/// True when `next` reads as the continuation of the paragraph that ends
/// `prev`: prose on both sides, no sentence end before the break, and a
/// lowercase start after it.
fn continues(prev: &str, next: &str) -> bool {
    let structural = |s: &str| {
        let s = s.trim_start();
        s.starts_with('#') || s.starts_with('|') || s.starts_with("$$") || s.starts_with("- ")
    };
    let last_line = prev.lines().last().unwrap_or("");
    if structural(last_line) || structural(next) || last_line.trim_end().ends_with("$$") {
        return false;
    }
    let ends_open = last_line
        .trim_end()
        .chars()
        .last()
        .is_some_and(|c| c.is_alphabetic() || matches!(c, ',' | '-' | '–'));
    let starts_lower = next.chars().next().is_some_and(|c| c.is_lowercase());
    ends_open && starts_lower
}

/// Rejoins an inline formula that a line break split in two
/// (`$0\leq k\leq$` ⏎ `$2^{N}-1$`): a line that ends in a formula whose last
/// token is a relation or operator, followed by a line that starts with a
/// formula, is one expression. The joined formula ends the first line.
fn merge_split_inline_formulas(md: &str) -> String {
    static SPLIT: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
        regex::Regex::new(r"\$([^$\n]+)\$\n\$([^$\n]+)\$").expect("valid regex")
    });
    const OPS: [&str; 16] = [
        "=",
        "<",
        ">",
        "+",
        "-",
        "\\leq",
        "\\geq",
        "\\le",
        "\\ge",
        "\\neq",
        "\\in",
        "\\to",
        "\\times",
        "\\cdot",
        "\\approx",
        "\\subseteq",
    ];
    let mut out = md.to_string();
    // Repeat: a formula can be split over three lines.
    for _ in 0..3 {
        let next = SPLIT
            .replace_all(&out, |c: &regex::Captures| {
                let (a, b) = (&c[1], &c[2]);
                let joins = OPS.iter().any(|op| a.trim_end().ends_with(op))
                    || ["=", "+", "-", "<", ">", "\\leq", "\\geq"]
                        .iter()
                        .any(|op| b.starts_with(op));
                if joins {
                    format!("${a} {b}$\n")
                } else {
                    c[0].to_string()
                }
            })
            .into_owned();
        if next == out {
            break;
        }
        out = next;
    }
    out
}

/// Typographic ligature code points spelled out (`ﬁ` → `fi`), as the
/// converter does for its own output.
fn expand_ligatures(s: &str) -> String {
    if !s.chars().any(|c| ('\u{FB00}'..='\u{FB06}').contains(&c)) {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len() + 4);
    for c in s.chars() {
        match c {
            '\u{FB00}' => out.push_str("ff"),
            '\u{FB01}' => out.push_str("fi"),
            '\u{FB02}' => out.push_str("fl"),
            '\u{FB03}' => out.push_str("ffi"),
            '\u{FB04}' => out.push_str("ffl"),
            '\u{FB05}' | '\u{FB06}' => out.push_str("st"),
            _ => out.push(c),
        }
    }
    out
}

/// True when most of the word's box lies inside `b`.
fn covers(b: &UserBox, w: &UserBox) -> bool {
    w.intersection(b) >= 0.5 * w.area().max(0.01)
}

/// A publisher's imprint line: a copyright notice with a year, a journal
/// citation ("VOL. 8, 71–90 (1993)"), or a copyright-clearance / ISSN code.
fn is_imprint(text: &str) -> bool {
    static IMPRINT: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
        regex::Regex::new(
            r"(?i)(?:©|\(c\)|copyright)\s*(?:©\s*)?(?:1[89]|20)\d\d\b|\bCCC\s*\d{4}-\d{3}[\dX]|\bISSN\s*:?\s*\d{4}-\d{3}[\dX]|\bvol\.?\s*\d+\s*,?\s*(?:no\.?\s*\d+\s*,?\s*)?(?:pp\.?\s*)?\d+\s*[-–]\s*\d+\s*\((?:1[89]|20)\d\d\)",
        )
        .expect("valid regex")
    });
    IMPRINT.is_match(text)
}

fn crop(img: &PageImage, b: &UserBox) -> image::RgbImage {
    // Exactly the detected box, then a white margin: the recognizer wants
    // some air around a formula, but real pixels from the margin would bring
    // in descenders and accents of the lines above and below.
    let (x0, y0, x1, y1) = img.to_px(b);
    let (w, h) = (
        (x1.saturating_sub(x0)).max(1),
        (y1.saturating_sub(y0)).max(1),
    );
    let inner = image::imageops::crop_imm(&img.rgb, x0, y0, w, h).to_image();
    let pad = (h / 8).max(2);
    let mut out =
        image::RgbImage::from_pixel(w + 2 * pad, h + 2 * pad, image::Rgb([255, 255, 255]));
    image::imageops::replace(&mut out, &inner, i64::from(pad), i64::from(pad));
    out
}

fn heading(level: usize, text: &str) -> String {
    let one_line = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if one_line.is_empty() {
        return String::new();
    }
    format!("{} {one_line}", "#".repeat(level))
}

/// `3.2 Results` reads as a subsection; anything else as a section.
fn heading_level(text: &str) -> usize {
    let first = text.split_whitespace().next().unwrap_or("");
    let dots = first.trim_end_matches('.').matches('.').count();
    let numbered = first.chars().next().is_some_and(|c| c.is_ascii_digit())
        && first.chars().all(|c| c.is_ascii_digit() || c == '.');
    if numbered { (2 + dots).min(4) } else { 2 }
}

/// Groups tokens into visual lines (top to bottom) and joins them.
fn lines_to_text(toks: &[Tok], idx: &[usize]) -> String {
    let lines = visual_lines(toks, idx);
    let mut out = String::new();
    for l in &lines {
        let line = join_line(toks, l);
        if out.ends_with('\u{ad}') {
            // A soft hyphen marks a syllable break by definition: rejoin.
            out.pop();
            out.push_str(line.trim_start());
            continue;
        }
        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str(&line);
    }
    // Soft hyphens anywhere else are invisible break hints.
    out.replace('\u{ad}', "")
}

/// Joins one visual line's tokens. Closing punctuation hugs the token before
/// it, and so does a word whose span the text layer runs on from the
/// previous one without a space (a separately-encoded apostrophe, a
/// footnote mark or significance star, a word split across two spans).
fn join_line(toks: &[Tok], line: &[usize]) -> String {
    let mut s = String::new();
    let mut prev: Option<&Tok> = None;
    for &i in line {
        let tok = &toks[i];
        let t = tok.text.as_str();
        // Spacing is the text layer's: a word joins the previous one only
        // when its span runs on from the previous span with no space between
        // them (and the previous token really is that neighbour).
        let hug = t
            .chars()
            .all(|c| matches!(c, ',' | '.' | ';' | ':' | ')' | '!' | '?'))
            // A hyphen set as its own word at the end of a line is the
            // line-break hyphen of the word before it.
            || ((t == "-" || t == "\u{ad}") && line.last() == Some(&i))
            // A hyphen set against the word after it but apart from the
            // word before, by less than a word space ("ATTENTION -BASED"),
            // joins the compound.
            || (t.starts_with('-')
                && t[1..].starts_with(char::is_alphabetic)
                && prev.is_some_and(|p| {
                    p.text.ends_with(char::is_alphabetic)
                        && tok.b.x0 - p.b.x1 < 0.25 * p.size.max(1.0)
                }))
            // A lone sign or currency symbol set just before a number
            // belongs to it ("− 92" → "−92").
            || prev.is_some_and(|p| {
                matches!(p.text.as_str(), "−" | "±" | "$" | "€" | "£")
                    && t.starts_with(|c: char| c.is_ascii_digit() || c == '.')
                    && tok.b.x0 - p.b.x1 < 0.35 * p.size.max(1.0)
            })
            || (tok.glued
                && prev.is_some_and(|p| {
                    !p.text.ends_with('$') && tok.b.x0 > p.b.x0 && tok.b.x0 - p.b.x1 < p.size
                }));
        if !s.is_empty() && !hug {
            s.push(' ');
        }
        s.push_str(t);
        prev = Some(tok);
    }
    s
}

/// Clusters tokens into lines by their vertical centres; each line sorted
/// left to right, lines sorted top to bottom. A token joins a line when its
/// centre is within half a typical text height of the line's mean centre —
/// line bands are never grown, so one tall token (a formula, a drop cap)
/// cannot glue neighbouring lines together.
fn visual_lines(toks: &[Tok], idx: &[usize]) -> Vec<Vec<usize>> {
    if idx.is_empty() {
        return Vec::new();
    }
    let mut heights: Vec<f32> = idx
        .iter()
        .filter(|&&i| !toks[i].text.starts_with('$'))
        .map(|&i| toks[i].b.height())
        .collect();
    if heights.is_empty() {
        heights = idx.iter().map(|&i| toks[i].b.height()).collect();
    }
    heights.sort_by(f32::total_cmp);
    let h_ref = heights[heights.len() / 2].max(0.5);
    let centre = |i: usize| (toks[i].b.y0 + toks[i].b.y1) / 2.0;
    let mut order: Vec<usize> = idx.to_vec();
    order.sort_by(|&a, &b| centre(b).total_cmp(&centre(a)));
    // (sum of centres, count, members)
    let mut lines: Vec<(f32, f32, Vec<usize>)> = Vec::new();
    for i in order {
        let c = centre(i);
        let hit = lines
            .iter_mut()
            .rev()
            .take(3)
            .find(|(sum, n, _)| (c - *sum / *n).abs() <= 0.5 * h_ref);
        match hit {
            Some((sum, n, m)) => {
                *sum += c;
                *n += 1.0;
                m.push(i);
            }
            None => lines.push((c, 1.0, vec![i])),
        }
    }
    lines.sort_by(|a, b| (b.0 / b.1).total_cmp(&(a.0 / a.1)));
    lines
        .into_iter()
        .map(|(_, _, mut m)| {
            m.sort_by(|&a, &b| toks[a].b.x0.total_cmp(&toks[b].b.x0));
            m
        })
        .collect()
}

/// Groups unclaimed tokens into blocks of consecutive, horizontally
/// overlapping lines.
fn group_orphans(toks: &[Tok], orphans: &[usize]) -> Vec<Vec<usize>> {
    if orphans.is_empty() {
        return Vec::new();
    }
    let lines = visual_lines(toks, orphans);
    let bbox = |l: &[usize]| {
        l.iter()
            .map(|&i| toks[i].b)
            .reduce(|a, b| a.union(&b))
            .expect("non-empty line")
    };
    let mut blocks: Vec<(UserBox, f32, Vec<usize>)> = Vec::new();
    for l in lines {
        let lb = bbox(&l);
        let size = l
            .iter()
            .map(|&i| toks[i].size)
            .fold(0.0f32, f32::max)
            .max(lb.height());
        let joined = blocks.last_mut().filter(|(bb, sz, _)| {
            let gap = bb.y0 - lb.y1;
            let hov = (bb.x1.min(lb.x1) - bb.x0.max(lb.x0)).max(0.0);
            gap < 1.2 * sz.max(size) && hov > 0.0
        });
        match joined {
            Some((bb, sz, m)) => {
                *bb = bb.union(&lb);
                *sz = sz.max(size);
                m.extend(l);
            }
            None => blocks.push((lb, size, l)),
        }
    }
    blocks.into_iter().map(|(_, _, m)| m).collect()
}

/// Reading-order key for an unclaimed block: just after the nearest region
/// above it that shares its column, else just before the nearest region
/// below it, else at the end.
fn orphan_key(bb: &UserBox, content: &[&Region]) -> f32 {
    let hov = |r: &Region| (r.bbox.x1.min(bb.x1) - r.bbox.x0.max(bb.x0)).max(0.0);
    let above = content
        .iter()
        .filter(|r| hov(r) > 0.0 && r.bbox.y0 >= bb.y1 - 2.0)
        .min_by(|a, b| (a.bbox.y0 - bb.y1).total_cmp(&(b.bbox.y0 - bb.y1)));
    if let Some(r) = above {
        return r.order as f32 + 0.5;
    }
    let below = content
        .iter()
        .filter(|r| hov(r) > 0.0 && r.bbox.y1 <= bb.y0 + 2.0)
        .min_by(|a, b| (bb.y0 - a.bbox.y1).total_cmp(&(bb.y0 - b.bbox.y1)));
    if let Some(r) = below {
        return r.order as f32 - 0.5;
    }
    f32::MAX
}

/// "Table 3: …", "TABLE II. …", "Exhibit 4 – …": a table's title.
static TABLE_TITLE: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
    regex::Regex::new(
        r"^(?:Table|TABLE|Exhibit|EXHIBIT)\s+(?:[0-9]+[A-Za-z]?(?:[-.][0-9]+)*|[IVXLC]+)\b",
    )
    .expect("valid regex")
});

static LEADER: std::sync::LazyLock<regex::Regex> =
    std::sync::LazyLock::new(|| regex::Regex::new(r"(?:\s?\.){4,}").expect("valid regex"));

/// Fills the model's rows × columns grid with the region's tokens and writes
/// a pipe table. A spanning cell's text is repeated in every cell it covers,
/// so each row and column still reads on its own.
fn grid_markdown(
    st: &crate::vision::table::Structure,
    toks: &[Tok],
    idx: &[usize],
) -> Option<String> {
    let (nr, nc) = (st.rows.len(), st.cols.len());
    let nearest = |v: f32, spans: &[(f32, f32)]| -> usize {
        spans
            .iter()
            .enumerate()
            .min_by(|a, b| {
                let da = if v >= a.1.0 && v <= a.1.1 {
                    0.0
                } else {
                    (v - (a.1.0 + a.1.1) / 2.0).abs()
                };
                let db = if v >= b.1.0 && v <= b.1.1 {
                    0.0
                } else {
                    (v - (b.1.0 + b.1.1) / 2.0).abs()
                };
                da.total_cmp(&db)
            })
            .map(|(i, _)| i)
            .unwrap_or(0)
    };
    let rspan: Vec<(f32, f32)> = st.rows.iter().map(|r| (r.y0, r.y1)).collect();
    let cspan: Vec<(f32, f32)> = st.cols.iter().map(|c| (c.x0, c.x1)).collect();
    let mut cells: Vec<Vec<Vec<usize>>> = vec![vec![Vec::new(); nc]; nr];
    for &ti in idx {
        let t = &toks[ti];
        let (cx, cy) = ((t.b.x0 + t.b.x1) / 2.0, (t.b.y0 + t.b.y1) / 2.0);
        cells[nearest(cy, &rspan)][nearest(cx, &cspan)].push(ti);
    }
    // Rows the model merged: the row-label cell holds several lines, and
    // another column has lines at the same heights — each line is a row of
    // its own. (A value with its standard error beneath it has one label
    // line, and stays one row.)
    if let Some(split) = split_stacked_rows(st, toks, &cells) {
        return grid_markdown(&split, toks, idx);
    }
    // A cell's lines read as one: a line ending in a hyphen continues
    // without a space ("Automotive-⏎Industrial").
    let cell_text = |members: &[usize]| {
        let mut text = String::new();
        for l in visual_lines(toks, members) {
            let line = join_line(toks, &l);
            if !text.is_empty() && !text.ends_with('-') {
                text.push(' ');
            }
            text.push_str(&line);
        }
        // Dot leaders ("Hettinger.........") are layout, not content.
        LEADER.replace_all(&text, "").trim().replace('|', "\\|")
    };
    let mut grid: Vec<Vec<String>> = cells
        .iter()
        .map(|row| row.iter().map(|c| cell_text(c)).collect())
        .collect();
    // Each cell belongs to one span only, the largest. Reading a span from
    // cells an earlier span had already filled repeated its text once per
    // overlap (the model's spans overlap on dense forms: "Form 1040 … Form
    // 1040 …").
    let mut claimed = vec![vec![false; nc]; nr];
    let mut spans: Vec<&UserBox> = st.spans.iter().collect();
    spans.sort_by(|a, b| b.area().total_cmp(&a.area()));
    for sp in spans {
        let rs: Vec<usize> = (0..nr)
            .filter(|&r| {
                (st.rows[r].y1.min(sp.y1) - st.rows[r].y0.max(sp.y0)).max(0.0)
                    > 0.5 * st.rows[r].height()
            })
            .collect();
        let cs: Vec<usize> = (0..nc)
            .filter(|&c| {
                (st.cols[c].x1.min(sp.x1) - st.cols[c].x0.max(sp.x0)).max(0.0)
                    > 0.5 * st.cols[c].width()
            })
            .collect();
        if rs.len() * cs.len() < 2 || rs.iter().any(|&r| cs.iter().any(|&c| claimed[r][c])) {
            continue;
        }
        // A span is read from all of its words at once, in visual order:
        // read cell by cell, a heading centred over two columns came out as
        // "KE 10 mg/kg 3 YTRUDA every 2 or weeks".
        let mut members: Vec<usize> = Vec::new();
        for &r in &rs {
            for &c in &cs {
                members.extend_from_slice(&cells[r][c]);
            }
        }
        let text = cell_text(&members);
        for &r in &rs {
            for &c in &cs {
                grid[r][c] = text.clone();
                claimed[r][c] = true;
            }
        }
    }
    // Drop empty rows and columns.
    grid.retain(|row| row.iter().any(|c| !c.is_empty()));
    // A title the table region took in as its first row ("Table 3: …" in
    // one cell, or across all of them) is the table's caption, not its
    // header: it goes above the table.
    let mut title = None;
    if let Some(first) = grid.first() {
        let filled: Vec<&String> = first.iter().filter(|c| !c.is_empty()).collect();
        let one_text = !filled.is_empty() && filled.iter().all(|c| *c == filled[0]);
        if one_text && TABLE_TITLE.is_match(filled[0]) && grid.len() > 2 {
            title = Some(filled[0].replace("\\|", "|"));
            grid.remove(0);
        }
    }
    let keep: Vec<usize> = (0..nc)
        .filter(|&c| grid.iter().any(|row| !row[c].is_empty()))
        .collect();
    if grid.len() < 2 || keep.len() < 2 {
        return None;
    }
    let mut out = String::new();
    if let Some(t) = title {
        out.push_str(&t);
        out.push_str("\n\n");
    }
    for (ri, row) in grid.iter().enumerate() {
        out.push('|');
        for &c in &keep {
            out.push(' ');
            out.push_str(&row[c]);
            out.push_str(" |");
        }
        out.push('\n');
        if ri == 0 {
            out.push('|');
            for _ in &keep {
                out.push_str("---|");
            }
            out.push('\n');
        }
    }
    Some(out.trim_end().to_string())
}

/// The structure a fully ruled table draws for itself. When the ruling lines
/// inside a table region close into a lattice of cells that tiles the region
/// and holds its words, those cells are the table exactly as printed — rows,
/// columns and merged cells — and need no estimate from the model. Tables
/// ruled only in part (booktabs rules, row lines without column lines) are
/// left to the model.
fn ruled_structure(
    paths: &[pdf_oxide::elements::PathContent],
    page_top: f32,
    region: &UserBox,
    toks: &[Tok],
) -> Option<crate::vision::table::Structure> {
    use crate::parsers::pdf_table as pt;
    // Top-down coordinates, as the lattice code works.
    let (rx0, rx1) = (region.x0 - 4.0, region.x1 + 4.0);
    let (rt, rb) = (page_top - region.y1 - 4.0, page_top - region.y0 + 4.0);
    let edges: Vec<pt::Edge> = super::pdf::ruling_edges(paths, page_top)
        .into_iter()
        .filter(|e| e.x0 >= rx0 && e.x1 <= rx1 && e.top >= rt && e.bottom <= rb)
        .collect();
    if edges.len() < 6 {
        return None;
    }
    let settings = pt::TableSettings::default();
    let tol = settings.intersection_tolerance;
    let snapped = pt::snap_edges(&edges, settings.snap_tolerance, settings.snap_tolerance);
    let merged = pt::merge_edges(&snapped, &settings);
    let cells = pt::intersections_to_cells(&pt::find_intersections(&merged, tol, tol), tol);
    let lattice = pt::cells_to_tables(&cells, tol)
        .into_iter()
        .max_by_key(|t| t.len())?;
    let bx0 = lattice.iter().map(|c| c.x0).fold(f32::MAX, f32::min);
    let bx1 = lattice.iter().map(|c| c.x1).fold(f32::MIN, f32::max);
    let bt = lattice.iter().map(|c| c.top).fold(f32::MAX, f32::min);
    let bb = lattice.iter().map(|c| c.bottom).fold(f32::MIN, f32::max);
    let area = (bx1 - bx0).max(0.0) * (bb - bt).max(0.0);
    let region_area = region.area().max(1.0);
    // The lattice is this table: it covers the region, and its cells tile it.
    let cell_area: f32 = lattice.iter().map(|c| c.width() * c.height()).sum();
    if area < 0.7 * region_area || area > 1.4 * region_area || cell_area < 0.9 * area {
        return None;
    }
    // Every word of the region level with the lattice's rows sits in a
    // cell: a column with no ruling at its outer edge lies outside the
    // lattice, beside it. (A title or notes above or below are not rows.)
    let inside: Vec<&Tok> = toks
        .iter()
        .filter(|t| {
            let (cx, cy) = ((t.b.x0 + t.b.x1) / 2.0, page_top - (t.b.y0 + t.b.y1) / 2.0);
            cx >= bx0 && cx <= bx1 && cy >= bt && cy <= bb
        })
        .collect();
    let level = toks
        .iter()
        .filter(|t| {
            let (cx, cy) = ((t.b.x0 + t.b.x1) / 2.0, page_top - (t.b.y0 + t.b.y1) / 2.0);
            cx >= rx0 && cx <= rx1 && cy >= bt && cy <= bb
        })
        .count();
    let held = inside
        .iter()
        .filter(|t| {
            let (cx, cy) = ((t.b.x0 + t.b.x1) / 2.0, page_top - (t.b.y0 + t.b.y1) / 2.0);
            lattice.iter().any(|c| c.contains_point(cx, cy, 0.5))
        })
        .count();
    if inside.len() < 2 || held * 10 < level.max(inside.len()) * 9 {
        return None;
    }
    // Typical text height in the table.
    let size = {
        let mut h: Vec<f32> = inside.iter().map(|t| t.b.height()).collect();
        h.sort_by(f32::total_cmp);
        h[h.len() / 2].max(1.0)
    };
    // Grid lines: every distinct cell edge, merged within the tolerance.
    let lines = |mut v: Vec<f32>| {
        v.sort_by(f32::total_cmp);
        let mut out: Vec<f32> = Vec::new();
        for x in v {
            if out.last().is_none_or(|&l| x - l > tol) {
                out.push(x);
            }
        }
        out
    };
    let xs = lines(lattice.iter().flat_map(|c| [c.x0, c.x1]).collect());
    let ys = lines(lattice.iter().flat_map(|c| [c.top, c.bottom]).collect());
    if xs.len() < 3 || ys.len() < 3 {
        return None;
    }
    let user = |x0: f32, top: f32, x1: f32, bottom: f32| UserBox {
        x0,
        y0: page_top - bottom,
        x1,
        y1: page_top - top,
    };
    let rows: Vec<UserBox> = ys.windows(2).map(|w| user(bx0, w[0], bx1, w[1])).collect();
    let cols: Vec<UserBox> = xs.windows(2).map(|w| user(w[0], bt, w[1], bb)).collect();
    // Tables ruled only between groups of rows hold several rows in one
    // ruled band. The rulings do not describe that table's rows; the model
    // reads it instead.
    let line_centres = |cell: &UserBox| -> Vec<f32> {
        let mut cy: Vec<f32> = inside
            .iter()
            .filter(|t| cell.contains_point((t.b.x0 + t.b.x1) / 2.0, (t.b.y0 + t.b.y1) / 2.0))
            .map(|t| (t.b.y0 + t.b.y1) / 2.0)
            .collect();
        cy.sort_by(f32::total_cmp);
        let mut lines: Vec<f32> = Vec::new();
        for y in cy {
            if lines.last().is_none_or(|&l| y - l > 0.5 * size) {
                lines.push(y);
            }
        }
        lines
    };
    // The rows show in the row labels: a first-column cell, one ruled band
    // high, with several lines, each level with a line in a cell beside it.
    // (A merged label over two header rows lines up with sub-headers by
    // design, and does not count.)
    let one_band = |c: &pt::CellBbox| !ys.iter().any(|&y| y > c.top + tol && y < c.bottom - tol);
    let as_user = |c: &pt::CellBbox| user(c.x0, c.top, c.x1, c.bottom);
    for label in lattice
        .iter()
        .filter(|c| (c.x0 - bx0).abs() <= tol && one_band(c))
    {
        let lines = line_centres(&as_user(label));
        let stacked = lines.len() >= 2
            && lattice.iter().any(|c| {
                !std::ptr::eq(c, label)
                    && (c.top - label.top).abs() <= tol
                    && (c.bottom - label.bottom).abs() <= tol
                    && {
                        let other = line_centres(&as_user(c));
                        other.len() == lines.len()
                            && other
                                .iter()
                                .zip(&lines)
                                .all(|(p, q)| (p - q).abs() < 0.4 * size)
                    }
            });
        if stacked {
            if std::env::var_os("FILEGOBLIN_DEBUG").is_some() {
                eprintln!("table rulings skipped: rows stacked in one ruled band");
            }
            return None;
        }
    }
    // A column whose words leave a clear gutter through every row is two
    // columns with no ruling drawn between them; the model reads the table.
    for c in &cols {
        let mut spans: Vec<(f32, f32)> = inside
            .iter()
            .filter(|t| c.contains_point((t.b.x0 + t.b.x1) / 2.0, (t.b.y0 + t.b.y1) / 2.0))
            .map(|t| (t.b.x0, t.b.x1))
            .collect();
        spans.sort_by(|a, b| a.0.total_cmp(&b.0));
        let mut reach = f32::MIN;
        let mut split = false;
        for &(x0, x1) in &spans {
            if reach > f32::MIN && x0 - reach >= 1.5 * size {
                split = true;
            }
            reach = reach.max(x1);
        }
        if split {
            if std::env::var_os("FILEGOBLIN_DEBUG").is_some() {
                eprintln!("table rulings skipped: a ruled column holds two columns of words");
            }
            return None;
        }
    }
    // A cell that covers more than one grid row or column is a merged cell.
    let spans: Vec<UserBox> = lattice
        .iter()
        .filter(|c| {
            let ncols = xs
                .iter()
                .filter(|&&x| x > c.x0 + tol && x < c.x1 - tol)
                .count();
            let nrows = ys
                .iter()
                .filter(|&&y| y > c.top + tol && y < c.bottom - tol)
                .count();
            ncols + nrows > 0
        })
        .map(|c| user(c.x0, c.top, c.x1, c.bottom))
        .collect();
    if std::env::var_os("FILEGOBLIN_DEBUG").is_some() {
        eprintln!(
            "table read from its rulings: {} rows x {} columns, {} merged cells",
            rows.len(),
            cols.len(),
            spans.len()
        );
    }
    Some(crate::vision::table::Structure {
        rows,
        cols,
        header_rows: Vec::new(),
        spans,
    })
}

/// Centres of the visual lines the tokens `members` form (top to bottom).
fn line_centres(toks: &[Tok], members: &[usize], size: f32) -> Vec<f32> {
    let mut cy: Vec<f32> = members
        .iter()
        .map(|&i| (toks[i].b.y0 + toks[i].b.y1) / 2.0)
        .collect();
    cy.sort_by(|a, b| b.total_cmp(a));
    let mut lines: Vec<f32> = Vec::new();
    for y in cy {
        if lines.last().is_none_or(|&l| l - y > 0.5 * size) {
            lines.push(y);
        }
    }
    lines
}

/// Whether a cell's lines read as one text wrapped at the column's right
/// edge: the first word of each line would not have fitted at the end of
/// the line before it. Lines of numbers are values, never a wrap.
fn wraps(toks: &[Tok], cell: &[usize], size: f32, right: f32) -> bool {
    let lines = visual_lines(toks, cell);
    let numeric = |l: &Vec<usize>| {
        l.iter().all(|&i| {
            toks[i]
                .text
                .chars()
                .all(|c| c.is_ascii_digit() || ",.%()$€£+-–−±*".contains(c))
        })
    };
    lines.len() >= 2
        && !lines.iter().all(numeric)
        && lines.windows(2).all(|w| {
            let end = w[0].iter().map(|&i| toks[i].b.x1).fold(f32::MIN, f32::max);
            let next = w[1]
                .iter()
                .min_by(|&&a, &&b| toks[a].b.x0.total_cmp(&toks[b].b.x0))
                .map_or(0.0, |&i| toks[i].b.width());
            end + 0.25 * size + next > right + 0.1 * size
        })
}

/// The structure with every merged row split into its stacked rows (see
/// [`grid_markdown`]); `None` when no row is merged.
fn split_stacked_rows(
    st: &crate::vision::table::Structure,
    toks: &[Tok],
    cells: &[Vec<Vec<usize>>],
) -> Option<crate::vision::table::Structure> {
    let mut heights: Vec<f32> = cells
        .iter()
        .flatten()
        .flatten()
        .map(|&i| toks[i].b.height())
        .collect();
    if heights.is_empty() || st.cols.len() < 2 {
        return None;
    }
    heights.sort_by(f32::total_cmp);
    let size = heights[heights.len() / 2].max(1.0);
    // Each column's right edge: the furthest any of its words reaches.
    let right: Vec<f32> = (0..st.cols.len())
        .map(|c| {
            cells
                .iter()
                .flat_map(|row| &row[c])
                .map(|&i| toks[i].b.x1)
                .fold(f32::MIN, f32::max)
        })
        .collect();
    let mut rows: Vec<UserBox> = Vec::new();
    let mut split_any = false;
    for (r, row) in st.rows.iter().enumerate() {
        let labels = line_centres(toks, &cells[r][0], size);
        let aligned: Vec<usize> = (1..st.cols.len())
            .filter(|&c| {
                let lines = line_centres(toks, &cells[r][c], size);
                lines.len() == labels.len()
                    && lines
                        .iter()
                        .zip(&labels)
                        .all(|(p, q)| (p - q).abs() < 0.4 * size)
            })
            .collect();
        // Text wrapped at the column edges lines up the same way; it is one
        // row when every cell holding several lines wraps.
        let wrapped = || {
            (0..st.cols.len())
                .filter(|&c| line_centres(toks, &cells[r][c], size).len() >= 2)
                .all(|c| wraps(toks, &cells[r][c], size, right[c]))
        };
        let stacked = labels.len() >= 2 && !aligned.is_empty() && !wrapped();
        if !stacked {
            rows.push(*row);
            continue;
        }
        split_any = true;
        // Cut halfway between consecutive lines (y up: top line first).
        let mut top = row.y1;
        for pair in labels.windows(2) {
            let cut = (pair[0] + pair[1]) / 2.0;
            rows.push(UserBox {
                y0: cut,
                y1: top,
                ..*row
            });
            top = cut;
        }
        rows.push(UserBox { y1: top, ..*row });
    }
    split_any.then(|| crate::vision::table::Structure {
        rows,
        cols: st.cols.clone(),
        header_rows: Vec::new(),
        spans: st.spans.clone(),
    })
}

/// A table region converted by pdf_oxide's own table extractor, when that
/// yields a pipe table.
fn table_markdown(
    doc: &PdfDocument,
    page: usize,
    b: &UserBox,
    excluded: &[Rect],
    base_options: &pdf_oxide::converters::ConversionOptions,
) -> Option<String> {
    let mut opts = base_options.clone();
    opts.exclude_regions = excluded.to_vec();
    opts.exclude_regions_mode = RectFilterMode::MinOverlap(0.5);
    let pad = 2.0;
    opts.include_region = Some((
        Rect::new(
            b.x0 - pad,
            b.y0 - pad,
            b.width() + 2.0 * pad,
            b.height() + 2.0 * pad,
        ),
        RectFilterMode::MinOverlap(0.5),
    ));
    let md = doc.to_markdown(page, &opts).ok()?;
    md.lines()
        .any(|l| l.trim_start().starts_with('|'))
        .then(|| md.trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tok(text: &str, x0: f32, y0: f32, x1: f32, y1: f32) -> Tok {
        Tok {
            text: text.into(),
            b: UserBox { x0, y0, x1, y1 },
            size: y1 - y0,
            glued: false,
            math: false,
        }
    }

    #[test]
    fn overlapping_spans_do_not_repeat_text() {
        // 2 rows x 3 columns, PDF coordinates (y up). One word per cell in
        // the top row; the model reports two overlapping spans over it.
        let ub = |x0, y0, x1, y1| UserBox { x0, y0, x1, y1 };
        let st = crate::vision::table::Structure {
            rows: vec![ub(0.0, 680.0, 300.0, 700.0), ub(0.0, 660.0, 300.0, 680.0)],
            cols: vec![
                ub(0.0, 660.0, 100.0, 700.0),
                ub(100.0, 660.0, 200.0, 700.0),
                ub(200.0, 660.0, 300.0, 700.0),
            ],
            spans: vec![ub(0.0, 680.0, 300.0, 700.0), ub(0.0, 680.0, 200.0, 700.0)],
            ..Default::default()
        };
        let toks = vec![
            tok("Form", 10.0, 685.0, 40.0, 695.0),
            tok("1040", 110.0, 685.0, 140.0, 695.0),
            tok("2025", 210.0, 685.0, 240.0, 695.0),
            tok("a", 10.0, 665.0, 20.0, 675.0),
            tok("b", 110.0, 665.0, 120.0, 675.0),
            tok("c", 210.0, 665.0, 220.0, 675.0),
        ];
        let idx: Vec<usize> = (0..toks.len()).collect();
        let md = grid_markdown(&st, &toks, &idx).unwrap();
        let header = md.lines().next().unwrap();
        assert_eq!(header.matches("Form").count(), 3, "{md}");
        assert!(!header.contains("Form 1040 2025 Form"), "{md}");
    }

    #[test]
    fn rows_the_model_merged_are_split_but_value_and_error_stay() {
        let ub = |x0, y0, x1, y1| UserBox { x0, y0, x1, y1 };
        // Header row, then one model row holding two printed rows.
        let st = crate::vision::table::Structure {
            rows: vec![ub(0.0, 680.0, 200.0, 700.0), ub(0.0, 640.0, 200.0, 680.0)],
            cols: vec![ub(0.0, 640.0, 100.0, 700.0), ub(100.0, 640.0, 200.0, 700.0)],
            ..Default::default()
        };
        let toks = vec![
            tok("Item", 10.0, 685.0, 40.0, 695.0),
            tok("Value", 110.0, 685.0, 140.0, 695.0),
            tok("Passband", 10.0, 665.0, 50.0, 675.0),
            tok("390", 110.0, 665.0, 130.0, 675.0),
            tok("Fibres", 10.0, 650.0, 40.0, 660.0),
            tok("1600", 110.0, 650.0, 130.0, 660.0),
        ];
        let idx: Vec<usize> = (0..toks.len()).collect();
        let md = grid_markdown(&st, &toks, &idx).unwrap();
        assert!(
            md.contains("| Passband | 390 |") && md.contains("| Fibres | 1600 |"),
            "{md}"
        );

        // A coefficient over its standard error: one label line, one row.
        let toks = vec![
            tok("Item", 10.0, 685.0, 40.0, 695.0),
            tok("Value", 110.0, 685.0, 140.0, 695.0),
            tok("Income", 10.0, 665.0, 50.0, 675.0),
            tok("0.53", 110.0, 665.0, 130.0, 675.0),
            tok("(0.12)", 110.0, 650.0, 140.0, 660.0),
        ];
        let idx: Vec<usize> = (0..toks.len()).collect();
        let md = grid_markdown(&st, &toks, &idx).unwrap();
        assert!(md.contains("| Income | 0.53 (0.12) |"), "{md}");
    }

    #[test]
    fn a_hyphen_apart_from_one_word_joins_the_compound() {
        let toks = vec![
            tok("ATTENTION", 72.0, 700.0, 153.0, 713.0),
            tok("-BASED", 155.5, 700.0, 190.0, 713.0),
            tok("ls", 200.0, 700.0, 210.0, 713.0),
            tok("-la", 214.0, 700.0, 230.0, 713.0),
        ];
        assert_eq!(join_line(&toks, &[0, 1, 2, 3]), "ATTENTION-BASED ls -la");
    }

    #[test]
    fn text_wrapped_at_the_column_edge_stays_one_row() {
        let ub = |x0, y0, x1, y1| UserBox { x0, y0, x1, y1 };
        let st = crate::vision::table::Structure {
            rows: vec![ub(0.0, 680.0, 300.0, 700.0), ub(0.0, 640.0, 300.0, 680.0)],
            cols: vec![
                ub(0.0, 640.0, 100.0, 700.0),
                ub(100.0, 640.0, 200.0, 700.0),
                ub(200.0, 640.0, 300.0, 700.0),
            ],
            ..Default::default()
        };
        // Words laid out at 5 pt a character, one space between them.
        let mut toks = Vec::new();
        let mut line = |text: &str, x: f32, y: f32| {
            let mut x = x;
            for w in text.split(' ') {
                let x1 = x + 5.0 * w.chars().count() as f32;
                toks.push(tok(w, x, y, x1, y + 10.0));
                x = x1 + 3.0;
            }
        };
        line("Milestone", 2.0, 685.0);
        line("Date", 102.0, 685.0);
        line("Notes", 202.0, 685.0);
        line("Final design", 2.0, 665.0);
        line("(approx. 90%)", 2.0, 650.0);
        line("May 8, 2003", 102.0, 665.0);
        line("Plans sent to", 202.0, 665.0);
        line("EPA for review", 202.0, 650.0);
        let idx: Vec<usize> = (0..toks.len()).collect();
        let md = grid_markdown(&st, &toks, &idx).unwrap();
        assert!(
            md.contains(
                "| Final design (approx. 90%) | May 8, 2003 | Plans sent to EPA for review |"
            ),
            "{md}"
        );
    }

    #[test]
    fn inline_formulas_take_in_their_math_words() {
        let math = |text: &str, x0: f32, x1: f32| Tok {
            math: true,
            ..tok(text, x0, 700.0, x1, 710.0)
        };
        let words = vec![
            tok("the", 10.0, 700.0, 30.0, 710.0),
            math("T", 32.0, 38.0),
            math("ΔE", 39.0, 52.0),
            tok("and", 60.0, 700.0, 80.0, 710.0),
            math("f(x)", 90.0, 110.0),
            math("=", 112.0, 118.0),
            math("y", 120.0, 126.0),
        ];
        let boxed = Region {
            label: Label::InlineFormula,
            score: 0.9,
            bbox: UserBox {
                x0: 39.0,
                y0: 699.0,
                x1: 52.0,
                y1: 711.0,
            },
            order: 3,
        };
        let mut inline = vec![boxed.clone()];
        complete_inline_formulas(&mut inline, &words, &[boxed]);
        // The tight box grows over "T"; "f(x) = y" becomes a formula.
        assert_eq!(inline.len(), 2);
        assert_eq!(inline[0].bbox.x0, 32.0);
        assert_eq!((inline[1].bbox.x0, inline[1].bbox.x1), (90.0, 126.0));
    }

    #[test]
    fn a_title_row_goes_above_its_table() {
        let ub = |x0, y0, x1, y1| UserBox { x0, y0, x1, y1 };
        let st = crate::vision::table::Structure {
            rows: vec![
                ub(0.0, 700.0, 200.0, 720.0),
                ub(0.0, 680.0, 200.0, 700.0),
                ub(0.0, 660.0, 200.0, 680.0),
            ],
            cols: vec![ub(0.0, 660.0, 100.0, 720.0), ub(100.0, 660.0, 200.0, 720.0)],
            spans: vec![ub(0.0, 700.0, 200.0, 720.0)],
            ..Default::default()
        };
        let toks = vec![
            tok("Table", 10.0, 705.0, 40.0, 715.0),
            tok("2:", 42.0, 705.0, 50.0, 715.0),
            tok("Results", 52.0, 705.0, 90.0, 715.0),
            tok("Item", 10.0, 685.0, 40.0, 695.0),
            tok("Value", 110.0, 685.0, 140.0, 695.0),
            tok("a", 10.0, 665.0, 20.0, 675.0),
            tok("1", 110.0, 665.0, 120.0, 675.0),
        ];
        let idx: Vec<usize> = (0..toks.len()).collect();
        let md = grid_markdown(&st, &toks, &idx).unwrap();
        assert!(
            md.starts_with("Table 2: Results\n\n| Item | Value |"),
            "{md}"
        );
    }

    #[test]
    fn publisher_imprints_are_recognised() {
        assert!(is_imprint(
            "INTERNATIONAL JOURNAL OF INTELLIGENT SYSTEMS, VOL. 8, 71–90 (1993)"
        ));
        assert!(is_imprint(
            "© 1993 John Wiley & Sons, Inc. CCC 0884-8173/93/010071-20"
        ));
        assert!(is_imprint(
            "Copyright © 2019 Elsevier B.V. All rights reserved."
        ));
        assert!(is_imprint("ISSN 0018-9286"));
        assert!(!is_imprint("The committee met in 1993 to review volume 8."));
        assert!(!is_imprint("See Table 3 (1993) and Section 2."));
    }

    #[test]
    fn visual_lines_read_top_down_left_right() {
        let toks = vec![
            tok("world", 60.0, 700.0, 100.0, 710.0),
            tok("second", 10.0, 685.0, 50.0, 695.0),
            tok("hello", 10.0, 700.0, 50.0, 710.0),
            tok("$x^2$", 105.0, 698.0, 125.0, 713.0),
        ];
        let idx: Vec<usize> = (0..toks.len()).collect();
        assert_eq!(lines_to_text(&toks, &idx), "hello world $x^2$\nsecond");
    }

    #[test]
    fn split_inline_formulas_rejoin_only_across_an_operator() {
        assert_eq!(
            merge_split_inline_formulas("with $0\\leq k\\leq$\n$2^{N}-1$. Notice"),
            "with $0\\leq k\\leq 2^{N}-1$\n. Notice"
        );
        assert_eq!(merge_split_inline_formulas("$a$\n$b$ x"), "$a$\n$b$ x");
    }

    #[test]
    fn paragraphs_continue_across_region_breaks_only_mid_sentence() {
        assert!(continues("ment rural et départe-", "ment rural"));
        assert!(continues("the results of the", "experiment show"));
        assert!(!continues("The end.", "next paragraph"));
        assert!(!continues("## Heading", "lower"));
        assert!(!continues("some text", "Capitalized"));
    }

    #[test]
    fn doubled_words_and_soft_hyphens_are_cleaned() {
        let toks = vec![
            tok("modi\u{ad}", 10.0, 700.0, 40.0, 710.0),
            tok("fied", 10.0, 685.0, 30.0, 695.0),
            tok("fied", 10.2, 685.1, 30.2, 695.1),
        ];
        let toks = drop_doubled(toks);
        assert_eq!(toks.len(), 2);
        let idx: Vec<usize> = (0..toks.len()).collect();
        assert_eq!(lines_to_text(&toks, &idx), "modified");
    }

    #[test]
    fn blank_pages_are_told_from_pages_with_ink() {
        let mut page = image::RgbImage::from_pixel(200, 260, image::Rgb([236, 234, 230]));
        // Scanner specks on grey paper are still a blank page.
        for (x, y) in [(10, 10), (150, 40), (90, 200)] {
            page.put_pixel(x, y, image::Rgb([40, 40, 40]));
        }
        assert!(is_blank(&page));
        // One short line of text is not.
        for x in 40..120 {
            for y in 100..103 {
                if x % 3 != 0 {
                    page.put_pixel(x, y, image::Rgb([60, 60, 60]));
                }
            }
        }
        assert!(!is_blank(&page));
    }

    #[test]
    fn ligatures_are_spelled_out() {
        assert_eq!(expand_ligatures("ﬁve ﬂoors oﬃce"), "five floors office");
        assert_eq!(expand_ligatures("plain"), "plain");
    }

    #[test]
    fn heading_levels_follow_section_numbers() {
        assert_eq!(heading_level("3.2 Results"), 3);
        assert_eq!(heading_level("4 Method"), 2);
        assert_eq!(heading_level("Introduction"), 2);
        assert_eq!(heading_level("1.2.3. Details"), 4);
    }
}
