//! Inline mathematics assembled around what the formula recognizer read.
//!
//! The layout model boxes inline formulas tightly, and often too tightly: an
//! upright operator name in front of a formula ("det", "log") is left in
//! the prose, and one expression is cut into two formulas at a comma or a
//! quantifier. On scanned pages it misses inline formulas altogether, and
//! OCR writes them as prose ("r = a (1 + cos θ)"). These passes put such
//! pieces back together. They work on the page's markdown and never change
//! a word of prose: they only move formula delimiters, and spell the glue
//! they pull into a formula the way LaTeX writes it.

use std::sync::LazyLock;

/// Operator names with their own LaTeX command.
const COMMANDS: [&str; 22] = [
    "det", "dim", "ln", "log", "exp", "sin", "cos", "tan", "sinh", "cosh", "tanh", "max", "min",
    "sup", "inf", "lim", "ker", "deg", "gcd", "arg", "Pr", "hom",
];

static OPERATOR_BEFORE: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(
        r"(?:^|(?P<pre>[^A-Za-z\\$]))(?P<op>det|dim|ln|log|exp|sin|cos|tan|sinh|cosh|tanh|max|min|sup|inf|lim|ker|deg|gcd|arg|Pr|hom|tr|Tr|rank|span|diag|sgn|supp|vol|div|Hom|End|Aut|Spec|Im|Re|id|ord|codim|coker|card|dist|diam|grad|curl)[ \t]+\$(?P<next>[^$])",
    )
    .expect("valid regex")
});

/// A dollar sign the prose escaped (`\$`) while these passes run, so it is
/// never taken for a formula delimiter.
const ESCAPED: char = '\u{E000}';

fn mask(md: &str) -> String {
    md.replace("\\$", &ESCAPED.to_string())
}

fn unmask(md: &str) -> String {
    md.replace(ESCAPED, "\\$")
}

/// An operator name set in the prose right before an inline formula belongs
/// to it: "det $A$" is `$\det A$`. Only for a page whose prose dollars are
/// escaped, so every `$` left is a formula's.
pub(crate) fn absorb_operator_names(md: &str) -> String {
    let md = mask(md);
    let out = OPERATOR_BEFORE
        .replace_all(&md, |c: &regex::Captures| {
            let op = &c["op"];
            let cmd = if COMMANDS.contains(&op) {
                format!("\\{op}")
            } else {
                format!("\\operatorname{{{op}}}")
            };
            let pre = c.name("pre").map_or("", |m| m.as_str());
            format!("{pre}${cmd} {}", &c["next"])
        })
        .into_owned();
    unmask(&out)
}

/// Two inline formulas on one line with nothing between them but a little
/// math glue (`$a$, $b$`, `$x$ ∈ $X$`, `$f$ = $g$`) are one expression. A
/// letter between them is a word ("$H$ a $p$-subgroup", Spanish "$x$ y
/// $y$"), not glue. Only for a page whose prose dollars are escaped.
pub(crate) fn join_split_inline(md: &str) -> String {
    let md = mask(md);
    unmask(&join_split(&md))
}

fn join_split(md: &str) -> String {
    static SEG: LazyLock<regex::Regex> =
        LazyLock::new(|| regex::Regex::new(r"(?s)\$\$.+?\$\$|\$[^$\n]+?\$").expect("valid regex"));
    // At most four glue symbols in between.
    static GLUE: LazyLock<regex::Regex> =
        LazyLock::new(|| regex::Regex::new(r"^[ ,;:=+\-−∀∃∈⊂≤≥→×·≠∞]{0,4}$").expect("valid regex"));
    let spell = |glue: &str| -> String {
        glue.chars()
            .map(|c| match c {
                '∀' => "\\forall ".to_string(),
                '∃' => "\\exists ".to_string(),
                '∈' => "\\in ".to_string(),
                '⊂' => "\\subset ".to_string(),
                '≤' => "\\leq ".to_string(),
                '≥' => "\\geq ".to_string(),
                '→' => "\\to ".to_string(),
                '×' => "\\times ".to_string(),
                '·' => "\\cdot ".to_string(),
                '≠' => "\\neq ".to_string(),
                '∞' => "\\infty ".to_string(),
                '−' => "-".to_string(),
                c => c.to_string(),
            })
            .collect::<String>()
            .trim()
            .to_string()
    };
    let mut out = String::with_capacity(md.len());
    let mut last = 0;
    let mut pending: Option<String> = None;
    for m in SEG.find_iter(md) {
        let between = &md[last..m.start()];
        let seg = m.as_str();
        let inline = !seg.starts_with("$$");
        match pending.take() {
            Some(body) if inline && !between.contains('\n') && GLUE.is_match(between) => {
                let glue = spell(between);
                let inner = &seg[1..seg.len() - 1];
                pending = Some(if glue.is_empty() {
                    format!("{body} {inner}")
                } else {
                    format!("{body} {glue} {inner}")
                });
            }
            Some(body) => {
                out.push('$');
                out.push_str(&body);
                out.push('$');
                out.push_str(between);
                if inline {
                    pending = Some(seg[1..seg.len() - 1].to_string());
                } else {
                    out.push_str(seg);
                }
            }
            None => {
                out.push_str(between);
                if inline {
                    pending = Some(seg[1..seg.len() - 1].to_string());
                } else {
                    out.push_str(seg);
                }
            }
        }
        last = m.end();
    }
    if let Some(body) = pending {
        out.push('$');
        out.push_str(&body);
        out.push('$');
    }
    out.push_str(&md[last..]);
    out
}

