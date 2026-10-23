//! Markdown headings recovered from how a converter wrote a title.
//!
//! Converters often know that a line is set apart but not that it is a
//! heading. A PDF section title in the body font, bold, becomes `**Title**`;
//! an HTML filing that styles its headings with CSS becomes a bold or plain
//! line. Chunk breadcrumbs and every reader's sense of structure need real
//! `#` headings, so a standalone block is promoted when it is either
//!
//! * **one bold span**, short, not a sentence (`**The Scorecard in 2023**`,
//!   `**Item 1A. Risk Factors**`); or
//! * **a numbered title**, short and not a sentence (`1.1 Granting of rights`,
//!   `ARTICLE IV`, `Section 2.14 Defaulting Lenders`, `Item 7. Management's
//!   Discussion`).
//!
//! The level follows the numbering: `ARTICLE`/`PART`/`Item`/`1.` → 2,
//! `1.1`/`Section 2.14` → 3, `1.1.1` → 4. An unnumbered bold title is level
//! 2 when it is set in capitals and 3 otherwise, so it nests under a
//! numbered part. Tables, lists, code, quotes and existing headings are
//! never touched, and nothing that reads as a sentence is promoted.

use std::sync::LazyLock;

static RE_NUMBERED: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(
        r"(?x)^
        (?P<num>
            (?:ARTICLE|Article|PART|Part|CHAPTER|Chapter|ITEM|Item|SCHEDULE|Schedule|EXHIBIT|Exhibit|APPENDIX|Appendix)
                \s+ (?:[0-9]+[A-Z]?|[IVXLC]+) \.?
          | (?:SECTION|Section|§)\s* [0-9]+(?:\.[0-9]+)* \.?
          | [0-9]{1,2}(?:\.[0-9]{1,3}){0,3} \.?
        )
        (?:\s+(?P<title>\S.*))?$",
    )
    .unwrap()
});

/// "Table 3-13. R11 Register Field Descriptions", "Figure 4: Results".
static RE_CAPTION: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(r"^(?:Table|TABLE|Figure|FIGURE|Fig\.|Exhibit|EXHIBIT|Schedule|Appendix)\s+[A-Z]?[0-9]+(?:[-.][0-9]+)*[A-Za-z]?[.:]?\s+\S").unwrap()
});

/// A bold all-caps lead-in run into its paragraph: "**INDICATIONS AND
/// USAGE** KEYTRUDA is a …".
static RE_RUN_IN: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r"^\*\*([^*]+)\*\*\s+(\p{Lu}.*)$").unwrap());

/// Promotes heading-like standalone blocks to ATX headings; see the module
/// docs.
pub(crate) fn promote_headings(md: &str) -> String {
    let lines: Vec<&str> = md.lines().collect();
    let mut out: Vec<String> = Vec::with_capacity(lines.len());
    let mut in_code = false;
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        let t = line.trim();
        if t.starts_with("```") || t.starts_with("~~~") {
            in_code = !in_code;
            out.push(line.to_string());
            i += 1;
            continue;
        }
        let standalone = !in_code
            && (i == 0 || lines[i - 1].trim().is_empty())
            && lines.get(i + 1).is_none_or(|n| n.trim().is_empty());
        let indented = line.starts_with("    ") || line.starts_with('\t');
        // A bold title wrapped onto two lines ("**Exclusive⏎Call Option
        // Agreement**"): one heading.
        if !in_code
            && !indented
            && (i == 0 || lines[i - 1].trim().is_empty())
            && t.starts_with("**")
            && !t.ends_with("**")
            && let Some(next) = lines.get(i + 1).map(|n| n.trim())
            && next.ends_with("**")
            && !next.starts_with("**")
            && lines.get(i + 2).is_none_or(|n| n.trim().is_empty())
            && let Some(h) = heading_for(&format!("{t} {next}"))
        {
            out.push(h);
            i += 2;
            continue;
        }
        if standalone
            && !indented
            && let Some(h) = heading_for(t)
        {
            out.push(h);
        } else if !in_code
            && !indented
            && (i == 0 || lines[i - 1].trim().is_empty())
            && let Some(c) = RE_RUN_IN.captures(t)
            && is_caps(&c[1])
            && c[1].split_whitespace().count() >= 2
            && let Some(h) = heading_for(&format!("**{}**", c[1].trim()))
        {
            out.push(h);
            out.push(String::new());
            out.push(c[2].to_string());
        } else {
            out.push(line.to_string());
        }
        i += 1;
    }
    out.join("\n")
}

