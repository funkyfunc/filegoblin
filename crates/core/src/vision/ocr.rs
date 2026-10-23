//! Text recognition for scanned pages with PP-OCR (Apache-2.0).
//!
//! Detection is a DB (differentiable binarization) network: it predicts a
//! per-pixel "text" probability; connected regions above a threshold become
//! line boxes, grown back out by the shrink ratio the model was trained with.
//! Recognition reads each line crop at a fixed height and CTC-decodes the
//! character sequence.
//!
//! Three model generations can be installed: PP-OCRv6 medium and small (50
//! languages, much better on handwriting and degraded print) and the
//! English PP-OCRv5 mobile recognizer. The best installed detector and
//! recognizer are used; `FILEGOBLIN_OCR_DET` / `FILEGOBLIN_OCR_REC`
//! (`v6m`, `v6s`, `v5`) pick one for comparisons.

use super::models;
use image::RgbImage;
use rten::{Model, ValueOrView};
use rten_tensor::NdTensor;
use rten_tensor::prelude::*;
use std::sync::OnceLock;

/// Longest side the detector sees. Larger finds smaller text, slower.
const DET_LONG_SIDE: u32 = 2400;
const REC_HEIGHT: u32 = 48;
const REC_BATCH: usize = 24;

struct Ocr {
    det: Model,
    rec: Model,
    /// CTC class → text; index 0 is the blank.
    chars: Vec<String>,
    /// CTC class is a CJK symbol (Han, kana, Hangul, CJK punctuation,
    /// full-width forms): the classes a Latin page must not read.
    cjk: Vec<bool>,
    /// DB post-processing as each detector was trained: pixel threshold,
    /// mean box score, unclip ratio.
    det_thresh: f32,
    box_thresh: f32,
    unclip: f32,
}

/// (name, model file, DB thresholds) per detector, best first.
const DETECTORS: [(&str, &str, (f32, f32, f32)); 3] = [
    ("v6m", models::OCR_V6M_DET, (0.2, 0.45, 1.4)),
    ("v6s", models::OCR_V6S_DET, (0.2, 0.45, 1.4)),
    ("v5", models::OCR_DET, (0.3, 0.6, 1.5)),
];
/// (name, model file, dictionary) per recognizer, best first.
const RECOGNIZERS: [(&str, &str, &str); 3] = [
    ("v6m", models::OCR_V6M_REC, models::OCR_V6_DICT),
    ("v6s", models::OCR_V6S_REC, models::OCR_V6_DICT),
    ("v5", models::OCR_REC, models::OCR_DICT),
];

/// The first installed entry, or the one `env` names.
fn pick<T: Copy>(env: &str, list: &[T], name: fn(&T) -> &str, file: fn(&T) -> &str) -> Option<T> {
    let wanted = std::env::var(env).ok();
    list.iter()
        .copied()
        .filter(|e| wanted.as_deref().is_none_or(|w| w == name(e)))
        .find(|e| models::available(file(e)))
}

fn ocr() -> Option<&'static Ocr> {
    static M: OnceLock<Option<Ocr>> = OnceLock::new();
    M.get_or_init(|| {
        let (dn, dfile, (det_thresh, box_thresh, unclip)) =
            pick("FILEGOBLIN_OCR_DET", &DETECTORS, |e| e.0, |e| e.1)?;
        let (rn, rfile, dict_file) = pick("FILEGOBLIN_OCR_REC", &RECOGNIZERS, |e| e.0, |e| e.1)?;
        let det = models::load(dfile)?;
        let rec = models::load(rfile)?;
        let dict = models::read_text(dict_file)?;
        let mut chars = vec![String::new()];
        chars.extend(dict.lines().map(|l| l.trim_end_matches('\r').to_string()));
        chars.push(" ".to_string());
        let cjk = chars
            .iter()
            .map(|s| s.chars().next().is_some_and(is_cjk))
            .collect();
        if std::env::var_os("FILEGOBLIN_DEBUG").is_some() {
            eprintln!("ocr: detector {dn}, recognizer {rn}");
        }
        Some(Ocr {
            det,
            rec,
            chars,
            cjk,
            det_thresh,
            box_thresh,
            unclip,
        })
    })
    .as_ref()
}

pub fn available() -> bool {
    ocr().is_some()
}

/// One recognized text line, in image pixels.
#[derive(Debug, Clone)]
pub struct Line {
    pub text: String,
    pub x0: f32,
    pub y0: f32,
    pub x1: f32,
    pub y1: f32,
    pub score: f32,
}

