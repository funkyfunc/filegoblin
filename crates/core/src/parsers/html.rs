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
static SEL_ANCHOR: LazyLock<Selector> = LazyLock::new(|| Selector::parse("a").unwrap());
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
        let mut doc = Html::parse_document(content);

        // Checked before noise removal (the removal detaches every <script>).
        let has_scripts = doc.select(&SEL_SCRIPT).next().is_some();
        let title = extract_title(&doc);

        remove_matching(&mut doc, if full { &SEL_NOISE_FULL } else { &SEL_NOISE });
        if !full {
            remove_link_farms(&mut doc);
        }

        // JS-gated / auth-walled pages: script-driven with essentially no static
        // text. Fail loudly instead of emitting an empty "success".
        let body = doc.select(&SEL_BODY).next();
        let body_words = body
            .map(|b| b.text().flat_map(str::split_whitespace).count())
            .unwrap_or(0);
        if body_words < 10 && has_scripts {
            return Err(anyhow::Error::new(JsGated).context(
                "This page appears to require JavaScript rendering or authentication.\n\
                 filegoblin's static fetcher cannot execute JavaScript.\n\
                 💡 Try again with --browser to render it with your installed Chrome",
            ));
        }

        let fragments = if full {
            vec![
                body.map(|b| b.inner_html())
                    .unwrap_or_else(|| doc.root_element().inner_html()),
            ]
        } else {
            select_content_roots(&doc)
        };

        let mut markdown = tidy_markdown(
            &fragments
                .iter()
                .map(|f| html_fragment_to_md(f).trim().to_string())
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>()
                .join("\n\n"),
        );

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
    let ids: Vec<_> = doc
        .select(&SEL_LINK_FARM)
        .filter(|el| {
            let text = text_len(el);
            if text < 60 {
                return false;
            }
            let link_text: usize = el.select(&SEL_ANCHOR).map(|a| text_len(&a)).sum();
            link_text * 100 >= text * 85
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

/// Readability-style score: text volume damped by link density, so navigation
/// clusters (lots of text, nearly all of it inside `<a>`) score near zero.
fn content_score(el: &ElementRef) -> f64 {
    let text = text_len(el) as f64;
    if text == 0.0 {
        return 0.0;
    }
    let link_text: usize = el.select(&SEL_ANCHOR).map(|a| text_len(&a)).sum();
    let link_density = (link_text as f64 / text).min(1.0);
    text * (1.0 - link_density)
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
    let body_text = text_len(&body);

    let best = body
        .select(&SEL_CANDIDATE)
        .filter(|el| text_len(el) >= 100)
        .max_by(|a, b| content_score(a).total_cmp(&content_score(b)));

    match best {
        Some(el) if text_len(&el) * 10 >= body_text * 6 => vec![el.inner_html()],
        _ => vec![body.inner_html()],
    }
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
    let mut out = String::with_capacity(md.len());
    let mut rest = md;
    while let Some(pos) = rest.find("](") {
        let (head, tail) = rest.split_at(pos + 2);
        out.push_str(head);
        let Some(close) = tail.find(')') else {
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
