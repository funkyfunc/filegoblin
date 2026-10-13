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
use crate::vision::render::{PageImage, Renderer};
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
}

/// Converts one text-layer page with the layout model. `None` when the page
/// cannot be rendered or the model is unavailable — the caller then falls
/// back to the geometric converter.
pub(crate) fn convert_page(
    renderer: &Renderer,
    doc: &PdfDocument,
    page: usize,
    geom: &PageGeom,
    excluded: &[Rect],
    base_options: &pdf_oxide::converters::ConversionOptions,
) -> Option<String> {
    crate::vision::PageGate::global()
        .run(|| convert_page_gated(renderer, doc, page, geom, excluded, base_options))
}

fn convert_page_gated(
    renderer: &Renderer,
    doc: &PdfDocument,
    page: usize,
    geom: &PageGeom,
    excluded: &[Rect],
    base_options: &pdf_oxide::converters::ConversionOptions,
) -> Option<String> {
    let t = std::time::Instant::now();
    let img = renderer.render(page, RENDER_LONG_SIDE, MAX_SCALE)?;
    let regions = layout::detect(&img)?;
    let t_layout = t.elapsed();
    let mut words = span_words(doc, page, excluded).unwrap_or_default();
    // Regions the model sees as text but the text layer leaves empty are
    // pictures of text (a scanned page behind a thin text layer, a pasted
    // image of a paragraph or table): read those with OCR.
    let empty_text_regions = regions.iter().any(|r| {
        !r.label.is_furniture()
            && !r.label.is_graphic()
            && r.label != Label::InlineFormula
            && r.label != Label::DisplayFormula
            && r.bbox.area() > 2000.0
            && !words.iter().any(|w| {
                r.bbox
                    .contains_point((w.b.x0 + w.b.x1) / 2.0, (w.b.y0 + w.b.y1) / 2.0)
            })
    });
    if empty_text_regions && crate::vision::ocr::available() {
        // OCR wants more pixels per character than layout does.
        let hi = renderer.render(page, SCAN_LONG_SIDE, SCAN_MAX_SCALE);
        let ocr = ocr_words(hi.as_ref().unwrap_or(&img));
        let filled: Vec<Tok> = ocr
            .into_iter()
            .filter(|o| {
                let (cx, cy) = ((o.b.x0 + o.b.x1) / 2.0, (o.b.y0 + o.b.y1) / 2.0);
                // Only where the text layer has nothing: never duplicate it.
                !words
                    .iter()
                    .any(|w| w.b.intersection(&o.b) > 0.3 * o.b.area().min(w.b.area()))
                    && regions
                        .iter()
                        .any(|r| !r.label.is_furniture() && r.bbox.contains_point(cx, cy))
            })
            .collect();
        words.extend(filled);
    }
    let table_fallback = |b: &UserBox| table_markdown(doc, page, b, excluded, base_options);
    let md = assemble(&img, &regions, words, geom.bx, &table_fallback);
    debug_timing(page, "text layer", t_layout, t.elapsed(), regions.len());
    Some(md)
}

/// Converts a page that has no text layer: the layout model places regions
/// and PP-OCRv5 reads the words. `None` when OCR or layout is unavailable.
pub(crate) fn convert_scanned_page(renderer: &Renderer, page: usize) -> Option<String> {
    if !crate::vision::ocr::available() {
        return None;
    }
    crate::vision::PageGate::global().run(|| convert_scanned_page_gated(renderer, page))
}

fn convert_scanned_page_gated(renderer: &Renderer, page: usize) -> Option<String> {
    let t = std::time::Instant::now();
    let img = renderer.render(page, SCAN_LONG_SIDE, SCAN_MAX_SCALE)?;
    let regions = layout::detect(&img)?;
    let t_layout = t.elapsed();
    let words = ocr_words(&img);
    let (w, h) = (img.rgb.width() as f32, img.rgb.height() as f32);
    let pb = img.to_user(0.0, 0.0, w, h);
    let md = assemble(&img, &regions, words, (pb.x0, pb.y0, pb.x1, pb.y1), &|_| {
        None
    });
    debug_timing(page, "scan", t_layout, t.elapsed(), regions.len());
    Some(md)
}