/// A detected text line as a rotated rectangle in image pixels: centre,
/// unit direction along the line, length and thickness.
#[derive(Debug, Clone, Copy)]
struct RotBox {
    cx: f32,
    cy: f32,
    ux: f32,
    uy: f32,
    len: f32,
    thick: f32,
}

impl RotBox {
    /// Axis-aligned bounds `(x0, y0, x1, y1)`.
    fn bounds(&self) -> (f32, f32, f32, f32) {
        let (hx, hy) = (self.ux * self.len / 2.0, self.uy * self.len / 2.0);
        let (vx, vy) = (-self.uy * self.thick / 2.0, self.ux * self.thick / 2.0);
        let xs = [hx + vx, hx - vx, -hx + vx, -hx - vx];
        let ys = [hy + vy, hy - vy, -hy + vy, -hy - vy];
        let f = |v: &[f32], c: f32, min: bool| {
            v.iter()
                .fold(if min { f32::MAX } else { f32::MIN }, |a, &b| {
                    if min { a.min(b) } else { a.max(b) }
                })
                + c
        };
        (
            f(&xs, self.cx, true),
            f(&ys, self.cy, true),
            f(&xs, self.cx, false),
            f(&ys, self.cy, false),
        )
    }

    /// Samples the box upright at `height` pixels (bilinear), so a skewed
    /// line is read straight and its neighbours stay out of the crop.
    fn warp(&self, img: &RgbImage, height: u32) -> RgbImage {
        let width = ((self.len / self.thick.max(1.0)) * height as f32)
            .round()
            .clamp(8.0, 3200.0) as u32;
        let mut out = RgbImage::new(width, height);
        let (iw, ih) = (img.width() as f32, img.height() as f32);
        for j in 0..height {
            let v = ((j as f32 + 0.5) / height as f32 - 0.5) * self.thick;
            for i in 0..width {
                let u = ((i as f32 + 0.5) / width as f32 - 0.5) * self.len;
                let x = (self.cx + u * self.ux - v * self.uy - 0.5).clamp(0.0, iw - 1.0);
                let y = (self.cy + u * self.uy + v * self.ux - 0.5).clamp(0.0, ih - 1.0);
                let (x0, y0) = (x.floor() as u32, y.floor() as u32);
                let (x1, y1) = (
                    (x0 + 1).min(img.width() - 1),
                    (y0 + 1).min(img.height() - 1),
                );
                let (fx, fy) = (x - x0 as f32, y - y0 as f32);
                let mut px = [0u8; 3];
                for (c, slot) in px.iter_mut().enumerate() {
                    let a = f32::from(img.get_pixel(x0, y0)[c]) * (1.0 - fx)
                        + f32::from(img.get_pixel(x1, y0)[c]) * fx;
                    let b = f32::from(img.get_pixel(x0, y1)[c]) * (1.0 - fx)
                        + f32::from(img.get_pixel(x1, y1)[c]) * fx;
                    *slot = (a * (1.0 - fy) + b * fy).round() as u8;
                }
                out.put_pixel(i, j, image::Rgb(px));
            }
        }
        out
    }
}

/// Detects and reads every text line in the image.
pub fn read(img: &RgbImage) -> Option<Vec<Line>> {
    let o = ocr()?;
    let t = std::time::Instant::now();
    let boxes = detect(o, img)?;
    let t_det = t.elapsed();
    let mut best: Vec<Reading> = vec![Reading::default(); boxes.len()];
    read_crops(o, &boxes, &mut best, |b| b.warp(img, REC_HEIGHT))?;
    keep_one_script(&mut best);
    let mut lines: Vec<Line> = Vec::with_capacity(boxes.len());
    for (b, Reading { text, score, .. }) in boxes.iter().zip(best) {
        let t = text.trim().to_string();
        if t.is_empty() || score < 0.5 {
            continue;
        }
        let (x0, y0, x1, y1) = b.bounds();
        lines.push(Line {
            text: t,
            x0,
            y0,
            x1,
            y1,
            score,
        });
    }
    if std::env::var_os("FILEGOBLIN_DEBUG").is_some() {
        eprintln!(
            "ocr: {} lines, detect {t_det:?}, recognize {:?}",
            lines.len(),
            t.elapsed() - t_det
        );
    }
    Some(lines)
}

