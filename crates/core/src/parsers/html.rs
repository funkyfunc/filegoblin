use crate::parsers::gobble::{Gobble, ParseOptions};
use anyhow::Result;
use scraper::{ElementRef, Html, Selector};
use std::path::Path;
use std::sync::LazyLock;

/// DOM-based HTML → markdown extractor.
///
/// Parses real HTML (html5ever via `scraper`), strips non-content elements,
/// picks the main content with a readability-style text/link-density heuristic,
/// and converts the surviving subtree to markdown. Hyperlink targets are kept
/// (an LLM needs URLs to follow or cite anything) and resolved against
/// `source_url` when one is known.
/// Typed marker inside the "page needs JavaScript" error chain, so routing can
/// detect it (via `anyhow` downcast) and offer / trigger `--browser` rendering.
#[derive(Debug)]
pub struct JsGated;

impl std::fmt::Display for JsGated {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "page requires JavaScript rendering")
    }
}

impl std::error::Error for JsGated {}

#[derive(Default)]
pub struct HtmlGobbler {
    /// Convert the whole document instead of isolating the main content.
    pub extract_full: bool,
    /// The URL this HTML was fetched from, when known. Used to resolve relative
    /// links and recorded in the front-matter.
    pub source_url: Option<String>,
    /// Suppress the `--- title/source ---` front-matter block. Used by callers
    /// that add their own document headings (e.g. EPUB chapters).
    pub skip_front_matter: bool,
}

