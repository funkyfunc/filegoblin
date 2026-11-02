//! Right-to-left text in reading order.
//!
//! A PDF draws glyphs where they appear on the page, and most generators
//! (Chromium/Skia, LibreOffice, Word) draw an Arabic or Hebrew run in the
//! order the eye meets it from the left: the *visual* order. The glyphs
//! still map to the right letters, so a line read left to right spells each
//! right-to-left word backwards ("םולש" for "שלום"), puts the words of a
//! sentence last to first, and leaves numbers and Latin words inside it the
//! right way round.
//!
//! The Unicode bidirectional algorithm (UAX #9) turns logical text into
//! visual order by reversing its right-to-left runs. On one line those
//! reversals undo themselves: running the algorithm over the visual line
//! finds the same runs (letters, numbers and neutrals resolve from their
//! neighbours either way) and reversing them again gives the logical text.
//! Two details complete it:
//!
//! * A mirrored character (a bracket) in a right-to-left run is drawn with
//!   its mirror glyph, so it maps to its partner: "(" for the ")" that
//!   closes the run. It is swapped back (rule L4).
//! * A combining mark (Hebrew points, Arabic vowel marks) follows its base
//!   letter in the text layer; it moves with that letter instead of ending
//!   up in front of it (rule L3).
//!
//! The paragraph direction is the line's: right to left when the letters
//! at both its ends are right-to-left ones, otherwise by majority.
//! Lines without a right-to-left letter come back unchanged.
//!
//! Around that line pass: the page's glyphs get the letters only their
//! `/ActualText` names ([`visual_chars`]); the fast path's markdown lines
//! are rewritten from those glyphs ([`logical_markdown`]); and pipe tables
//! of right-to-left text list their right-hand column first
//! ([`rtl_tables_right_to_left`]). The vision path joins its lines from the
//! same glyphs and puts each in logical order (`pdf_vision::join_line`).

use unicode_bidi::{BidiClass, BidiInfo, Level, bidi_class};

/// A strong right-to-left character: Hebrew, Arabic, Syriac, Thaana, N'Ko,
/// their presentation forms.
pub(crate) fn is_rtl(c: char) -> bool {
    matches!(bidi_class(c), BidiClass::R | BidiClass::AL)
}

/// Whether the text has more right-to-left letters than left-to-right ones
/// (ties go right to left).
pub(crate) fn rtl_dominant(text: &str) -> bool {
    let (mut rtl, mut ltr) = (0usize, 0usize);
    for c in text.chars() {
        match bidi_class(c) {
            BidiClass::R | BidiClass::AL => rtl += 1,
            BidiClass::L => ltr += 1,
            _ => {}
        }
    }
    rtl > 0 && rtl >= ltr
}

/// The direction of a visual line: right to left when it starts and ends
/// with right-to-left letters (a Hebrew sentence quoting a long English
/// title), left to right when both ends are left-to-right letters; when the
/// ends differ, the majority ([`rtl_dominant`]).
fn line_is_rtl(line: &str) -> bool {
    let strong = |c: &char| matches!(bidi_class(*c), BidiClass::R | BidiClass::AL | BidiClass::L);
    match (line.chars().find(strong), line.chars().rev().find(strong)) {
        (Some(a), Some(b)) if is_rtl(a) && is_rtl(b) => true,
        (Some(a), Some(b)) if !is_rtl(a) && !is_rtl(b) => false,
        _ => rtl_dominant(line),
    }
}

/// The mirror partner of a bracket or other mirrored pair.
fn mirror(c: char) -> char {
    match c {
        '(' => ')',
        ')' => '(',
        '[' => ']',
        ']' => '[',
        '{' => '}',
        '}' => '{',
        '<' => '>',
        '>' => '<',
        '«' => '»',
        '»' => '«',
        '‹' => '›',
        '›' => '‹',
        _ => c,
    }
}

/// One visual line (glyphs read left to right) in logical order. Each line
/// of a multi-line text is its own paragraph.
pub(crate) fn visual_to_logical(text: &str) -> String {
    if !text.chars().any(is_rtl) {
        return text.to_string();
    }
    text.split('\n')
        .map(visual_line_to_logical)
        .collect::<Vec<_>>()
        .join("\n")
}

