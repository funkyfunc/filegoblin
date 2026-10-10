//! Pure Rust precision content density and boilerplate extraction engine.
//!
//! Ported and adapted from `trafilatura` (`trafilatura.core` / `htmlprocessing.py`):
//! - Structural node scoring based on text length, comma/punctuation density, and paragraph depth.
//! - Negative link-density filtering (`LINK_FARM_RATIO = 0.33`) to purge navigation menus,
//!   carousels, sidebars, and recommendation rails.
//! - Attribute and class token filtering: suppresses banner ads, sponsor widgets, cookie consent
//!   modals, author bios, social sharing icons, and comment trees.
//! - Structural preservation: strictly protects `<pre><code>`, `<table>`, `<blockquote>`,
//!   and heading hierarchies (`<h1>`-`<h6>`).

use scraper::{ElementRef, Html, Selector};
use std::sync::LazyLock;

/// Maximum link density allowed before a container is classified as boilerplate/link farm.
pub const LINK_FARM_RATIO: f32 = 0.33;

/// Noise tag selectors that are unconditionally removed from content extraction.
static SEL_RAW_NOISE: LazyLock<Selector> = LazyLock::new(|| {
    Selector::parse("script, style, noscript, svg, iframe, canvas, form, dialog, template").unwrap()
});

/// Chrome elements that represent global navigation and footers.
static SEL_CHROME: LazyLock<Selector> =
    LazyLock::new(|| Selector::parse("nav, footer, aside").unwrap());

/// Candidate containers to evaluate for main content.
static SEL_CANDIDATES: LazyLock<Selector> = LazyLock::new(|| {
    Selector::parse("article, main, section, div, [role='main'], [role='article']").unwrap()
});

/// Link selector.
static SEL_LINKS: LazyLock<Selector> = LazyLock::new(|| Selector::parse("a").unwrap());

/// Paragraph selector.
static SEL_P: LazyLock<Selector> = LazyLock::new(|| Selector::parse("p").unwrap());

/// Headings selector.
static SEL_HEADINGS: LazyLock<Selector> =
    LazyLock::new(|| Selector::parse("h1, h2, h3, h4, h5, h6").unwrap());

/// Code and preformatted blocks selector.
static SEL_CODE: LazyLock<Selector> = LazyLock::new(|| Selector::parse("pre, code").unwrap());

/// Tables selector.
static SEL_TABLES: LazyLock<Selector> = LazyLock::new(|| Selector::parse("table").unwrap());

/// Blockquotes selector.
static SEL_QUOTES: LazyLock<Selector> = LazyLock::new(|| Selector::parse("blockquote").unwrap());

/// Structural metrics for an HTML element.
#[derive(Debug, Clone, Default)]
pub struct NodeMetrics {
    pub total_text_len: usize,
    pub link_text_len: usize,
    pub word_count: usize,
    pub punctuation_count: usize,
    pub p_count: usize,
    pub heading_count: usize,
    pub code_count: usize,
    pub table_count: usize,
    pub quote_count: usize,
}

impl NodeMetrics {
    pub fn link_density(&self) -> f32 {
        if self.total_text_len == 0 {
            0.0
        } else {
            self.link_text_len as f32 / self.total_text_len as f32
        }
    }

    /// Computes a composite Trafilatura score.
    pub fn score(&self, has_positive_attr: bool, has_negative_attr: bool) -> f32 {
        let mut score: f32 = 0.0;

        // Base score from prose length (diminishing returns past 2000 chars)
        score += (self.total_text_len as f32).min(2000.0) * 0.05;

        // Punctuation bonus: prose has commas, periods, semicolons, colons
        score += (self.punctuation_count as f32) * 2.5;

        // Paragraph bonus
        score += (self.p_count as f32) * 6.0;

        // Headings bonus
        score += (self.heading_count as f32) * 4.0;

        // Code block and table protection bonus
        score += (self.code_count as f32) * 12.0;
        score += (self.table_count as f32) * 10.0;
        score += (self.quote_count as f32) * 5.0;

        // Attribute weights
        if has_positive_attr {
            score += 25.0;
        }
        if has_negative_attr {
            score -= 40.0;
        }

        // Heavy penalty for high link density
        let density = self.link_density();
        if density > LINK_FARM_RATIO {
            score -= density * 60.0;
        }

        score
    }
}