static SEL_SCRIPT: LazyLock<Selector> = LazyLock::new(|| Selector::parse("script").unwrap());
static SEL_TITLE: LazyLock<Selector> = LazyLock::new(|| Selector::parse("title").unwrap());
static SEL_OG_TITLE: LazyLock<Selector> =
    LazyLock::new(|| Selector::parse(r#"meta[property="og:title"]"#).unwrap());
static SEL_BODY: LazyLock<Selector> = LazyLock::new(|| Selector::parse("body").unwrap());
static SEL_MAIN: LazyLock<Selector> = LazyLock::new(|| Selector::parse("main").unwrap());
static SEL_ARTICLE: LazyLock<Selector> = LazyLock::new(|| Selector::parse("article").unwrap());
static SEL_CANDIDATE: LazyLock<Selector> =
    LazyLock::new(|| Selector::parse("div, section, td").unwrap());
/// Blocks that can be pure link farms (language pickers, tag clouds, "related
/// articles" rails) even inside `<main>`.
static SEL_LINK_FARM: LazyLock<Selector> =
    LazyLock::new(|| Selector::parse("ul, ol, table, div").unwrap());
/// Elements that never carry article content.
static SEL_NOISE: LazyLock<Selector> = LazyLock::new(|| {
    Selector::parse(
        "script, style, noscript, svg, iframe, canvas, form, button, dialog, template, nav, aside, footer",
    )
    .unwrap()
});
/// Noise removal for `--full` mode: only strip elements with no text meaning at
/// all — chrome like nav/footer is deliberately kept.
static SEL_NOISE_FULL: LazyLock<Selector> = LazyLock::new(|| {
    Selector::parse("script, style, noscript, svg, iframe, canvas, template").unwrap()
});

impl Gobble for HtmlGobbler {
    fn gobble(&self, path: &Path, opts: &ParseOptions) -> Result<String> {
        let content = std::fs::read_to_string(path)?;
        self.gobble_str(&content, opts)
    }

    fn gobble_str(&self, content: &str, opts: &ParseOptions) -> Result<String> {
        let full = self.extract_full || opts.full;
        let doc = Html::parse_document(content);

        // html5ever and html2md both cost more than linearly in nesting depth
        // — measured at ~4x for 2x depth, in the dependencies rather than here
        // — so a page of a million nested divs would pin a CPU for a very long
        // time. Real documents nest tens of levels; browsers themselves give up
        // far below this. Refuse rather than hang.
        if let Some(depth) = max_depth_exceeding(&doc, MAX_DOM_DEPTH) {
            anyhow::bail!(
                "HTML nests more than {} levels deep ({}+ found) — refusing to convert it, \
                 as this is far past anything a real document does",
                MAX_DOM_DEPTH,
                depth
            );
        }

        // Checked before noise removal (the removal detaches every <script>).
        let has_scripts = doc.select(&SEL_SCRIPT).next().is_some();
        let title = extract_title(&doc);
        let total_words = doc
            .select(&SEL_BODY)
            .next()
            .map(|b| b.text().flat_map(str::split_whitespace).count())
            .unwrap_or(0);
        drop(doc);

        let mut markdown = self.extract_markdown(content, full, true, opts.already_rendered)?;

        // A page whose content genuinely *is* links — a docs table of contents,
        // a sitemap, an "awesome" list — had every candidate block deleted as a
        // link farm and came back empty with Ok. Retry keeping them rather than
        // reporting an empty success.
        if markdown.trim().is_empty() && !full {
            markdown = self.extract_markdown(content, full, false, opts.already_rendered)?;
        }

        if markdown.trim().is_empty() {
            if has_scripts {
                return Err(
                    anyhow::Error::new(JsGated).context(js_gated_message(opts.already_rendered))
                );
            }
            anyhow::bail!(
                "No readable content found in this HTML ({} words in the body)",
                total_words
            );
        }

        if let Some(base) = self
            .source_url
            .as_deref()
            .and_then(|s| url::Url::parse(s).ok())
        {
            markdown = absolutize_md_links(&markdown, &base);
        }

        let front_matter = if self.skip_front_matter {
            String::new()
        } else {
            build_front_matter(title.as_deref(), self.source_url.as_deref())
        };

        Ok(format!("{}{}", front_matter, markdown).trim().to_string())
    }
}

impl HtmlGobbler {
    /// One extraction pass. `drop_link_farms` is what the empty-output retry
    /// turns off.
    fn extract_markdown(
        &self,
        content: &str,
        full: bool,
        drop_link_farms: bool,
        already_rendered: bool,
    ) -> Result<String> {
        let mut doc = Html::parse_document(content);
        let has_scripts = doc.select(&SEL_SCRIPT).next().is_some();

        remove_matching(&mut doc, if full { &SEL_NOISE_FULL } else { &SEL_NOISE });
        if !full && drop_link_farms {
            remove_link_farms(&mut doc);
        }

        // JS-gated / auth-walled pages: script-driven with essentially no static
        // text. Fail loudly instead of emitting an empty "success".
        let body = doc.select(&SEL_BODY).next();
        let body_words = body
            .map(|b| b.text().flat_map(str::split_whitespace).count())
            .unwrap_or(0);
        if body_words < 10 && has_scripts {
            return Err(anyhow::Error::new(JsGated).context(js_gated_message(already_rendered)));
        }

        let fragments = if full {
            vec![
                body.map(|b| b.inner_html())
                    .unwrap_or_else(|| doc.root_element().inner_html()),
            ]
        } else {
            select_content_roots(&doc)
        };

        Ok(tidy_markdown(
            &fragments
                .iter()
                .map(|f| html_fragment_to_md(f).trim().to_string())
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>()
                .join("\n\n"),
        ))
    }
}

/// The "this page needs JavaScript" message, which must not recommend
/// `--browser` to someone who already used it.
fn js_gated_message(already_rendered: bool) -> &'static str {
    if already_rendered {
        "This page still has essentially no text after a headless-browser render.\n\
         It likely requires authentication, or blocks headless Chrome outright."
    } else {
        "This page appears to require JavaScript rendering or authentication.\n\
         filegoblin's static fetcher cannot execute JavaScript.\n\
         💡 Try again with --browser to render it with your installed Chrome"
    }
}

/// Detaches every element matching `selector` from the tree.
fn remove_matching(doc: &mut Html, selector: &Selector) {
    let ids: Vec<_> = doc.select(selector).map(|el| el.id()).collect();
    for id in ids {
        if let Some(mut node) = doc.tree.get_mut(id) {
            node.detach();
        }
    }
}

/// Detaches list/table blocks that are almost entirely link text (≥85% link
/// density with a meaningful amount of it). Semantic containers like `<main>`
/// still carry navigation rails on real sites (language pickers, "related"
/// boxes); prose blocks never come close to this density, so this only ever
/// removes navigation-shaped noise.
fn remove_link_farms(doc: &mut Html) {
    // Sizes for the whole document in one pass. Measuring each candidate on its
    // own re-walked its entire subtree, and since every ancestor div contains
    // its descendants' text, deeply nested markup made the work multiply out —
    // 3.5s of a 4.3s conversion on a 35 KB page of nested divs.
    let sizes = accumulate_text_sizes(&doc.root_element());
    let ids: Vec<_> = doc
        .select(&SEL_LINK_FARM)
        .filter(|el| {
            let (text, link_text) = sizes.get(&el.id()).copied().unwrap_or((0, 0));
            text >= 60 && link_text * 100 >= text * 85
        })
        .map(|el| el.id())
        .collect();
    for id in ids {
        if let Some(mut node) = doc.tree.get_mut(id) {
            node.detach();
        }
    }
}

/// `<title>` text, falling back to `og:title`. Whitespace-collapsed.
fn extract_title(doc: &Html) -> Option<String> {
    let from_tag = doc.select(&SEL_TITLE).next().map(|t| {
        t.text()
            .flat_map(str::split_whitespace)
            .collect::<Vec<_>>()
            .join(" ")
    });
    from_tag.filter(|s| !s.is_empty()).or_else(|| {
        doc.select(&SEL_OG_TITLE)
            .next()
            .and_then(|m| m.value().attr("content"))
            .map(|s| s.split_whitespace().collect::<Vec<_>>().join(" "))
            .filter(|s| !s.is_empty())
    })
}

/// Non-whitespace character count of an element's visible text.
fn text_len(el: &ElementRef) -> usize {
    el.text()
        .flat_map(str::split_whitespace)
        .map(str::len)
        .sum()
}

/// Picks the content root(s) after noise removal:
/// 1. every `<main>` with text, else
/// 2. every `<article>` with text (ALL of them — a blog index or forum thread
///    is more than its first article), else
/// 3. the best-scoring block candidate, guarded: if it holds less than 60% of
///    the page's text, fall back to `<body>` so content is never dropped.
fn select_content_roots(doc: &Html) -> Vec<String> {
    for sel in [&*SEL_MAIN, &*SEL_ARTICLE] {
        let hits: Vec<String> = doc
            .select(sel)
            .filter(|el| text_len(el) > 0)
            .map(|el| el.inner_html())
            .collect();
        if !hits.is_empty() {
            return hits;
        }
    }

    let Some(body) = doc.select(&SEL_BODY).next() else {
        return vec![doc.root_element().inner_html()];
    };

    // One bottom-up pass for every node's text and link-text totals.
    //
    // Scoring each candidate independently re-walked its whole subtree, and on
    // nested div-soup every candidate contains all the others' text — so the
    // work multiplied out. A 35 KB page of 3000 nested divs took 4.3 seconds;
    // this pass makes it instant, and it is trivially craftable input.
    let sizes = accumulate_text_sizes(&body);
    let size_of = |el: &ElementRef| sizes.get(&el.id()).copied().unwrap_or((0, 0));
    let body_text = size_of(&body).0;

    let best = body
        .select(&SEL_CANDIDATE)
        .map(|el| (score_from(size_of(&el)), el))
        .filter(|(_, el)| size_of(el).0 >= 100)
        .max_by(|(a, _), (b, _)| a.total_cmp(b))
        .map(|(_, el)| el);

    match best {
        Some(el) if size_of(&el).0 * 10 >= body_text * 6 => vec![el.inner_html()],
        _ => vec![body.inner_html()],
    }
}

/// Deepest nesting we will convert. Chosen far above any real document (typical
/// pages nest a few dozen levels) and far below where the HTML dependencies
/// become pathologically slow.
const MAX_DOM_DEPTH: usize = 1_000;

/// `Some(depth)` when the document nests deeper than `limit`, else `None`.
/// Single pre-order pass: a parent always precedes its children, so each node's
/// depth is its parent's plus one.
fn max_depth_exceeding(doc: &Html, limit: usize) -> Option<usize> {
    let mut depths: std::collections::HashMap<ego_tree::NodeId, usize> =
        std::collections::HashMap::new();
    for node in doc.root_element().descendants() {
        let depth = node
            .parent()
            .and_then(|p| depths.get(&p.id()).copied())
            .map(|d| d + 1)
            .unwrap_or(0);
        if depth > limit {
            return Some(depth);
        }
        depths.insert(node.id(), depth);
    }
    None
}

/// `(text_len, link_text_len)` for every node under `root`, in a single O(nodes)
/// pass.
///
/// `descendants()` yields pre-order, so walking it in reverse visits every
/// child before its parent — each node adds its finished totals to its parent
/// exactly once.
fn accumulate_text_sizes(
    root: &ElementRef,
) -> std::collections::HashMap<ego_tree::NodeId, (usize, usize)> {
    let mut sizes: std::collections::HashMap<ego_tree::NodeId, (usize, usize)> =
        std::collections::HashMap::new();
    let nodes: Vec<_> = root.descendants().collect();

    for node in nodes.into_iter().rev() {
        let id = node.id();
        let own_text: usize = node
            .value()
            .as_text()
            .map(|t| t.split_whitespace().map(str::len).sum())
            .unwrap_or(0);

        let entry = sizes.entry(id).or_insert((0, 0));
        entry.0 += own_text;
        let (text, link) = *entry;

        // An `<a>` contributes all of its text as link text to its ancestors.
        let is_anchor = node
            .value()
            .as_element()
            .is_some_and(|e| e.name().eq_ignore_ascii_case("a"));
        let link = if is_anchor { text } else { link };
        sizes.insert(id, (text, link));

        if let Some(parent) = node.parent() {
            let p = sizes.entry(parent.id()).or_insert((0, 0));
            p.0 += text;
            p.1 += link;
        }
    }
    sizes
}

/// Readability-style score from precomputed sizes: text volume damped by link
/// density, so navigation clusters score near zero.
fn score_from((text, link): (usize, usize)) -> f64 {
    if text == 0 {
        return 0.0;
    }
    let density = (link as f64 / text as f64).min(1.0);
    text as f64 * (1.0 - density)
}

/// html2md conversion hardened against panics: its handlers unwrap on some
/// malformed input, and a panic on one untrusted web page must not kill a whole
/// horde run. Falls back to plain text extraction.
fn html_fragment_to_md(fragment: &str) -> String {
    std::panic::catch_unwind(|| html2md::parse_html(fragment)).unwrap_or_else(|_| {
        Html::parse_fragment(fragment)
            .root_element()
            .text()
            .collect::<Vec<_>>()
            .join(" ")
    })
}

static RE_RAW_IMG: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r"<img\b[^>]*>").unwrap());