fn visual_line_to_logical(line: &str) -> String {
    if !line.chars().any(is_rtl) {
        return line.to_string();
    }
    let level = if line_is_rtl(line) {
        Level::rtl()
    } else {
        Level::ltr()
    };
    let info = BidiInfo::new(line, Some(level));
    let Some(para) = info.paragraphs.first() else {
        return line.to_string();
    };
    let levels = info.reordered_levels_per_char(para, para.range.clone());
    // Clusters: a character and the combining marks after it.
    let mut clusters: Vec<(Level, Vec<char>)> = Vec::new();
    for (c, lv) in line.chars().zip(levels) {
        if bidi_class(c) == BidiClass::NSM
            && let Some(last) = clusters.last_mut()
        {
            last.1.push(c);
        } else {
            clusters.push((lv, vec![c]));
        }
    }
    let cluster_levels: Vec<Level> = clusters.iter().map(|c| c.0).collect();
    let mut out = String::with_capacity(line.len());
    for i in BidiInfo::reorder_visual(&cluster_levels) {
        let (lv, chars) = &clusters[i];
        for &c in chars {
            out.push(if lv.is_rtl() { mirror(c) } else { c });
        }
    }
    out
}

/// The page's glyphs, on a page with right-to-left text, with a letter for
/// every glyph the text layer names only in `/ActualText`. `None` when the
/// page has no right-to-left text.
///
/// The glyphs are the reliable source of order: their words
/// ([`super::pdf_words::glyph_words`]) follow the pen left to right, so
/// they spell each line in visual order. (A generator that draws a
/// right-to-left run from its right end, glyph by glyph leftwards, is not
/// handled: its words come apart into letters.) Their letters come from the
/// font's character
/// map, which does not reach the joined forms of Arabic letters: Chromium
/// names each of those with an `/ActualText` of its own, which the
/// converter's text spans read and its single glyphs (U+FFFD) do not. The
/// spans are no source of order, though: the converter turns some
/// right-to-left spans and not others, and inside a span it keeps the two
/// letters of a lam-alef ligature in logical order. So each span is
/// aligned with the glyphs under it ([`fill_from_span`]) and lends them
/// only the letters they lack.
pub(crate) fn visual_chars(
    doc: &pdf_oxide::PdfDocument,
    page: usize,
) -> Option<Vec<pdf_oxide::layout::TextChar>> {
    let mut chars = doc.extract_chars(page).ok()?;
    let spans = doc.extract_spans(page).ok()?;
    if !spans.iter().any(|s| s.text.chars().any(is_rtl)) && !chars.iter().any(|c| is_rtl(c.char)) {
        return None;
    }
    if chars.iter().any(|c| c.char == '\u{FFFD}') {
        // Glyph indices sorted left to right (content order on ties: the
        // two glyphs of a lam-alef share one origin).
        let mut by_x: Vec<usize> = (0..chars.len()).collect();
        by_x.sort_by(|&a, &b| chars[a].origin_x.total_cmp(&chars[b].origin_x));
        for s in &spans {
            fill_from_span(&mut chars, &by_x, s);
        }
    }
    Some(chars)
}

/// A letter or digit that can say which way a text runs.
fn letter(c: char) -> bool {
    c.is_alphanumeric() && c != '\u{FFFD}' && bidi_class(c) != BidiClass::NSM
}

