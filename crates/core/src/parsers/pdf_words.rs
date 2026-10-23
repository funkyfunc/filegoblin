//! Words from glyph positions.
//!
//! A PDF does not have to spell a word space as a space character. Many
//! generators move the pen instead: a kern in a `TJ` array, character
//! spacing (`Tc`) on a two-letter run, or one `Tm` per word. The converter's
//! text spans keep the characters but lose those moves, so the words they
//! separate come out glued together (`ofthe`, `andIlearnt`), and a span can
//! even be cut in the middle of a word (`thirdr` | `ound`).
//!
//! Each glyph's origin and advance say exactly how far the pen moved beyond
//! the glyph itself. On a line, letters of one word sit at about the same
//! extra spacing (the font's kerning, or the line's tracking); a word gap
//! sits well above it. A word ends at a space character or where the extra
//! spacing exceeds the line's typical letter spacing by a clear margin, so
//! tracked headings and tightly set lines adapt to themselves.
//!
//! The glyphs also say what the text is when the text layer garbles it:
//! a ligature whose text carries a space ("ﬂ " in "inﬂ uenced") has that
//! space inside its own advance; an accent drawn as a glyph of its own sits
//! on its letter ("Ecole ´"); a glyph whose text is UTF-8 read as
//! Windows-1252 ("â€‰") is one glyph at one pen position.
//!
//! The vision path takes its words from here. The fast path keeps the
//! converter's markdown and puts its word spaces and spellings where the
//! glyphs have them ([`respace_lines`]).

use pdf_oxide::layout::TextChar;
use std::sync::LazyLock;

/// One word of the text layer, in user space.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct GlyphWord {
    pub text: String,
    pub x0: f32,
    pub y0: f32,
    pub x1: f32,
    pub y1: f32,
    pub size: f32,
    /// Starts right against the previous word without a gap, on a shifted
    /// baseline (a footnote mark, a raised or lowered letter): readers see
    /// one word.
    pub glued: bool,
    /// Every glyph is mathematics: set in a math font (TeX's math italic and
    /// symbol fonts, AMS, STIX, Cambria Math, …) or a math symbol.
    pub math: bool,
}

/// Fonts that only typeset mathematics. (Not "Symbol": word processors set
/// bullets and arrows in it. Not STIX's text faces, STIXGeneral and
/// STIXTwoText, which set whole papers' prose: only its math faces count,
/// and "math" covers STIXMath and STIXTwoMath.)
const MATH_FONTS: [&str; 18] = [
    "cmmi",
    "cmsy",
    "cmex",
    "cmbsy",
    "msam",
    "msbm",
    "eufm",
    "rsfs",
    "stixsize",
    "stixintegrals",
    "stixnonuni",
    "stixvariants",
    "math",
    "mtmi",
    "mtsy",
    "txsy",
    "pxsy",
    "lmmath",
];

fn mathy(c: &TextChar) -> bool {
    let font = c.font_name.to_lowercase();
    MATH_FONTS.iter().any(|f| font.contains(f))
        || matches!(c.char as u32,
            0x2200..=0x22FF          // mathematical operators
            | 0x27C0..=0x27EF        // misc math symbols A
            | 0x2980..=0x2AFF        // misc math symbols B, supplemental operators
            | 0x1D400..=0x1D7FF) // mathematical alphanumerics
}

/// The glyphs drawn on the page: text set entirely outside the page box (the
/// other half of a two-page spread, a slug beyond the trim) is not part of
/// it, as the converter's own spans already leave it out.
pub(crate) fn on_page(mut chars: Vec<TextChar>, page_box: (f32, f32, f32, f32)) -> Vec<TextChar> {
    let (x0, y0, x1, y1) = page_box;
    chars.retain(|c| {
        c.bbox.x + c.bbox.width > x0
            && c.bbox.x < x1
            && c.bbox.y + c.bbox.height > y0
            && c.bbox.y < y1
    });
    chars
}

/// The size to measure spacing against: the font size, unless it is far off
/// the glyph's drawn height (Type 3 fonts often report a nominal size of 1).
fn em(c: &TextChar) -> f32 {
    let h = c.bbox.height.abs();
    let s = c.font_size.abs();
    if h > 0.5 && (s < 0.3 * h || s > 3.0 * h) {
        h
    } else {
        s.max(0.5)
    }
}

fn is_horizontal(c: &TextChar) -> bool {
    let r = c.rotation_degrees.rem_euclid(360.0);
    !(1.0..=359.0).contains(&r)
}

/// Pen movement after `a` before `b` was set, beyond `a`'s own advance.
fn extra_space(a: &TextChar, b: &TextChar) -> f32 {
    let advance = if a.advance_width > 0.0 {
        a.advance_width
    } else {
        a.bbox.width
    };
    b.origin_x - (a.origin_x + advance)
}

/// The byte a character stands for when UTF-8 was read as Windows-1252 (or
/// Latin-1).
fn cp1252_byte(c: char) -> Option<u8> {
    let b = match c {
        '€' => 0x80,
        '‚' => 0x82,
        'ƒ' => 0x83,
        '„' => 0x84,
        '…' => 0x85,
        '†' => 0x86,
        '‡' => 0x87,
        'ˆ' => 0x88,
        '‰' => 0x89,
        'Š' => 0x8A,
        '‹' => 0x8B,
        'Œ' => 0x8C,
        'Ž' => 0x8E,
        '‘' => 0x91,
        '’' => 0x92,
        '“' => 0x93,
        '”' => 0x94,
        '•' => 0x95,
        '–' => 0x96,
        '—' => 0x97,
        '˜' => 0x98,
        '™' => 0x99,
        'š' => 0x9A,
        '›' => 0x9B,
        'œ' => 0x9C,
        'ž' => 0x9E,
        'Ÿ' => 0x9F,
        c if (c as u32) < 0x100 => c as u8,
        _ => return None,
    };
    Some(b)
}