/// Cleans up html2md artifacts that waste tokens or leak raw HTML:
/// - `<img ...>` tags passed through verbatim become `![alt](src)` (or just the
///   alt text when there is no src)
/// - table rows padded to column width with runs of spaces are collapsed
///   (alignment is cosmetic in markdown and explodes token counts)
/// - setext headings (`Title` + `======`) become ATX (`# Title`)
fn tidy_markdown(md: &str) -> String {
    let md = RE_RAW_IMG.replace_all(md, |caps: &regex::Captures| {
        let tag = &caps[0];
        let alt = html_attr(tag, "alt").unwrap_or_default();
        match html_attr(tag, "src") {
            Some(src) if !src.is_empty() => format!("![{}]({})", alt, src),
            _ => alt,
        }
    });

    let lines: Vec<&str> = md.lines().collect();
    let mut out: Vec<String> = Vec::with_capacity(lines.len());
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        let underline = lines.get(i + 1).copied().and_then(setext_level);
        if let Some(level) = underline
            && is_heading_text(line)
        {
            out.push(format!("{} {}", "#".repeat(level), line.trim()));
            i += 2;
            continue;
        }
        if line.trim_start().starts_with('|') {
            out.push(collapse_spaces(line));
        } else {
            out.push(line.to_string());
        }
        i += 1;
    }
    out.join("\n")
}