/// Fills the unnamed glyphs under span `s` with its letters. The span's
/// characters are first put in the glyphs' order — as they are, or turned,
/// whichever shares more pairs of neighbouring letters with the glyphs —
/// then aligned with them (an edit-distance alignment in which an unnamed
/// glyph matches any character), so a swapped pair or a missing glyph only
/// costs its own place.
fn fill_from_span(
    chars: &mut [pdf_oxide::layout::TextChar],
    by_x: &[usize],
    s: &pdf_oxide::layout::TextSpan,
) {
    const UNNAMED: char = '\u{FFFD}';
    let size = s.font_size.max(1.0);
    let (x0, x1) = (s.bbox.x - 0.1 * size, s.bbox.x + s.bbox.width + 0.1 * size);
    let lo = by_x.partition_point(|&i| chars[i].origin_x < x0);
    let glyphs: Vec<usize> = by_x[lo..]
        .iter()
        .copied()
        .take_while(|&i| chars[i].origin_x <= x1)
        .filter(|&i| {
            (chars[i].origin_y - s.bbox.y).abs() <= 0.3 * size && !chars[i].char.is_control()
        })
        .collect();
    if !glyphs.iter().any(|&i| chars[i].char == UNNAMED) {
        return;
    }
    let mut text: Vec<char> = s.text.chars().filter(|c| !c.is_control()).collect();
    // A span is the glyphs under it: about as many characters. (The
    // converter also has spans that repeat a whole table's text across the
    // page; they lend nothing.)
    if text.is_empty() || glyphs.len() * 10 < text.len() * 7 || glyphs.len() * 10 > text.len() * 14
    {
        return;
    }
    // Which way round: each named glyph looks for its letter at the same
    // relative place in the span (give or take a little), read forwards and
    // backwards. Most of the named glyphs must find it one way.
    let (n, m) = (glyphs.len(), text.len());
    let found = |turn: bool| -> usize {
        glyphs
            .iter()
            .enumerate()
            .filter(|&(_, &g)| letter(chars[g].char))
            .filter(|&(i, &g)| {
                let p = if n > 1 {
                    (i * (m - 1) + (n - 1) / 2) / (n - 1)
                } else {
                    0
                };
                let slack = 1 + m / 12;
                (p.saturating_sub(slack)..=(p + slack).min(m - 1)).any(|j| {
                    let t = if turn { text[m - 1 - j] } else { text[j] };
                    t == chars[g].char
                })
            })
            .count()
    };
    let known = glyphs.iter().filter(|&&g| letter(chars[g].char)).count();
    let (forward, backward) = (found(false), found(true));
    if 2 * forward.max(backward) < known || forward == backward && known > 0 {
        return;
    }
    if backward > forward {
        text.reverse();
    }
    // Edit-distance alignment of glyphs (rows) and text (columns).
    let named: Vec<char> = glyphs.iter().map(|&i| chars[i].char).collect();
    let cost = |g: char, t: char| -> u32 {
        if g == t || (g == UNNAMED && !t.is_whitespace()) {
            0
        } else {
            2
        }
    };
    let mut d = vec![vec![0u32; m + 1]; n + 1];
    for (i, row) in d.iter_mut().enumerate() {
        row[0] = i as u32;
    }
    for (j, cell) in d[0].iter_mut().enumerate() {
        *cell = j as u32;
    }
    // Two named glyphs against the same two characters swapped (the span's
    // lam-alef) are one step.
    let swapped = |i: usize, j: usize| {
        i > 1 && j > 1 && {
            let (a, b) = (named[i - 2], named[i - 1]);
            a != b && a != UNNAMED && b != UNNAMED && a == text[j - 1] && b == text[j - 2]
        }
    };
    for i in 1..=n {
        for j in 1..=m {
            let g = named[i - 1];
            d[i][j] = (d[i - 1][j - 1] + cost(g, text[j - 1]))
                .min(d[i - 1][j] + 1)
                .min(d[i][j - 1] + 1);
            if swapped(i, j) {
                d[i][j] = d[i][j].min(d[i - 2][j - 2] + 1);
            }
        }
    }
    let (mut i, mut j) = (n, m);
    while i > 0 && j > 0 {
        let g = named[i - 1];
        if swapped(i, j) && d[i][j] == d[i - 2][j - 2] + 1 {
            i -= 2;
            j -= 2;
        } else if d[i][j] == d[i - 1][j - 1] + cost(g, text[j - 1]) {
            if g == UNNAMED && cost(g, text[j - 1]) == 0 {
                chars[glyphs[i - 1]].char = text[j - 1];
            }
            i -= 1;
            j -= 1;
        } else if d[i][j] == d[i - 1][j] + 1 {
            i -= 1;
        } else {
            j -= 1;
        }
    }
}

