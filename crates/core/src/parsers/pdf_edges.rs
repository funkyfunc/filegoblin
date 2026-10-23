//! Page furniture that one page shows on its own.
//!
//! The recurrence detector in [`super::pdf_furniture`] finds running heads
//! and folios by their repeating across pages, which a one- or two-page
//! document cannot do, and which one-off furniture never does: a letterhead,
//! a print stamp, a document ID, a licence line, a "Downloaded from …" note.
//! These rules find such lines from the page alone. They were fitted and
//! checked on 479 single pages and about 800 edge blocks read by eye, where
//! they removed half the furniture the benchmark marks and four blocks of
//! real content (see docs/ADR.md, 2026-10-23).
//!
//! * **Geometric running line** — a short block (≤ 3.5% of the page height,
//!   ≤ 150 characters) lying wholly in the top 8% or bottom 13% of the page,
//!   outside all body text, at least two body line heights clear of the text
//!   inward, and set no larger than the body (bottom: twice the body). Not a
//!   caption, not a footnote, not a figure's label (no drawing between it and
//!   the body), not prose the page break cut.
//! * **Small-type notice** — at the foot, type at most 0.8× body may run to
//!   800 characters (imprint and legal boilerplate).
//! * **Lexical** — in the top 10% / bottom 15%, a line of a block of at most
//!   two lines that is a page number alone on its row, "Page x of y", a URL,
//!   a DOI/ISSN/ISBN, a copyright or licence line, a "downloaded from" note,
//!   a journal citation or a short date stamp.

use super::pdf_furniture::{GLine, PageGeom};
use pdf_oxide::PdfDocument;
use pdf_oxide::geometry::Rect;
use rayon::prelude::*;
use std::sync::LazyLock;

/// The edge of the page a furniture line sits at.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum Side {
    Top,
    Bottom,
}

/// A line found to be furniture, and whether only its words said so
/// (lexical) or its place on the page did (geometric).
#[derive(Debug, Clone)]
pub(crate) struct EdgeHit {
    pub side: Side,
    pub rect: Rect,
    pub geometric: bool,
}

const MONTHS: &str = r"jan(?:uary|vier|uar|eiro)?|feb(?:ruary|ruar)?|f[ée]v(?:rier)?|fev(?:ereiro)?|m[aä]r(?:ch|s|z|ço)?|apr(?:il)?|avr(?:il)?|abr(?:il)?|may|mai|maio|mayo|jun(?:e|i|ho|io)?|juin|jul(?:y|i|ho|io)?|juil(?:let)?|aug(?:ust)?|ao[uû]t|ago(?:sto)?|sep(?:t|tember|tembre|tembro)?|set(?:embro)?|oct(?:ober|obre)?|okt(?:ober)?|out(?:ubro)?|nov(?:ember|embre|embro)?|dec(?:ember)?|d[ée]c(?:embre)?|dez(?:ember|embro)?|dic(?:iembre)?";

/// The kinds of furniture a line's words can show, in the order tried.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Lexical {
    PageOf,
    PageNumber,
    Url,
    Identifier,
    Copyright,
    Provenance,
    Citation,
    Date,
}

