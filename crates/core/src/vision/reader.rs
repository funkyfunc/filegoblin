//! Whole-page reading with LightOnOCR-2-1B (Apache-2.0), for scanned pages
//! that PP-OCR reads poorly: handwriting, and printed math.
//!
//! A Pixtral encoder (Apache-2.0) turns the page image into one feature per
//! 28×28 pixel block; a Qwen3-0.6B decoder (Apache-2.0) then writes the
//! page's Markdown token by token. Both run as 4-bit graphs (`MatMulNBits`)
//! rebuilt from the released weights by `scripts/vision_models.py`; the
//! decoder takes a key/value cache per layer, so each token reads the page so
//! far instead of recomputing it.
//!
//! It is a heavy model (about 0.8 GB of weights, 1–2 GB while it runs, and
//! tens of seconds a page on a CPU), so it reads only the scanned pages the
//! pipeline hands it; see [`ROUTE_BELOW`] and [`ROUTE_FORMULAS`].

use super::models;
use image::RgbImage;
use rten::{Model, ValueOrView};
use rten_tensor::prelude::*;
use rten_tensor::{NdTensor, NdTensorView};
use std::sync::OnceLock;

/// A scanned page goes to the reader when PP-OCR's confidence in it,
/// averaged over its characters, is at most this. On the olmOCR-bench dev
/// split handwritten pages score 0.61–0.85 and printed ones 0.94 or more
/// (2026-11-01 scan-model study).
pub const ROUTE_BELOW: f32 = 0.92;
/// A scanned page also goes to the reader when the layout model finds at
/// least this many formulas on it. On the dev split's scans of printed math
/// it found 6–22; on every other scanned page, 0 or 1.
pub const ROUTE_FORMULAS: usize = 3;
/// …but only when OCR is below this confidence in it. The dev split's math
/// scans, old and worn, score 0.94–0.97; a clean scan of a printed math book
/// scores 0.98–0.99 (the gauntlet's R2), and the pipeline already reads it
/// better than the reader, and nine times faster.
pub const ROUTE_FORMULAS_BELOW: f32 = 0.975;

/// Longest side of the image the encoder sees when it can afford no more:
/// an encoder built before attention ran one head at a time (no
/// `filegoblin.max_side` in its metadata), or a budget without room for
/// [`NEED_NATIVE_MB`]. Attention over the page's patches grows with their
/// square: at 1008 pixels a page is about 4,000 patches, at 1540 about 9,000.
const MAX_SIDE: u32 = 1008;
/// The longest side the encoder may declare (`filegoblin.max_side`): the
/// model's own input size. Read at 1540 rather than 1008, the reader passed
/// 62 more olmOCR-bench test-split scan tests and failed 13 more (old_scans
/// 39.9 → 43.7, old_scans_math 67.2 → 75.7), mostly worn letters it had read
/// one or two characters off (2026-11-06).
const NATIVE_SIDE: u32 = 1540;
/// The encoder's patch (14 pixels) times its 2×2 merge.
const BLOCK: u32 = 28;
/// Most tokens one page may take. A handwritten letter is 150–300, a dense
/// page of math 700.
const MAX_TOKENS: usize = 1536;
/// Below this budget the reader is not loaded at all: a handwritten page
/// peaked at 3.1 GB with the reader and the rest of the pipeline loaded.
const MIN_BUDGET_MB: u64 = 4096;
/// What reading a page may add to the process: the weights when they are
/// not loaded yet, and the encoder's and decoder's working memory. The
/// reader added 0.7–1.1 GB to the pipeline's peak on a dev math scan and on
/// a six-page scanned book (the gauntlet's R2).
const NEED_MB: u64 = 1280;
/// The same at [`NATIVE_SIDE`], with the encoder's heads one at a time: the
/// reader added 1.3–1.8 GB to the pipeline's peak (2.1–2.7 GB) on three
/// test-split scans, a worn letter, a handwritten one and a page of math.
const NEED_NATIVE_MB: u64 = 2560;

const MEAN: [f32; 3] = [0.481_454_66, 0.457_827_5, 0.408_210_73];
const STD: [f32; 3] = [0.268_629_54, 0.261_302_6, 0.275_777_1];
/// `<|im_start|>system<|im_end|>\n<|im_start|>user\n`
const PROMPT_HEAD: [i32; 7] = [151_644, 8948, 151_645, 198, 151_644, 872, 198];
/// `<|im_end|>\n<|im_start|>assistant\n`
const PROMPT_TAIL: [i32; 5] = [151_645, 198, 151_644, 77091, 198];
/// `<|image_pad|>`: one per image block, replaced by its feature.
const IMAGE: i32 = 151_655;
/// `<|im_end|>` and `<|endoftext|>`.
const STOP: [i32; 2] = [151_645, 151_643];
const LAYERS: usize = 28;
const KV_HEADS: usize = 8;
const HEAD_DIM: usize = 128;

