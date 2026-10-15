//! DOM clean-up that runs before HTML is converted to markdown.
//!
//! html2md reads tags, not CSS, and copies `<sup>`/`<sub>` through as raw
//! HTML. Filings written by document-assembly tools (SEC EDGAR, most legal
//! and financial HTML) carry their structure almost entirely in CSS and their
//! page layout in the HTML itself. This pass:
//!
//! * removes the EDGAR submission wrapper (`<DOCUMENT><TYPE>…<FILENAME>…`)
//!   and elements hidden with `display:none` (inline-XBRL fact headers);
//! * removes page furniture: the running footers, page numbers and
//!   "Table of Contents" back-links that sit at page breaks. They are found
//!   by recurrence at the breaks, the same evidence the PDF path uses, never
//!   by matching known strings;
//! * unwraps layout tables (a one-row table around a paragraph, or rows of
//!   "marker | text" that typeset a numbered or bulleted list) into
//!   paragraphs, so prose is not written as pipe tables;
//! * turns CSS bold (`font-weight: bold | 600–900` on a `span` or `font`)
//!   into `<b>`, so headings set that way survive as `**…**`;
//! * makes short blocks set clearly larger than the body text headings
//!   (`<h2>`–`<h4>`, by size rank), the way the PDF path reads font sizes;
//! * writes `<sup>` footnote markers as `[^n]`, and other superscripts and
//!   subscripts as plain text, instead of raw tags with style attributes.
//!
//! An ordinary page with none of this comes back unchanged.

use ego_tree::NodeId;
use scraper::node::{Node, Text};
use scraper::{Html, Selector};
use std::collections::HashMap;
use std::sync::LazyLock;

static RE_PAGE_NUMBER: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(r"(?i)^(?:page\s+)?[a-z]{0,3}[-–—]?\s*(?:\d{1,4}|[ivxlcdm]{1,7})\s*[-–—]?$")
        .unwrap()
});
static RE_FOOTNOTE_MARK: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r"^\(?([0-9]{1,3}|[A-Za-z]|[*†‡§¶]{1,3})\)?$").unwrap());
static RE_DIGITS: LazyLock<regex::Regex> = LazyLock::new(|| regex::Regex::new(r"\d+").unwrap());

/// Whole-document clean-up, before content extraction: the EDGAR wrapper,
/// hidden elements and page furniture.
pub(crate) fn prepare_document(content: &str) -> String {
    let content = strip_edgar_wrapper(content);
    let mut doc = Html::parse_document(content);
    let mut changed = false;
    changed |= remove_hidden(&mut doc);
    changed |= remove_page_furniture(&mut doc);
    if changed {
        doc.html()
    } else {
        content.to_string()
    }
}

/// Per-fragment clean-up, after content extraction and before html2md:
/// layout tables, large type, CSS bold, superscripts. (Unwrapping layout
/// tables before extraction would change what the extractor sees: a filing
/// laid out in tables stops looking tabular and gets cut to one block.)
pub(crate) fn prepare_fragment(fragment: &str) -> String {
    let mut doc = Html::parse_fragment(fragment);
    let mut changed = false;
    changed |= unwrap_layout_tables(&mut doc);
    changed |= promote_large_type(&mut doc);
    changed |= normalize_inline(&mut doc);
    if changed {
        doc.root_element().inner_html()
    } else {
        fragment.to_string()
    }
}

/// Both passes, for a whole document converted in one piece.
#[cfg(test)]
pub(crate) fn prepare(content: &str) -> String {
    prepare_fragment(&prepare_document(content))
}

/// EDGAR's full-submission files open with an SGML header around the HTML:
/// `<DOCUMENT><TYPE>S-1/A<SEQUENCE>1<FILENAME>d752207ds1a.htm<TEXT>`.
fn strip_edgar_wrapper(content: &str) -> &str {
    let head = &content[..content.len().min(4096)];
    let upper = head.to_ascii_uppercase();
    if !(upper.contains("<TYPE>") && upper.contains("<FILENAME>")) {
        return content;
    }
    let Some(start) = upper.find("<TEXT>") else {
        return content;
    };
    let body = &content[start + "<TEXT>".len()..];
    match body.to_ascii_uppercase().rfind("</TEXT>") {
        Some(end) => &body[..end],
        None => body,
    }
}