static PATTERNS: LazyLock<Vec<(Lexical, regex::Regex)>> = LazyLock::new(|| {
    let r = |p: &str| regex::Regex::new(p).expect("valid regex");
    vec![
        (
            Lexical::PageOf,
            r(
                r"(?i)\b(?:page|p[áa]gina|seite|pag\.?|strana|sivu)\s*\d{1,4}\s*(?:of|/|de|von|di|z|sur|af|av)\s*\d{1,4}\b",
            ),
        ),
        (
            Lexical::PageNumber,
            r(
                r"(?i)^[\W_]*(?:(?:page|pag\.?|p\.|pg\.?|seite|p[áa]gina|strana|sivu|side|blatt|folio)\s*)?(?:\d{1,4}|c{1,3}(?:xc|xl|l?x{0,3})(?:ix|iv|v?i{0,3})|(?:xc|xl|l?x{1,3}|l)(?:ix|iv|v?i{0,3})|ix|iv|v?i{1,3}|v)(?:\s*(?:of|/|de|von|di|z|sur|af|av)\s*\d{1,4})?[\W_]*$",
            ),
        ),
        (
            Lexical::Url,
            r(
                r"(?i)(?:https?://|\bwww\.)\S+|\b[\w.-]+@[\w-]+\.[\w.]+\b|\b[a-z0-9-]+(?:\.[a-z0-9-]+)*\.(?:com|org|edu|gov|net|int|info|ch|de|fr|it|es|pt|nl|be|at|se|no|dk|fi|pl|cz|lt|lv|ee|ru|ua|uk|us|ca|au|nz|in|br|jp|cn|kr|io|eu)(?:/\S*)?\b",
            ),
        ),
        (
            Lexical::Identifier,
            r(
                r"(?i)\bdoi\s*:?\s*10\.\d{4,}|\b10\.\d{4,9}/\S+|\bISSN\b|\bISBN\b|\barXiv:\s*\d{4}\.\d{4,5}|\bPMID\b|\bPMCID\b",
            ),
        ),
        (
            Lexical::Copyright,
            r(
                r"(?i)©|\(c\)\s*(?:19|20)\d\d|\bcopyright\b|all rights reserved|creative commons|\bCC[- ]BY\b|\blicen[cs]ed? under\b|open access article|tous droits r[ée]serv[ée]s|alle rechte vorbehalten|todos los derechos|direitos reservados",
            ),
        ),
        (
            Lexical::Provenance,
            r(
                r"(?i)downloaded from|retrieved from|accessed (?:on|from)|printed (?:on|from)|for personal use only|by guest on|t[ée]l[ée]charg[ée] depuis|heruntergeladen",
            ),
        ),
        (
            Lexical::Citation,
            r(
                r"(?i)\bvol(?:ume)?\.?\s*\d+|\bno\.\s*\d+\s*[,(]|\biss(?:ue)?\.?\s*\d+|\bpp\.\s*\d+\s*[-–]\s*\d+|\b(?:19|20)\d\d\s*[;,]\s*\d+\s*\(\d+\)\s*:\s*\d+",
            ),
        ),
        (
            Lexical::Date,
            r(&format!(
                r"(?i)\b\d{{1,2}}[./-]\d{{1,2}}[./-](?:\d{{2}}|\d{{4}})\b|\b(?:19|20)\d\d-\d\d-\d\d\b|\b\d{{1,2}}\.?\s+(?:{MONTHS})\.?\s+(?:19|20)\d\d\b|\b(?:{MONTHS})\.?\s+\d{{1,2}},?\s+(?:19|20)\d\d\b|\b(?:{MONTHS})\.?\s+(?:19|20)\d\d\b"
            )),
        ),
    ]
});

static FOOTNOTE: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(r"(?s)^\s*(?:\d{1,3}|[*†‡§¶]+|\(\d{1,3}\)|\[\d{1,3}\])\s*\.?\s+\S.{12,}")
        .expect("valid regex")
});
static CAPTION: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(r"(?i)^\s*(?:fig(?:ure)?s?\.?|tab(?:le|\.)|abb(?:ildung)?\.|source|sources|fonte|quelle|note|notes|nota|légende|photo)\b")
        .expect("valid regex")
});
static REFSTART: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(r"^\s*(?:\[\d{1,3}\]|\d{1,3}\.\s|[A-Z][\w'’-]+,\s+(?:[A-Z]{1,3}\b|[A-Z]\.))")
        .expect("valid regex")
});

fn lexical_class(text: &str) -> Option<Lexical> {
    let t = text.trim();
    PATTERNS
        .iter()
        .find(|(k, re)| re.is_match(t) && !(*k == Lexical::Date && t.chars().count() > 60))
        .map(|(k, _)| *k)
}