/// `====` underlines mean h1, `----` mean h2 (3+ chars, nothing else on the line).
fn setext_level(line: &str) -> Option<usize> {
    let t = line.trim();
    if t.len() >= 3 && t.chars().all(|c| c == '=') {
        Some(1)
    } else if t.len() >= 3 && t.chars().all(|c| c == '-') {
        Some(2)
    } else {
        None
    }
}

/// The line above an underline only counts as heading text when it's a plain
/// short-ish line — not empty, not a list item / table row / existing heading
/// (an `----` after those is a horizontal rule, not an underline).
fn is_heading_text(line: &str) -> bool {
    let t = line.trim();
    !t.is_empty() && !t.starts_with(['|', '#', '-', '*', '>', '='])
}

/// Extracts a double-quoted attribute value out of a raw HTML tag string.
fn html_attr(tag: &str, name: &str) -> Option<String> {
    let marker = format!("{}=\"", name);
    let start = tag.find(&marker)? + marker.len();
    let end = tag[start..].find('"')?;
    Some(tag[start..start + end].to_string())
}

/// Collapses runs of 2+ spaces into one.
fn collapse_spaces(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut last_was_space = false;
    for c in line.chars() {
        if c == ' ' {
            if !last_was_space {
                out.push(c);
            }
            last_was_space = true;
        } else {
            out.push(c);
            last_was_space = false;
        }
    }
    out
}

