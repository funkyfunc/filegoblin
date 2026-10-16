//! Formula recognition (image → LaTeX) with pix2text MFR 1.5 (MIT).
//!
//! A DeiT encoder reads a 384×384 crop and a TrOCR-style decoder emits
//! byte-level BPE tokens greedily. The decoder runs as two graphs: `init`
//! projects the encoder states into per-layer cross-attention keys/values
//! once, and `step` advances every sequence in the batch by one token with a
//! self-attention cache, so a step costs the same at token 100 as at token 1.
//! (Upstream ships an uncached decoder; ours is rebuilt from its weights and
//! checked against it to 1e-3 in logits.)

use super::models;
use image::RgbImage;
use rten::{Model, ValueOrView};
use rten_tensor::NdTensor;
use rten_tensor::prelude::*;
use std::collections::HashMap;
use std::sync::OnceLock;

const SIDE: u32 = 384;
const BOS: i32 = 1;
const EOS: i32 = 2;
const BATCH: usize = 16;

struct Mfr {
    encoder: Model,
    init: Model,
    step: Model,
    /// token id → decoded bytes
    vocab: Vec<Vec<u8>>,
}

fn mfr() -> Option<&'static Mfr> {
    static M: OnceLock<Option<Mfr>> = OnceLock::new();
    M.get_or_init(|| {
        let encoder = models::load(models::MFR_ENCODER)?;
        let init = models::load(models::MFR_DECODER_INIT)?;
        let step = models::load(models::MFR_DECODER_STEP)?;
        let tok = models::read_text(models::MFR_TOKENIZER)?;
        let vocab = load_vocab(&tok)?;
        Some(Mfr {
            encoder,
            init,
            step,
            vocab,
        })
    })
    .as_ref()
}

pub fn available() -> bool {
    mfr().is_some()
}

/// Recognizes each crop; `None` where recognition failed or produced junk.
/// `max_tokens` bounds each decode.
pub fn recognize(crops: &[RgbImage], max_tokens: usize) -> Vec<Option<String>> {
    let Some(m) = mfr() else {
        return vec![None; crops.len()];
    };
    let mut out = Vec::with_capacity(crops.len());
    let debug = std::env::var_os("FILEGOBLIN_DEBUG").is_some();
    for chunk in crops.chunks(BATCH) {
        let t = std::time::Instant::now();
        match recognize_batch(m, chunk, max_tokens) {
            Some(r) => out.extend(r),
            None => out.extend(std::iter::repeat_n(None, chunk.len())),
        }
        if debug {
            eprintln!("mfr batch of {}: {:?}", chunk.len(), t.elapsed());
        }
    }
    out
}