fn is_footnote(text: &str) -> bool {
    FOOTNOTE.is_match(text) && lexical_class(text) != Some(Lexical::PageNumber)
}

/// A line in page coordinates measured down from the top edge.
struct Line<'a> {
    src: &'a GLine,
    top: f32,
    bot: f32,
    x0: f32,
    x1: f32,
    chars: usize,
}

impl Line<'_> {
    fn h(&self) -> f32 {
        (self.bot - self.top).max(0.1)
    }
}

/// Lines merged into a block.
struct Block {
    lines: Vec<usize>,
    top: f32,
    bot: f32,
    x0: f32,
    x1: f32,
}

/// Furniture lines on one page, found from the page alone. `graphics`
/// supplies the boxes of drawn shapes (asked for only when a bottom
/// candidate needs the figure guard), in page user space.
pub(crate) fn edge_furniture(
    p: &PageGeom,
    graphics: impl FnOnce() -> Vec<(f32, f32, f32, f32)>,
) -> Vec<EdgeHit> {
    let (px0, py0, px1, py1) = p.bx;
    let (w, h) = ((px1 - px0).max(1.0), (py1 - py0).max(1.0));
    let lines: Vec<Line> = p
        .lines
        .iter()
        .filter(|l| l.text.chars().any(char::is_alphanumeric))
        .map(|l| Line {
            src: l,
            top: py1 - l.y1,
            bot: py1 - l.y0,
            x0: l.x0 - px0,
            x1: l.x1 - px0,
            chars: l.text.chars().count(),
        })
        .collect();
    if lines.len() < 2 {
        return Vec::new();
    }

    // Body metrics: line height and width of lines of running text, and
    // the size most of the page's characters are set in.
    let mut long: Vec<&Line> = lines.iter().filter(|l| l.chars >= 20).collect();
    long.sort_by(|a, b| a.h().total_cmp(&b.h()));
    let lh = long.get(long.len() / 2).map_or(10.0, |l| l.h());
    let mut widths: Vec<f32> = long.iter().map(|l| l.x1 - l.x0).collect();
    widths.sort_by(f32::total_cmp);
    let bw = widths
        .get((0.75 * widths.len().saturating_sub(1) as f32) as usize)
        .copied()
        .unwrap_or(0.6 * w)
        .max(1.0);
    let body = {
        let mut weight: std::collections::HashMap<i32, usize> = std::collections::HashMap::new();
        for wd in &p.words {
            *weight.entry((wd.size * 2.0).round() as i32).or_default() += wd.text.chars().count();
        }
        weight
            .into_iter()
            .max_by_key(|&(size, n)| (n, size))
            .map_or(10.0, |(size, _)| size as f32 / 2.0)
            .max(1.0)
    };

    let blocks = merge_blocks(&lines, lh);
    if blocks.len() < 2 {
        return Vec::new();
    }
    let mut figures: Option<Vec<(f32, f32, f32, f32)>> = None;
    let mut graphics = Some(graphics);
    let mut out = Vec::new();

    for side in [Side::Top, Side::Bottom] {
        // The widest zone any rule uses; the body is everything else.
        let in_zone = |b: &Block, frac: f32| match side {
            Side::Top => b.bot / h <= frac,
            Side::Bottom => b.top / h >= 1.0 - frac,
        };
        let wide = |b: &Block| in_zone(b, if side == Side::Top { 0.10 } else { 0.15 });
        let body_blocks: Vec<&Block> = blocks.iter().filter(|b| !wide(b)).collect();
        if body_blocks.is_empty() {
            continue;
        }
        let body_top = body_blocks.iter().map(|b| b.top).fold(f32::MAX, f32::min);
        let body_bot = body_blocks.iter().map(|b| b.bot).fold(f32::MIN, f32::max);
        // Candidates: zone blocks with nothing of the body between them and
        // the edge, walked from the edge inward.
        let mut cand: Vec<&Block> = blocks
            .iter()
            .filter(|b| {
                wide(b)
                    && match side {
                        Side::Top => b.bot <= body_top + 0.5,
                        Side::Bottom => b.top >= body_bot - 0.5,
                    }
            })
            .collect();
        match side {
            Side::Top => cand.sort_by(|a, b| a.top.total_cmp(&b.top)),
            Side::Bottom => cand.sort_by(|a, b| b.bot.total_cmp(&a.bot)),
        }
        // Clusters: a new one after a gap of a body line height or more.
        let mut clusters: Vec<Vec<&Block>> = Vec::new();
        for b in cand {
            if let Some(c) = clusters.last_mut() {
                let gap = match side {
                    Side::Top => b.top - c.iter().map(|x| x.bot).fold(f32::MIN, f32::max),
                    Side::Bottom => c.iter().map(|x| x.top).fold(f32::MAX, f32::min) - b.bot,
                };
                if gap < lh {
                    c.push(b);
                    continue;
                }
            }
            clusters.push(vec![b]);
        }

        // Innermost cluster first: a cluster outside one taken as furniture
        // needs no gap of its own.
        let mut inner_taken = false;
        for ci in (0..clusters.len()).rev() {
            let c = &clusters[ci];
            let (cy0, cy1) = (
                c.iter().map(|x| x.top).fold(f32::MAX, f32::min),
                c.iter().map(|x| x.bot).fold(f32::MIN, f32::max),
            );
            let gap = match side {
                Side::Top => {
                    let mut inner = body_top;
                    if let Some(n) = clusters.get(ci + 1) {
                        inner = inner.min(n.iter().map(|x| x.top).fold(f32::MAX, f32::min));
                    }
                    inner - cy1
                }
                Side::Bottom => {
                    let mut inner = body_bot;
                    if let Some(n) = clusters.get(ci + 1) {
                        inner = inner.max(n.iter().map(|x| x.bot).fold(f32::MIN, f32::max));
                    }
                    cy0 - inner
                }
            } / lh;
            let gap_ok = gap >= 2.0 || inner_taken;
            let mut all_taken = true;
            let mut any_real = false;
            for b in c {
                let text = block_text(b, &lines);
                let real = text.chars().filter(|ch| ch.is_alphanumeric()).count() >= 2;
                any_real |= real;
                let decision = decide(Decide {
                    side,
                    b,
                    lines: &lines,
                    text: &text,
                    h,
                    bw,
                    body,
                    gap,
                    gap_ok,
                    figure_between: &mut || {
                        let figs = figures.get_or_insert_with(|| {
                            let f = graphics.take().map_or(Vec::new(), |g| g());
                            f.into_iter()
                                .map(|(x0, y0, x1, y1)| (x0 - px0, py1 - y1, x1 - px0, py1 - y0))
                                .collect()
                        });
                        figure_between(figs, b, body_bot, cy0, w, h)
                    },
                });
                let block_taken = matches!(decision, Decision::Block);
                if real && !block_taken {
                    all_taken = false;
                }
                let taken: Vec<usize> = match decision {
                    Decision::Block => b.lines.clone(),
                    Decision::Lines(idx) => idx,
                    Decision::Keep => Vec::new(),
                };
                for li in taken {
                    let l = lines[li].src;
                    out.push(EdgeHit {
                        side,
                        rect: Rect::new(l.x0, l.y0, l.x1 - l.x0, l.y1 - l.y0),
                        geometric: block_taken,
                    });
                }
            }
            inner_taken = any_real && all_taken;
        }
    }
    out
}