/// A run of words on one baseline with no column-sized gap between them,
/// as drawn: a line of a paragraph, or one line of a table cell.
#[derive(Debug, Clone)]
pub(crate) struct Segment {
    /// Its text in visual order.
    pub visual: String,
    pub x0: f32,
    pub x1: f32,
    /// Vertical centre (PDF user space: larger is higher on the page).
    pub y: f32,
    pub size: f32,
}

/// The page's segments from its glyph words, top to bottom and, on a line,
/// left to right.
pub(crate) fn segments(words: &[super::pdf_words::GlyphWord]) -> Vec<Segment> {
    let centre = |w: &super::pdf_words::GlyphWord| (w.y0 + w.y1) / 2.0;
    let mut order: Vec<usize> = (0..words.len()).collect();
    order.sort_by(|&a, &b| centre(&words[b]).total_cmp(&centre(&words[a])));
    let mut lines: Vec<Vec<usize>> = Vec::new();
    for i in order {
        let w = &words[i];
        match lines.last_mut() {
            Some(l)
                if (centre(&words[l[0]]) - centre(w)).abs()
                    <= 0.4 * words[l[0]].size.max(w.size).max(1.0) =>
            {
                l.push(i)
            }
            _ => lines.push(vec![i]),
        }
    }
    let mut out = Vec::new();
    for mut l in lines {
        l.sort_by(|&a, &b| words[a].x0.total_cmp(&words[b].x0));
        let mut cur: Option<Segment> = None;
        for i in l {
            let w = &words[i];
            let size = w.size.max(1.0);
            match cur.as_mut() {
                Some(c) if w.x0 - c.x1 <= 1.0 * size.max(c.size) => {
                    if !w.glued {
                        c.visual.push(' ');
                    }
                    c.visual.push_str(&w.text);
                    c.x1 = c.x1.max(w.x1);
                    c.size = c.size.max(size);
                }
                _ => {
                    out.extend(cur.take());
                    cur = Some(Segment {
                        visual: w.text.clone(),
                        x0: w.x0,
                        x1: w.x1,
                        y: centre(w),
                        size,
                    });
                }
            }
        }
        out.extend(cur.take());
    }
    out
}

/// Letters and digits of a text, counted: what a line keeps however its
/// words and letters were ordered.
fn bag(text: &str) -> std::collections::HashMap<char, usize> {
    let mut m = std::collections::HashMap::new();
    for c in text.chars().filter(|&c| letter(c)) {
        *m.entry(c).or_insert(0) += 1;
    }
    m
}

/// Characters the converter puts around directional runs (U+2066–U+2069)
/// or as direction marks; they belong to no glyph.
fn is_isolate(c: char) -> bool {
    matches!(c, '\u{2066}'..='\u{2069}' | '\u{200E}' | '\u{200F}' | '\u{202A}'..='\u{202E}')
}