fn style_of(node: &Node) -> Option<String> {
    match node {
        Node::Element(e) => e
            .attr("style")
            .map(|s| s.to_ascii_lowercase().replace([' ', '\t', '\n'], "")),
        _ => None,
    }
}

fn remove_hidden(doc: &mut Html) -> bool {
    let hidden: Vec<NodeId> = doc
        .tree
        .root()
        .descendants()
        .filter(|n| style_of(n.value()).is_some_and(|s| s.contains("display:none")))
        .map(|n| n.id())
        .collect();
    for id in &hidden {
        if let Some(mut n) = doc.tree.get_mut(*id) {
            n.detach();
        }
    }
    !hidden.is_empty()
}

fn is_page_break(node: &Node) -> bool {
    style_of(node).is_some_and(|s| {
        s.contains("page-break-after:always")
            || s.contains("page-break-before:always")
            || s.contains("break-after:page")
            || s.contains("break-before:page")
    })
}

fn element_name(node: &Node) -> Option<&str> {
    match node {
        Node::Element(e) => Some(e.name.local.as_ref()),
        _ => None,
    }
}

/// Running footers, page numbers and back-links at page breaks.
///
/// For each break, the two text runs before it and the three after it are
/// candidates. A candidate is furniture when it is a bare page number
/// ("12", "-5-", "F-3", "ii"), or when the same text sits at the same side of
/// the breaks again and again: exactly the same at a fifth of them or more,
/// or the same up to its digits ("Apple Inc. | 2025 Form 10-K | 5") at half
/// of them or more. Section headings that happen to follow a break
/// ("Section 2.14" on 25% of the pages of a credit agreement) fall short.
fn remove_page_furniture(doc: &mut Html) -> bool {
    let order: Vec<NodeId> = doc.tree.root().descendants().map(|n| n.id()).collect();
    let mut breaks: Vec<usize> = Vec::new();
    // (position in `order`, node, text)
    let mut texts: Vec<(usize, NodeId, String)> = Vec::new();
    for (i, id) in order.iter().enumerate() {
        let node = doc.tree.get(*id).unwrap();
        if is_page_break(node.value()) {
            breaks.push(i);
        }
        if let Node::Text(t) = node.value() {
            let s = t.text.trim();
            if s.is_empty() || s.chars().all(|c| c == '\u{a0}' || c.is_whitespace()) {
                continue;
            }
            let in_code = node.ancestors().any(|a| {
                matches!(
                    element_name(a.value()),
                    Some("script" | "style" | "pre" | "code")
                )
            });
            if !in_code {
                texts.push((i, *id, collapse(s)));
            }
        }
    }
    let n = breaks.len();
    if n < 3 || texts.is_empty() {
        return false;
    }

    // Candidates per side: (side, text index).
    let mut cands: Vec<(u8, usize)> = Vec::new();
    for &b in &breaks {
        let split = texts.partition_point(|t| t.0 < b);
        for k in split.saturating_sub(2)..split {
            cands.push((0, k));
        }
        for k in split..(split + 3).min(texts.len()) {
            cands.push((1, k));
        }
    }
    let mut exact: HashMap<(u8, &str), usize> = HashMap::new();
    let mut norm: HashMap<(u8, String), usize> = HashMap::new();
    for &(side, k) in &cands {
        let t = texts[k].2.as_str();
        *exact.entry((side, t)).or_default() += 1;
        *norm
            .entry((side, RE_DIGITS.replace_all(t, "#").into_owned()))
            .or_default() += 1;
    }
    let exact_min = 3.max(n / 5);
    let norm_min = 3.max(n / 2);

    let mut doomed: Vec<NodeId> = Vec::new();
    for &(side, k) in &cands {
        let (_, id, ref t) = texts[k];
        if t.chars().count() > 120 {
            continue;
        }
        let node = doc.tree.get(id).unwrap();
        let in_table = node
            .ancestors()
            .any(|a| matches!(element_name(a.value()), Some("td" | "th")));
        let page_number = RE_PAGE_NUMBER.is_match(t) && !in_table;
        let recurring = exact.get(&(side, t.as_str())).copied().unwrap_or(0) >= exact_min
            || norm
                .get(&(side, RE_DIGITS.replace_all(t, "#").into_owned()))
                .copied()
                .unwrap_or(0)
                >= norm_min;
        if !(page_number || recurring) {
            continue;
        }
        // Remove the largest ancestor that holds nothing but this text, so
        // no empty link or paragraph is left behind.
        let mut target = id;
        for a in node.ancestors() {
            if matches!(
                element_name(a.value()),
                Some("body" | "html" | "td" | "th" | "tr" | "table") | None
            ) {
                break;
            }
            let all: String = a
                .descendants()
                .filter_map(|d| match d.value() {
                    Node::Text(x) => Some(x.text.to_string()),
                    _ => None,
                })
                .collect();
            if collapse(&all) == *t {
                target = a.id();
            } else {
                break;
            }
        }
        doomed.push(target);
    }
    doomed.sort();
    doomed.dedup();
    for id in &doomed {
        if let Some(mut n) = doc.tree.get_mut(*id) {
            n.detach();
        }
    }
    !doomed.is_empty()
}