/// Adds what [`edge_furniture`] finds to the furniture recurrence decided.
/// Beyond two pages the geometric rules apply only at a page edge where
/// recurrence found nothing (a running head it missed is the case they
/// exist for there); the lexical rules apply everywhere.
pub(crate) fn add_edge_furniture(
    doc: &PdfDocument,
    geoms: &[Option<PageGeom>],
    furniture: &mut [Vec<Rect>],
) {
    let text_pages = geoms.iter().filter(|g| g.is_some()).count();
    let found: Vec<Vec<Rect>> = geoms
        .par_iter()
        .zip(furniture.par_iter())
        .enumerate()
        .map(|(page, (g, rects))| {
            // A page with /Rotate mixes raw and displayed coordinates.
            let Some(p) = g
                .as_ref()
                .filter(|_| !super::pdf_furniture::rotated(doc, page))
            else {
                return Vec::new();
            };
            let h = p.height();
            let at = |side: Side| {
                rects.iter().any(|r| {
                    let cy = r.y + r.height / 2.0 - p.bx.1;
                    match side {
                        Side::Top => cy >= 0.85 * h,
                        Side::Bottom => cy <= 0.15 * h,
                    }
                })
            };
            let recurred = [at(Side::Top), at(Side::Bottom)];
            edge_furniture(p, || graphics(doc, page))
                .into_iter()
                .filter(|hit| {
                    !(hit.geometric
                        && text_pages > 2
                        && recurred[(hit.side == Side::Bottom) as usize])
                })
                .map(|hit| hit.rect)
                .collect()
        })
        .collect();
    for (rects, more) in furniture.iter_mut().zip(found) {
        rects.extend(more);
    }
}