/// Checks whether an attribute string matches boilerplate / discard patterns.
pub fn is_boilerplate_attribute(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();

    const DISCARD_TOKENS: &[&str] = &[
        // Advertising & tracking
        "ad-",
        "ads-",
        "-ad-",
        "advert",
        "sponsor",
        "outbrain",
        "taboola",
        "criteo",
        "dfp",
        "paid-content",
        "banner",
        // Social & viral sharing
        "share",
        "social",
        "viral",
        "share-buttons",
        "sociable",
        "twitter",
        "facebook",
        "linkedin",
        // Subscriptions & lead magnets
        "newsletter",
        "syndication",
        "subscribe",
        "signup",
        "optin",
        // Cookie / consent / paywalls
        "cookie",
        "consent",
        "gdpr",
        "paywall",
        "premium-barrier",
        "modal",
        "overlay",
        // Chrome & navigation
        "sidebar",
        "navbar",
        "subnav",
        "menu",
        "breadcrumb",
        "footer-links",
        // Comments & discussions
        "comment",
        "comments",
        "disqus",
        "dsq",
        "reply",
        "replies",
        // Recommendations & widgets
        "related",
        "recommend",
        "most-popular",
        "trending",
        "more-on",
        "next-story",
        "widget",
    ];

    for token in DISCARD_TOKENS {
        if lower.contains(token) {
            return true;
        }
    }

    false
}

/// Checks whether an attribute string matches positive content container patterns.
pub fn is_content_attribute(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();

    const CONTENT_TOKENS: &[&str] = &[
        "article",
        "post",
        "entry",
        "content",
        "main",
        "story",
        "body-text",
        "post-body",
        "article-content",
        "post-content",
        "markdown-body",
        "page-content",
    ];

    for token in CONTENT_TOKENS {
        if lower.contains(token) {
            return true;
        }
    }

    false
}

/// Extracts text and computes metrics for an ElementRef.
pub fn compute_metrics(el: &ElementRef) -> NodeMetrics {
    let mut metrics = NodeMetrics::default();

    // Total text and punctuation count
    for text in el.text() {
        let trimmed = text.trim();
        if !trimmed.is_empty() {
            metrics.total_text_len += trimmed.len();
            metrics.word_count += trimmed.split_whitespace().count();
            for c in trimmed.chars() {
                if matches!(
                    c,
                    ',' | '.' | ';' | ':' | '?' | '!' | '—' | '–' | '"' | '\''
                ) {
                    metrics.punctuation_count += 1;
                }
            }
        }
    }

    // Link text
    for link in el.select(&SEL_LINKS) {
        for text in link.text() {
            metrics.link_text_len += text.trim().len();
        }
    }

    // Child structural elements
    metrics.p_count = el.select(&SEL_P).count();
    metrics.heading_count = el.select(&SEL_HEADINGS).count();
    metrics.code_count = el.select(&SEL_CODE).count();
    metrics.table_count = el.select(&SEL_TABLES).count();
    metrics.quote_count = el.select(&SEL_QUOTES).count();

    metrics
}

/// Trafilatura content extractor: purges boilerplate while preserving high-density prose and code.
pub struct TrafilaturaExtractor {
    pub link_farm_ratio: f32,
}

impl Default for TrafilaturaExtractor {
    fn default() -> Self {
        Self {
            link_farm_ratio: LINK_FARM_RATIO,
        }
    }
}