fn collapse(s: &str) -> String {
    s.split(|c: char| c.is_whitespace() || c == '\u{a0}')
        .filter(|w| !w.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

static RE_MARKER: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(
        r"^(?:\(?[0-9]{1,3}(?:\.[0-9]{1,3}){0,3}\)?\.?|\(?[A-Za-z]{1,2}\)\.?|[A-Za-z]\.|\(?[ivxlcIVXLC]{1,6}\)\.?|[•●◦▪■□‣⁃·\-–—o*]|§\s*[0-9.]+|Section\s+[0-9.]+|ARTICLE\s+[0-9IVXLC]+\.?)$",
    )
    .unwrap()
});

fn text_of(n: ego_tree::NodeRef<'_, Node>) -> String {
    collapse(
        &n.descendants()
            .filter_map(|d| match d.value() {
                Node::Text(t) => Some(t.text.to_string()),
                _ => None,
            })
            .collect::<String>(),
    )
}

/// Rows (`tr`) of a table, through `thead`/`tbody`/`tfoot`, not nested
/// tables'.
fn rows_of(table: ego_tree::NodeRef<'_, Node>) -> Vec<NodeId> {
    let mut rows = Vec::new();
    for c in table.children() {
        match element_name(c.value()) {
            Some("tr") => rows.push(c.id()),
            Some("thead" | "tbody" | "tfoot") => rows.extend(
                c.children()
                    .filter(|r| element_name(r.value()) == Some("tr"))
                    .map(|r| r.id()),
            ),
            _ => {}
        }
    }
    rows
}

/// A table row and its non-empty cells.
type RowCells = (NodeId, Vec<NodeId>);