/// Boxes (x0, y0, x1, y1) of the page's drawn paths and placed images.
fn graphics(doc: &PdfDocument, page: usize) -> Vec<(f32, f32, f32, f32)> {
    let corners = |r: &Rect| {
        let (xa, xb) = (r.x, r.x + r.width);
        let (ya, yb) = (r.y, r.y + r.height);
        (xa.min(xb), ya.min(yb), xa.max(xb), ya.max(yb))
    };
    let mut out: Vec<_> = doc
        .extract_paths(page)
        .map(|paths| paths.iter().map(|p| corners(&p.bbox)).collect())
        .unwrap_or_default();
    if let Ok(images) = doc.page_image_handles(page) {
        out.extend(images.iter().map(|i| corners(&i.bbox)));
    }
    out
}

/// Lines merged into blocks: lines that overlap horizontally with a small
/// vertical gap and similar heights, or that share a row with a small
/// horizontal gap.
fn merge_blocks(lines: &[Line], lh: f32) -> Vec<Block> {
    let n = lines.len();
    let mut parent: Vec<usize> = (0..n).collect();
    fn find(p: &mut [usize], mut i: usize) -> usize {
        while p[i] != i {
            p[i] = p[p[i]];
            i = p[i];
        }
        i
    }
    // Walked top-down: a line further down than 0.8 of a line's height
    // below it can merge with neither it nor anything after.
    let mut order: Vec<usize> = (0..n).collect();
    order.sort_by(|&a, &b| lines[a].top.total_cmp(&lines[b].top));
    for (oi, &i) in order.iter().enumerate() {
        for &j in &order[oi + 1..] {
            let (a, b) = (&lines[i], &lines[j]);
            if b.top - a.bot >= 0.8 * a.h() {
                break;
            }
            let (ha, hb) = (a.h(), b.h());
            let hov = a.x1.min(b.x1) - a.x0.max(b.x0);
            let vgap = (b.top - a.bot).max(a.top - b.bot);
            let stacked = hov > 0.0 && vgap < 0.8 * ha.min(hb) && ha.max(hb) < 1.6 * ha.min(hb);
            let vov = a.bot.min(b.bot) - a.top.max(b.top);
            let hgap = (b.x0 - a.x1).max(a.x0 - b.x1);
            let row = vov > 0.5 * ha.min(hb) && hgap < 1.2 * lh;
            if stacked || row {
                let (ri, rj) = (find(&mut parent, i), find(&mut parent, j));
                parent[ri] = rj;
            }
        }
    }
    let mut groups: std::collections::BTreeMap<usize, Vec<usize>> =
        std::collections::BTreeMap::new();
    for i in 0..n {
        let r = find(&mut parent, i);
        groups.entry(r).or_default().push(i);
    }
    groups
        .into_values()
        .map(|mut idx| {
            idx.sort_by(|&a, &b| {
                lines[a]
                    .top
                    .total_cmp(&lines[b].top)
                    .then(lines[a].x0.total_cmp(&lines[b].x0))
            });
            Block {
                top: idx.iter().map(|&i| lines[i].top).fold(f32::MAX, f32::min),
                bot: idx.iter().map(|&i| lines[i].bot).fold(f32::MIN, f32::max),
                x0: idx.iter().map(|&i| lines[i].x0).fold(f32::MAX, f32::min),
                x1: idx.iter().map(|&i| lines[i].x1).fold(f32::MIN, f32::max),
                lines: idx,
            }
        })
        .collect()
}

