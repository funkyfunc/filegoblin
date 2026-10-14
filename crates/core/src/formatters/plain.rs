use regex::Regex;
use std::sync::LazyLock;

/// Converts markdown text to clean, human-readable plain text:
/// - Pipe tables rendered as clean space-aligned columns
/// - Links stripped to anchor text only
/// - Headings stripped of `#` prefixes
/// - Math stripped of delimiter tokens (`$x$` -> `x`, `$$y$$` -> `y`)
/// - Code fences, blockquotes, emphasis, and HTML tags stripped
pub fn markdown_to_plain(md: &str) -> String {
    let mut out = Vec::new();
    let lines: Vec<&str> = md.lines().collect();
    let mut i = 0;

    while i < lines.len() {
        let line = lines[i];
        let trimmed = line.trim();

        // 1. Preserve breadcrumb comment if present
        if trimmed.starts_with("<!-- Context:") && trimmed.ends_with("-->") {
            out.push(line.to_string());
            i += 1;
            continue;
        }

        // 2. Fenced code block: preserve code content, omit fences
        if trimmed.starts_with("```") {
            i += 1;
            while i < lines.len() && !lines[i].trim().starts_with("```") {
                out.push(lines[i].to_string());
                i += 1;
            }
            if i < lines.len() {
                i += 1; // skip closing fence
            }
            continue;
        }

        // 3. Display math block: `$$ ... $$`
        if let Some(rest) = trimmed.strip_prefix("$$") {
            if rest.contains("$$") {
                let math = trimmed.trim_matches('$').trim();
                out.push(math.to_string());
                i += 1;
                continue;
            } else {
                let mut math_lines = Vec::new();
                let first = trimmed.trim_start_matches('$').trim();
                if !first.is_empty() {
                    math_lines.push(first);
                }
                i += 1;
                while i < lines.len() && !lines[i].trim().starts_with("$$") {
                    math_lines.push(lines[i].trim());
                    i += 1;
                }
                if i < lines.len() {
                    let last = lines[i].trim().trim_end_matches('$').trim();
                    if !last.is_empty() {
                        math_lines.push(last);
                    }
                    i += 1;
                }
                out.push(math_lines.join(" "));
                continue;
            }
        }

        // 4. Pipe table: render as clean space-aligned columns
        if is_table_line(trimmed) && i + 1 < lines.len() && is_table_separator(lines[i + 1].trim())
        {
            let mut table_rows = Vec::new();
            table_rows.push(parse_table_cells(trimmed));
            i += 2; // skip header and separator
            while i < lines.len() && is_table_line(lines[i].trim()) {
                table_rows.push(parse_table_cells(lines[i].trim()));
                i += 1;
            }
            let formatted_table = format_plain_table(&table_rows);
            out.push(formatted_table);
            continue;
        }

        // 5. Thematic break: `---`, `***`, `___`
        if is_thematic_break(trimmed) {
            out.push(String::new());
            i += 1;
            continue;
        }

        // 6. Regular line cleaning
        let cleaned = clean_plain_line(line);
        out.push(cleaned);
        i += 1;
    }

    let joined = out.join("\n");
    collapse_blank_lines(&joined)
}

fn is_table_line(trimmed: &str) -> bool {
    trimmed.starts_with('|') && trimmed.ends_with('|') && trimmed.contains('|')
}

fn is_table_separator(trimmed: &str) -> bool {
    if !is_table_line(trimmed) {
        return false;
    }
    let inner = &trimmed[1..trimmed.len() - 1];
    !inner.is_empty()
        && inner
            .chars()
            .all(|c| c == '-' || c == ':' || c == '|' || c.is_whitespace())
}

fn parse_table_cells(line: &str) -> Vec<String> {
    let trimmed = line.trim();
    let inner = if trimmed.starts_with('|') && trimmed.ends_with('|') && trimmed.len() >= 2 {
        &trimmed[1..trimmed.len() - 1]
    } else {
        trimmed
    };
    inner
        .split('|')
        .map(|c| clean_inline_formatting(c.trim()))
        .collect()
}

fn format_plain_table(rows: &[Vec<String>]) -> String {
    if rows.is_empty() {
        return String::new();
    }
    let num_cols = rows.iter().map(|r| r.len()).max().unwrap_or(0);
    if num_cols == 0 {
        return String::new();
    }

    let mut col_widths = vec![0; num_cols];
    for row in rows {
        for (j, cell) in row.iter().enumerate() {
            col_widths[j] = col_widths[j].max(cell.chars().count());
        }
    }

    let mut lines = Vec::new();
    for row in rows {
        let mut parts = Vec::new();
        for (j, width) in col_widths.iter().enumerate() {
            let cell = row.get(j).map(|s| s.as_str()).unwrap_or("");
            let char_count = cell.chars().count();
            let pad = if *width > char_count {
                " ".repeat(*width - char_count)
            } else {
                String::new()
            };
            parts.push(format!("{}{}", cell, pad));
        }
        lines.push(parts.join("   ").trim_end().to_string());
    }
    lines.join("\n")
}

fn is_thematic_break(trimmed: &str) -> bool {
    let t: String = trimmed.chars().filter(|c| !c.is_whitespace()).collect();
    t.len() >= 3
        && (t.chars().all(|c| c == '-')
            || t.chars().all(|c| c == '*')
            || t.chars().all(|c| c == '_'))
}