fn recognize_batch(m: &Mfr, crops: &[RgbImage], max_tokens: usize) -> Option<Vec<Option<String>>> {
    let b = crops.len();
    let s = SIDE as usize;
    let mut x = NdTensor::<f32, 4>::zeros([b, 3, s, s]);
    for (i, crop) in crops.iter().enumerate() {
        let r = image::imageops::resize(crop, SIDE, SIDE, image::imageops::FilterType::CatmullRom);
        for (px, py, p) in r.enumerate_pixels() {
            for c in 0..3 {
                x[[i, c, py as usize, px as usize]] = (f32::from(p[c]) / 255.0 - 0.5) / 0.5;
            }
        }
    }
    let enc_in = *m.encoder.input_ids().first()?;
    let hidden = m
        .encoder
        .run(
            vec![(enc_in, ValueOrView::from(x.view()))],
            m.encoder.output_ids(),
            None,
        )
        .ok()?;
    let hidden: NdTensor<f32, 3> = hidden.first()?.clone().try_into().ok()?;

    let init_in = *m.init.input_ids().first()?;
    let cross = m
        .init
        .run(
            vec![(init_in, ValueOrView::from(hidden.view()))],
            m.init.output_ids(),
            None,
        )
        .ok()?;
    let mut cross_k: NdTensor<f32, 5> = cross.first()?.clone().try_into().ok()?;
    let mut cross_v: NdTensor<f32, 5> = cross.get(1)?.clone().try_into().ok()?;
    let [layers, _, heads, _, hd] = cross_k.shape();
    let mut self_k = NdTensor::<f32, 5>::zeros([layers, b, heads, 0, hd]);
    let mut self_v = NdTensor::<f32, 5>::zeros([layers, b, heads, 0, hd]);

    let node = |name: &str| m.step.find_node(name);
    let (n_ids, n_pos, n_sk, n_sv, n_ck, n_cv) = (
        node("input_ids")?,
        node("pos")?,
        node("self_k")?,
        node("self_v")?,
        node("cross_k")?,
        node("cross_v")?,
    );
    let mut seqs: Vec<Vec<i32>> = vec![vec![BOS]; b];
    let mut done = vec![false; b];
    // Batch slot → sequence index, for the sequences still decoding.
    let mut live: Vec<usize> = (0..b).collect();
    for t in 0..max_tokens {
        let ids = NdTensor::<i32, 2>::from_data(
            [live.len(), 1],
            live.iter()
                .map(|&s| *seqs[s].last().unwrap_or(&BOS))
                .collect::<Vec<i32>>(),
        );
        let pos = NdTensor::<i32, 1>::from_data([1], vec![t as i32]);
        let o = m
            .step
            .run(
                vec![
                    (n_ids, ValueOrView::from(ids.view())),
                    (n_pos, ValueOrView::from(pos.view())),
                    (n_sk, ValueOrView::from(self_k.view())),
                    (n_sv, ValueOrView::from(self_v.view())),
                    (n_ck, ValueOrView::from(cross_k.view())),
                    (n_cv, ValueOrView::from(cross_v.view())),
                ],
                m.step.output_ids(),
                None,
            )
            .ok()?;
        let logits: NdTensor<f32, 2> = o.first()?.clone().try_into().ok()?;
        self_k = o.get(1)?.clone().try_into().ok()?;
        self_v = o.get(2)?.clone().try_into().ok()?;
        let vocab = logits.size(1);
        let mut keep: Vec<usize> = Vec::with_capacity(live.len());
        for (slot, &s) in live.iter().enumerate() {
            let mut best = (f32::MIN, EOS);
            for v in 0..vocab {
                let l = logits[[slot, v]];
                if l > best.0 {
                    best = (l, v as i32);
                }
            }
            seqs[s].push(best.1);
            if best.1 == EOS {
                done[s] = true;
            } else {
                keep.push(slot);
            }
        }
        if keep.is_empty() {
            break;
        }
        if keep.len() < live.len() {
            self_k = select_batch(&self_k, &keep);
            self_v = select_batch(&self_v, &keep);
            cross_k = select_batch(&cross_k, &keep);
            cross_v = select_batch(&cross_v, &keep);
            live = keep.iter().map(|&slot| live[slot]).collect();
        }
    }
    Some(
        seqs.iter()
            .zip(done.iter())
            .map(|(seq, &finished)| {
                // A decode that never stopped is a loop, not a formula.
                if !finished {
                    return None;
                }
                let toks: Vec<i32> = seq[1..].iter().copied().take_while(|&t| t != EOS).collect();
                let text = decode(&m.vocab, &toks);
                let tidy = tidy_latex(&text);
                (!tidy.is_empty()).then_some(tidy)
            })
            .collect(),
    )
}

/// Keeps only the given batch slots (axis 1) of a `[L, B, H, T, D]` cache.
fn select_batch(t: &NdTensor<f32, 5>, slots: &[usize]) -> NdTensor<f32, 5> {
    let [l, b, h, s, d] = t.shape();
    let block = h * s * d;
    let src = t.to_vec();
    let mut out = Vec::with_capacity(l * slots.len() * block);
    for li in 0..l {
        for &oi in slots {
            let start = (li * b + oi) * block;
            out.extend_from_slice(&src[start..start + block]);
        }
    }
    NdTensor::from_data([l, slots.len(), h, s, d], out)
}

/// Byte-level BPE vocabulary from a HuggingFace `tokenizer.json`.
fn load_vocab(json: &str) -> Option<Vec<Vec<u8>>> {
    let v: serde_json::Value = serde_json::from_str(json).ok()?;
    let map = v.get("model")?.get("vocab")?.as_object()?;
    let unicode_to_byte = byte_decoder();
    let size = map.values().filter_map(|i| i.as_u64()).max()? as usize + 1;
    let mut vocab = vec![Vec::new(); size];
    for (tok, id) in map {
        let id = id.as_u64()? as usize;
        if tok.starts_with('<') && tok.ends_with('>') {
            continue; // special tokens decode to nothing
        }
        vocab[id] = tok
            .chars()
            .map(|c| unicode_to_byte.get(&c).copied().unwrap_or(b'?'))
            .collect();
    }
    Some(vocab)
}