/// Resolves relative markdown link/image targets (`](/docs/x)`) against the
/// page URL, leaving absolute URLs, fragments, and mailto/data links untouched.
fn absolutize_md_links(md: &str, base: &url::Url) -> String {
    // Rewriting is line-based so fenced and indented code blocks can be
    // skipped: a shell snippet containing `](` used to have its "target"
    // resolved into a URL, corrupting the code.
    let mut out = String::with_capacity(md.len());
    let mut in_fence = false;
    for (i, line) in md.split_inclusive('\n').enumerate() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            in_fence = !in_fence;
            out.push_str(line);
            continue;
        }
        // An indented code block starts with 4 spaces or a tab. Line 0 of a
        // document is never a continuation, so this is safe to apply directly.
        let indented_code = line.starts_with("    ") || line.starts_with('\t');
        if in_fence || (indented_code && i > 0) {
            out.push_str(line);
            continue;
        }
        out.push_str(&absolutize_line(line, base));
    }
    out
}

fn absolutize_line(line: &str, base: &url::Url) -> String {
    let mut out = String::with_capacity(line.len());
    let mut rest = line;
    while let Some(pos) = rest.find("](") {
        let (head, tail) = rest.split_at(pos + 2);
        out.push_str(head);
        let Some(close) = closing_paren(tail) else {
            rest = tail;
            break;
        };
        out.push_str(&resolve_target(&tail[..close], base));
        out.push(')');
        rest = &tail[close + 1..];
    }
    out.push_str(rest);
    out
}

/// Index of the `)` that closes a markdown link target, honouring balanced
/// parens inside it.
///
/// Stopping at the *first* `)` truncated any target containing one —
/// `/wiki/Foo_(bar)` resolved to `/wiki/Foo_(bar` and left a stray `)` in the
/// text, which is exactly the shape Wikipedia links take.
fn closing_paren(s: &str) -> Option<usize> {
    let mut depth = 0usize;
    for (i, c) in s.char_indices() {
        match c {
            '(' => depth += 1,
            ')' if depth == 0 => return Some(i),
            ')' => depth -= 1,
            // A target never spans lines; bail rather than swallowing the rest
            // of the document looking for a balancing paren.
            '\n' => return None,
            _ => {}
        }
    }
    None
}

fn resolve_target(target: &str, base: &url::Url) -> String {
    let t = target.trim();
    let keep_as_is = t.is_empty()
        || t.starts_with('#')
        || t.contains("://")
        || t.starts_with("mailto:")
        || t.starts_with("data:")
        || t.starts_with("tel:");
    if keep_as_is {
        return target.to_string();
    }
    base.join(t)
        .map(|u| u.to_string())
        .unwrap_or_else(|_| target.to_string())
}

/// A minimal YAML front-matter block carrying document identity. Emitted only
/// when there is something to say.
fn build_front_matter(title: Option<&str>, source: Option<&str>) -> String {
    if title.is_none() && source.is_none() {
        return String::new();
    }
    let mut fm = String::from("---\n");
    if let Some(t) = title {
        fm.push_str(&format!("title: \"{}\"\n", t.replace('"', "\\\"")));
    }
    if let Some(s) = source {
        fm.push_str(&format!("source: {}\n", s));
    }
    fm.push_str("---\n\n");
    fm
}