struct Reader {
    vision: Model,
    embed: Model,
    step: Model,
    /// token id → decoded bytes
    vocab: Vec<Vec<u8>>,
    /// The longest side the encoder reads within [`NEED_NATIVE_MB`].
    max_side: u32,
}

fn reader() -> Option<&'static Reader> {
    static M: OnceLock<Option<Reader>> = OnceLock::new();
    M.get_or_init(|| {
        if std::env::var("FILEGOBLIN_READER")
            .is_ok_and(|v| v == "0" || v.eq_ignore_ascii_case("off"))
            || crate::memory::budget_mb() < MIN_BUDGET_MB
        {
            return None;
        }
        let vision = models::load(models::READER_VISION)?;
        let max_side = vision
            .metadata()
            .get("filegoblin.max_side")
            .and_then(|v| v.parse::<u32>().ok())
            .map_or(MAX_SIDE, |v| v.clamp(MAX_SIDE, NATIVE_SIDE));
        Some(Reader {
            vision,
            max_side,
            embed: models::load(models::READER_EMBED)?,
            step: models::load(models::READER_STEP)?,
            vocab: super::formula::load_vocab(&models::read_text(models::READER_VOCAB)?)?,
        })
    })
    .as_ref()
}

/// True when the reader's weights are installed (without loading them).
pub fn installed() -> bool {
    [
        models::READER_VISION,
        models::READER_EMBED,
        models::READER_STEP,
        models::READER_VOCAB,
    ]
    .iter()
    .all(|f| models::available(f))
}

/// A page as the reader wrote it.
pub struct Page {
    /// The reader's Markdown, as it came out (see [`finish`]).
    pub text: String,
    /// For each byte of `text`, the log-probability the reader gave the
    /// token it came from: near 0 where it was sure, well below where it
    /// chose between readings. Empty if the tokens did not decode to UTF-8.
    pub byte_lp: Vec<f32>,
}

/// The page as the reader wrote it, or `None` when the reader is not
/// installed, is switched off (`FILEGOBLIN_READER=0`), does not fit the
/// memory budget (or what is left of it), or did not finish the page (a
/// decode that runs to the token limit is a loop, not a page).
pub fn read_page(img: &RgbImage) -> Option<Page> {
    // Room for the larger read is judged before the weights load, so it
    // counts them even when they are already in.
    let roomy = fits(NEED_NATIVE_MB);
    if !installed() || !(roomy || fits(NEED_MB)) {
        return None;
    }
    let r = reader()?;
    let t = std::time::Instant::now();
    let side = if roomy { r.max_side } else { MAX_SIDE };
    let (w, h) = encoder_size(img.width(), img.height(), side);
    let features = encode(r, img, w, h)?;
    crate::parsers::pdf_worker::still_working();
    let t_vision = t.elapsed();
    let (toks, lps) = generate(r, &features)?;
    if std::env::var_os("FILEGOBLIN_DEBUG").is_some() {
        eprintln!(
            "reader: {w}x{h}, {} blocks, {} tokens, vision {t_vision:?}, total {:?}",
            features.size(0),
            toks.len(),
            t.elapsed()
        );
    }
    let mut bytes = Vec::new();
    let mut byte_lp = Vec::new();
    for (&t, &lp) in toks.iter().zip(&lps) {
        if let Some(b) = r.vocab.get(t as usize) {
            bytes.extend_from_slice(b);
            byte_lp.extend(std::iter::repeat_n(lp, b.len()));
        }
    }
    Some(match String::from_utf8(bytes) {
        Ok(text) => Page { text, byte_lp },
        Err(e) => Page {
            text: String::from_utf8_lossy(e.as_bytes()).into_owned(),
            byte_lp: Vec::new(),
        },
    })
}

/// The page's Markdown from what the reader wrote: without the links to
/// pictures it does not produce, or `None` if nothing is left.
pub fn finish(text: &str) -> Option<String> {
    let text = without_pictures(text);
    let text = text.trim();
    (!text.is_empty()).then(|| text.to_string())
}