/// The GPT-2 byte↔unicode table, inverted.
fn byte_decoder() -> HashMap<char, u8> {
    let mut bs: Vec<u32> = (u32::from(b'!')..=u32::from(b'~'))
        .chain(0xA1..=0xAC)
        .chain(0xAE..=0xFF)
        .collect();
    let mut cs: Vec<u32> = bs.clone();
    let mut n = 0;
    for b in 0..=255u32 {
        if !bs.contains(&b) {
            bs.push(b);
            cs.push(256 + n);
            n += 1;
        }
    }
    bs.into_iter()
        .zip(cs)
        .filter_map(|(b, c)| char::from_u32(c).map(|ch| (ch, b as u8)))
        .collect()
}

fn decode(vocab: &[Vec<u8>], toks: &[i32]) -> String {
    let bytes: Vec<u8> = toks
        .iter()
        .filter_map(|&t| vocab.get(t as usize))
        .flatten()
        .copied()
        .collect();
    String::from_utf8_lossy(&bytes).into_owned()
}

/// Drops the spaces the tokenizer puts between LaTeX tokens, except where
/// they separate a control word from a following letter (`\leq k`).
pub fn tidy_latex(s: &str) -> String {
    // Spacing commands are typography, not content: `\,` `\;` `\:` `\!` and
    // a control space. They also stop the formula from matching the same
    // formula typed without them.
    static SPACING: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
        regex::Regex::new(r"\\\\|\\[,;:!]|\\ |~").expect("valid regex")
    });
    // `\\` (a row break) is matched first so its second backslash is never
    // read as the start of a control space.
    let s = SPACING.replace_all(s, |c: &regex::Captures| {
        if &c[0] == "\\\\" {
            "\\\\".to_string()
        } else {
            " ".to_string()
        }
    });
    // Inside font commands (`\mathrm{G L}`) the recognizer spaces out every
    // letter; math mode ignores those spaces, so drop them for readability.
    static FONT_ARG: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
        regex::Regex::new(r"\\(mathrm|operatorname|mathbf|mathsf|mathit|mathbb|mathcal|mathfrak|boldsymbol)\s*\{([^{}]*)\}")
            .expect("valid regex")
    });
    let s = FONT_ARG.replace_all(&s, |c: &regex::Captures| {
        let inner: String = c[2].chars().filter(|ch| !ch.is_whitespace()).collect();
        format!("\\{}{{{}}}", &c[1], inner)
    });
    let s = canonical_latex(&s);
    let chars: Vec<char> = s.trim().chars().collect();
    let mut out = String::with_capacity(chars.len());
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c.is_whitespace() {
            let mut j = i;
            while j < chars.len() && chars[j].is_whitespace() {
                j += 1;
            }
            let next_letter = chars.get(j).is_some_and(|n| n.is_ascii_alphabetic());
            // The only space math mode needs is the one ending a control word
            // before a letter (`\leq k`, not `\leqk`). Spaces between plain
            // letters (`s c o r e`) are the tokenizer's, and go.
            let tail: String = out
                .chars()
                .rev()
                .take_while(|c| c.is_ascii_alphabetic())
                .collect();
            let ends_control_word =
                !tail.is_empty() && out[..out.len() - tail.len()].ends_with('\\');
            if next_letter && ends_control_word {
                out.push(' ');
            }
            i = j;
            continue;
        }
        out.push(c);
        i += 1;
    }
    katex_safe(&out)
}