#[cfg(test)]
mod regression_tests {
    use super::*;

    /// Regression: `remove_link_farms` deleted every candidate block on a page
    /// whose content genuinely is links, and the result was an empty success.
    #[test]
    fn a_page_that_is_mostly_links_still_produces_content() {
        let toc = r#"<html><head><title>API Reference</title></head><body>
            <main>
              <ul>
                <li><a href="/api/create">create — make a new widget</a></li>
                <li><a href="/api/read">read — fetch an existing widget</a></li>
                <li><a href="/api/update">update — change a widget in place</a></li>
                <li><a href="/api/delete">delete — remove a widget forever</a></li>
                <li><a href="/api/list">list — enumerate every widget you own</a></li>
              </ul>
            </main>
        </body></html>"#;
        let out = HtmlGobbler::default()
            .gobble_str(toc, &ParseOptions::default())
            .expect("a link-only page must not be an error");
        assert!(out.contains("create"), "got: {out}");
        assert!(out.contains("delete"), "got: {out}");
    }

    /// Regression: the rewriter stopped at the first `)`, so a Wikipedia-style
    /// target lost its tail and left a stray paren in the text.
    #[test]
    fn link_targets_containing_parens_survive() {
        let base = url::Url::parse("https://en.wikipedia.org/wiki/Main").unwrap();
        let md = "See [Mercury](/wiki/Mercury_(planet)) for details.";
        let out = absolutize_md_links(md, &base);
        assert!(
            out.contains("(https://en.wikipedia.org/wiki/Mercury_(planet))"),
            "got: {out}"
        );
        assert!(out.ends_with("for details."), "got: {out}");
    }

    /// Regression: `](` inside a code block was treated as a link target.
    #[test]
    fn code_blocks_are_left_alone() {
        let base = url::Url::parse("https://example.com/docs/").unwrap();
        let md =
            "Text [a](b)\n\n```bash\narr=(x); echo ${arr[0]}](/not/a/link)\n```\n\nMore [c](d)";
        let out = absolutize_md_links(md, &base);
        assert!(
            out.contains("${arr[0]}](/not/a/link)"),
            "code must be untouched: {out}"
        );
        assert!(
            out.contains("[a](https://example.com/docs/b)"),
            "got: {out}"
        );
        assert!(
            out.contains("[c](https://example.com/docs/d)"),
            "got: {out}"
        );
    }

    /// Pathologically nested markup is refused rather than converted: the HTML
    /// dependencies cost more than linearly in depth, so a hostile page could
    /// otherwise pin a CPU for a very long time.
    #[test]
    fn absurdly_nested_html_is_refused() {
        let deep = format!(
            "<html><body>{}<p>text</p>{}</body></html>",
            "<div>".repeat(2_000),
            "</div>".repeat(2_000)
        );
        let err = HtmlGobbler::default()
            .gobble_str(&deep, &ParseOptions::default())
            .unwrap_err();
        assert!(err.to_string().contains("nests more than"), "got: {err}");

        // Ordinary nesting is unaffected.
        let normal = format!(
            "<html><body>{}<p>Plenty of real article text right here.</p>{}</body></html>",
            "<div>".repeat(40),
            "</div>".repeat(40)
        );
        assert!(
            HtmlGobbler::default()
                .gobble_str(&normal, &ParseOptions::default())
                .is_ok()
        );
    }