fn block_text(b: &Block, lines: &[Line]) -> String {
    b.lines
        .iter()
        .map(|&i| lines[i].src.text.trim())
        .collect::<Vec<_>>()
        .join(" ")
}

/// Drawn shapes (at least 6 pt each way, not page-sized backgrounds, not a
/// box behind the candidate) lying mostly between the body's bottom and the
/// candidate, beside it: a figure or table the candidate labels.
fn figure_between(
    figs: &[(f32, f32, f32, f32)],
    b: &Block,
    lo: f32,
    hi: f32,
    w: f32,
    h: f32,
) -> bool {
    figs.iter().any(|&(gx0, gy0, gx1, gy1)| {
        let (gw, gh) = (gx1 - gx0, gy1 - gy0);
        if gw * gh > 0.5 * w * h || gw < 6.0 || gh < 6.0 {
            return false;
        }
        if gx0 <= b.x0 + 1.0 && gx1 >= b.x1 - 1.0 && gy0 <= b.top + 1.0 && gy1 >= b.bot - 1.0 {
            return false;
        }
        let hov = gx1.min(b.x1 + 0.05 * w) - gx0.max(b.x0 - 0.05 * w);
        let vov = gy1.min(hi) - gy0.max(lo);
        vov >= 0.6 * gh && hov > 0.0
    })
}

enum Decision {
    Keep,
    Block,
    Lines(Vec<usize>),
}

struct Decide<'a> {
    side: Side,
    b: &'a Block,
    lines: &'a [Line<'a>],
    text: &'a str,
    h: f32,
    bw: f32,
    body: f32,
    gap: f32,
    gap_ok: bool,
    figure_between: &'a mut dyn FnMut() -> bool,
}