/// Repairs LaTeX that KaTeX — and so most Markdown renderers — refuses, so a
/// near-miss formula still renders instead of showing as an error:
/// text-mode and layout commands the recognizer sometimes emits are dropped
/// or replaced with their math-mode equivalents, plain-TeX `\root … \of`
/// becomes `\sqrt[…]{…}`, unpaired `\left`/`\right` lose their sizing, and
/// braces are balanced.
fn katex_safe(s: &str) -> String {
    use std::sync::LazyLock;
    // Layout and text-mode commands with no math-mode meaning. Each pattern
    // starts with the character before the backslash so that the second
    // backslash of a row break (`\\d`) is never read as a command.
    static DROP: LazyLock<regex::Regex> = LazyLock::new(|| {
        regex::Regex::new(
            r"(^|[^\\])(?:\\kern-?\\nulldelimiterspace|\\(?:boldmath|unboldmath|protect|medskip|bigskip|smallskip|hfill|hfil|indent|noindent|d|b|par)(?:([^A-Za-z ])|\s+|$))",
        )
        .expect("valid regex")
    });
    static RENAME: LazyLock<regex::Regex> = LazyLock::new(|| {
        regex::Regex::new(r"(^|[^\\])\\(sb|sp|textsc|l|i|j)(?:([^A-Za-z])|$)").expect("valid regex")
    });
    static ROOT: LazyLock<regex::Regex> = LazyLock::new(|| {
        regex::Regex::new(r"\\root\s*(\{[^{}]*\}|[^\\\s{}]+)\s*\\of\s*").expect("valid regex")
    });
    // A size or font switch run into the next letters (`\largex`,
    // `\ttA`, `\tinyMMV`) is one unknown command to TeX; split the switch
    // off and drop it (math mode ignores sizes, and the letters are the
    // content).
    static GLUED_SWITCH: LazyLock<regex::Regex> = LazyLock::new(|| {
        regex::Regex::new(
            r"(^|[^\\])\\(?:large|Large|LARGE|huge|Huge|tiny|normalsize|tt|rm|it|sf|bf)([A-Za-z]+)",
        )
        .expect("valid regex")
    });
    static CIRC_LETTER: LazyLock<regex::Regex> =
        LazyLock::new(|| regex::Regex::new(r"\\circ([A-Z])").expect("valid regex"));
    let mut out = s.to_string();
    // Twice: a match consumes the character after it, which can be the
    // backslash of the next droppable command (`\boldmath\protect`).
    for _ in 0..2 {
        out = DROP
            .replace_all(&out, |c: &regex::Captures| {
                // A dropped command that ended at a space leaves the space,
                // so `\\vdots\\protect x` does not become `\\vdotsx`.
                let after = match c.get(2) {
                    Some(m) => m.as_str(),
                    None if c[0].ends_with(char::is_whitespace) => " ",
                    None => "",
                };
                format!("{}{after}", &c[1])
            })
            .into_owned();
    }
    out = RENAME
        .replace_all(&out, |c: &regex::Captures| {
            let head = match &c[2] {
                "sb" => "_",
                "sp" => "^",
                "textsc" => "\\text",
                "l" => "l",
                "i" => "i",
                _ => "j",
            };
            let sep = if c[1].ends_with(|ch: char| ch.is_ascii_alphabetic())
                && head.starts_with(|ch: char| ch.is_ascii_alphabetic())
            {
                " "
            } else {
                ""
            };
            format!(
                "{}{sep}{head}{}",
                &c[1],
                c.get(3).map_or("", |m| m.as_str())
            )
        })
        .into_owned();
    out = ROOT
        .replace_all(&out, |c: &regex::Captures| {
            let idx = c[1].trim_start_matches('{').trim_end_matches('}');
            format!("\\sqrt[{idx}]")
        })
        .into_owned();
    out = GLUED_SWITCH
        .replace_all(&out, |c: &regex::Captures| {
            let rest = &c[2];
            // Real commands that start like a switch.
            let whole = &c[0][c[1].len() + 1..];
            if matches!(
                whole,
                "itshape" | "rmfamily" | "bfseries" | "sffamily" | "ttfamily" | "rmoustache"
            ) {
                return c[0].to_string();
            }
            let sep = if c[1].ends_with(|ch: char| ch.is_ascii_alphabetic()) {
                " "
            } else {
                ""
            };
            format!("{}{sep}{rest}", &c[1])
        })
        .into_owned();
    out = CIRC_LETTER.replace_all(&out, "\\circ $1").into_owned();
    // A trailing lone backslash (a cut-off command) can only fail.
    while out.ends_with('\\') && !out.ends_with("\\\\") {
        out.pop();
    }
    // `\left` and `\right` must pair; when they don't, drop the sizing and
    // keep the delimiters (`\left.` is an invisible one and goes entirely).
    let lefts = out.matches("\\left").count()
        - out.matches("\\leftarrow").count()
        - out.matches("\\leftrightarrow").count()
        - out.matches("\\leftharpoon").count()
        - out.matches("\\leftleftarrows").count();
    let rights = out.matches("\\right").count()
        - out.matches("\\rightarrow").count()
        - out.matches("\\rightharpoon").count()
        - out.matches("\\rightleftharpoons").count()
        - out.matches("\\rightrightarrows").count();
    if lefts != rights {
        static SIZING: LazyLock<regex::Regex> = LazyLock::new(|| {
            regex::Regex::new(r"\\(?:left|right)(?:\.|([^A-Za-z]))").expect("valid regex")
        });
        out = SIZING
            .replace_all(&out, |c: &regex::Captures| {
                c.get(1).map_or(String::new(), |m| m.as_str().to_string())
            })
            .into_owned();
    }
    balance_braces(out.trim())
}