/// The converter's markdown for a page with right-to-left text, with each
/// line that holds right-to-left letters rewritten in logical order and
/// right-to-left tables read from their right-hand column.
///
/// The converter writes each visual line's spans left to right, some turned
/// and some not (see [`visual_chars`]), so its lines are neither visual nor
/// logical. Its paragraphs, headings and table cells are right, though, and
/// so are the letters in them: a line's letters, counted, are those of the
/// page segments it was made from. Each such line (or table cell) takes the
/// segments whose letters it holds — the first one and the lines that follow
/// it down the same column — and is written as their logical text. A line
/// whose segments cannot be found keeps the converter's text. Links on such
/// lines are left out: the converter cuts them along its scrambled spans.
pub(crate) fn logical_markdown(doc: &pdf_oxide::PdfDocument, page: usize, md: &str) -> String {
    let Some(chars) = visual_chars(doc, page) else {
        return md.to_string();
    };
    let Some(words) = super::pdf_words::glyph_words(&chars) else {
        return md.to_string();
    };
    let segs = segments(&words);
    let mut page_words = Words::new(&segs);
    let mut out: Vec<String> = Vec::new();
    for line in md.split('\n') {
        let line: String = line.chars().filter(|&c| !is_isolate(c)).collect();
        if !line.chars().any(is_rtl) {
            out.push(line);
            continue;
        }
        let t = line.trim_start();
        if t.starts_with('|') {
            let cells: Vec<String> = t
                .trim()
                .trim_matches('|')
                .split('|')
                .map(|cell| {
                    let plain = unlink(cell.trim());
                    if !plain.chars().any(is_rtl) {
                        return plain;
                    }
                    page_words.claim(&plain).unwrap_or(plain)
                })
                .collect();
            out.push(format!("| {} |", cells.join(" | ")));
            continue;
        }
        let body = t.trim_start_matches('#');
        let hashes = &t[..t.len() - body.len()];
        let (marker, body) = match body.trim_start() {
            b if b.starts_with("- ") || b.starts_with("* ") || b.starts_with("> ") => {
                (&b[..2], &b[2..])
            }
            b => ("", b),
        };
        let plain = unlink(body.trim());
        match page_words.claim(&plain) {
            Some(text) => {
                let indent = &line[..line.len() - t.len()];
                let sep = if hashes.is_empty() { "" } else { " " };
                out.push(format!("{indent}{hashes}{sep}{marker}{text}"));
            }
            None => out.push(line),
        }
    }
    rtl_tables_right_to_left(&out.join("\n"))
}

/// A line's text without link markup: `[text](url)` becomes `text`, and
/// emphasis markers go.
fn unlink(text: &str) -> String {
    static LINK: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
        regex::Regex::new(r"\[([^\]]*)\]\([^)\s]*\)").expect("valid regex")
    });
    LINK.replace_all(text, "$1").replace("**", "")
}

/// The page's text as logical words, segment by segment, with which of
/// them a line has already taken.
struct Words<'a> {
    segs: &'a [Segment],
    words: Vec<Vec<String>>,
    bags: Vec<Vec<std::collections::HashMap<char, usize>>>,
    used: Vec<Vec<bool>>,
}

impl<'a> Words<'a> {
    fn new(segs: &'a [Segment]) -> Self {
        let words: Vec<Vec<String>> = segs
            .iter()
            .map(|s| {
                visual_to_logical(s.visual.trim())
                    .split_whitespace()
                    .map(str::to_string)
                    .collect()
            })
            .collect();
        let bags = words
            .iter()
            .map(|ws| ws.iter().map(|w| bag(w)).collect())
            .collect();
        let used = words.iter().map(|ws| vec![false; ws.len()]).collect();
        Words {
            segs,
            words,
            bags,
            used,
        }
    }

    /// The segment that continues segment `k` down its column: the next line
    /// below it that overlaps it.
    fn below(&self, k: usize) -> Option<usize> {
        let p = &self.segs[k];
        (k + 1..self.segs.len())
            .take_while(|&j| p.y - self.segs[j].y <= 2.2 * p.size.max(self.segs[j].size))
            .find(|&j| {
                let s = &self.segs[j];
                s.y < p.y - 0.35 * p.size && s.x0 < p.x1 && s.x1 > p.x0
            })
    }

