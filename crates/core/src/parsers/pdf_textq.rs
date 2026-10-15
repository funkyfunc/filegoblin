//! How trustworthy is a page's text layer?
//!
//! Scanned books and papers often carry an OCR text layer made by whatever
//! engine the scanner used. On clean prose those layers are usually good; on
//! mathematics, small print and degraded pages they are noise: formulas come
//! out as runs of single letters, words gain stray capitals and punctuation,
//! and fonts without a Unicode map produce replacement characters. Text
//! extraction cannot repair that — the page has to be read again from its
//! image.
//!
//! The score here is a handful of cheap, language-light counts over the
//! layer's words (single letters, "garbage" tokens in the sense of Taghva et
//! al.'s OCR garbage rules, unmappable characters). It is only used for pages
//! that are a picture of themselves — a page-sized image under the text — so
//! a born-digital page full of variable names or register mnemonics is never
//! re-read.

/// Counts over a page's text layer.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(crate) struct TextQuality {
    /// Words with at least one letter.
    pub words: usize,
    /// Share of those words that are one letter other than "a", "A" or "I".
    pub single: f32,
    /// Share of those words that look like OCR garbage.
    pub garbage: f32,
    /// Share of all non-space characters that are U+FFFD, private-use or
    /// control characters.
    pub unmapped: f32,
}

impl TextQuality {
    /// The layer is too damaged to keep: re-read the page from its image.
    /// Below 30 words there is too little to judge, unless most characters
    /// are unmappable.
    ///
    /// The bars are low because they only apply to scans, where our own OCR
    /// is the alternative: clean prose in a scanner's layer has almost no
    /// loose letters (under 2% on the books and papers measured), while a
    /// page with formulas in it has 4–37%, and re-reading such a page scored
    /// higher than keeping its layer (Zeitz slice: 91.3 against 88.0).
    pub(crate) fn is_poor(&self) -> bool {
        if self.unmapped >= 0.10 {
            return true;
        }
        self.words >= 30 && (self.single >= 0.04 || self.garbage >= 0.08 || self.unmapped >= 0.03)
    }
}

pub(crate) fn measure_words<'a>(tokens: impl Iterator<Item = &'a str>) -> TextQuality {
    let (mut words, mut single, mut garbage) = (0usize, 0usize, 0usize);
    let (mut chars, mut unmapped) = (0usize, 0usize);
    for raw in tokens {
        for c in raw.chars().filter(|c| !c.is_whitespace()) {
            chars += 1;
            if is_unmapped(c) {
                unmapped += 1;
            }
        }
        for tok in raw.split_whitespace() {
            let w = tok
                .trim_start_matches(['"', '\'', '(', '[', '“', '‘'])
                .trim_end_matches(['"', '\'', ')', ']', '.', ',', ';', ':', '!', '?', '”', '’']);
            if !w.chars().any(|c| c.is_alphabetic()) {
                continue;
            }
            words += 1;
            if w.chars().count() == 1 && !matches!(w, "a" | "A" | "I") {
                single += 1;
            } else if is_garbage(w) {
                garbage += 1;
            }
        }
    }
    let share = |n: usize, d: usize| if d == 0 { 0.0 } else { n as f32 / d as f32 };
    TextQuality {
        words,
        single: share(single, words),
        garbage: share(garbage, words),
        unmapped: share(unmapped, chars),
    }
}

/// Most of the page's text is set in fonts Acrobat's ClearScan synthesized
/// from a scan (named `Fd` and digits): the "text layer" is OCR output drawn
/// visibly in place of the picture.
pub(crate) fn clearscan_fonts(doc: &pdf_oxide::PdfDocument, page: usize) -> bool {
    let Ok(spans) = doc.extract_spans(page) else {
        return false;
    };
    let (mut all, mut rebuilt) = (0usize, 0usize);
    for s in &spans {
        let n = s.text.chars().filter(|c| !c.is_whitespace()).count();
        all += n;
        let name = s.font_name.rsplit('+').next().unwrap_or("");
        let digits = name
            .strip_prefix("Fd")
            .map(|r| r.chars().take_while(char::is_ascii_digit).count());
        if digits.is_some_and(|d| d >= 4) {
            rebuilt += n;
        }
    }
    all >= 50 && rebuilt * 2 >= all
}

fn is_unmapped(c: char) -> bool {
    c == '\u{FFFD}'
        || ('\u{E000}'..='\u{F8FF}').contains(&c)
        || (c.is_control() && !matches!(c, '\t' | '\n' | '\r'))
}