/// `Regex::replace_all` for LaTeX: when a replacement that starts with a
/// letter lands right after a control word (`\\vdots` + `v`), a space goes
/// between them so the two do not fuse into an unknown command.
fn replace_tex(re: &regex::Regex, s: &str, rep: impl Fn(&regex::Captures) -> String) -> String {
    let mut out = String::with_capacity(s.len());
    let mut last = 0;
    for c in re.captures_iter(s) {
        let m = c.get(0).expect("whole match");
        out.push_str(&s[last..m.start()]);
        let r = rep(&c);
        if r.starts_with(|ch: char| ch.is_ascii_alphabetic()) {
            let tail: String = out
                .chars()
                .rev()
                .take_while(char::is_ascii_alphabetic)
                .collect();
            if !tail.is_empty() && out[..out.len() - tail.len()].ends_with('\\') {
                out.push(' ');
            }
        }
        out.push_str(&r);
        last = m.end();
    }
    out.push_str(&s[last..]);
    out
}

/// Drops `}` that close nothing and closes groups left open, ignoring
/// escaped braces (`\{`, `\}`).
fn balance_braces(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 4);
    let mut depth = 0usize;
    let mut escaped = false;
    for c in s.chars() {
        if escaped {
            escaped = false;
            out.push(c);
            continue;
        }
        match c {
            '\\' => escaped = true,
            '{' => depth += 1,
            '}' if depth == 0 => continue,
            '}' => depth -= 1,
            _ => {}
        }
        out.push(c);
    }
    if escaped {
        out.pop();
    }
    out.extend(std::iter::repeat_n('}', depth));
    out
}