/// The reader marks each picture on the page with a link to an image file
/// it does not produce (`![image](image_1.png)`, sometimes in a centered
/// `<div>`): drop them.
fn without_pictures(text: &str) -> String {
    static PICTURE: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
        regex::Regex::new(r"(?:<div[^>]*>\s*)?!\[image\]\(image_\d+\.\w+\)(?:\s*</div>)?\n?")
            .expect("valid regex")
    });
    static BLANK: std::sync::LazyLock<regex::Regex> =
        std::sync::LazyLock::new(|| regex::Regex::new(r"\n{3,}").expect("valid regex"));
    BLANK
        .replace_all(&PICTURE.replace_all(text, ""), "\n\n")
        .into_owned()
}

/// Whether the process can take on `need_mb` more for the reader without
/// crossing its budget: a long scan already holds more than a single page,
/// and a PDF worker past its ceiling is killed with the whole document.
fn fits(need_mb: u64) -> bool {
    crate::memory::footprint()
        .is_none_or(|held| (held >> 20) + need_mb <= crate::memory::budget_mb())
}

/// The encoder's input size for an image: the longest side at most `side`,
/// both sides whole blocks.
fn encoder_size(w: u32, h: u32, side: u32) -> (u32, u32) {
    let s = (side as f32 / w.max(h) as f32).min(1.0);
    let fit = |v: u32| (((v as f32 * s) as u32) / BLOCK * BLOCK).max(BLOCK);
    (fit(w), fit(h))
}

/// One feature per image block, `[blocks, 1024]`.
fn encode(r: &Reader, img: &RgbImage, w: u32, h: u32) -> Option<NdTensor<f32, 2>> {
    let small = image::imageops::resize(img, w, h, image::imageops::FilterType::CatmullRom);
    let (wu, hu) = (w as usize, h as usize);
    let mut x = NdTensor::<f32, 4>::zeros([1, 3, hu, wu]);
    for (px, py, p) in small.enumerate_pixels() {
        for c in 0..3 {
            x[[0, c, py as usize, px as usize]] = (f32::from(p[c]) / 255.0 - MEAN[c]) / STD[c];
        }
    }
    let input = *r.vision.input_ids().first()?;
    let out = r
        .vision
        .run(
            vec![(input, ValueOrView::from(x.view()))],
            r.vision.output_ids(),
            None,
        )
        .ok()?;
    out.into_iter().next()?.try_into().ok()
}

/// Token embeddings, `[1, n, 1024]`.
fn embed(r: &Reader, ids: &[i32]) -> Option<NdTensor<f32, 3>> {
    let ids = NdTensor::<i32, 2>::from_data([1, ids.len()], ids.to_vec());
    let input = *r.embed.input_ids().first()?;
    let out = r
        .embed
        .run(
            vec![(input, ValueOrView::from(ids.view()))],
            r.embed.output_ids(),
            None,
        )
        .ok()?;
    out.into_iter().next()?.try_into().ok()
}

/// The prompt around `blocks` image features.
fn prompt(blocks: usize) -> Vec<i32> {
    let mut ids = PROMPT_HEAD.to_vec();
    ids.extend(std::iter::repeat_n(IMAGE, blocks));
    ids.extend(PROMPT_TAIL);
    ids
}

/// Greedy decoding after the image; the tokens before the stop token, or
/// `None` if the page never ended.
/// The page's tokens, and the log-probability the reader gave each.
fn generate(r: &Reader, features: &NdTensor<f32, 2>) -> Option<(Vec<i32>, Vec<f32>)> {
    let ids = prompt(features.size(0));
    let mut x = embed(r, &ids)?;
    for b in 0..features.size(0) {
        for d in 0..features.size(1) {
            x[[0, PROMPT_HEAD.len() + b, d]] = features[[b, d]];
        }
    }
    let node = |name: &str| r.step.find_node(name);
    let (n_x, n_start) = (node("inputs_embeds")?, node("start")?);
    let n_past = (0..LAYERS)
        .flat_map(|i| [format!("past_k_{i}"), format!("past_v_{i}")])
        .map(|name| node(&name))
        .collect::<Option<Vec<_>>>()?;
    // Per layer, keys stored transposed `[1, KV, 1, HEAD_DIM, t]` and values
    // `[1, KV, 1, t, HEAD_DIM]`: the step attends to them as they are and
    // appends the new token with a Concat. The cache is passed in owned, with
    // room reserved for the whole decode, so rten appends in place; passed as
    // a view, every token copied the whole cache (twice as slow, and slower
    // the longer the page).
    let cap = ids.len() + MAX_TOKENS + 1;
    let mut past: Vec<NdTensor<f32, 5>> = (0..2 * LAYERS)
        .map(|i| {
            if i % 2 == 0 {
                NdTensor::with_capacity([1, KV_HEADS, 1, HEAD_DIM, cap], 4)
            } else {
                NdTensor::with_capacity([1, KV_HEADS, 1, cap, HEAD_DIM], 3)
            }
        })
        .collect();
    let mut pos = 0usize;
    let mut out: Vec<i32> = Vec::new();
    let mut lps: Vec<f32> = Vec::new();
    while out.len() < MAX_TOKENS {
        let start = NdTensor::<i32, 1>::from_data([1], vec![pos as i32]);
        let mut inputs = vec![
            (n_x, ValueOrView::from(x.view())),
            (n_start, ValueOrView::from(start.view())),
        ];
        inputs.extend(
            n_past
                .iter()
                .zip(past.drain(..))
                .map(|(&id, t)| (id, ValueOrView::from(t))),
        );
        let o = r.step.run(inputs, r.step.output_ids(), None).ok()?;
        pos += x.size(1);
        let mut o = o.into_iter();
        let logits: NdTensor<f32, 2> = o.next()?.try_into().ok()?;
        past = o.map(|v| v.try_into().ok()).collect::<Option<Vec<_>>>()?;
        let next = argmax(logits.view())?;
        if STOP.contains(&next) {
            return Some((out, lps));
        }
        out.push(next);
        lps.push(log_prob(logits.view(), next));
        if looping(&out) {
            return None;
        }
        if out.len().is_multiple_of(32) {
            crate::parsers::pdf_worker::still_working();
        }
        x = embed(r, &[next])?;
    }
    None
}