    /// The logical text of the words that make up `text` (letters
    /// counted), marking them taken: a run of words that starts anywhere in
    /// a segment, runs on into the segments below it in its column and stops
    /// anywhere. The run that covers most of the line wins (the earliest on
    /// a tie). `None` when no run covers most of it.
    fn claim(&mut self, text: &str) -> Option<String> {
        let want = bag(text);
        let total: usize = want.values().sum();
        if total < 2 {
            return None;
        }
        let fits = |have: &std::collections::HashMap<char, usize>,
                    room: &std::collections::HashMap<char, usize>| {
            !have.is_empty()
                && have
                    .iter()
                    .all(|(c, n)| room.get(c).copied().unwrap_or(0) >= *n)
        };
        let mut best: Option<(usize, Vec<(usize, usize)>)> = None;
        for k in 0..self.segs.len() {
            for w in 0..self.words[k].len() {
                if self.used[k][w] || !fits(&self.bags[k][w], &want) {
                    continue;
                }
                let mut room = want.clone();
                let mut taken: Vec<(usize, usize)> = Vec::new();
                let (mut seg, mut at) = (k, w);
                loop {
                    let ok = !self.used[seg][at] && fits(&self.bags[seg][at], &room);
                    if !ok {
                        break;
                    }
                    for (c, n) in &self.bags[seg][at] {
                        *room.get_mut(c).expect("fits") -= n;
                    }
                    taken.push((seg, at));
                    at += 1;
                    if at == self.words[seg].len() {
                        match self.below(seg) {
                            Some(next) if !self.words[next].is_empty() => {
                                seg = next;
                                at = 0;
                            }
                            _ => break,
                        }
                    }
                }
                let covered = total - room.values().sum::<usize>();
                if best.as_ref().is_none_or(|(c, _)| covered > *c) {
                    best = Some((covered, taken));
                }
            }
        }
        let (covered, taken) = best?;
        if covered * 100 < total * 85 {
            return None;
        }
        for &(k, w) in &taken {
            self.used[k][w] = true;
        }
        Some(
            taken
                .iter()
                .map(|&(k, w)| self.words[k][w].as_str())
                .collect::<Vec<_>>()
                .join(" "),
        )
    }
}