/// The ATX heading this block should be, if any.
fn heading_for(t: &str) -> Option<String> {
    if t.is_empty()
        || t.starts_with(['#', '|', '>', '-', '+', '<', '!', '['])
        || t.starts_with("* ")
    {
        return None;
    }
    // One bold span covering the whole block.
    if let Some(inner) = t
        .strip_prefix("**")
        .and_then(|r| r.strip_suffix("**"))
        .filter(|r| !r.contains("**"))
    {
        let inner = inner.trim();
        if !title_like(inner, 14) {
            return None;
        }
        let level = numbered_level(inner).unwrap_or_else(|| if is_caps(inner) { 2 } else { 3 });
        return Some(format!("{} {}", "#".repeat(level), inner));
    }
    // A table or figure caption on its own line acts as the table's title.
    // Table titles run longer than figure captions ("Table 4: Selected*
    // Adverse Reactions Occurring in ≥10% of Patients Receiving …"); a lone
    // footnote mark is part of the title, paired asterisks are emphasis.
    let table_title = ["Table", "TABLE", "Exhibit", "EXHIBIT"]
        .iter()
        .any(|k| t.starts_with(k));
    let lone_marks = t.matches('*').count() == 1 && !t.starts_with('*');
    if RE_CAPTION.is_match(t)
        && title_like(t, if table_title { 20 } else { 12 })
        && !t.contains('|')
        && (lone_marks || !t.contains('*'))
    {
        return Some(format!("### {t}"));
    }
    // A numbered title, not bold.
    let c = RE_NUMBERED.captures(t)?;
    let title = c.name("title").map(|m| m.as_str().trim()).unwrap_or("");
    let num = c.name("num").map(|m| m.as_str()).unwrap_or("");
    // "1. foo" and bare numbers are list items or values unless a real title
    // follows; a bare "ARTICLE IV" is a heading on its own.
    let keyword = num
        .chars()
        .next()
        .is_some_and(|ch| ch.is_alphabetic() || ch == '§');
    if title.is_empty() {
        return keyword.then(|| format!("{} {}", "#".repeat(numbered_level(num).unwrap_or(2)), t));
    }
    // "Granting of rights", "2.4 GHz Transmitter": a capital or a figure
    // first, and a capitalized word somewhere.
    // A keyword-numbered title may end in one period ("Section 2.14
    // Defaulting Lenders.", "ARTICLE V. COVENANTS.").
    let title = if keyword && title.ends_with('.') && !title.ends_with("..") {
        &title[..title.len() - 1]
    } else if title.ends_with(':') && is_caps(title) {
        // Specification outlines: "1.01 SECTION INCLUDES:".
        &title[..title.len() - 1]
    } else {
        title
    };
    let first_upper = title
        .chars()
        .next()
        .is_some_and(|ch| ch.is_uppercase() || ch.is_ascii_digit())
        && title
            .split_whitespace()
            .any(|w| w.chars().next().is_some_and(char::is_uppercase));
    if !first_upper || !title_like(title, 10) || title.contains(['*', '_']) {
        return None;
    }
    // Plain numbered lines need more than one word unless they are keyword
    // headings ("Item 1A. Risk Factors" yes, "3 Results" maybe a value row).
    // A one-word title after a bare number must be a plain capitalised word
    // ("1 INTRODUCTION"), not a name ("2 VNGRS-AI" is an affiliation mark).
    if !keyword
        && title.split_whitespace().count() < 2
        && (!is_caps(title) || !title.chars().all(char::is_alphabetic))
    {
        return None;
    }
    let level = numbered_level(num).unwrap_or(2);
    Some(format!("{} {}", "#".repeat(level), t))
}

/// Short, has letters, and not a sentence or a label.
fn title_like(s: &str, max_words: usize) -> bool {
    let words = s.split_whitespace().count();
    if words == 0 || words > max_words || s.chars().count() > 120 {
        return false;
    }
    if !s.chars().any(|c| c.is_alphabetic()) {
        return false;
    }
    if s.chars().next().is_some_and(|c| c.is_lowercase()) {
        return false;
    }
    // Sentences and labels end in punctuation; a title does not ("Item 1A."
    // is handled by the numbering, which ends before the title).
    if s.ends_with(['.', ',', ';', ':', '!', '?', '(', '-', '—', '–']) && !s.ends_with("...") {
        return false;
    }
    // Two sentences, an equation, a URL or an e-mail address. Numbered
    // titles and captions carry their own periods and may name a value
    // ("R11 Register (Offset = Bh)").
    let labelled = RE_NUMBERED.is_match(s) || RE_CAPTION.is_match(s);
    if (s.contains(". ") || s.contains('=')) && !labelled || s.contains("://") || s.contains('@') {
        return false;
    }
    // A run of digits and symbols with a word or two: a table value, a date.
    let letters = s.chars().filter(|c| c.is_alphabetic()).count();
    letters * 2 >= s.chars().filter(|c| !c.is_whitespace()).count()
}

fn is_caps(s: &str) -> bool {
    let letters: Vec<char> = s.chars().filter(|c| c.is_alphabetic()).collect();
    letters.len() >= 3 && letters.iter().all(|c| !c.is_lowercase())
}