/// One glyph whose text came out as several characters that are the UTF-8
/// bytes of its real character read as Windows-1252 (a thin space as
/// `â€‰`): the character those bytes spell. `None` for anything else, a
/// ligature's letters included.
fn undo_mojibake(chars: &[char]) -> Option<String> {
    if chars.len() < 2 || chars.iter().all(char::is_ascii) {
        return None;
    }
    let bytes: Vec<u8> = chars
        .iter()
        .map(|&c| cp1252_byte(c))
        .collect::<Option<_>>()?;
    let s = String::from_utf8(bytes).ok()?;
    (s.chars().count() < chars.len()).then_some(s)
}

/// The page's glyphs with any glyph that expanded into mojibake (several
/// characters at one pen position) spelled as its real character.
fn repair_glyph_text(glyphs: Vec<&TextChar>) -> Vec<TextChar> {
    let mut out: Vec<TextChar> = Vec::with_capacity(glyphs.len());
    let mut i = 0;
    while i < glyphs.len() {
        let g = glyphs[i];
        let mut j = i + 1;
        while j < glyphs.len()
            && glyphs[j].origin_x == g.origin_x
            && glyphs[j].origin_y == g.origin_y
            && glyphs[j].font_name == g.font_name
        {
            j += 1;
        }
        let text: Vec<char> = glyphs[i..j].iter().map(|c| c.char).collect();
        match undo_mojibake(&text) {
            Some(fixed) => {
                let advance: f32 = glyphs[i..j].iter().map(|c| c.advance_width).sum();
                for ch in fixed.chars() {
                    let mut c = g.clone();
                    c.char = ch;
                    c.advance_width = advance;
                    out.push(c);
                }
            }
            None => out.extend(glyphs[i..j].iter().map(|&c| c.clone())),
        }
        i = j;
    }
    out
}

/// The combining mark a spacing accent glyph stands for.
fn combining(c: char) -> Option<char> {
    Some(match c {
        '´' | 'ˊ' => '\u{301}',
        'ˋ' => '\u{300}',
        'ˆ' => '\u{302}',
        '˜' => '\u{303}',
        '¯' | 'ˉ' => '\u{304}',
        '˘' => '\u{306}',
        '˙' => '\u{307}',
        '¨' => '\u{308}',
        '˚' => '\u{30A}',
        '˝' => '\u{30B}',
        'ˇ' => '\u{30C}',
        '¸' => '\u{327}',
        '˛' => '\u{328}',
        _ => return None,
    })
}

/// Accents some producers (TeX among them) draw as glyphs of their own,
/// placed over or under their letter but drawn apart from it, often in a
/// pass of their own ("Ecole ´" for "École", "Paraıba ´" for "Paraíba").
/// Each such accent is
/// put on the letter it sits on — the nearest letter on its line whose
/// centre lies within a third of an em of the accent's — and the pair is
/// written as one character.
fn compose_accents(mut glyphs: Vec<TextChar>) -> Vec<TextChar> {
    use unicode_normalization::UnicodeNormalization;
    let centre = |c: &TextChar| c.bbox.x + c.bbox.width / 2.0;
    let mut gone = vec![false; glyphs.len()];
    for i in 0..glyphs.len() {
        let Some(mark) = combining(glyphs[i].char) else {
            continue;
        };
        let a = &glyphs[i];
        let size = em(a);
        let base = (0..glyphs.len())
            .filter(|&j| j != i && !gone[j] && glyphs[j].char.is_alphabetic())
            .filter(|&j| {
                let b = &glyphs[j];
                (b.origin_y - a.origin_y).abs() <= 0.6 * size.max(em(b))
                    && (centre(b) - centre(a)).abs() <= 0.35 * size.max(em(b))
            })
            .min_by(|&x, &y| {
                (centre(&glyphs[x]) - centre(a))
                    .abs()
                    .total_cmp(&(centre(&glyphs[y]) - centre(a)).abs())
            });
        let Some(j) = base else { continue };
        // A dotless i under an accent is the accented i.
        let letter = match glyphs[j].char {
            'ı' => 'i',
            c => c,
        };
        let composed: Vec<char> = [letter, mark].iter().copied().nfc().collect();
        if let [single] = composed[..] {
            glyphs[j].char = single;
            gone[i] = true;
        }
    }
    glyphs
        .into_iter()
        .zip(gone)
        .filter(|(_, g)| !g)
        .map(|(c, _)| c)
        .collect()
}

/// Characters some fonts give single glyphs although their text runs decode
/// right: the replacement character (a no-break space with no mapping of its
/// own) and the lead characters of UTF-8 read as cp1252 ("Â", "Ã", "â€")
/// when the halves land at different pen positions, where they cannot be put
/// back together.
fn suspect(c: char) -> bool {
    matches!(c, '\u{FFFD}' | 'Â' | 'Ã' | '€')
}