/// Words of the text layer, taken from the converter's own text spans so
/// spacing follows the PDF's space characters and the converter's word-gap
/// rules (word boxes alone misjudge both tight italics and OCR text layers
/// whose glyph widths are wrong). A word that starts a span which the
/// previous span runs into without a space is marked `glued`.
fn span_words(doc: &PdfDocument, page: usize, excluded: &[Rect]) -> Option<Vec<Tok>> {
    let spans = doc.extract_spans(page).ok()?;
    let mut words = Vec::new();
    let mut prev: Option<&pdf_oxide::layout::TextSpan> = None;
    for s in &spans {
        let chars: Vec<char> = s.text.chars().collect();
        let n = chars.len();
        if n == 0 {
            continue;
        }
        let exact = s.char_x_offsets.len() == n && s.char_widths.len() == n;
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
                        b: UserBox { x0, y0, x1, y1 },
                        size: s.font_size,
                        glued: i == 0 && runs_on,
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
fn ocr_words(img: &PageImage) -> Vec<Tok> {
    let mut words: Vec<Tok> = Vec::new();
    for l in crate::vision::ocr::read(&img.rgb).unwrap_or_default() {
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
                });
            }
            pos += n;
        }
    }
    words
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
    table_fallback: &dyn Fn(&UserBox) -> Option<String>,
) -> String {
    let (_, by0, _, by1) = bx;
    let ph = (by1 - by0).max(1.0);

    // Furniture regions only count near the top or bottom edge: a model that
    // calls a mid-page line a "header" is wrong more often than not.
    let furniture: Vec<&Region> = regions
        .iter()
        .filter(|r| r.label.is_furniture())
        .filter(|r| {
            let cy = (r.bbox.y0 + r.bbox.y1) / 2.0;
            cy > by1 - 0.2 * ph || cy < by0 + 0.2 * ph
        })
        .collect();
    let content: Vec<&Region> = regions
        .iter()
        .filter(|r| !r.label.is_furniture() && r.label != Label::InlineFormula)
        .collect();

    // Recognize formulas: display formulas replace their region's text,
    // inline formulas replace the words under them.
    // Inside a table, a value like "48.96 ± 2.84" reads better — and is
    // searched for — as the text it is than as LaTeX; the text layer (or
    // OCR) already has it.
    let tables: Vec<&Region> = regions.iter().filter(|r| r.label == Label::Table).collect();
    let inline: Vec<&Region> = regions
        .iter()
        .filter(|r| r.label == Label::InlineFormula)
        .filter(|r| {
            let (cx, cy) = ((r.bbox.x0 + r.bbox.x1) / 2.0, (r.bbox.y0 + r.bbox.y1) / 2.0);
            !tables.iter().any(|t| t.bbox.contains_point(cx, cy))
        })
        .collect();
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
            Label::ParagraphTitle => {
                let t = text();
                heading(heading_level(&t), &t)
            }
            Label::Table => crate::vision::table::recognize(img, &r.bbox)
                .and_then(|st| grid_markdown(&st, &toks, &members[i]))
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
    out
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
    let mut grid: Vec<Vec<String>> = cells
        .iter()
        .map(|row| {
            row.iter()
                .map(|c| {
                    // A cell's lines read as one: a line ending in a hyphen
                    // continues without a space ("Automotive-⏎Industrial").
                    let mut text = String::new();
                    for l in visual_lines(toks, c) {
                        let line = join_line(toks, &l);
                        if !text.is_empty() && !text.ends_with('-') {
                            text.push(' ');
                        }
                        text.push_str(&line);
                    }
                    // Dot leaders ("Hettinger.........") are layout, not content.
                    LEADER.replace_all(&text, "").trim().replace('|', "\\|")
                })
                .collect()
        })
        .collect();
    for sp in &st.spans {
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
        if rs.len() * cs.len() < 2 {
            continue;
        }
        let text = rs
            .iter()
            .flat_map(|&r| cs.iter().map(move |&c| (r, c)))
            .map(|(r, c)| grid[r][c].clone())
            .filter(|t| !t.is_empty())
            .collect::<Vec<_>>()
            .join(" ");
        for &r in &rs {
            for &c in &cs {
                grid[r][c] = text.clone();
            }
        }
    }
    // Drop empty rows and columns.
    grid.retain(|row| row.iter().any(|c| !c.is_empty()));
    let keep: Vec<usize> = (0..nc)
        .filter(|&c| grid.iter().any(|row| !row[c].is_empty()))
        .collect();
    if grid.len() < 2 || keep.len() < 2 {
        return None;
    }
    let mut out = String::new();
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
        }
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