/// Tables that only lay text out become paragraphs.
///
/// A table is layout when it has one row of text, or when at least 80% of
/// its rows are a marker cell ("1.1", "(a)", "iv", "•") followed by one text
/// cell of two or more words: a numbered or bulleted list set in a grid.
/// Each row becomes a paragraph, its cells joined by a space ("1.1 Granting
/// of rights"). Data tables (several rows of values, header rows, figures)
/// do not match either shape. Only tables without nested tables are
/// considered, innermost first.
fn unwrap_layout_tables(doc: &mut Html) -> bool {
    let frag = Html::parse_fragment("<div></div><p></p><span></span>");
    let name = |h: &str| {
        let sel = Selector::parse(h).expect("valid selector");
        frag.select(&sel)
            .next()
            .expect("parsed")
            .value()
            .name
            .clone()
    };
    let (div, para, span) = (name("div"), name("p"), name("span"));
    let mut changed = false;
    for _round in 0..4 {
        let mut plan: Vec<(NodeId, Vec<RowCells>)> = Vec::new();
        for t in doc.tree.root().descendants() {
            if element_name(t.value()) != Some("table") {
                continue;
            }
            if t.descendants()
                .skip(1)
                .any(|d| element_name(d.value()) == Some("table"))
            {
                continue;
            }
            let mut rows: Vec<RowCells> = Vec::new();
            let mut marker_rows = 0;
            for r in rows_of(t) {
                let row = doc.tree.get(r).unwrap();
                let cells: Vec<NodeId> = row
                    .children()
                    .filter(|c| {
                        matches!(element_name(c.value()), Some("td" | "th"))
                            && !text_of(*c).is_empty()
                    })
                    .map(|c| c.id())
                    .collect();
                if cells.is_empty() {
                    continue;
                }
                let texts: Vec<String> = cells
                    .iter()
                    .map(|c| text_of(doc.tree.get(*c).unwrap()))
                    .collect();
                if texts.len() == 2
                    && RE_MARKER.is_match(&texts[0])
                    && texts[1].split_whitespace().count() >= 2
                {
                    marker_rows += 1;
                }
                rows.push((r, cells));
            }
            if rows.is_empty()
                || t.descendants()
                    .any(|d| element_name(d.value()) == Some("th"))
            {
                continue;
            }
            let one_row_of_text = rows.len() == 1 && {
                let texts: Vec<String> = rows[0]
                    .1
                    .iter()
                    .map(|c| text_of(doc.tree.get(*c).unwrap()))
                    .collect();
                let longest = texts.iter().map(|t| t.chars().count()).max().unwrap_or(0);
                // A paragraph, a marker and its text, or a lone cell; never
                // a row of short values ("Total | 1,234 | 5,678").
                (texts.len() == 1 && longest >= 2)
                    || (texts.len() == 2 && RE_MARKER.is_match(&texts[0]))
                    || (longest >= 80 && texts.len() <= 3)
            };
            let list_grid = rows.len() >= 2 && marker_rows * 5 >= rows.len() * 4;
            if one_row_of_text || list_grid {
                plan.push((t.id(), rows));
            }
        }
        if plan.is_empty() {
            break;
        }
        for (table, rows) in plan {
            for (row, cells) in &rows {
                // Cells inline, separated by a space; a cell's single
                // paragraph inline too, so "1.1" and its title share a line.
                for (k, c) in cells.iter().enumerate() {
                    let single_p: Vec<NodeId> = doc
                        .tree
                        .get(*c)
                        .unwrap()
                        .children()
                        .filter(|x| matches!(element_name(x.value()), Some("p" | "div")))
                        .map(|x| x.id())
                        .collect();
                    if single_p.len() == 1
                        && let Some(mut p) = doc.tree.get_mut(single_p[0])
                        && let Node::Element(e) = p.value()
                    {
                        e.name = span.clone();
                    }
                    if let Some(mut cell) = doc.tree.get_mut(*c) {
                        if let Node::Element(e) = cell.value() {
                            e.name = span.clone();
                        }
                        if k + 1 < cells.len() {
                            cell.insert_after(Node::Text(Text { text: " ".into() }));
                        }
                    }
                }
                // Empty cells go.
                let empties: Vec<NodeId> = doc
                    .tree
                    .get(*row)
                    .unwrap()
                    .children()
                    .filter(|c| matches!(element_name(c.value()), Some("td" | "th")))
                    .map(|c| c.id())
                    .collect();
                for e in empties {
                    if let Some(mut n) = doc.tree.get_mut(e) {
                        n.detach();
                    }
                }
                if let Some(mut r) = doc.tree.get_mut(*row)
                    && let Node::Element(e) = r.value()
                {
                    e.name = para.clone();
                }
            }
            // The table and its row groups become plain containers; rows
            // without text are dropped.
            let leftovers: Vec<NodeId> = doc
                .tree
                .get(table)
                .unwrap()
                .descendants()
                .filter(|d| element_name(d.value()) == Some("tr"))
                .map(|d| d.id())
                .collect();
            for id in leftovers {
                if let Some(mut n) = doc.tree.get_mut(id) {
                    n.detach();
                }
            }
            let groups: Vec<NodeId> = doc
                .tree
                .get(table)
                .unwrap()
                .descendants()
                .filter(|d| {
                    matches!(
                        element_name(d.value()),
                        Some("table" | "thead" | "tbody" | "tfoot" | "colgroup" | "col")
                    )
                })
                .map(|d| d.id())
                .collect();
            for id in groups {
                if let Some(mut n) = doc.tree.get_mut(id)
                    && let Node::Element(e) = n.value()
                {
                    e.name = div.clone();
                }
            }
            changed = true;
        }
    }
    changed
}