/// Whether the glyph words can stand in for `reference`, the converter's own
/// words for the page. Some text layers spell single glyphs worse than their
/// text runs: a no-break space or a ligature with no mapping of its own,
/// mojibake split across pen positions, an OCR layer that draws each word
/// on its own skewed baseline so the glyphs of neighbouring words touch.
/// Then the glyphs hold more suspect characters, letters the words lack or
/// lack letters they have, or run many words together; the page keeps the
/// converter's words.
pub(crate) fn agrees(words: &[GlyphWord], reference: &[&str]) -> bool {
    use std::collections::HashMap;
    let letters = |texts: &mut dyn Iterator<Item = &str>| {
        let (mut counts, mut words, mut suspects) = (HashMap::new(), 0usize, 0usize);
        for t in texts {
            let t = expand_ligatures(t);
            let mut any = false;
            for c in t.chars() {
                suspects += usize::from(suspect(c));
                // Letters and digits as their plain Latin base: where lost
                // ligatures and doubled words show. (Other scripts are left
                // out; a font's runs can decode as stray CJK the glyphs
                // spell right.)
                if bare(c).is_ascii_alphanumeric() {
                    *counts.entry(bare(c)).or_insert(0isize) += 1;
                }
                any |= c.is_alphanumeric();
            }
            words += usize::from(any);
        }
        (counts, words, suspects)
    };
    let (glyphs, glyph_words, glyph_suspects) = letters(&mut words.iter().map(|w| w.text.as_str()));
    let (theirs, their_words, their_suspects) = letters(&mut reference.iter().copied());
    let total: isize = theirs.values().sum();
    let keys: std::collections::HashSet<&char> = glyphs.keys().chain(theirs.keys()).collect();
    let differ: isize = keys
        .into_iter()
        .map(|c| (glyphs.get(c).copied().unwrap_or(0) - theirs.get(c).copied().unwrap_or(0)).abs())
        .sum();
    glyph_suspects <= their_suspects
        && differ <= (total / 100).max(4)
        && glyph_words as f32 >= 0.97 * their_words as f32 - 3.0
}

/// Groups the page's horizontal glyphs into words. `None` when the page has
/// a fair share of rotated text, where the converter's own word order is
/// the safer reading.
pub(crate) fn glyph_words(chars: &[TextChar]) -> Option<Vec<GlyphWord>> {
    let repaired = compose_accents(repair_glyph_text(chars.iter().collect()));
    let visible: Vec<&TextChar> = repaired.iter().filter(|c| !c.char.is_control()).collect();
    if visible.is_empty() {
        return Some(Vec::new());
    }
    let rotated = visible.iter().filter(|c| !is_horizontal(c)).count();
    if rotated * 5 > visible.len() {
        return None;
    }
    let glyphs: Vec<&TextChar> = visible.into_iter().filter(|c| is_horizontal(c)).collect();

    // Segments: consecutive glyphs (in content order) on one baseline,
    // moving rightwards without a jump. A new segment starts a new word.
    let mut segments: Vec<Vec<&TextChar>> = Vec::new();
    for c in glyphs {
        let joins = segments.last().and_then(|s| s.last()).is_some_and(|p| {
            let size = em(p).max(em(c));
            (c.origin_y - p.origin_y).abs() <= 0.2 * size
                && c.origin_x >= p.origin_x - 0.3 * size
                && extra_space(p, c) <= 1.5 * size
        });
        if joins {
            segments.last_mut().expect("non-empty").push(c);
        } else {
            segments.push(vec![c]);
        }
    }

    let mut words: Vec<GlyphWord> = Vec::new();
    // Whether each word starts a segment (rather than following a space or
    // a gap within one).
    let mut seg_first: Vec<bool> = Vec::new();
    let mut prev_seg_end: Option<&TextChar> = None;
    for seg in &segments {
        // Typical letter spacing on this segment: the median extra space
        // between two glyphs that are not spaces.
        let mut letter: Vec<f32> = seg
            .windows(2)
            .filter(|w| !w[0].char.is_whitespace() && !w[1].char.is_whitespace())
            .map(|w| extra_space(w[0], w[1]) / em(w[0]).max(em(w[1])))
            .collect();
        letter.sort_by(f32::total_cmp);
        let typical = if letter.len() >= 3 {
            letter[letter.len() / 2].clamp(-0.1, 0.3)
        } else {
            0.0
        };
        let threshold = (typical + 0.12).max(0.12);

        let mut cur: Option<GlyphWord> = None;
        let mut prev: Option<&TextChar> = None;
        for (k, &c) in seg.iter().enumerate() {
            if c.char.is_whitespace() {
                // A space that is part of the previous glyph's own text (a
                // ligature mapped to "ﬂ ", drawn as one glyph) sits inside
                // that glyph's advance, and the next letter follows it
                // without a gap: no reader sees a space there.
                let phantom = prev.is_some_and(|p| {
                    let advance = if p.advance_width > 0.0 {
                        p.advance_width
                    } else {
                        p.bbox.width
                    };
                    !p.char.is_whitespace()
                        && c.origin_x < p.origin_x + advance - 0.05 * em(p)
                        && seg.get(k + 1).is_some_and(|n| {
                            !n.char.is_whitespace()
                                && extra_space(p, n) / em(p).max(em(n)) <= threshold
                        })
                });
                if phantom {
                    continue;
                }
                words.extend(cur.take());
                prev = Some(c);
                continue;
            }
            let gap_break = prev.is_some_and(|p| {
                !p.char.is_whitespace() && extra_space(p, c) / em(p).max(em(c)) > threshold
            });
            if gap_break {
                words.extend(cur.take());
            }
            match cur.as_mut() {
                Some(w) => {
                    w.text.push(c.char);
                    w.math &= mathy(c);
                    w.x0 = w.x0.min(c.bbox.x);
                    w.y0 = w.y0.min(c.bbox.y);
                    w.x1 = w.x1.max(c.bbox.x + c.bbox.width);
                    w.y1 = w.y1.max(c.bbox.y + c.bbox.height);
                    w.size = w.size.max(em(c));
                }
                None => {
                    // The first word of a segment runs on from the previous
                    // segment when that ended in a letter right beside it:
                    // a raised mark, a subscript, a word cut in two spans.
                    let glued = prev.is_none()
                        && prev_seg_end.is_some_and(|p| {
                            !p.char.is_whitespace()
                                && (c.origin_y - p.origin_y).abs() <= em(p).max(em(c))
                                && extra_space(p, c).abs() < 0.1 * em(p).max(em(c))
                        });
                    seg_first.push(prev.is_none());
                    cur = Some(GlyphWord {
                        text: c.char.to_string(),
                        x0: c.bbox.x,
                        y0: c.bbox.y,
                        x1: c.bbox.x + c.bbox.width,
                        y1: c.bbox.y + c.bbox.height,
                        size: em(c),
                        glued,
                        math: mathy(c),
                    });
                }
            }
            prev = Some(c);
        }
        words.extend(cur.take());
        prev_seg_end = seg.last().copied();
    }

    // A segment that starts right where another word ends, on the same line
    // band, continues that word wherever the content stream drew it: a
    // raised ordinal or footnote mark set after the rest of the line ("9" …
    // "th"), the halves of a word drawn apart.
    let mut by_end: Vec<usize> = (0..words.len()).collect();
    by_end.sort_by(|&a, &b| words[a].x1.total_cmp(&words[b].x1));
    let ends: Vec<f32> = by_end.iter().map(|&i| words[i].x1).collect();
    let joins: Vec<bool> = (0..words.len())
        .map(|i| {
            let w = &words[i];
            if !seg_first[i] || w.glued {
                return false;
            }
            let tol = 0.1 * w.size;
            let lo = ends.partition_point(|&x| x < w.x0 - tol);
            by_end[lo..]
                .iter()
                .take_while(|&&k| words[k].x1 <= w.x0 + tol)
                .any(|&k| {
                    let o = &words[k];
                    let overlap = w.y1.min(o.y1) - w.y0.max(o.y0);
                    k != i
                        && o.x0 < w.x0
                        && overlap > 0.3 * (w.y1 - w.y0).min(o.y1 - o.y0)
                        && !footnote_mark(o, w)
                })
        })
        .collect();
    for (w, join) in words.iter_mut().zip(joins) {
        w.glued |= join;
    }
    for i in 1..words.len() {
        if words[i].glued && footnote_mark(&words[i - 1], &words[i]) {
            words[i].glued = false;
        }
    }
    Some(words)
}