/// Canonical spellings for what the recognizer writes several ways: emphasis
/// wrappers it adds on bold or heavy print (`\boldsymbol{x}`, `\textbf{d}`)
/// are dropped, and operator names become their commands
/// (`\operatorname{log}` → `\log`). Punctuation stays where the recognizer
/// put it: a display formula carries its sentence's comma or period.
fn canonical_latex(s: &str) -> String {
    static EMPHASIS: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
        regex::Regex::new(r"\\(?:boldsymbol|textbf|textit|pmb|bm)\s*\{([^{}]*)\}")
            .expect("valid regex")
    });
    static OPERATOR: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
        regex::Regex::new(
            r"\\operatorname\s*\{\s*(log|ln|exp|sin|cos|tan|sec|csc|cot|sinh|cosh|tanh|coth|arcsin|arccos|arctan|lim|max|min|sup|inf|det|dim|ker|arg|deg|gcd|Pr|hom)\s*\}",
        )
        .expect("valid regex")
    });
    let mut out = s.to_string();
    // Emphasis can nest (`\textbf{\textit{a x}}`): unwrap until stable.
    for _ in 0..4 {
        let next = replace_tex(&EMPHASIS, &out, |c| c[1].to_string());
        if next == out {
            break;
        }
        out = next;
    }
    // Style switches are typesetting, and the slanted ⩽/⩾ glyphs are the
    // recognizer telling fonts apart, not authors choosing a symbol.
    static STYLE: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
        regex::Regex::new(r"\\(?:display|text|script|scriptscript)style\b\s*").expect("valid regex")
    });
    let out = STYLE.replace_all(&out, "");
    let out = out
        .replace("\\leqslant", "\\leq")
        .replace("\\geqslant", "\\geq");
    OPERATOR.replace_all(&out, "\\$1 ").into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn latex_spacing_is_tidied_without_merging_control_words() {
        assert_eq!(tidy_latex("( n / 2 ^ { N } : 1 )"), "(n/2^{N}:1)");
        assert_eq!(
            tidy_latex("0 \\leq k \\leq 2 ^ { N } - 1"),
            "0\\leq k\\leq2^{N}-1"
        );
        assert_eq!(tidy_latex("\\alpha \\beta"), "\\alpha\\beta");
        assert_eq!(tidy_latex("N \\ \\geq \\ 0"), "N\\geq0");
        assert_eq!(tidy_latex("( i , j ) \\, \\in \\, I"), "(i,j)\\in I");
        assert_eq!(tidy_latex("a \\\\ b"), "a\\\\b");
        assert_eq!(tidy_latex("\\mathrm { G L } ( n )"), "\\mathrm{GL}(n)");
        assert_eq!(
            tidy_latex(
                "\\int \\frac { d \\boldsymbol { x } } { \\boldsymbol { x } } = \\operatorname { l o g } \\boldsymbol { x } ."
            ),
            "\\int\\frac{dx}{x}=\\log x."
        );
        assert_eq!(tidy_latex("\\textbf { \\textit { a x } }"), "ax");
        assert_eq!(tidy_latex("F _ { 1 } - s c o r e"), "F_{1}-score");
        assert_eq!(
            tidy_latex("g ( t ) = \\textstyle \\sum _ { k } b _ { k } \\leqslant 1"),
            "g(t)=\\sum_{k}b_{k}\\leq1"
        );
    }

    #[test]
    fn katex_rejects_are_repaired() {
        assert_eq!(katex_safe(r"\d d(x^{2})=0"), "d(x^{2})=0");
        assert_eq!(katex_safe(r"x=\root3\of{-1}"), r"x=\sqrt[3]{-1}");
        assert_eq!(katex_safe(r"w\sb1"), "w_1");
        assert_eq!(katex_safe(r"\textsc{MMV}"), r"\text{MMV}");
        assert_eq!(katex_safe(r"\mathrm{\boldmath\kappa}"), r"\mathrm{\kappa}");
        assert_eq!(katex_safe(r"-40^\circC"), r"-40^\circ C");
        assert_eq!(katex_safe(r"\frac{a}{b"), r"\frac{a}{b}");
        assert_eq!(katex_safe(r"a}+b"), "a+b");
        assert_eq!(katex_safe(r"\{x\}"), r"\{x\}");
        assert_eq!(katex_safe(r"\left(x+y"), "(x+y");
        assert_eq!(katex_safe(r"\left(x\right)"), r"\left(x\right)");
        assert_eq!(katex_safe(r"x\to y\leftarrow z"), r"x\to y\leftarrow z");
        assert_eq!(katex_safe(r"\delta\beta\iota\ell"), r"\delta\beta\iota\ell");
        assert_eq!(katex_safe(r"H_{N}=\"), "H_{N}=");
        assert_eq!(katex_safe(r"a\\d"), r"a\\d");
        assert_eq!(katex_safe(r"\mathrm{\largex}"), r"\mathrm{x}");
        assert_eq!(katex_safe(r"u_{\tinyMMV}"), r"u_{MMV}");
        assert_eq!(katex_safe(r"\ttA+\itl"), r"A+l");
        assert_eq!(katex_safe(r"\iota+\rho"), r"\iota+\rho");
        assert_eq!(katex_safe(r"x\\i"), r"x\\i");
    }

    #[test]
    fn removed_commands_never_fuse_neighbours() {
        assert_eq!(katex_safe(r"\vdots\protect x"), r"\vdots x");
        assert_eq!(katex_safe(r"\cdot\ttA"), r"\cdot A");
        assert_eq!(tidy_latex(r"\cdot \boldsymbol { x }"), r"\cdot x");
    }

    #[test]
    fn byte_decoder_covers_all_bytes() {
        let d = byte_decoder();
        assert_eq!(d.len(), 256);
        assert_eq!(d.get(&'Ġ'), Some(&b' '));
    }
}