/// One line's reading, and — when it holds CJK symbols — the reading with
/// every CJK class ruled out at each time step.
#[derive(Debug, Clone, Default)]
struct Reading {
    text: String,
    score: f32,
    without_cjk: Option<(String, f32)>,
}

/// Is `c` written in a CJK script (or is it CJK punctuation or a full-width
/// form)?
fn is_cjk(c: char) -> bool {
    matches!(c as u32,
        0x2E80..=0x9FFF      // radicals, CJK punctuation, kana, Han
        | 0xAC00..=0xD7AF    // Hangul syllables
        | 0xF900..=0xFAFF    // compatibility ideographs
        | 0xFF00..=0xFFEF    // half- and full-width forms
        | 0x20000..=0x3FFFF) // Han extensions
}

/// Keeps a page in one script family. The multilingual recognizer reads a
/// few marks on Latin pages as look-alike CJK characters (a dash as `一`, a
/// plus as `十`, a check box as `口`), which no reader of that page would
/// write. When CJK symbols are a stray few on a page of Latin, Greek or
/// Cyrillic text (at most one letter in twenty, against at least twenty
/// letters of that text), every line that has them takes its reading with
/// the CJK classes ruled out. Pages that are really written in Chinese,
/// Japanese or Korean have far more than that and keep their reading.
fn keep_one_script(readings: &mut [Reading]) {
    let (mut cjk, mut spaced) = (0usize, 0usize);
    for r in readings.iter() {
        for c in r.text.chars() {
            if is_cjk(c) {
                cjk += 1;
            } else if c.is_alphabetic() && (c as u32) < 0x0590 {
                spaced += 1;
            }
        }
    }
    if cjk == 0 || spaced < 20 || cjk * 20 > cjk + spaced {
        return;
    }
    for r in readings.iter_mut() {
        if let Some((text, score)) = r.without_cjk.take() {
            r.text = text;
            r.score = score;
        }
    }
}

/// Recognizes `boxes` (batched by aspect ratio so padding stays small) and
/// stores each reading in `out`.
fn read_crops(
    o: &Ocr,
    boxes: &[RotBox],
    out: &mut [Reading],
    crop: impl Fn(&RotBox) -> RgbImage,
) -> Option<()> {
    let mut order: Vec<usize> = (0..boxes.len()).collect();
    order.sort_by(|&a, &b| {
        let ra = boxes[a].len / boxes[a].thick.max(1.0);
        let rb = boxes[b].len / boxes[b].thick.max(1.0);
        ra.total_cmp(&rb)
    });
    for chunk in order.chunks(REC_BATCH) {
        let crops: Vec<RgbImage> = chunk.iter().map(|&i| crop(&boxes[i])).collect();
        let texts = recognize(o, &crops)?;
        for (&i, r) in chunk.iter().zip(texts) {
            out[i] = r;
        }
    }
    Some(())
}