/// A footnote mark: raised digits or symbols, set smaller, after a word of
/// three or more letters ("income¹"). The converter writes it apart
/// ("income 1"), and so does this. Raised letters after digits (an ordinal,
/// "10th") and marks after a short word or in math (an exponent, "x²",
/// "mc²") still join.
fn footnote_mark(word: &GlyphWord, mark: &GlyphWord) -> bool {
    let letters = word
        .text
        .chars()
        .rev()
        .take_while(|c| c.is_alphabetic())
        .count();
    letters >= 3
        && !word.math
        && !mark.math
        && mark
            .text
            .chars()
            .all(|c| c.is_ascii_digit() || "*†‡§¶,".contains(c))
        && mark.size < 0.85 * word.size
        && mark.y0 > word.y0 + 0.15 * word.size
}

/// Puts the converter's word spaces where the page's glyph words are. Each
/// line of the converter's markdown is found, spaces ignored, in the page's
/// glyph lines; where exactly one place matches, a space goes in wherever a
/// glyph word starts inside one of the line's tokens, and a space the
/// converter put inside a glyph word (where it cut a span: `thirdr ound`)
/// comes out. Lines with markup the glyphs cannot match (tables, emphasis,
/// code, links) are left as they are.
pub(crate) fn respace_lines(md: &str, words: &[GlyphWord]) -> String {
    // The page's glyph words in content order as characters without spaces
    // — the converter writes a paragraph's lines as one line, so a match
    // may run on from one glyph line into the next — with, for each
    // character, its letter without accents (the key lines are matched on)
    // and whether a word starts there.
    // A hyphen that ends a line before a lowercase continuation breaks one
    // word across the lines ("hospi-" ⏎ "tals"): it is left out of the key,
    // and the continuation does not start a word. The converter drops such
    // a hyphen but keeps a space where it stood ("hospi tals").
    let mut glyph: Vec<char> = Vec::new();
    let mut key = String::new();
    let mut starts: Vec<bool> = Vec::new();
    let mut prev: Option<&GlyphWord> = None;
    for w in words {
        let new_line =
            prev.is_some_and(|p| (w.y0 - p.y0).abs() > 0.3 * p.size.max(w.size) || w.x0 < p.x0);
        let continues = new_line
            && prev.is_some_and(|p| {
                let mut tail = p.text.chars().rev();
                matches!(tail.next(), Some('-' | '\u{ad}'))
                    && tail.next().is_some_and(char::is_alphabetic)
            })
            && w.text.chars().next().is_some_and(char::is_lowercase);
        if continues {
            glyph.pop();
            key.pop();
            starts.pop();
        }
        for (k, c) in expand_ligatures(&w.text).chars().enumerate() {
            glyph.push(c);
            key.push(bare(c));
            starts.push(k == 0 && !w.glued && !continues);
        }
        prev = Some(w);
    }
    if glyph.is_empty() {
        return md.to_string();
    }
    // Byte offset in `key` → character index.
    let mut char_at: std::collections::HashMap<usize, usize> = std::collections::HashMap::new();
    for (i, (b, _)) in key.char_indices().enumerate() {
        char_at.insert(b, i);
    }

    // Searching the page's key costs its length each time; a page whose
    // text is drawn twice (fake bold, overprinted layers) matches nowhere
    // uniquely and would search on for every word. Past this budget the
    // remaining lines are left as the converter wrote them.
    let budget = key.len().saturating_mul(4096).max(64 << 20);
    let scanned = std::cell::Cell::new(0usize);
    let mut out = String::with_capacity(md.len() + 64);
    let mut in_code = false;
    for (n, line) in md.split('\n').enumerate() {
        if n > 0 {
            out.push('\n');
        }
        let t = line.trim_start();
        if t.starts_with("```") {
            in_code = !in_code;
        }
        let markup =
            t.starts_with(['|', '>', '<', '`']) || line.contains(['\\', '`', '[', ']', '$']);
        // Emphasis markers are markdown, not text: matched without them and
        // written back where they stand.
        let emphasis = |c: char| matches!(c, '*' | '_');
        // Heading hashes and list bullets are the converter's, not glyphs.
        let mut heading = t.trim_start_matches('#').trim_start();
        for bullet in ["- ", "* ", "+ "] {
            if let Some(rest) = heading.strip_prefix(bullet) {
                heading = rest.trim_start();
            }
        }
        // Ligature characters the converter left in ("inﬂ uenced") are
        // spelled out, as in the glyph words.
        let body = expand_ligatures(heading);
        let chars: Vec<char> = body.chars().collect();
        // The line's key: its letters without spaces, accents, or the
        // accents the converter wrote as characters of their own; with, for
        // each key character, its place in the line and whether the line
        // starts a word there.
        let mut sk: Vec<char> = Vec::new();
        let mut sk_at: Vec<usize> = Vec::new();
        let mut sk_word: Vec<bool> = Vec::new();
        let mut after_space = true;
        for (i, &c) in chars.iter().enumerate() {
            if c.is_whitespace() {
                after_space = true;
                continue;
            }
            if combining(c).is_some() || emphasis(c) {
                continue;
            }
            sk.push(bare(c));
            sk_at.push(i);
            sk_word.push(after_space);
            after_space = false;
        }
        if in_code || markup || sk.len() < 12 || scanned.get() > budget {
            out.push_str(line);
            continue;
        }
        // Align the line to the glyphs piece by piece: at each point the
        // longest stretch found in exactly one place on the page (at least
        // twelve letters); where nothing is, skip a word.
        let mut glyph_of: Vec<Option<usize>> = vec![None; sk.len()];
        let mut cur = 0usize;
        while cur < sk.len() {
            let found = |len: usize| -> Option<usize> {
                let piece: String = sk[cur..cur + len].iter().collect();
                scanned.set(scanned.get() + key.len());
                let mut hits = key.match_indices(&piece);
                match (hits.next(), hits.next()) {
                    (Some((at, _)), None) => Some(char_at[&at]),
                    _ => None,
                }
            };
            let occurs = |len: usize| -> bool {
                let piece: String = sk[cur..cur + len].iter().collect();
                scanned.set(scanned.get() + key.len());
                key.contains(&piece)
            };
            let (mut lo, mut hi) = (0usize, sk.len() - cur);
            while lo < hi {
                let mid = (lo + hi).div_ceil(2);
                if occurs(mid) {
                    lo = mid;
                } else {
                    hi = mid - 1;
                }
            }
            if lo >= 12
                && let Some(base) = found(lo)
            {
                for k in 0..lo {
                    glyph_of[cur + k] = Some(base + k);
                }
                cur += lo;
            } else {
                cur += 1;
                while cur < sk.len() && !sk_word[cur] {
                    cur += 1;
                }
            }
        }
        // An accent character next to a word whose glyphs carry an accent
        // is that same mark, written apart by the converter ("Ecole ´"): it
        // is dropped. Elsewhere ("wouldn´t") it is the text's own.
        let accented: Vec<bool> = glyph_of
            .iter()
            .map(|g| g.is_some_and(|g| bare(glyph[g]) != glyph[g]))
            .collect();
        let word_accented = |k: usize| {
            let start = (0..=k).rev().find(|&j| sk_word[j]).unwrap_or(0);
            let end = (k + 1..sk.len()).find(|&j| sk_word[j]).unwrap_or(sk.len());
            accented[start..end].iter().any(|&a| a)
        };
        let prefix_len = line.len() - heading.len();
        let line_start = out.len();
        out.push_str(&line[..prefix_len]);
        let mut k = 0usize;
        let mut pending = String::new();
        for (i, &c) in chars.iter().enumerate() {
            if c.is_whitespace() {
                pending.push(c);
                continue;
            }
            if combining(c).is_some() {
                let composed =
                    (k > 0 && word_accented(k - 1)) || (k < sk.len() && word_accented(k));
                if composed {
                    // The spaces around a dropped accent are one gap.
                    if pending.chars().all(|p| p == ' ') && !pending.is_empty() {
                        pending = " ".to_string();
                    }
                } else {
                    out.push_str(&pending);
                    pending.clear();
                    out.push(c);
                }
                continue;
            }
            if emphasis(c) {
                out.push_str(&pending);
                pending.clear();
                out.push(c);
                continue;
            }
            debug_assert_eq!(sk_at[k], i);
            let g = glyph_of[k];
            // A space only counts as decided by the glyphs when both of its
            // letters were matched in one run.
            let decided = k > 0 && g.is_some() && glyph_of[k - 1].is_some_and(|p| Some(p + 1) == g);
            if pending.chars().all(|p| p == ' ') && pending.len() > 1 {
                pending = " ".to_string();
            }
            if decided {
                let g = g.expect("decided");
                if starts[g] {
                    // An opening marker stays against its word.
                    let opened = out.ends_with(emphasis);
                    out.push_str(if !pending.is_empty() {
                        &pending
                    } else if opened {
                        ""
                    } else {
                        " "
                    });
                } else if !pending.chars().all(|p| p == ' ') {
                    out.push_str(&pending);
                }
            } else {
                out.push_str(&pending);
            }
            pending.clear();
            out.push(g.map_or(c, |g| glyph[g]));
            k += 1;
        }
        out.push_str(&pending);
        // A space on one side of a hyphen between letters ("ATTENTION
        // -BASED") is half a decision: the glyphs set the hyphen apart from
        // one word only. The compound is the reading.
        static ONE_SIDED: LazyLock<regex::Regex> = LazyLock::new(|| {
            regex::Regex::new(r"(\p{L}) -(\p{L})|(\p{L})- (\p{L})").expect("valid regex")
        });
        if ONE_SIDED.is_match(&out[line_start..]) && !ONE_SIDED.is_match(line) {
            let fixed = ONE_SIDED
                .replace_all(&out[line_start..], |c: &regex::Captures| {
                    let (a, b) = match (c.get(1), c.get(3)) {
                        (Some(a), _) => (a.as_str(), &c[2]),
                        (None, Some(a)) => (a.as_str(), &c[4]),
                        _ => unreachable!("one branch matched"),
                    };
                    format!("{a}-{b}")
                })
                .into_owned();
            out.truncate(line_start);
            out.push_str(&fixed);
        }
    }
    out
}