    /// An HTML file with no readable text and no scripts is an error, not an
    /// empty string dressed in front matter.
    #[test]
    fn empty_html_without_scripts_fails_loudly() {
        let empty = "<html><head><title>Nothing</title></head><body></body></html>";
        let err = HtmlGobbler::default()
            .gobble_str(empty, &ParseOptions::default())
            .unwrap_err();
        assert!(
            err.to_string().contains("No readable content"),
            "got: {err}"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gobble(html: &str) -> String {
        HtmlGobbler::default()
            .gobble_str(html, &ParseOptions::default())
            .unwrap()
    }

    #[test]
    fn test_web_heuristic_extraction() {
        let html = r#"
            <html><body>
                <nav>Ignore me</nav>
                <article>
                    <h1>The Title</h1>
                    <p>The core content</p>
                </article>
                <footer>footer</footer>
            </body></html>
        "#;
        let result = gobble(html);
        assert!(result.contains("The core content"));
        assert!(!result.contains("Ignore me"));
        assert!(!result.contains("footer"));
    }

    #[test]
    fn all_articles_are_kept_not_just_the_first() {
        let html = r#"
            <html><body>
                <article><h2>First post</h2><p>Alpha body text.</p></article>
                <article><h2>Second post</h2><p>Beta body text.</p></article>
            </body></html>
        "#;
        let result = gobble(html);
        assert!(result.contains("Alpha body text."));
        assert!(result.contains("Beta body text."));
    }

    #[test]
    fn title_inside_header_survives() {
        // The old string-stripper deleted <header> wholesale, destroying the h1
        // on sites that keep the article title inside it.
        let html = r#"
            <html><head><title>Site</title></head><body>
                <main>
                    <header><h1>Actual Article Title</h1></header>
                    <p>Body paragraph.</p>
                </main>
            </body></html>
        "#;
        let result = gobble(html);
        assert!(result.contains("Actual Article Title"));
        assert!(result.contains("Body paragraph."));
    }

    #[test]
    fn hyperlink_urls_are_preserved() {
        let html = r#"
            <html><body><main>
                <p>See the <a href="https://docs.example.com/api">API docs</a> for details.</p>
            </main></body></html>
        "#;
        let result = gobble(html);
        assert!(result.contains("[API docs](https://docs.example.com/api)"));
    }

    #[test]
    fn relative_links_resolve_against_source_url() {
        let gobbler = HtmlGobbler {
            source_url: Some("https://example.com/blog/post".to_string()),
            ..Default::default()
        };
        let html = r##"<html><body><main>
            <p><a href="/pricing">Pricing</a> and <a href="#section">jump</a>.</p>
        </main></body></html>"##;
        let result = gobbler.gobble_str(html, &ParseOptions::default()).unwrap();
        assert!(result.contains("(https://example.com/pricing)"));
        assert!(result.contains("(#section)"));
    }

    #[test]
    fn front_matter_carries_title_and_source() {
        let gobbler = HtmlGobbler {
            source_url: Some("https://example.com/a".to_string()),
            ..Default::default()
        };
        let html = r#"<html><head><title>Doc Title</title></head>
            <body><main><p>Content here.</p></main></body></html>"#;
        let result = gobbler.gobble_str(html, &ParseOptions::default()).unwrap();
        assert!(result.starts_with("---\n"));
        assert!(result.contains("title: \"Doc Title\""));
        assert!(result.contains("source: https://example.com/a"));
    }

    #[test]
    fn link_dense_navigation_loses_to_content_in_div_soup() {
        // No <main>/<article>: the scorer must pick the text-dense block over a
        // link farm, without dropping most of the page (60% guard → body).
        let nav_links: String = (0..30)
            .map(|i| format!("<a href=\"/p{i}\">Link number {i}</a> "))
            .collect();
        let html = format!(
            r#"<html><body>
                <div class="menu">{nav_links}</div>
                <div class="post">{}</div>
            </body></html>"#,
            "<p>Real paragraph content that goes on for a while. It has substantially more prose than markup and represents the actual article body a reader came for. More sentences pad the density so the heuristic has something to measure.</p>"
        );
        let result = gobble(&html);
        assert!(result.contains("Real paragraph content"));
    }

    #[test]
    fn js_gated_page_fails_loudly() {
        let html = r#"<html><body><script>window.app.render()</script></body></html>"#;
        let err = HtmlGobbler::default()
            .gobble_str(html, &ParseOptions::default())
            .unwrap_err();
        assert!(err.to_string().contains("JavaScript"));
    }

    #[test]
    fn page_mentioning_google_signin_is_not_rejected() {
        // Regression: the old detector hard-failed any page containing the
        // words "sign in" + "google", even ordinary articles.
        let html = r#"<html><body><main>
            <h1>How OAuth works</h1>
            <p>Many sites let you sign in with Google using accounts.google.com
            redirects. This article explains the flow in detail with plenty of
            real content around it.</p>
        </main></body></html>"#;
        let result = gobble(html);
        assert!(result.contains("How OAuth works"));
    }
}