/// Line boxes in image pixels.
fn detect(o: &Ocr, img: &RgbImage) -> Option<Vec<RotBox>> {
    let (w, h) = img.dimensions();
    let r = (DET_LONG_SIDE as f32 / w.max(h) as f32).min(1.0);
    let round32 = |v: f32| (((v / 32.0).round() as u32) * 32).max(32);
    let (nw, nh) = (round32(w as f32 * r), round32(h as f32 * r));
    let small = image::imageops::resize(img, nw, nh, image::imageops::FilterType::Triangle);
    const MEAN: [f32; 3] = [0.485, 0.456, 0.406];
    const STD: [f32; 3] = [0.229, 0.224, 0.225];
    let (nwu, nhu) = (nw as usize, nh as usize);
    let mut x = NdTensor::<f32, 4>::zeros([1, 3, nhu, nwu]);
    for (px, py, p) in small.enumerate_pixels() {
        // The network was trained on BGR input.
        for c in 0..3 {
            x[[0, c, py as usize, px as usize]] = (f32::from(p[2 - c]) / 255.0 - MEAN[c]) / STD[c];
        }
    }
    let input = *o.det.input_ids().first()?;
    let out = o
        .det
        .run(
            vec![(input, ValueOrView::from(x.view()))],
            o.det.output_ids(),
            None,
        )
        .ok()?;
    let prob: NdTensor<f32, 4> = out.first()?.clone().try_into().ok()?;
    let (ph, pw) = (prob.size(2), prob.size(3));
    let p: Vec<f32> = (0..ph * pw).map(|i| prob[[0, 0, i / pw, i % pw]]).collect();
    let mask: Vec<bool> = p.iter().map(|&v| v > o.det_thresh).collect();
    let (sx, sy) = (w as f32 / pw as f32, h as f32 / ph as f32);

    // Connected components (8-neighbour) over the binary map; each becomes a
    // rotated rectangle from the principal axis of its pixels.
    let mut seen = vec![false; ph * pw];
    let mut boxes = Vec::new();
    let mut stack = Vec::new();
    let mut pts: Vec<(f32, f32)> = Vec::new();
    for start in 0..ph * pw {
        if !mask[start] || seen[start] {
            continue;
        }
        seen[start] = true;
        stack.push(start);
        pts.clear();
        let mut sum = 0f32;
        while let Some(i) = stack.pop() {
            let (cy, cx) = (i / pw, i % pw);
            pts.push((cx as f32 + 0.5, cy as f32 + 0.5));
            sum += p[i];
            for dy in -1i64..=1 {
                for dx in -1i64..=1 {
                    let (ny, nx) = (cy as i64 + dy, cx as i64 + dx);
                    if ny < 0 || nx < 0 || ny >= ph as i64 || nx >= pw as i64 {
                        continue;
                    }
                    let j = ny as usize * pw + nx as usize;
                    if mask[j] && !seen[j] {
                        seen[j] = true;
                        stack.push(j);
                    }
                }
            }
        }
        if pts.len() < 6 || sum / (pts.len() as f32) < o.box_thresh {
            continue;
        }
        let Some(rb) = principal_box(&pts) else {
            continue;
        };
        if rb.len.min(rb.thick) < 3.0 {
            continue;
        }
        // The detector predicts shrunk text kernels; grow them back by the
        // DB unclip distance (area × ratio / perimeter).
        let d = rb.len * rb.thick * o.unclip / (2.0 * (rb.len + rb.thick));
        let mut grown = RotBox {
            len: rb.len + 2.0 * d,
            thick: rb.thick + 2.0 * d,
            ..rb
        };
        // Back to image pixels (non-uniform scale: transform the axes).
        let (ax, ay) = (grown.ux * sx, grown.uy * sy);
        let alen = (ax * ax + ay * ay).sqrt().max(1e-6);
        let (bx, by) = (-grown.uy * sx, grown.ux * sy);
        let blen = (bx * bx + by * by).sqrt().max(1e-6);
        grown = RotBox {
            cx: grown.cx * sx,
            cy: grown.cy * sy,
            ux: ax / alen,
            uy: ay / alen,
            len: grown.len * alen,
            thick: grown.thick * blen,
        };
        boxes.push(grown);
    }
    Some(boxes)
}

/// Joins CTC symbols, putting back word spaces the recognizer left out.
///
/// Symbols are read at a steady pace — one every few time steps — and a
/// space is a symbol of its own. The multilingual PP-OCRv6 recognizers
/// often emit nothing in a space's slot ("couldtell", "borderforfifteen"),
/// but the slot is still there: a gap of about two symbols between two
/// letters. Such a gap between two letters or digits of a spaced script
/// becomes a space; scripts written without spaces (Chinese, Japanese,
/// Thai…) are left alone.
fn join_with_missing_spaces(emitted: &[(&str, usize)]) -> String {
    let mut gaps: Vec<usize> = emitted.windows(2).map(|w| w[1].1 - w[0].1).collect();
    gaps.sort_unstable();
    let median = gaps.get(gaps.len() / 2).copied().unwrap_or(0) as f32;
    let spaced = |s: &str| {
        s.chars().all(|c| {
            c.is_ascii_alphanumeric()
                || (c.is_alphabetic() && (c as u32) < 0x0590) // Latin, Greek, Cyrillic
                || matches!(c, ',' | '.' | ';' | ':' | ')' | '!' | '?' | '"' | '\'')
        })
    };
    let mut out = String::new();
    for (i, &(sym, t)) in emitted.iter().enumerate() {
        if i > 0 && median >= 2.0 {
            let (prev, pt) = emitted[i - 1];
            let gap = (t - pt) as f32;
            if gap >= (1.8 * median).max(median + 2.5)
                && prev != " "
                && sym != " "
                && spaced(prev)
                && spaced(sym)
                && sym
                    .chars()
                    .all(|c| c.is_alphanumeric() || c == '"' || c == '(')
            {
                out.push(' ');
            }
        }
        out.push_str(sym);
    }
    out
}