fn decide(d: Decide) -> Decision {
    let b = d.b;
    let lines: Vec<&Line> = b.lines.iter().map(|&i| &d.lines[i]).collect();
    let height = (b.bot - b.top) / d.h;
    let chars = d.text.chars().count();
    let size = lines.iter().map(|l| l.src.size).fold(0.0f32, f32::max) / d.body;
    let size_min = lines.iter().map(|l| l.src.size).fold(f32::MAX, f32::min) / d.body;
    let first = lines.first().map_or("", |l| l.src.text.as_str());
    let footnote = is_footnote(first);
    let caption = CAPTION.is_match(d.text);
    let bottom = d.side == Side::Bottom;
    let in_geo = match d.side {
        Side::Top => b.bot / d.h <= 0.08,
        Side::Bottom => b.top / d.h >= 0.87,
    };

    // Geometric running line.
    if in_geo && height <= 0.035 && chars > 0 && chars <= 150 {
        let mut ok = d.gap_ok && size <= if bottom { 2.0 } else { 1.15 } && !caption;
        if ok && bottom {
            ok = !footnote && !(d.figure_between)();
        }
        if ok && bottom {
            // Prose the page break cut: body-size words running the
            // measure, or ending a sentence.
            let measure = lines.iter().map(|l| l.x1 - l.x0).fold(0.0f32, f32::max) / d.bw;
            let ends = d
                .text
                .trim_end()
                .ends_with(['.', '!', '?', ':', ';', ')', '"', '\'', '”']);
            if size_min >= 0.85 && wordy(d.text) && (measure >= 0.85 || ends) {
                ok = false;
            }
        }
        if ok {
            return Decision::Block;
        }
    }
    // Small-type notice at the foot.
    if bottom
        && in_geo
        && size <= 0.80
        && chars > 0
        && chars <= 800
        && height <= 0.06
        && d.gap_ok
        && !footnote
        && !caption
        && !(d.figure_between)()
    {
        return Decision::Block;
    }
    // Lexical, line by line, in a short block.
    let in_lex = match d.side {
        Side::Top => b.bot / d.h <= 0.10,
        Side::Bottom => b.top / d.h >= 0.85,
    };
    if !in_lex || d.gap < 0.0 || height > 0.035 || lines.len() > 2 {
        return Decision::Keep;
    }
    let reference = REFSTART.is_match(first);
    let mut idx = Vec::new();
    for (k, l) in b.lines.iter().zip(&lines) {
        let text = l.src.text.trim();
        let Some(class) = lexical_class(text) else {
            continue;
        };
        if text.chars().count() > 150 || l.src.size > 1.5 * d.body {
            continue;
        }
        if reference
            && matches!(
                class,
                Lexical::Citation | Lexical::Identifier | Lexical::Url
            )
        {
            continue;
        }
        if bottom
            && footnote
            && !matches!(
                class,
                Lexical::Url | Lexical::Identifier | Lexical::Copyright | Lexical::Provenance
            )
        {
            continue;
        }
        if class == Lexical::PageNumber && !alone_on_row(l, d.lines) {
            continue;
        }
        if class == Lexical::Date && text.chars().count() > 40 {
            continue;
        }
        // A body-size line of words close under the text is the text's
        // last line ("…orders at shop.example.com, volume 30…"), whatever
        // it mentions; only a page-number, "page x of y" or "downloaded
        // from" line is furniture there.
        let prose = l.src.size >= 0.85 * d.body && wordy(text);
        if prose
            && d.gap < 1.5
            && !matches!(
                class,
                Lexical::PageOf | Lexical::PageNumber | Lexical::Provenance
            )
        {
            continue;
        }
        idx.push(*k);
    }
    if idx.is_empty() {
        Decision::Keep
    } else {
        Decision::Lines(idx)
    }
}

/// Six or more words, at least half of them mostly letters: running text.
fn wordy(text: &str) -> bool {
    let words: Vec<&str> = text.split_whitespace().collect();
    let alpha = words
        .iter()
        .filter(|w| {
            w.chars().filter(|c| c.is_alphabetic()).count() >= (w.chars().count() / 2).max(1)
        })
        .count();
    words.len() >= 6 && alpha * 2 >= words.len()
}