/// Pipe tables whose cells are mostly right-to-left text, with each row's
/// cells in reading order: the right-hand column first.
pub(crate) fn rtl_tables_right_to_left(md: &str) -> String {
    let lines: Vec<&str> = md.split('\n').collect();
    let mut out: Vec<String> = Vec::with_capacity(lines.len());
    let mut i = 0;
    while i < lines.len() {
        if !lines[i].trim_start().starts_with('|') {
            out.push(lines[i].to_string());
            i += 1;
            continue;
        }
        let start = i;
        while i < lines.len() && lines[i].trim_start().starts_with('|') {
            i += 1;
        }
        let block = &lines[start..i];
        if !rtl_dominant(&block.join(" ")) {
            out.extend(block.iter().map(|l| l.to_string()));
            continue;
        }
        for l in block {
            let t = l.trim();
            // Split on unescaped pipes only.
            let mut cells: Vec<String> = vec![String::new()];
            let mut prev = ' ';
            for c in t.trim_start_matches('|').chars() {
                if c == '|' && prev != '\\' {
                    cells.push(String::new());
                } else {
                    cells.last_mut().expect("non-empty").push(c);
                }
                prev = c;
            }
            if t.ends_with('|') && !t.ends_with("\\|") {
                cells.pop();
            }
            cells.reverse();
            let row: Vec<String> = cells.iter().map(|c| c.trim().to_string()).collect();
            out.push(format!("| {} |", row.join(" | ")));
        }
    }
    out.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Logical text as a left-to-right reader of the page meets it: what a
    /// visual-order PDF's glyphs spell. Built by hand per case below.
    #[test]
    fn pure_hebrew_line_reverses() {
        // "מחוז הצפון" drawn left to right.
        assert_eq!(visual_to_logical("ןופצה זוחמ"), "מחוז הצפון");
    }

    #[test]
    fn numbers_inside_rtl_keep_their_digit_order() {
        // "יש 1,513,300 תושבים ב-2023." as the page shows it.
        let visual = ".2023-ב םיבשות 1,513,300 שי";
        assert_eq!(visual_to_logical(visual), "יש 1,513,300 תושבים ב-2023.");
    }

    #[test]
    fn latin_word_inside_rtl_stays_left_to_right() {
        let visual = "םויה Wikipedia האר";
        assert_eq!(visual_to_logical(visual), "ראה Wikipedia היום");
    }

    #[test]
    fn brackets_are_mirrored_back() {
        // "שטח (בקילומטר רבוע)" with mirrored glyphs for the parentheses,
        // and a footnote reference "[1]" after a comma.
        assert_eq!(
            visual_to_logical("(עובר רטמוליקב) חטש"),
            "שטח (בקילומטר רבוע)"
        );
        assert_eq!(visual_to_logical("[1],םורדה"), "הדרום,[1]");
    }

    #[test]
    fn combining_marks_stay_after_their_letter() {
        // "שָׁלוֹם": shin + qamats + shin dot, lamed, vav + holam, final mem.
        let logical = "ש\u{5B8}\u{5C1}לו\u{5B9}ם";
        let visual = "םו\u{5B9}לש\u{5B8}\u{5C1}";
        assert_eq!(visual_to_logical(visual), logical);
    }

    #[test]
    fn arabic_with_digits() {
        // "تتكون جمهورية السودان من 18 ولاية"
        let visual = "ةيالو 18 نم نادوسلا ةيروهمج نوكتت";
        assert_eq!(
            visual_to_logical(visual),
            "تتكون جمهورية السودان من 18 ولاية"
        );
    }

    #[test]
    fn hebrew_inside_an_english_line() {
        // An English sentence quoting "שלום": only the Hebrew run turns.
        assert_eq!(
            visual_to_logical("They said םולש and left."),
            "They said שלום and left."
        );
    }

    #[test]
    fn ltr_and_cjk_lines_are_untouched() {
        for s in [
            "Plain (English) text, 1,234.",
            "中文文本（测试）",
            "x < y [1]",
        ] {
            assert_eq!(visual_to_logical(s), s);
        }
    }

    #[test]
    fn each_line_is_its_own_paragraph() {
        assert_eq!(visual_to_logical("םולש\nabc\nםלוע"), "שלום\nabc\nעולם");
    }

    #[test]
    fn rtl_tables_read_from_the_right() {
        let md = "intro\n| צפיפות | שטח | מחוז | מספר |\n|---|---|---|---|\n| 1,728 | 1,293 | מחוז המרכז | 1 |\nend";
        assert_eq!(
            rtl_tables_right_to_left(md),
            "intro\n| מספר | מחוז | שטח | צפיפות |\n| --- | --- | --- | --- |\n| 1 | מחוז המרכז | 1,293 | 1,728 |\nend"
        );
        let ltr = "| a | b |\n|---|---|\n| 1 | 2 |";
        assert_eq!(rtl_tables_right_to_left(ltr), ltr);
    }

    fn glyph(c: char, x: f32) -> pdf_oxide::layout::TextChar {
        let mut g = pdf_oxide::layout::TextChar::simple(
            c,
            pdf_oxide::geometry::Rect::new(x, 100.0, 5.0, 10.0),
            "F".into(),
            10.0,
        );
        g.origin_x = x;
        g.origin_y = 100.0;
        g
    }

    fn span(text: &str, x: f32, width: f32) -> pdf_oxide::layout::TextSpan {
        pdf_oxide::layout::TextSpan {
            text: text.to_string(),
            bbox: pdf_oxide::geometry::Rect::new(x, 100.0, width, 10.0),
            font_size: 10.0,
            ..Default::default()
        }
    }

    #[test]
    fn unnamed_glyphs_take_the_span_letters_either_way_round() {
        // "השלום" drawn left to right as ם ו ל ש ה; two glyphs have no
        // character of their own.
        let drawn = ['ם', '\u{FFFD}', 'ל', '\u{FFFD}', 'ה'];
        for text in ["םולשה", "השלום"] {
            let mut chars: Vec<_> = drawn
                .iter()
                .enumerate()
                .map(|(i, &c)| glyph(c, 100.0 + 6.0 * i as f32))
                .collect();
            let by_x: Vec<usize> = (0..chars.len()).collect();
            fill_from_span(&mut chars, &by_x, &span(text, 100.0, 30.0));
            let visual: String = chars.iter().map(|c| c.char).collect();
            assert_eq!(visual, "םולשה", "span {text}");
            assert_eq!(visual_to_logical(&visual), "השלום");
        }
    }

    #[test]
    fn dominance() {
        assert!(rtl_dominant("שלום abc"));
        assert!(!rtl_dominant("hello שלום world"));
        assert!(!rtl_dominant("12 34"));
    }
}