/// A character's letter without its accents (`é` → `e`, a dotless `ı` →
/// `i`).
fn bare(c: char) -> char {
    use unicode_normalization::UnicodeNormalization;
    match c {
        'ı' => 'i',
        _ => std::iter::once(c).nfd().next().unwrap_or(c),
    }
}

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

#[cfg(test)]
mod tests {
    use super::*;
    use pdf_oxide::geometry::Rect;

    /// Glyphs of `text` on one baseline: every letter 5 pt wide at size 10,
    /// with `extra[i]` pt of pen movement after glyph `i`.
    fn line(text: &str, y: f32, extra: &[f32]) -> Vec<TextChar> {
        let mut x = 72.0;
        let mut out = Vec::new();
        for (i, ch) in text.chars().enumerate() {
            let mut c = TextChar::simple(ch, Rect::new(x, y, 5.0, 10.0), "F".into(), 10.0);
            c.origin_x = x;
            c.origin_y = y;
            c.advance_width = 5.0;
            out.push(c);
            x += 5.0 + extra.get(i).copied().unwrap_or(0.0);
        }
        out
    }

    fn texts(words: &[GlyphWord]) -> Vec<&str> {
        words.iter().map(|w| w.text.as_str()).collect()
    }

    #[test]
    fn pen_moves_between_words_are_word_breaks() {
        // "andIlearnt": letters at -0.1 pt, word gaps of 1.7 pt (0.17 em).
        let mut extra = vec![-0.1; 10];
        extra[2] = 1.7;
        extra[3] = 1.7;
        let w = glyph_words(&line("andIlearnt", 700.0, &extra)).unwrap();
        assert_eq!(texts(&w), ["and", "I", "learnt"]);
    }