fn argmax(logits: NdTensorView<f32, 2>) -> Option<i32> {
    let mut best: Option<(f32, usize)> = None;
    for v in 0..logits.size(1) {
        let l = logits[[0, v]];
        if best.is_none_or(|(b, _)| l > b) {
            best = Some((l, v));
        }
    }
    best.map(|(_, v)| v as i32)
}

/// The log-probability of token `t` under the softmax of `logits`.
fn log_prob(logits: NdTensorView<f32, 2>, t: i32) -> f32 {
    let n = logits.size(1);
    let max = (0..n).map(|v| logits[[0, v]]).fold(f32::MIN, f32::max);
    let sum: f32 = (0..n).map(|v| (logits[[0, v]] - max).exp()).sum();
    logits[[0, t as usize]] - max - sum.ln()
}

/// The decode has fallen into a cycle: its last 96 tokens are one run of at
/// most 24 tokens repeated (a model that loses its place on a page writes
/// the same line, or the same "1 1 1", until the token limit).
fn looping(toks: &[i32]) -> bool {
    const SPAN: usize = 96;
    if toks.len() < SPAN {
        return false;
    }
    let tail = &toks[toks.len() - SPAN..];
    (1..=24).any(|p| tail.iter().skip(p).zip(tail).all(|(a, b)| a == b))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encoder_input_is_whole_blocks_within_the_cap() {
        assert_eq!(encoder_size(2550, 3300, MAX_SIDE), (756, 1008));
        assert_eq!(encoder_size(3300, 2550, MAX_SIDE), (1008, 756));
        assert_eq!(encoder_size(2550, 3300, NATIVE_SIDE), (1176, 1540));
        // Smaller than the cap: not enlarged, only cut to whole blocks.
        assert_eq!(encoder_size(600, 450, MAX_SIDE), (588, 448));
        assert_eq!(encoder_size(10, 10, MAX_SIDE), (28, 28));
    }

    #[test]
    fn the_prompt_holds_one_image_token_per_block() {
        let p = prompt(4);
        assert_eq!(p.len(), PROMPT_HEAD.len() + 4 + PROMPT_TAIL.len());
        assert_eq!(&p[..7], &PROMPT_HEAD);
        assert!(p[7..11].iter().all(|&t| t == IMAGE));
        assert_eq!(&p[11..], &PROMPT_TAIL);
    }

    #[test]
    fn picture_placeholders_are_dropped() {
        let md = "# Notice\n\n<div style=\"text-align: center;\">\n    ![image](image_1.png)\n</div>\n\nText.\n\n![image](image_2.png)\nFig. 3 Viscosity";
        assert_eq!(
            without_pictures(md),
            "# Notice\n\nText.\n\nFig. 3 Viscosity"
        );
    }

    #[test]
    fn a_repeating_tail_is_a_loop_and_prose_is_not() {
        let cycle: Vec<i32> = (0..200).map(|i| [5, 9, 13][i % 3]).collect();
        assert!(looping(&cycle));
        let ones = vec![16; 120];
        assert!(looping(&ones));
        let prose: Vec<i32> = (0..300).map(|i| i * 7919 % 1013).collect();
        assert!(!looping(&prose));
        assert!(!looping(&ones[..50]));
    }
}