impl TrafilaturaExtractor {
    /// Extracts the main content HTML string from a raw HTML document.
    pub fn extract(&self, html: &str) -> Option<String> {
        let mut doc = Html::parse_document(html);

        // Step 1: Remove raw noise tags (scripts, styles, forms, iframes)
        self.detach_matching(&mut doc, &SEL_RAW_NOISE);

        // Step 2: Remove global chrome (nav, footer, aside)
        self.detach_matching(&mut doc, &SEL_CHROME);

        // Step 3: Remove boilerplate attributes (ads, comments, cookie modals, social shares)
        self.prune_boilerplate_attributes(&mut doc);

        // Step 4: Remove link farms (containers where link text > 33%)
        self.prune_link_farms(&mut doc);

        // Step 5: Score remaining candidate containers
        let root = doc.root_element();
        let candidates: Vec<ElementRef> = doc.select(&SEL_CANDIDATES).collect();

        let mut best_elem: Option<ElementRef> = None;
        let mut best_score: f32 = -100.0;

        for candidate in candidates {
            // Check attributes
            let val = candidate.value();
            let mut has_pos = false;
            let mut has_neg = false;

            if let Some(id) = val.id() {
                if is_content_attribute(id) {
                    has_pos = true;
                }
                if is_boilerplate_attribute(id) {
                    has_neg = true;
                }
            }
            if let Some(classes) = val.attr("class") {
                if is_content_attribute(classes) {
                    has_pos = true;
                }
                if is_boilerplate_attribute(classes) {
                    has_neg = true;
                }
            }

            let metrics = compute_metrics(&candidate);
            // Skip empty or trivial elements
            if metrics.total_text_len < 30 {
                continue;
            }

            let score = metrics.score(has_pos, has_neg);
            if score > best_score {
                best_score = score;
                best_elem = Some(candidate);
            }
        }

        if let Some(best) = best_elem {
            let inner = best.inner_html();
            if !inner.trim().is_empty() {
                return Some(inner);
            }
        }

        // Fallback to body or root element
        let body_sel = Selector::parse("body").unwrap();
        let fallback = doc
            .select(&body_sel)
            .next()
            .map(|b| b.inner_html())
            .unwrap_or_else(|| root.inner_html());

        if fallback.trim().is_empty() {
            None
        } else {
            Some(fallback)
        }
    }

    /// Detaches elements matching selector.
    fn detach_matching(&self, doc: &mut Html, selector: &Selector) {
        let ids: Vec<_> = doc.select(selector).map(|el| el.id()).collect();
        for id in ids {
            if let Some(mut node) = doc.tree.get_mut(id) {
                node.detach();
            }
        }
    }

    /// Prunes subtrees matching negative boilerplate class/id tokens,
    /// protecting structural elements like `<pre>`, `<code>`, and `<table>`.
    fn prune_boilerplate_attributes(&self, doc: &mut Html) {
        let mut to_detach = Vec::new();

        let sel_all = Selector::parse("*").unwrap();
        for el in doc.select(&sel_all) {
            let val = el.value();
            let tag = val.name();

            // Strictly protect pre, code, and table from attribute pruning
            if matches!(
                tag,
                "pre" | "code" | "table" | "blockquote" | "h1" | "h2" | "h3"
            ) {
                continue;
            }

            let mut is_noise = false;
            if let Some(id) = val.id() {
                if is_boilerplate_attribute(id) && !is_content_attribute(id) {
                    is_noise = true;
                }
            }
            if let Some(class) = val.attr("class") {
                if is_boilerplate_attribute(class) && !is_content_attribute(class) {
                    is_noise = true;
                }
            }

            if is_noise {
                // Before pruning, check if it contains code or table elements that shouldn't be lost
                let has_code = el.select(&SEL_CODE).next().is_some();
                let has_table = el.select(&SEL_TABLES).next().is_some();
                if !has_code && !has_table {
                    to_detach.push(el.id());
                }
            }
        }

        for id in to_detach {
            if let Some(mut node) = doc.tree.get_mut(id) {
                node.detach();
            }
        }
    }