    #[test]
    fn space_characters_still_break_words() {
        let w = glyph_words(&line("of the", 700.0, &[])).unwrap();
        assert_eq!(texts(&w), ["of", "the"]);
    }

    #[test]
    fn tracked_text_keeps_its_words() {
        // Letter-spaced heading: every gap 3 pt, the word gap 9 pt.
        let mut extra = vec![3.0; 15];
        extra[8] = 9.0;
        let w = glyph_words(&line("FINANCIALSUMMARY", 700.0, &extra)).unwrap();
        assert_eq!(texts(&w), ["FINANCIAL", "SUMMARY"]);
    }

    #[test]
    fn kerning_does_not_split_a_word() {
        let extra = [-0.5, 0.3, -0.8, 0.4, -0.2, 0.6, 0.0];
        let w = glyph_words(&line("AVATARS", 700.0, &extra)).unwrap();
        assert_eq!(texts(&w), ["AVATARS"]);
    }

    #[test]
    fn a_raised_mark_after_a_short_word_runs_on_from_it() {
        let mut glyphs = line("mc", 700.0, &[]);
        let mut mark = line("2", 704.0, &[]);
        mark[0].origin_x = 82.0;
        mark[0].bbox = Rect::new(82.0, 704.0, 3.0, 6.0);
        mark[0].font_size = 6.0;
        glyphs.extend(mark);
        let w = glyph_words(&glyphs).unwrap();
        assert_eq!(texts(&w), ["mc", "2"]);
        assert!(w[1].glued);
    }

    fn word(text: &str, x0: f32, y: f32) -> GlyphWord {
        GlyphWord {
            text: text.into(),
            x0,
            y0: y,
            x1: x0 + 5.0 * text.chars().count() as f32,
            y1: y + 10.0,
            size: 10.0,
            glued: false,
            math: false,
        }
    }