/// Heading level implied by a leading section number, if there is one.
fn numbered_level(s: &str) -> Option<usize> {
    let c = RE_NUMBERED.captures(s)?;
    let num = c.name("num")?.as_str();
    let first = num.chars().next()?;
    if first.is_alphabetic() || first == '§' {
        let lower = num.to_lowercase();
        if lower.starts_with("section") || first == '§' {
            let dots = num.matches('.').count() - usize::from(num.ends_with('.'));
            return Some((3 + dots.saturating_sub(1)).min(6));
        }
        return Some(2);
    }
    let dots = num.matches('.').count() - usize::from(num.ends_with('.'));
    Some((2 + dots).min(6))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(md: &str) -> String {
        promote_headings(md)
    }

    #[test]
    fn bold_titles_become_headings() {
        assert_eq!(
            p("Intro.\n\n**The Scorecard in 2023**\n\nBody."),
            "Intro.\n\n### The Scorecard in 2023\n\nBody."
        );
        assert_eq!(
            p("**OPERATING RESULTS**\n\nBody."),
            "## OPERATING RESULTS\n\nBody."
        );
        assert_eq!(
            p("**Item 1A. Risk Factors**\n\nBody."),
            "## Item 1A. Risk Factors\n\nBody."
        );
        assert_eq!(
            p("**1.1 Granting of rights**"),
            "### 1.1 Granting of rights"
        );
    }

    #[test]
    fn numbered_titles_become_headings() {
        assert_eq!(
            p("1.1 Granting of rights\n\nParty B hereby…"),
            "### 1.1 Granting of rights\n\nParty B hereby…"
        );
        assert_eq!(
            p("Section 2.14 Defaulting Lenders\n\nx"),
            "### Section 2.14 Defaulting Lenders\n\nx"
        );
        assert_eq!(p("ARTICLE IV\n\nx"), "## ARTICLE IV\n\nx");
        assert_eq!(p("1 INTRODUCTION\n\nx"), "## 1 INTRODUCTION\n\nx");
        assert_eq!(
            p("4.6.2 2.4 GHz Transmitter"),
            "#### 4.6.2 2.4 GHz Transmitter"
        );
    }

    #[test]
    fn captions_spec_outlines_and_run_in_titles() {
        assert_eq!(
            p("**3.11 R11 Register (Offset = Bh) [Reset = 34h]**"),
            "### 3.11 R11 Register (Offset = Bh) [Reset = 34h]"
        );
        assert_eq!(
            p("**Table 3-13. R11 Register Field Descriptions**"),
            "### Table 3-13. R11 Register Field Descriptions"
        );
        assert_eq!(
            p("Table 4-1. Memory and Peripheral Mapping"),
            "### Table 4-1. Memory and Peripheral Mapping"
        );
        assert_eq!(
            p(
                "Table 4: Selected* Adverse Reactions Occurring in ≥10% of Patients Receiving KEYTRUDA in KEYNOTE-006"
            ),
            "### Table 4: Selected* Adverse Reactions Occurring in ≥10% of Patients Receiving KEYTRUDA in KEYNOTE-006"
        );
        // Emphasis inside a caption is not a footnote mark.
        assert_eq!(
            p("Figure 23: *mortality* = .030"),
            "Figure 23: *mortality* = .030"
        );
        // A long figure caption is a description, not a title.
        assert_eq!(
            p(
                "Figure 2: Validation loss of every model across the training steps for each of the four learning rate schedules"
            ),
            "Figure 2: Validation loss of every model across the training steps for each of the four learning rate schedules"
        );
        assert_eq!(
            p("1.01 SECTION INCLUDES:\n\nA."),
            "### 1.01 SECTION INCLUDES:\n\nA."
        );
        assert_eq!(
            p(
                "**INDICATIONS AND USAGE** KEYTRUDA is a programmed death receptor-1 blocking antibody."
            ),
            "## INDICATIONS AND USAGE\n\nKEYTRUDA is a programmed death receptor-1 blocking antibody."
        );
        // A one-word bold lead-in, or one followed by punctuation, stays.
        assert_eq!(
            p("**WHEREAS** the Parties agree"),
            "**WHEREAS** the Parties agree"
        );
        assert_eq!(p("**NOTICE:** This form"), "**NOTICE:** This form");
        // A long figure caption is a caption, not a heading.
        let cap = "Figure 2: (left) Scaled Dot-Product Attention. (right) Multi-Head Attention consists of several attention layers running in parallel.";
        assert_eq!(p(cap), cap);
    }

    #[test]
    fn sentences_labels_values_and_structure_are_left_alone() {
        for keep in [
            "**This is an important sentence that ends.**",
            "**Note:**",
            "**$1,245.6**",
            "**see below**",
            "1. The Company shall pay the fee.",
            "2023 Annual Report", // a year, not a section number
            "| **Bold** | cell |",
            "- **Bold item**",
            "**a = b + c**",
            "12 345",
            "1 Year",
            "2 VNGRS-AI",
        ] {
            assert_eq!(p(keep), keep, "should not change: {keep}");
        }
        // Not standalone: part of a paragraph.
        let para = "**Bold run-in.**\ncontinues the paragraph";
        assert_eq!(p(para), para);
        // Inside code.
        let code = "```\n**Bold**\n```";
        assert_eq!(p(code), code);
    }
}