/// Rotated rectangle around a point set along its principal axis. Text
/// lines are near-horizontal, so the axis is oriented left to right and
/// clamped to within 45° of horizontal (a tall box stays a tall box).
fn principal_box(pts: &[(f32, f32)]) -> Option<RotBox> {
    let n = pts.len() as f32;
    let (mx, my) = pts.iter().fold((0.0, 0.0), |a, p| (a.0 + p.0, a.1 + p.1));
    let (mx, my) = (mx / n, my / n);
    let (mut sxx, mut syy, mut sxy) = (0f32, 0f32, 0f32);
    for &(x, y) in pts {
        sxx += (x - mx) * (x - mx);
        syy += (y - my) * (y - my);
        sxy += (x - mx) * (y - my);
    }
    let mut theta = 0.5 * (2.0 * sxy).atan2(sxx - syy);
    if theta.abs() > std::f32::consts::FRAC_PI_4 {
        theta = 0.0;
    }
    let (ux, uy) = (theta.cos(), theta.sin());
    let (mut u0, mut u1, mut v0, mut v1) = (f32::MAX, f32::MIN, f32::MAX, f32::MIN);
    for &(x, y) in pts {
        let (dx, dy) = (x - mx, y - my);
        let u = dx * ux + dy * uy;
        let v = -dx * uy + dy * ux;
        u0 = u0.min(u);
        u1 = u1.max(u);
        v0 = v0.min(v);
        v1 = v1.max(v);
    }
    // Pixel centres: extend half a pixel each side.
    let (len, thick) = (u1 - u0 + 1.0, v1 - v0 + 1.0);
    let (cu, cv) = ((u0 + u1) / 2.0, (v0 + v1) / 2.0);
    Some(RotBox {
        cx: mx + cu * ux - cv * uy,
        cy: my + cu * uy + cv * ux,
        ux,
        uy,
        len,
        thick,
    })
}

/// Greedy CTC decoding of one class sequence: (text, mean confidence of the
/// emitted symbols).
fn ctc_collapse(o: &Ocr, steps: &[(usize, f32)]) -> (String, f32) {
    let (mut prev, mut conf, mut nconf) = (0usize, 0f32, 0usize);
    // Every emitted symbol with the time step it was read at.
    let mut emitted: Vec<(&str, usize)> = Vec::new();
    for (t, &(best, bp)) in steps.iter().enumerate() {
        if best != 0 && best != prev {
            if let Some(s) = o.chars.get(best) {
                emitted.push((s.as_str(), t));
            }
            conf += bp;
            nconf += 1;
        }
        prev = best;
    }
    let text = join_with_missing_spaces(&emitted);
    if std::env::var_os("FILEGOBLIN_DEBUG_CTC").is_some() {
        let v: Vec<String> = emitted.iter().map(|(c, t)| format!("{c}{t}")).collect();
        eprintln!("ctc used={} | {} | {text}", steps.len(), v.join(" "));
    }
    (text, if nconf == 0 { 0.0 } else { conf / nconf as f32 })
}