    #[test]
    fn converter_lines_take_the_glyph_word_breaks() {
        let words = [
            word("and", 72.0, 700.0),
            word("I", 90.0, 700.0),
            word("learnt", 97.0, 700.0),
            word("a", 130.0, 700.0),
            word("great", 137.0, 700.0),
            word("deal", 165.0, 700.0),
            word("in", 72.0, 686.0),
            word("the", 84.0, 686.0),
            word("third", 102.0, 686.0),
            word("round", 130.0, 686.0),
        ];
        let md = "## andIl earntagreatdeal\n\nin the thirdr ound\n\n| andIlearnt | x |";
        assert_eq!(
            respace_lines(md, &words),
            "## and I learnt a great deal\n\nin the third round\n\n| andIlearnt | x |"
        );
    }

    #[test]
    fn accents_the_converter_wrote_apart_are_put_back() {
        let words = [
            word("UMR", 72.0, 700.0),
            word("8146,", 100.0, 700.0),
            word("École", 130.0, 700.0),
            word("Centrale", 160.0, 700.0),
        ];
        assert_eq!(
            respace_lines("UMR 8146, Ecole ´ Centrale", &words),
            "UMR 8146, École Centrale"
        );
    }

    #[test]
    fn glyph_text_worse_than_the_converters_is_not_used() {
        let words = [word("G.\u{FFFD}(2002)", 72.0, 700.0)];
        assert!(!agrees(&words, &["G.\u{a0}(2002)"]));
        assert!(agrees(&words, &["G.\u{FFFD}(2002)"]));
        let words = [word("IIeÂmillénaire", 72.0, 700.0)];
        assert!(!agrees(&words, &["IIe", "millénaire"]));
        // The converter's words holding stray CJK do not outvote the glyphs.
        let words = [word("Gain", 72.0, 700.0), word("—Worksheet", 100.0, 700.0)];
        assert!(agrees(&words, &["Gain", "で@Worksheet"]));
        assert!(agrees(&[word("São", 72.0, 700.0)], &["São"]));
        // Lost ligatures, and words run together.
        let page: Vec<String> = (0..40).map(|i| format!("word{i}")).collect();
        let theirs: Vec<&str> = page.iter().map(String::as_str).collect();
        let mut ours: Vec<GlyphWord> = page.iter().map(|t| word(t, 72.0, 700.0)).collect();
        assert!(agrees(&ours, &theirs));
        ours.push(word("identied identied identied", 72.0, 700.0));
        let more = [theirs.clone(), vec!["identified"; 3]].concat();
        assert!(!agrees(&ours, &more));
        let glued = vec![word(&page.concat(), 72.0, 700.0)];
        assert!(!agrees(&glued, &theirs));
        // A split ligature put back together is a fix, not a disagreement.
        let ours = [word("influenced", 72.0, 700.0)];
        assert!(agrees(&ours, &["inﬂ", "uenced"]));
    }

    #[test]
    fn an_accent_away_from_accented_glyphs_is_the_texts_own() {
        let words = [
            word("José", 72.0, 700.0),
            word("said", 100.0, 700.0),
            word("he", 130.0, 700.0),
            word("wouldn´t", 150.0, 700.0),
            word("come", 200.0, 700.0),
        ];
        assert_eq!(
            respace_lines("José said he wouldn´t come", &words),
            "José said he wouldn´t come"
        );
    }

    #[test]
    fn ligatures_the_converter_left_and_split_are_spelled_out() {
        let words = [
            word("most", 72.0, 700.0),
            word("of", 100.0, 700.0),
            word("them", 115.0, 700.0),
            word("inﬂuenced", 140.0, 700.0),
            word("by", 190.0, 700.0),
        ];
        assert_eq!(
            respace_lines("most of them inﬂ uenced by", &words),
            "most of them influenced by"
        );
    }

    #[test]
    fn emphasis_markers_stay_where_they_are() {
        let words = [
            word("the", 72.0, 700.0),
            word("book", 90.0, 700.0),
            word("Thinking", 115.0, 700.0),
            word("in", 160.0, 700.0),
            word("Systems", 175.0, 700.0),
            word("inﬂuenced", 220.0, 700.0),
            word("many", 280.0, 700.0),
        ];
        assert_eq!(
            respace_lines("the book *Thinking in Systems* inﬂ uenced many", &words),
            "the book *Thinking in Systems* influenced many"
        );
    }

    #[test]
    fn a_word_broken_at_a_line_end_hyphen_is_whole_again() {
        let words = [
            word("doctors", 72.0, 700.0),
            word("and", 120.0, 700.0),
            word("hospi-", 145.0, 700.0),
            word("tals", 72.0, 686.0),
            word("has", 100.0, 686.0),
        ];
        assert_eq!(
            respace_lines("doctors and hospi tals has", &words),
            "doctors and hospitals has"
        );
    }

    #[test]
    fn unmatched_or_ambiguous_lines_are_left_alone() {
        let words = [word("same", 72.0, 700.0), word("text", 100.0, 700.0)];
        assert_eq!(respace_lines("sametext", &words), "sametext");
        let words = [
            word("repeated", 72.0, 700.0),
            word("words", 120.0, 700.0),
            word("repeated", 72.0, 686.0),
            word("words", 120.0, 686.0),
        ];
        assert_eq!(respace_lines("repeatedwords", &words), "repeatedwords");
    }