/// Font size in points set by one inline style, if any. Relative sizes
/// (`%`, `em`) scale the inherited size.
fn own_font_size(style: &str, inherited: f32) -> Option<f32> {
    let i = style.find("font-size:")?;
    let v = &style[i + "font-size:".len()..];
    let end = v.find(';').unwrap_or(v.len());
    let v = &v[..end];
    let num: String = v
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '.')
        .collect();
    let n: f32 = num.parse().ok()?;
    let unit = &v[num.len()..];
    match unit {
        "pt" => Some(n),
        "px" => Some(n * 0.75),
        "%" => Some(inherited * n / 100.0),
        "em" | "rem" => Some(inherited * n),
        _ => None,
    }
}

/// The font size in effect at `node`: the nearest inline `font-size`,
/// resolved against the ones above it (12pt when none is set).
fn font_size_at(node: ego_tree::NodeRef<'_, Node>) -> f32 {
    let mut chain: Vec<String> = node
        .ancestors()
        .chain(std::iter::once(node))
        .filter_map(|a| style_of(a.value()))
        .filter(|s| s.contains("font-size:"))
        .collect();
    // `ancestors()` runs upward; resolve from the root down.
    chain.reverse();
    let mut size = 12.0;
    for st in chain {
        if let Some(sz) = own_font_size(&st, size) {
            size = sz;
        }
    }
    size
}