/// On a page read by OCR, a run of three or more math-like tokens that
/// holds a relation ("r = a (1 + cos θ)", "5 x 7 = 35") is a formula the
/// layout model did not box, and goes between `$` signs. Words of more
/// than two letters (other than function names) end a run, so prose never
/// becomes a formula.
/// Lines that already hold a dollar sign (a formula, or a price) and code
/// blocks are left alone.
pub(crate) fn wrap_math_runs(md: &str) -> String {
    static CORE: LazyLock<regex::Regex> = LazyLock::new(|| {
        regex::Regex::new(
            r"^(?:[A-Za-z]{1,2}|\d+(?:\.\d+)?|sin|cos|tan|cot|sec|log|[θφπαβ]|[=+\-−—×·/<>≤≥]|[A-Za-z]?\^\d)$",
        )
        .expect("valid regex")
    });
    const STOP: [&str; 33] = [
        "if", "is", "in", "of", "to", "as", "at", "be", "by", "or", "on", "an", "we", "it", "so",
        "no", "do", "go", "he", "me", "my", "up", "us", "and", "the", "let", "then", "hence",
        "where", "when", "for", "A", "I",
    ];
    let mathy = |tok: &str| {
        let core = tok.trim_matches(|c| matches!(c, '(' | ')' | '[' | ']' | ',' | ';' | '.' | ':'));
        if core.is_empty() {
            return tok == "(" || tok == ")";
        }
        !STOP.contains(&core)
            && !STOP.contains(&core.to_lowercase().as_str())
            && CORE.is_match(core)
    };
    let latex = |tok: &str| {
        let mut t = tok.to_string();
        for (from, to) in [
            ("sin", "\\sin "),
            ("cos", "\\cos "),
            ("tan", "\\tan "),
            ("log", "\\log "),
            ("cot", "\\cot "),
            ("sec", "\\sec "),
            ("θ", "\\theta "),
            ("φ", "\\phi "),
            ("π", "\\pi "),
            ("α", "\\alpha "),
            ("β", "\\beta "),
            ("×", "\\times "),
            ("·", "\\cdot "),
            ("≤", "\\leq "),
            ("≥", "\\geq "),
            ("−", "-"),
            ("—", "-"),
        ] {
            t = t.replace(from, to);
        }
        t.trim_end().to_string()
    };
    static LIST_MARKER: LazyLock<regex::Regex> =
        LazyLock::new(|| regex::Regex::new(r"^(?:[-*+•]|\d{1,3}[.)])$").expect("valid regex"));
    let mut lines = Vec::new();
    let mut fenced = false;
    for line in md.split('\n') {
        let head = line.trim_start();
        if head.starts_with("```") || head.starts_with("~~~") {
            fenced = !fenced;
        }
        // Structural lines are not prose.
        if fenced || head.starts_with(['|', '#', '>', '`', '~']) || line.contains('$') {
            lines.push(line.to_string());
            continue;
        }
        let toks: Vec<&str> = line.split(' ').collect();
        let mut res: Vec<String> = Vec::new();
        let mut i = 0;
        // A list item's marker is not part of a formula.
        while i < toks.len() && toks[i].is_empty() {
            res.push(String::new());
            i += 1;
        }
        if i < toks.len() && LIST_MARKER.is_match(toks[i]) {
            res.push(toks[i].to_string());
            i += 1;
        }
        while i < toks.len() {
            let mut j = i;
            while j < toks.len() && mathy(toks[j]) {
                j += 1;
            }
            let run = &toks[i..j];
            let relation = run.iter().any(|t| t.contains(['=', '<', '>', '≤', '≥']));
            let content = run.iter().any(|t| t.chars().any(|c| c.is_alphanumeric()));
            if j - i >= 3 && relation && content {
                // Sentence punctuation stays outside the formula.
                let mut body: Vec<&str> = run.to_vec();
                let mut trail = String::new();
                if let Some(last) = body.pop() {
                    let kept = last.trim_end_matches([',', ';', '.', ':']);
                    trail = last[kept.len()..].to_string();
                    if !kept.is_empty() {
                        body.push(kept);
                    }
                }
                let spelled: Vec<String> = body
                    .iter()
                    .enumerate()
                    .map(|(k, t)| {
                        // "5 x 7": a multiplication sign read as a letter.
                        let digit =
                            |s: &str| s.chars().all(|c| c.is_ascii_digit()) && !s.is_empty();
                        if *t == "x"
                            && k > 0
                            && k + 1 < body.len()
                            && digit(body[k - 1])
                            && digit(body[k + 1])
                        {
                            "\\times".to_string()
                        } else {
                            latex(t)
                        }
                    })
                    .collect();
                res.push(format!("${}${trail}", spelled.join(" ")));
                i = j;
            } else if j > i {
                res.extend(run.iter().map(|t| t.to_string()));
                i = j;
            } else {
                res.push(toks[i].to_string());
                i += 1;
            }
        }
        lines.push(res.join(" "));
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn operator_names_join_their_formula() {
        assert_eq!(absorb_operator_names("so det $A$ is"), "so $\\det A$ is");
        assert_eq!(
            absorb_operator_names("the rank $M$"),
            "the $\\operatorname{rank} M$"
        );
        // Part of a word, or already inside a formula: unchanged.
        assert_eq!(absorb_operator_names("model $x$"), "model $x$");
        assert_eq!(absorb_operator_names("$\\log$ $x$"), "$\\log$ $x$");
        assert_eq!(
            absorb_operator_names("a gift card \\$25, max \\$10"),
            "a gift card \\$25, max \\$10"
        );
    }

    #[test]
    fn formulas_split_at_math_glue_are_joined() {
        assert_eq!(join_split_inline("for $x$ ∈ $X$ we"), "for $x \\in X$ we");
        assert_eq!(join_split_inline("$a$, $b$ and $c$"), "$a , b$ and $c$");
        // Real words between formulas keep them apart.
        assert_eq!(join_split_inline("$a$ and $b$"), "$a$ and $b$");
        assert_eq!(join_split_inline("$$x$$ $y$"), "$$x$$ $y$");
        // One-letter words, and escaped dollars, are prose.
        assert_eq!(join_split_inline("$H$ a $p$-group"), "$H$ a $p$-group");
        assert_eq!(join_split_inline("$x$ y $y$"), "$x$ y $y$");
        assert_eq!(
            join_split_inline("costs \\$5, $x$ and \\$6, $y$"),
            "costs \\$5, $x$ and \\$6, $y$"
        );
    }

    #[test]
    fn math_written_as_ocr_prose_becomes_a_formula() {
        assert_eq!(
            wrap_math_runs("the curve r = a (1 + cos θ) is shown"),
            "the curve $r = a (1 + \\cos \\theta )$ is shown"
        );
        assert_eq!(wrap_math_runs("so 5 x 7 = 35."), "so $5 \\times 7 = 35$.");
        // Prose with a relation sign but no run of math tokens stays prose.
        assert_eq!(
            wrap_math_runs("Revenue = the money a company makes"),
            "Revenue = the money a company makes"
        );
        assert_eq!(wrap_math_runs("| a = b | c |"), "| a = b | c |");
        // List markers stay outside; code and lines with dollars are left alone.
        assert_eq!(
            wrap_math_runs("- n = 100 participants"),
            "- $n = 100$ participants"
        );
        assert_eq!(wrap_math_runs("1. x = 5 and so"), "1. $x = 5$ and so");
        assert_eq!(
            wrap_math_runs("```\nlet x = y + 1\n```"),
            "```\nlet x = y + 1\n```"
        );
        assert_eq!(wrap_math_runs("pay $5 if x = y + 1"), "pay $5 if x = y + 1");
    }
}