    #[test]
    fn a_mark_drawn_later_still_runs_on_from_its_word() {
        // "9 January", then the raised "th" drawn at the end of the stream.
        let mut glyphs = line("9", 700.0, &[]);
        glyphs.extend(line(" January", 700.0, &[]).into_iter().map(|mut c| {
            c.origin_x += 10.0;
            c.bbox.x += 10.0;
            c
        }));
        let mut th = line("th", 704.0, &[]);
        for (k, c) in th.iter_mut().enumerate() {
            c.origin_x = 77.0 + 3.0 * k as f32;
            c.bbox = Rect::new(c.origin_x, 704.0, 3.0, 6.0);
            c.font_size = 6.0;
            c.advance_width = 3.0;
        }
        glyphs.extend(th);
        let w = glyph_words(&glyphs).unwrap();
        let th = w.iter().find(|w| w.text == "th").unwrap();
        assert!(th.glued);
        assert!(!w.iter().find(|w| w.text == "January").unwrap().glued);
    }

    #[test]
    fn a_hyphen_is_never_left_with_a_space_on_one_side() {
        let words = [
            word("ATTENTION", 72.0, 700.0),
            word("-BASED", 125.0, 700.0),
            word("SEQUENCE", 160.0, 700.0),
        ];
        assert_eq!(
            respace_lines("ATTENTION - BASED SEQUENCE", &words),
            "ATTENTION-BASED SEQUENCE"
        );
    }

    #[test]
    fn a_footnote_mark_stays_apart_from_its_word() {
        // "income" then a raised, smaller "1" right after it; "x" then "2".
        let raised = |text: &str, x: f32| {
            let mut g = line(text, 704.0, &[]);
            for c in g.iter_mut() {
                c.origin_x = x;
                c.bbox = Rect::new(x, 704.0, 3.0, 6.0);
                c.font_size = 6.0;
                c.advance_width = 3.0;
            }
            g
        };
        let mut glyphs = line("income", 700.0, &[]);
        glyphs.extend(raised("1", 102.0));
        let w = glyph_words(&glyphs).unwrap();
        assert!(!w.iter().find(|w| w.text == "1").unwrap().glued);
        let mut glyphs = line("x", 700.0, &[]);
        glyphs.extend(raised("2", 77.0));
        let w = glyph_words(&glyphs).unwrap();
        assert!(w.iter().find(|w| w.text == "2").unwrap().glued);
    }

    #[test]
    fn a_glyph_spelled_in_mojibake_gets_its_character_back() {
        let chars = |s: &str| s.chars().collect::<Vec<_>>();
        assert_eq!(undo_mojibake(&chars("â€‰")).as_deref(), Some("\u{2009}"));
        assert_eq!(undo_mojibake(&chars("Ã©")).as_deref(), Some("é"));
        // A ligature's letters and an accent drawn over its letter stay.
        assert_eq!(undo_mojibake(&chars("ffi")), None);
        assert_eq!(undo_mojibake(&chars("e´")), None);
    }

    #[test]
    fn accents_drawn_apart_are_put_back_on_their_letters() {
        // "Ecole" with the acute drawn after the word, over the "E", and a
        // dotless i under an acute at the end of "Paraıba".
        let mut glyphs = line("Ecole", 700.0, &[]);
        let mut acute = line("´", 700.0, &[]);
        acute[0].origin_x = 72.5;
        acute[0].bbox = Rect::new(72.5, 700.0, 4.0, 10.0);
        glyphs.extend(acute);
        let w = glyph_words(&glyphs).unwrap();
        assert_eq!(texts(&w), ["École"]);

        let mut glyphs = line("Paraıba", 700.0, &[]);
        let mut acute = line("´", 700.0, &[]);
        acute[0].origin_x = 92.5;
        acute[0].bbox = Rect::new(92.5, 700.0, 4.0, 10.0);
        glyphs.extend(acute);
        let w = glyph_words(&glyphs).unwrap();
        assert_eq!(texts(&w), ["Paraíba"]);

        // An accent standing on its own, beside letters, stays.
        let w = glyph_words(&line("a ´ b", 700.0, &[])).unwrap();
        assert_eq!(texts(&w), ["a", "´", "b"]);
    }

    #[test]
    fn a_space_inside_a_ligature_glyph_is_not_a_word_break() {
        // "inﬂ uenced": the ligature's text carries a space that sits
        // inside its advance, and "u" follows the ligature without a gap.
        let mut glyphs = line("inﬂ", 700.0, &[]);
        let mut space = line(" ", 700.0, &[]);
        space[0].origin_x = 85.0;
        space[0].bbox = Rect::new(85.0, 700.0, 2.5, 10.0);
        glyphs.extend(space);
        glyphs.extend(line("uenced", 700.0, &[]).into_iter().map(|mut c| {
            c.origin_x += 15.0;
            c.bbox.x += 15.0;
            c
        }));
        let w = glyph_words(&glyphs).unwrap();
        assert_eq!(texts(&w), ["inﬂuenced"]);
        // A real space, after the glyph, still breaks.
        let w = glyph_words(&line("of the", 700.0, &[])).unwrap();
        assert_eq!(texts(&w), ["of", "the"]);
    }

    #[test]
    fn mostly_rotated_pages_are_left_to_the_converter() {
        let mut glyphs = line("sideways", 700.0, &[]);
        for c in glyphs.iter_mut() {
            c.rotation_degrees = 90.0;
        }
        assert!(glyph_words(&glyphs).is_none());
    }
}