fn clean_plain_line(line: &str) -> String {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return String::new();
    }

    // 1. Headings: strip leading `#{1,6}\s+`
    let mut stripped = if let Some(rest) = trimmed.strip_prefix('#') {
        let mut level = 1;
        let mut rest_bytes = rest;
        while let Some(r) = rest_bytes.strip_prefix('#') {
            level += 1;
            rest_bytes = r;
            if level >= 6 {
                break;
            }
        }
        if rest_bytes.starts_with(' ') || rest_bytes.starts_with('\t') {
            rest_bytes.trim().trim_end_matches('#').trim()
        } else {
            trimmed
        }
    } else {
        trimmed
    };

    // 2. Blockquotes: strip `^>\s*`
    while let Some(rest) = stripped.strip_prefix('>') {
        stripped = rest.trim();
    }

    clean_inline_formatting(stripped)
}

fn clean_inline_formatting(text: &str) -> String {
    static IMAGE_RE: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"!\[([^\]]*)\]\([^\)]+\)").unwrap());
    static LINK_RE: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"\[([^\]]+)\]\([^\)]+\)").unwrap());
    static AUTOLINK_RE: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"<(https?://[^>]+)>").unwrap());
    static EMAIL_RE: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"<([^>@\s]+@[^>@\s]+)>").unwrap());
    static DISPLAY_MATH_RE: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"\$\$\s*([\s\S]*?)\s*\$\$").unwrap());
    static INLINE_MATH_RE: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"\$([^\s\$](?:[^\$]*?[^\s\$])?)\$").unwrap());
    static INLINE_CODE_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"`+([^`]+)`+").unwrap());
    static BOLD_ITALIC_STAR_RE: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"\*\*\*([^\*]+)\*\*\*").unwrap());
    static BOLD_STAR_RE: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"\*\*([^\*]+)\*\*").unwrap());
    static ITALIC_STAR_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\*([^\*]+)\*").unwrap());
    static BOLD_UNDER_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"__([^_]+)__").unwrap());
    static ITALIC_UNDER_RE: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"(^|\s)_([^_]+)_($|\s)").unwrap());
    static STRIKE_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"~~([^~]+)~~").unwrap());
    static HTML_TAG_RE: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"</?[a-zA-Z][^>]*>").unwrap());
    static HTML_COMMENT_RE: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"<!--[\s\S]*?-->").unwrap());

    let mut res = text.to_string();
    res = HTML_COMMENT_RE.replace_all(&res, "").to_string();
    res = DISPLAY_MATH_RE.replace_all(&res, "$1").to_string();
    res = INLINE_MATH_RE.replace_all(&res, "$1").to_string();
    res = IMAGE_RE.replace_all(&res, "$1").to_string();
    res = LINK_RE.replace_all(&res, "$1").to_string();
    res = AUTOLINK_RE.replace_all(&res, "$1").to_string();
    res = EMAIL_RE.replace_all(&res, "$1").to_string();
    res = INLINE_CODE_RE.replace_all(&res, "$1").to_string();
    res = BOLD_ITALIC_STAR_RE.replace_all(&res, "$1").to_string();
    res = BOLD_STAR_RE.replace_all(&res, "$1").to_string();
    res = ITALIC_STAR_RE.replace_all(&res, "$1").to_string();
    res = BOLD_UNDER_RE.replace_all(&res, "$1").to_string();
    res = ITALIC_UNDER_RE.replace_all(&res, "$1$2$3").to_string();
    res = STRIKE_RE.replace_all(&res, "$1").to_string();
    res = HTML_TAG_RE.replace_all(&res, "").to_string();

    res
}

fn collapse_blank_lines(text: &str) -> String {
    let mut out = Vec::new();
    let mut blank_count = 0;
    for line in text.lines() {
        if line.trim().is_empty() {
            blank_count += 1;
            if blank_count <= 1 {
                out.push("");
            }
        } else {
            blank_count = 0;
            out.push(line);
        }
    }
    out.join("\n").trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_markdown_to_plain_headings_and_styles() {
        let md =
            "# Title\n\n## Subtitle\n\nThis is **bold**, *italic*, and `code` with ~~strike~~.";
        let plain = markdown_to_plain(md);
        assert_eq!(
            plain,
            "Title\n\nSubtitle\n\nThis is bold, italic, and code with strike."
        );
    }

    #[test]
    fn test_markdown_to_plain_links_and_math() {
        let md = "See [our website](https://example.com) for $x = y^2$ formulas.\n\n$$\\int_0^1 f(x) dx$$";
        let plain = markdown_to_plain(md);
        assert!(plain.contains("See our website for x = y^2 formulas."));
        assert!(plain.contains("\\int_0^1 f(x) dx"));
    }

    #[test]
    fn test_markdown_to_plain_table() {
        let md = "| Metric | Value |\n|---|---|\n| Accuracy | 95.5% |\n| Latency | 12ms |";
        let plain = markdown_to_plain(md);
        assert!(!plain.contains('|'));
        assert!(!plain.contains("---"));
        assert!(plain.contains("Metric     Value"));
        assert!(plain.contains("Accuracy   95.5%"));
        assert!(plain.contains("Latency    12ms"));
    }

    #[test]
    fn test_markdown_to_plain_code_block() {
        let md = "Here is code:\n\n```rust\nfn main() {\n    println!(\"Hello\");\n}\n```\n\nEnd.";
        let plain = markdown_to_plain(md);
        assert!(!plain.contains("```"));
        assert!(plain.contains("fn main() {"));
        assert!(plain.contains("println!(\"Hello\");"));
    }
}