/// Short blocks set at least 15% larger than the body text become headings.
///
/// The body size is the size most of the document's characters are set in.
/// A block qualifies when all of its text is at the larger size, it is
/// short (at most 15 words) and does not end like a sentence, and it is not
/// inside a table. Larger sizes get higher levels: the biggest heading size
/// is `<h2>`, the next `<h3>`, the rest `<h4>`.
fn promote_large_type(doc: &mut Html) -> bool {
    use std::collections::BTreeMap;
    // Characters per size, in tenths of a point.
    let mut by_size: BTreeMap<i32, usize> = BTreeMap::new();
    for n in doc.tree.root().descendants() {
        if let Node::Text(t) = n.value() {
            let len = t.text.trim().chars().count();
            if len > 0 {
                *by_size
                    .entry((font_size_at(n) * 10.0).round() as i32)
                    .or_default() += len;
            }
        }
    }
    let Some((&body, _)) = by_size.iter().max_by_key(|(_, c)| **c) else {
        return false;
    };
    let body = body as f32 / 10.0;
    if by_size.len() < 2 {
        return false;
    }
    let mut found: Vec<(NodeId, f32)> = Vec::new();
    for n in doc.tree.root().descendants() {
        if !matches!(element_name(n.value()), Some("p" | "div")) {
            continue;
        }
        // Leaf-ish blocks only: no nested block holds text of its own.
        if n.descendants().skip(1).any(|d| {
            matches!(
                element_name(d.value()),
                Some("p" | "div" | "table" | "ul" | "ol" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6")
            )
        }) {
            continue;
        }
        if n.ancestors()
            .any(|a| matches!(element_name(a.value()), Some("td" | "th" | "li" | "table")))
        {
            continue;
        }
        let texts: Vec<(String, f32)> = n
            .descendants()
            .filter_map(|d| match d.value() {
                Node::Text(t) if !collapse(&t.text).is_empty() => {
                    Some((collapse(&t.text), font_size_at(d)))
                }
                _ => None,
            })
            .collect();
        if texts.is_empty() {
            continue;
        }
        let text = collapse(
            &texts
                .iter()
                .map(|(t, _)| t.as_str())
                .collect::<Vec<_>>()
                .join(" "),
        );
        let min_size = texts.iter().map(|(_, s)| *s).fold(f32::MAX, f32::min);
        let words = text.split_whitespace().count();
        let letters = text.chars().filter(|c| c.is_alphabetic()).count();
        if min_size < body * 1.15
            || words == 0
            || words > 15
            || letters < 3
            || text.ends_with(['.', ',', ';', ':'])
            || text.chars().next().is_some_and(char::is_lowercase)
        {
            continue;
        }
        found.push((n.id(), min_size));
    }
    if found.is_empty() {
        return false;
    }
    let mut sizes: Vec<i32> = found
        .iter()
        .map(|(_, s)| (s * 10.0).round() as i32)
        .collect();
    sizes.sort_unstable_by(|a, b| b.cmp(a));
    sizes.dedup();
    let frag = Html::parse_fragment("<h2></h2><h3></h3><h4></h4>");
    let names: Vec<_> = ["h2", "h3", "h4"]
        .iter()
        .map(|h| {
            let sel = Selector::parse(h).expect("valid selector");
            frag.select(&sel)
                .next()
                .expect("parsed heading")
                .value()
                .name
                .clone()
        })
        .collect();
    for (id, size) in &found {
        let rank = sizes
            .iter()
            .position(|s| *s == (size * 10.0).round() as i32)
            .unwrap_or(2)
            .min(2);
        if let Some(mut n) = doc.tree.get_mut(*id)
            && let Node::Element(e) = n.value()
        {
            e.name = names[rank].clone();
        }
    }
    true
}

fn is_bold_style(style: &str) -> bool {
    let Some(i) = style.find("font-weight:") else {
        return false;
    };
    let v = &style[i + "font-weight:".len()..];
    v.starts_with("bold")
        || v.starts_with("bolder")
        || ["600", "700", "800", "900"]
            .iter()
            .any(|w| v.starts_with(w))
}

/// CSS bold → `<b>`; `<sup>`/`<sub>` → text.
fn normalize_inline(doc: &mut Html) -> bool {
    let mut to_bold: Vec<NodeId> = Vec::new();
    let mut to_text: Vec<(NodeId, String)> = Vec::new();
    for node in doc.tree.root().descendants() {
        let Some(name) = element_name(node.value()) else {
            continue;
        };
        match name {
            "span" | "font" => {
                let bold = style_of(node.value()).is_some_and(|s| is_bold_style(&s));
                // Already inside bold: a second `**` pair would only nest.
                let inside_bold = node.ancestors().any(|a| {
                    matches!(
                        element_name(a.value()),
                        Some("b" | "strong" | "th" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6")
                    ) || (matches!(element_name(a.value()), Some("span" | "font"))
                        && style_of(a.value()).is_some_and(|s| is_bold_style(&s)))
                });
                if bold && !inside_bold {
                    to_bold.push(node.id());
                }
            }
            "sup" | "sub" => {
                let text: String = node
                    .descendants()
                    .filter_map(|d| match d.value() {
                        Node::Text(t) => Some(t.text.to_string()),
                        _ => None,
                    })
                    .collect();
                let t = collapse(&text);
                let replacement = if t.is_empty() {
                    String::new()
                } else if name == "sup"
                    && !matches!(t.to_ascii_lowercase().as_str(), "st" | "nd" | "rd" | "th")
                    && let Some(c) = RE_FOOTNOTE_MARK.captures(&t)
                {
                    format!("[^{}]", &c[1])
                } else {
                    t
                };
                to_text.push((node.id(), replacement));
            }
            _ => {}
        }
    }
    if !to_bold.is_empty() {
        // The qualified name of `<b>`, taken from a parsed element (html5ever
        // is not a direct dependency).
        let frag = Html::parse_fragment("<b></b>");
        let sel = Selector::parse("b").expect("valid selector");
        let b_name = frag
            .select(&sel)
            .next()
            .expect("parsed <b>")
            .value()
            .name
            .clone();
        for id in &to_bold {
            if let Some(mut n) = doc.tree.get_mut(*id)
                && let Node::Element(e) = n.value()
            {
                e.name = b_name.clone();
            }
        }
    }
    for (id, text) in &to_text {
        if let Some(mut n) = doc.tree.get_mut(*id) {
            if !text.is_empty() {
                n.insert_before(Node::Text(Text {
                    text: text.as_str().into(),
                }));
            }
            n.detach();
        }
    }
    !(to_bold.is_empty() && to_text.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn md(html: &str) -> String {
        html2md::parse_html(&prepare(html))
    }

    #[test]
    fn css_bold_becomes_markdown_bold() {
        let out = md(
            r#"<div><span style="font-weight:700">Item 1A. Risk Factors</span></div>
            <p><span style="font-weight: 400">plain</span></p>"#,
        );
        assert!(out.contains("**Item 1A. Risk Factors**"), "{out}");
        assert!(!out.contains("**plain**"), "{out}");
    }

    #[test]
    fn footnote_marks_lose_their_raw_tags() {
        let out = md(
            r#"<p>gross profit<sup style="font-size:85%">(1)</sup> and the 4<sup>th</sup> quarter, m<sup>2</sup>x</p>"#,
        );
        assert!(out.contains("gross profit[^1]"), "{out}");
        assert!(out.contains("4th quarter"), "{out}");
        assert!(!out.contains("<sup"), "{out}");
    }

    #[test]
    fn recurring_footers_at_page_breaks_are_removed() {
        let bodies = [
            "Revenue grew in every region.",
            "Margins narrowed on freight costs.",
            "We repurchased shares in March.",
            "Liquidity remains strong.",
            "Two lawsuits were settled.",
            "Headcount was flat.",
        ];
        let mut html = String::from("<html><body>");
        for (p, body) in bodies.iter().enumerate() {
            let section = if p % 3 == 0 {
                format!("<p>Section {p}.1 Title of section</p>")
            } else {
                String::new()
            };
            html.push_str(&format!(
                "<p>{body}</p>\
                 <p style=\"text-align:center\">Acme Corp. | 2025 Form 10-K | {p}</p>\
                 <hr style=\"page-break-after:always\"/>\
                 <p><a href=\"#toc\">Table of Contents</a></p>{section}"
            ));
        }
        html.push_str("</body></html>");
        let out = md(&html);
        assert!(!out.contains("Form 10-K"), "{out}");
        assert!(!out.contains("Table of Contents"), "{out}");
        assert!(out.contains("Section 3.1 Title of section"), "{out}");
        assert!(out.contains("Liquidity remains strong."), "{out}");
    }

    #[test]
    fn edgar_wrapper_and_hidden_facts_are_dropped() {
        let html = "<DOCUMENT>\n<TYPE>10-K\n<SEQUENCE>1\n<FILENAME>d123.htm\n<TEXT>\n<html><body>\
            <div style=\"display:none\">0000310158</div><p>Content</p></body></html>\n</TEXT>\n</DOCUMENT>";
        let out = md(html);
        assert!(out.contains("Content"), "{out}");
        assert!(
            !out.contains("d123.htm") && !out.contains("0000310158"),
            "{out}"
        );
    }

    #[test]
    fn larger_type_becomes_headings_by_size() {
        let body = "<p style=\"font-size:9.5pt\">Body text that runs long enough to set the body size of this page for sure.</p>";
        let html = format!(
            "<html><body><p style=\"font-size:18pt\"><span style=\"color:#00837b\">Narrative Information</span></p>\
             {body}<p style=\"font-size:11pt\"><span>General Information Regarding the EIP </span></p>{body}{body}\
             <p style=\"font-size:11pt\">A larger sentence that ends with a period.</p></body></html>"
        );
        use crate::parsers::gobble::Gobble as _;
        let out = crate::parsers::html::HtmlGobbler {
            extract_full: true,
            skip_front_matter: true,
            ..Default::default()
        }
        .gobble_str(&html, &crate::parsers::gobble::ParseOptions::default())
        .unwrap();
        assert!(out.contains("## Narrative Information"), "{out}");
        assert!(
            out.contains("### General Information Regarding the EIP"),
            "{out}"
        );
        assert!(!out.contains("# A larger sentence"), "{out}");
    }

    #[test]
    fn layout_tables_become_paragraphs_and_data_tables_stay() {
        let html = r#"<html><body>
            <table><tr><td width="7%">&nbsp;</td><td>1.1</td><td><p>Granting of rights </p></td></tr></table>
            <table><tr><td>(3)</td><td>Party A, Party B and Party C have entered into the Equity Pledge Agreement.</td></tr></table>
            <table><tr><td>(a)</td><td>first clause text here</td></tr><tr><td>(b)</td><td>second clause text here</td></tr></table>
            <table><tr><td>Region</td><td>Revenue</td></tr><tr><td>Americas</td><td>$1,245</td></tr><tr><td>Europe</td><td>$850</td></tr></table>
            </body></html>"#;
        let out = md(html);
        assert!(out.contains("1.1 Granting of rights"), "{out}");
        assert!(out.contains("(3) Party A, Party B"), "{out}");
        assert!(
            out.contains("(a) first clause") && out.contains("(b) second clause"),
            "{out}"
        );
        assert!(!out.contains("| (3)") && !out.contains("|1.1"), "{out}");
        assert!(
            out.contains("Americas") && out.contains('|'),
            "data table kept: {out}"
        );
    }

    #[test]
    fn an_ordinary_page_is_untouched() {
        let html = "<html><body><p>Hello <b>world</b></p></body></html>";
        assert_eq!(prepare(html), html);
    }
}