/// Reads a batch of line crops.
fn recognize(o: &Ocr, crops: &[RgbImage]) -> Option<Vec<Reading>> {
    let widths: Vec<u32> = crops
        .iter()
        .map(|c| {
            ((c.width() as f32 * REC_HEIGHT as f32 / c.height().max(1) as f32).round() as u32)
                .clamp(16, 3200)
        })
        .collect();
    let maxw = *widths.iter().max()? as usize;
    let hh = REC_HEIGHT as usize;
    let mut x = NdTensor::<f32, 4>::zeros([crops.len(), 3, hh, maxw]);
    for (k, c) in crops.iter().enumerate() {
        let r = image::imageops::resize(
            c,
            widths[k],
            REC_HEIGHT,
            image::imageops::FilterType::Triangle,
        );
        for (px, py, p) in r.enumerate_pixels() {
            for ch in 0..3 {
                x[[k, ch, py as usize, px as usize]] = (f32::from(p[2 - ch]) / 255.0 - 0.5) / 0.5;
            }
        }
    }
    let input = *o.rec.input_ids().first()?;
    let out = o
        .rec
        .run(
            vec![(input, ValueOrView::from(x.view()))],
            o.rec.output_ids(),
            None,
        )
        .ok()?;
    let probs: NdTensor<f32, 3> = out.first()?.clone().try_into().ok()?;
    let (steps, classes) = (probs.size(1), probs.size(2));
    let mut res = Vec::with_capacity(crops.len());
    for k in 0..crops.len() {
        // Only the steps covering this crop's real (unpadded) width.
        let used = ((widths[k] as usize * steps).div_ceil(maxw)).min(steps);
        // Per time step: the best class, and the best class that is not CJK.
        let mut any: Vec<(usize, f32)> = Vec::with_capacity(used);
        let mut no_cjk: Vec<(usize, f32)> = Vec::with_capacity(used);
        for t in 0..used {
            let (mut best, mut bp) = (0usize, f32::MIN);
            let (mut best_n, mut bp_n) = (0usize, f32::MIN);
            for c in 0..classes {
                let v = probs[[k, t, c]];
                if v > bp {
                    bp = v;
                    best = c;
                }
                if v > bp_n && !o.cjk.get(c).copied().unwrap_or(false) {
                    bp_n = v;
                    best_n = c;
                }
            }
            any.push((best, bp));
            no_cjk.push((best_n, bp_n));
        }
        let (text, score) = ctc_collapse(o, &any);
        let without_cjk = text.chars().any(is_cjk).then(|| ctc_collapse(o, &no_cjk));
        res.push(Reading {
            text,
            score,
            without_cjk,
        });
    }
    Some(res)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_spaces_come_back_from_symbol_timing() {
        let syms = |s: &[(&'static str, usize)]| join_with_missing_spaces(s);
        // "I could tell" with the space after "could" left out.
        let e = [
            ("I", 2),
            (" ", 5),
            ("c", 8),
            ("o", 11),
            ("u", 14),
            ("l", 17),
            ("d", 20),
            ("t", 26),
            ("e", 29),
            ("l", 32),
            ("l", 35),
        ];
        assert_eq!(syms(&e), "I could tell");
        // Ordinary spacing is untouched.
        let e = [("a", 2), ("m", 5), (" ", 8), ("s", 12), ("o", 15)];
        assert_eq!(syms(&e), "am so");
        // Scripts without spaces stay unspaced.
        let e = [("中", 2), ("文", 5), ("字", 11), ("符", 14)];
        assert_eq!(syms(&e), "中文字符");
    }

    #[test]
    fn stray_cjk_on_a_latin_page_is_read_again_without_it() {
        let reading = |text: &str, alt: Option<&str>| Reading {
            text: text.into(),
            score: 0.9,
            without_cjk: alt.map(|a| (a.to_string(), 0.8)),
        };
        let mut page = vec![
            reading("The quick brown fox jumps over the lazy dog", None),
            reading("x 十 1 = y", Some("x + 1 = y")),
            reading("一 Das Kennfeld", Some("- Das Kennfeld")),
        ];
        keep_one_script(&mut page);
        assert_eq!(page[1].text, "x + 1 = y");
        assert_eq!(page[2].text, "- Das Kennfeld");

        // A page written in Japanese keeps every reading.
        let mut page = vec![
            reading("被験物質の投与容量で投与した", Some("x")),
            reading("流体力学教育における一つの試み", Some("y")),
            reading("Table 1", None),
        ];
        keep_one_script(&mut page);
        assert_eq!(page[0].text, "被験物質の投与容量で投与した");

        // Too little Latin text to judge: leave it.
        let mut page = vec![reading("十", Some("+")), reading("ab", None)];
        keep_one_script(&mut page);
        assert_eq!(page[0].text, "十");
    }

    #[test]
    fn principal_box_follows_a_skewed_line() {
        // A 100×6 bar rotated by ~5°.
        let a = 5f32.to_radians();
        let mut pts = Vec::new();
        for u in 0..100 {
            for v in 0..6 {
                let (u, v) = (u as f32 - 50.0, v as f32 - 3.0);
                pts.push((
                    200.0 + u * a.cos() - v * a.sin(),
                    100.0 + u * a.sin() + v * a.cos(),
                ));
            }
        }
        let b = principal_box(&pts).unwrap();
        assert!((b.uy.atan2(b.ux) - a).abs() < 0.02);
        assert!((b.len - 100.0).abs() < 2.0 && (b.thick - 6.0).abs() < 2.0);
        assert!((b.cx - 199.5).abs() < 1.5 && (b.cy - 99.5).abs() < 1.5);
    }
}