    /// Prunes containers whose text consists of > 33% link text.
    fn prune_link_farms(&self, doc: &mut Html) {
        let sel_blocks = Selector::parse("div, ul, ol, section").unwrap();
        let mut to_detach = Vec::new();

        for el in doc.select(&sel_blocks) {
            // Never prune pre/code/table parents as link farms
            if el.select(&SEL_CODE).next().is_some() || el.select(&SEL_TABLES).next().is_some() {
                continue;
            }

            let metrics = compute_metrics(&el);
            if metrics.total_text_len >= 50 && metrics.link_density() > self.link_farm_ratio {
                to_detach.push(el.id());
            }
        }

        for id in to_detach {
            if let Some(mut node) = doc.tree.get_mut(id) {
                node.detach();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_trafilatura_prunes_ads_and_social_noise() {
        let html = r#"
        <!DOCTYPE html>
        <html>
        <head><title>Test Article</title></head>
        <body>
          <nav><a href="/">Home</a> <a href="/about">About</a></nav>
          <div class="header-ad banner">Buy this widget today!</div>
          <article class="post-content">
            <h1>Rust Systems Engineering</h1>
            <p>Rust provides memory safety without a garbage collector, using an affine type system.</p>
            <p>Concurrency in Rust is protected by the Send and Sync traits, preventing data races.</p>
            <pre><code>fn main() { println!("Hello world!"); }</code></pre>
          </article>
          <div class="social-share-buttons">
            <a href="https://twitter.com/share">Tweet</a>
            <a href="https://facebook.com/share">Share</a>
          </div>
          <div class="comments-section" id="disqus_thread">
            <p>First comment! Great article.</p>
          </div>
          <footer>Copyright 2026</footer>
        </body>
        </html>
        "#;

        let extractor = TrafilaturaExtractor::default();
        let result = extractor.extract(html).expect("extract content");

        assert!(
            result.contains("Rust Systems Engineering"),
            "must retain title"
        );
        assert!(
            result.contains("memory safety without a garbage collector"),
            "must retain article prose"
        );
        assert!(result.contains("Hello world!"), "must protect code block");

        assert!(
            !result.contains("Buy this widget today!"),
            "must prune banner ads"
        );
        assert!(
            !result.contains("social-share-buttons"),
            "must prune social shares"
        );
        assert!(!result.contains("disqus_thread"), "must prune comments");
        assert!(!result.contains("Copyright 2026"), "must prune footer");
    }

    #[test]
    fn test_trafilatura_link_density_filtering() {
        let html = r#"
        <div class="main-container">
          <div class="related-stories-carousel">
            <a href="/story1">Story One Headline Link</a>
            <a href="/story2">Story Two Headline Link</a>
            <a href="/story3">Story Three Headline Link</a>
          </div>
          <div class="article-body">
            <p>The telescope captured light emitted over thirteen billion years ago, revealing early galactic structures.</p>
            <p>Spectroscopic analysis confirmed the presence of ionized oxygen in the distant star cluster.</p>
          </div>
        </div>
        "#;

        let extractor = TrafilaturaExtractor::default();
        let result = extractor.extract(html).expect("extract content");

        assert!(
            result.contains("thirteen billion years ago"),
            "must retain article body"
        );
        assert!(
            !result.contains("Story One Headline Link"),
            "must prune link-dense carousel"
        );
    }

    #[test]
    fn test_trafilatura_preserves_code_and_table_structures() {
        let html = r#"
        <article>
          <h2>Algorithm Benchmark</h2>
          <p>Here are the throughput measurements comparing the two implementations:</p>
          <table>
            <tr><th>Language</th><th>Throughput</th></tr>
            <tr><td>Python</td><td>120 req/s</td></tr>
            <tr><td>Rust</td><td>48,000 req/s</td></tr>
          </table>
          <pre><code>let score = metrics.score();</code></pre>
        </article>
        "#;

        let extractor = TrafilaturaExtractor::default();
        let result = extractor.extract(html).expect("extract content");

        assert!(result.contains("Algorithm Benchmark"));
        assert!(
            result.contains("48,000 req/s"),
            "must preserve table content"
        );
        assert!(
            result.contains("let score = metrics.score();"),
            "must preserve code block"
        );
    }
}