/// A word an OCR engine made up: a long run of one character, punctuation
/// wedged between letters, a capital after lowercase letters, letters and
/// digits interleaved, or a long consonant run with no vowel.
fn is_garbage(w: &str) -> bool {
    let cs: Vec<char> = w.chars().collect();
    if cs.len() > 30 {
        return true;
    }
    if cs.windows(4).any(|q| q.iter().all(|&c| c == q[0])) {
        return true;
    }
    for t in cs.windows(3) {
        let (a, b, c) = (t[0], t[1], t[2]);
        // "wi|h", "t^e", "o;f" — but not "don't", "e-mail", "U.S.", "and/or".
        if a.is_alphabetic()
            && c.is_alphabetic()
            && !b.is_alphanumeric()
            && !matches!(b, '\'' | '’' | '-' | '–' | '.' | '/' | '&')
        {
            return true;
        }
    }
    // A word ending in a capital after lowercase letters: "worD", "theM".
    // A capital inside a word is left alone: camelCase names ("iPhone",
    // "McDonald", "JavaScript") have one too.
    for (i, w3) in cs.windows(2).enumerate() {
        if w3[0].is_lowercase() && w3[1].is_uppercase() {
            let at_end = i + 2 == cs.len();
            let lower_before = cs[..=i].iter().filter(|c| c.is_lowercase()).count();
            if at_end && lower_before >= 2 {
                return true;
            }
        }
    }
    // Letters and digits interleaved inside a word: "th1s", "w0rd", but not
    // "1990s", "4th", "A4", "B12", "H2O".
    let letters = cs.iter().filter(|c| c.is_alphabetic()).count();
    let digits = cs.iter().filter(|c| c.is_ascii_digit()).count();
    if letters >= 3 && digits >= 1 {
        let changes = cs
            .windows(2)
            .filter(|p| p[0].is_alphabetic() != p[1].is_alphabetic())
            .filter(|p| p[0].is_alphanumeric() && p[1].is_alphanumeric())
            .count();
        if changes >= 2 && cs.iter().all(|c| c.is_alphanumeric()) {
            return true;
        }
    }
    // Four or more letters and no vowel at all ("rnrn", "tlhc"), or six
    // consonants in a row, in an all-lowercase word (acronyms are upper).
    if cs.iter().all(|c| c.is_lowercase()) && letters >= 4 {
        let vowel = |c: &char| "aeiouy".contains(*c);
        if !cs.iter().any(vowel) {
            return true;
        }
        let mut run = 0;
        for c in &cs {
            run = if vowel(c) { 0 } else { run + 1 };
            if run >= 6 {
                return true;
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn q(text: &str) -> TextQuality {
        measure_words(std::iter::once(text))
    }

    const PROSE: &str = "The committee met on Tuesday to review the budget for the coming year. \
        Members agreed that the plan was sound, although several asked for more detail on \
        travel costs and on the new hires. A final vote is expected next month, after the \
        auditors have finished their report and the chair has circulated it to everyone.";

    #[test]
    fn clean_prose_is_not_poor() {
        let t = q(PROSE);
        assert!(t.words >= 30);
        assert!(!t.is_poor(), "{t:?}");
        assert_eq!(t.single, 0.0);
    }

    #[test]
    fn spaced_math_ocr_is_poor() {
        // What an OCR layer makes of a page of algebra.
        let t = q(
            "Let x n be the sum of the first n terms so that x n + 1 = x n + a n and \
             hence f ( x ) = x 2 + b x + c for all x in the set S where b and c are \
             given and the sum of the k th terms is t k which we call the partial \
             sum of the sequence and we write it as s n for short in the text",
        );
        assert!(t.single > 0.08, "{t:?}");
        assert!(t.is_poor());
    }

    #[test]
    fn garbage_tokens() {
        for w in ["wi|h", "worD", "th1s1", "rnrnrn", "aaaa", "tlhc"] {
            assert!(is_garbage(w), "{w}");
        }
        for w in [
            "don't",
            "e-mail",
            "U.S",
            "and/or",
            "iPhone",
            "McDonald",
            "JavaScript",
            "1990s",
            "4th",
            "A4",
            "B12",
            "H2O",
            "rhythm",
            "strengths",
            "AT&T",
        ] {
            assert!(!is_garbage(w), "{w}");
        }
    }

    #[test]
    fn unmapped_glyphs_are_poor() {
        let t = q("\u{FFFD}\u{FFFD}\u{FFFD} abc \u{E001}\u{E002} def");
        assert!(t.is_poor(), "{t:?}");
    }

    #[test]
    fn too_short_to_judge() {
        assert!(!q("x y z").is_poor());
    }
}