/// No other text shares the line's row.
fn alone_on_row(line: &Line, lines: &[Line]) -> bool {
    !lines.iter().any(|o| {
        !std::ptr::eq(o.src, line.src)
            && o.bot.min(line.bot) - o.top.max(line.top) > 0.3 * o.h().min(line.h())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parsers::pdf_furniture::GWord;

    fn line(text: &str, x0: f32, y_top: f32, size: f32) -> GLine {
        // Page 612 x 792; `y_top` measured down from the top edge.
        let y1 = 792.0 - y_top;
        let y0 = y1 - size;
        let x1 = x0 + 0.5 * size * text.chars().count() as f32;
        GLine {
            text: text.into(),
            x0,
            y0,
            x1,
            y1,
            size,
            words: vec![GWord {
                text: text.into(),
                x0,
                y0,
                x1,
                y1,
                size,
            }],
        }
    }

    fn page(lines: Vec<GLine>) -> PageGeom {
        let words = lines.iter().flat_map(|l| l.words.clone()).collect();
        PageGeom {
            bx: (0.0, 0.0, 612.0, 792.0),
            lines,
            words,
            glued: vec![],
            table_boxes: vec![],
        }
    }

    fn body(from: f32, n: usize) -> Vec<GLine> {
        (0..n)
            .map(|i| {
                line(
                    "Body text of the page that runs on for a while and more",
                    72.0,
                    from + 14.0 * i as f32,
                    10.0,
                )
            })
            .collect()
    }

    fn removed(p: &PageGeom) -> Vec<String> {
        let hits = edge_furniture(p, Vec::new);
        p.lines
            .iter()
            .filter(|l| {
                hits.iter()
                    .any(|h| (h.rect.y - l.y0).abs() < 0.01 && (h.rect.x - l.x0).abs() < 0.01)
            })
            .map(|l| l.text.clone())
            .collect()
    }

    #[test]
    fn running_lines_clear_of_the_body_are_furniture() {
        let mut lines = body(120.0, 30);
        lines.push(line("ACME Quarterly Review", 72.0, 30.0, 9.0));
        lines.push(line("Doc 2024-117-AX rev 3", 72.0, 760.0, 8.0));
        let p = page(lines);
        assert_eq!(
            removed(&p),
            ["ACME Quarterly Review", "Doc 2024-117-AX rev 3"]
        );
    }

    #[test]
    fn titles_footnotes_captions_and_cut_prose_stay() {
        let mut lines = body(120.0, 30);
        // A title larger than the body at the top.
        lines.push(line("A Big Title", 72.0, 30.0, 18.0));
        // A footnote and a caption at the foot.
        lines.push(line(
            "1 See the appendix for the full derivation.",
            72.0,
            740.0,
            8.0,
        ));
        lines.push(line("Figure 3: Results by region", 300.0, 760.0, 8.0));
        let p = page(lines);
        assert!(removed(&p).is_empty(), "{:?}", removed(&p));
    }

    #[test]
    fn the_texts_last_lines_stay_whatever_they_mention() {
        let mut lines = body(120.0, 40);
        // A paragraph after the usual paragraph gap, low on the page.
        let last_bottom = 120.0 + 14.0 * 39.0 + 10.0;
        lines.push(line(
            "Sales volume 30 percent higher than in the prior year",
            72.0,
            last_bottom + 8.0,
            10.0,
        ));
        lines.push(line(
            "and orders can be placed at shop.example.com today.",
            72.0,
            last_bottom + 22.0,
            10.0,
        ));
        let p = page(lines);
        assert!(removed(&p).is_empty(), "{:?}", removed(&p));
    }

    #[test]
    fn a_close_line_is_kept_unless_its_words_are_furniture() {
        let mut lines = body(120.0, 40);
        // One body line height under the body: too close for the
        // geometric rule, so only the line whose words are furniture goes.
        let last_bottom = 120.0 + 14.0 * 39.0 + 10.0;
        lines.push(line(
            "https://example.org/report",
            72.0,
            last_bottom + 10.0,
            8.0,
        ));
        lines.push(line(
            "Prepared by the review team",
            72.0,
            last_bottom + 24.0,
            8.0,
        ));
        let p = page(lines);
        assert_eq!(removed(&p), ["https://example.org/report"]);
    }

    #[test]
    fn furniture_patterns() {
        assert_eq!(lexical_class("Page 3 of 12"), Some(Lexical::PageOf));
        assert_eq!(lexical_class("- 14 -"), Some(Lexical::PageNumber));
        assert_eq!(
            lexical_class("doi:10.1016/j.x.2020.01.002"),
            Some(Lexical::Identifier)
        );
        assert_eq!(
            lexical_class("© 2019 Elsevier B.V."),
            Some(Lexical::Copyright)
        );
        assert_eq!(
            lexical_class("Downloaded from https://x.org by guest"),
            Some(Lexical::Url)
        );
        assert_eq!(lexical_class("12 March 2021"), Some(Lexical::Date));
        assert_eq!(lexical_class("The results were clear."), None);
        assert_eq!(lexical_class("xiv"), Some(Lexical::PageNumber));
        assert_eq!(lexical_class("civil"), None);
        assert_eq!(lexical_class("ill"), None);
        assert!(is_footnote("3 The survey ran from May to June."));
        assert!(!is_footnote("12"));
    }
}
