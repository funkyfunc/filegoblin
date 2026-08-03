//! The engine behind `--walk`: what a node renders as, and what it leads to.
//!
//! Deliberately headless. Deciding *what* to show (convert this page, list this
//! directory, these are the outbound links) is library work with real edge
//! cases worth testing; deciding *how* it looks belongs to the binary. The
//! terminal UI in `crates/cli/src/walk.rs` is a thin driver over these two
//! functions.
//!
//! Both sources behave the same way from the caller's side:
//!
//! | node        | preview                    | children                  |
//! |-------------|----------------------------|---------------------------|
//! | URL         | the converted page         | same-origin outbound links|
//! | file        | the converted file         | none                      |
//! | directory   | a listing summary          | entries, dirs first       |

use crate::cli::Cli;
use std::path::PathBuf;

/// Somewhere the walker can stand.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Node {
    Url(String),
    Path(PathBuf),
}

impl Node {
    /// What `gobble_app` would be given for this node.
    pub fn target(&self) -> String {
        match self {
            Node::Url(u) => u.clone(),
            Node::Path(p) => p.to_string_lossy().into_owned(),
        }
    }

    /// Short label for the header line.
    pub fn display(&self) -> String {
        match self {
            Node::Url(u) => u.clone(),
            Node::Path(p) => p
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| p.to_string_lossy().into_owned()),
        }
    }

    pub fn is_dir(&self) -> bool {
        matches!(self, Node::Path(p) if p.is_dir())
    }
}

impl Node {
    /// Borrowed form of [`Node::target`], for comparisons in tests and UI code.
    pub fn target_ref(&self) -> &str {
        match self {
            Node::Url(u) => u.as_str(),
            Node::Path(p) => p.to_str().unwrap_or_default(),
        }
    }
}

/// What a node looks like, ready to put on screen.
pub struct Preview {
    pub title: String,
    /// Converted markdown, or a human-readable summary for a directory.
    pub body: String,
    pub tokens: usize,
    /// Set when conversion failed. The walker still shows the node so the user
    /// can move past it — an unconvertible page is information, not a dead end.
    pub error: Option<String>,
}

/// Somewhere the walker can go next.
pub struct Child {
    pub node: Node,
    /// Link text, or file name — what the user actually reads.
    pub label: String,
}

/// Renders `node` for display.
///
/// Never returns `Err`: a page that fails to convert becomes a `Preview` whose
/// `error` is set, because the walker has to keep working when one page in a
/// site is broken.
pub fn preview(node: &Node, args: &Cli) -> Preview {
    match node {
        Node::Path(p) if p.is_dir() => preview_dir(p),
        _ => preview_convertible(node, args),
    }
}

fn preview_dir(dir: &std::path::Path) -> Preview {
    let mut files = 0usize;
    let mut dirs = 0usize;
    let mut bytes = 0u64;
    if let Ok(entries) = std::fs::read_dir(dir) {
        for e in entries.flatten() {
            match e.metadata() {
                Ok(m) if m.is_dir() => dirs += 1,
                Ok(m) => {
                    files += 1;
                    bytes += m.len();
                }
                Err(_) => {}
            }
        }
    }
    Preview {
        title: dir.display().to_string(),
        body: format!(
            "Directory\n\n- {} file(s)\n- {} subdirector{}\n- {:.1} KB on disk\n\n\
             Select it to include everything beneath it, or step in to choose.",
            files,
            dirs,
            if dirs == 1 { "y" } else { "ies" },
            bytes as f64 / 1024.0
        ),
        tokens: 0,
        error: None,
    }
}

fn preview_convertible(node: &Node, args: &Cli) -> Preview {
    // One page / one file: never a crawl, whatever the caller's flags say.
    // Repeat views are free because URL fetches and OCR/transcripts are cached.
    let mut one = args.clone();
    one.horde = false;
    one.quiet = true;
    one.json = false;
    one.split = false;
    one.chunk = None;
    one.write = None;
    one.copy = false;
    one.open = false;

    let target = node.target();
    match crate::gobble_to_string(std::slice::from_ref(&target), &one) {
        Ok(body) => {
            let tokens = crate::compressor::heuristic::estimate_tokens(&body, &target);
            Preview {
                title: first_heading(&body).unwrap_or_else(|| node.display()),
                body,
                tokens,
                error: None,
            }
        }
        Err(e) => Preview {
            title: node.display(),
            body: String::new(),
            tokens: 0,
            error: Some(format!("{:#}", e)),
        },
    }
}

/// The first markdown heading, which is usually the page or document title.
fn first_heading(markdown: &str) -> Option<String> {
    markdown
        .lines()
        .find(|l| l.trim_start().starts_with('#'))
        .map(|l| l.trim_start_matches('#').trim().to_string())
        .filter(|t| !t.is_empty())
}

/// Where `node` can lead: outbound same-origin links for a page, directory
/// entries for a folder, nothing for a file.
pub fn children(node: &Node, args: &Cli) -> Vec<Child> {
    match node {
        Node::Path(p) if p.is_dir() => dir_children(p),
        Node::Path(_) => Vec::new(),
        Node::Url(u) => url_children(u, args),
    }
}

fn dir_children(dir: &std::path::Path) -> Vec<Child> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut kids: Vec<Child> = entries
        .flatten()
        .filter(|e| {
            // Dotfiles are noise in a picker, and .git would dwarf everything.
            !e.file_name().to_string_lossy().starts_with('.')
        })
        .map(|e| {
            let path = e.path();
            let is_dir = path.is_dir();
            let name = e.file_name().to_string_lossy().into_owned();
            Child {
                label: if is_dir { format!("{}/", name) } else { name },
                node: Node::Path(path),
            }
        })
        .collect();

    // Directories first, then alphabetical — the order someone expects to read.
    kids.sort_by(|a, b| {
        let a_dir = a.node.is_dir();
        let b_dir = b.node.is_dir();
        b_dir.cmp(&a_dir).then_with(|| a.label.cmp(&b.label))
    });
    kids
}

#[cfg(not(feature = "web"))]
fn url_children(_url: &str, _args: &Cli) -> Vec<Child> {
    Vec::new()
}

#[cfg(feature = "web")]
fn url_children(url: &str, args: &Cli) -> Vec<Child> {
    let Ok(parsed) = url::Url::parse(url) else {
        return Vec::new();
    };
    if crate::net::guard_url(&parsed).is_err() {
        return Vec::new();
    }
    // Cached from the preview fetch a moment ago, so this is not a second
    // round trip in practice.
    let Ok(body) = crate::fetch_url(&parsed, true) else {
        return Vec::new();
    };
    if !body.looks_like_html() {
        return Vec::new();
    }
    links_from_html(&body.decode_text(), &parsed, args)
}

/// Same-origin outbound links, de-duplicated, in document order.
#[cfg(feature = "web")]
pub(crate) fn links_from_html(html: &str, base: &url::Url, _args: &Cli) -> Vec<Child> {
    let Some(host) = base.host_str() else {
        return Vec::new();
    };
    let port = base.port_or_known_default();

    let document = scraper::Html::parse_document(html);
    let Ok(selector) = scraper::Selector::parse("a") else {
        return Vec::new();
    };

    let mut seen = std::collections::HashSet::new();
    let mut kids = Vec::new();
    for el in document.select(&selector) {
        let Some(href) = el.value().attr("href") else {
            continue;
        };
        // Reuses the crawler's scoping, so walking and crawling agree on what
        // counts as "this site" — including the port, which a bare host
        // comparison misses.
        let Some(next) = crate::parsers::crawler::normalize_link(base, href, host, port) else {
            continue;
        };
        let url = next.to_string();
        if url == base.as_str() || !seen.insert(url.clone()) {
            continue;
        }
        let text: String = el
            .text()
            .collect::<String>()
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        let label = if text.trim().is_empty() {
            next.path().to_string()
        } else {
            text
        };
        kids.push(Child {
            node: Node::Url(url),
            label,
        });
    }
    kids
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    fn args() -> Cli {
        Cli::parse_from(["filegoblin", "dummy"])
    }

    #[test]
    fn directory_children_list_dirs_first_and_skip_dotfiles() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("b.txt"), "b").unwrap();
        std::fs::write(dir.path().join("a.txt"), "a").unwrap();
        std::fs::write(dir.path().join(".hidden"), "x").unwrap();
        std::fs::create_dir(dir.path().join("zsub")).unwrap();

        let kids = children(&Node::Path(dir.path().to_path_buf()), &args());
        let labels: Vec<&str> = kids.iter().map(|c| c.label.as_str()).collect();
        assert_eq!(
            labels,
            vec!["zsub/", "a.txt", "b.txt"],
            "dirs first, then alphabetical"
        );
    }

    #[test]
    fn a_file_has_no_children_and_previews_its_content() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("notes.md");
        std::fs::write(&file, "# Title\n\nSome body text here.").unwrap();

        let node = Node::Path(file);
        assert!(children(&node, &args()).is_empty());

        let p = preview(&node, &args());
        assert!(p.error.is_none(), "got: {:?}", p.error);
        assert!(p.body.contains("Some body text"), "got: {}", p.body);
        assert_eq!(p.title, "Title", "the first heading is the title");
        assert!(p.tokens > 0);
    }

    #[test]
    fn a_directory_previews_as_a_listing_not_a_conversion() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("one.txt"), "hello").unwrap();
        std::fs::create_dir(dir.path().join("sub")).unwrap();

        let p = preview(&Node::Path(dir.path().to_path_buf()), &args());
        assert!(p.error.is_none());
        assert!(p.body.contains("1 file(s)"), "got: {}", p.body);
        assert!(p.body.contains("1 subdirectory"), "got: {}", p.body);
    }

    /// An unconvertible node must still be displayable — one broken page in a
    /// site cannot be allowed to end the walk.
    #[test]
    fn a_failing_node_previews_as_an_error_rather_than_blowing_up() {
        let p = preview(
            &Node::Path(PathBuf::from("/definitely/not/here.pdf")),
            &args(),
        );
        assert!(p.error.is_some(), "expected a captured error");
        assert!(p.body.is_empty());
    }

    #[cfg(feature = "web")]
    #[test]
    fn page_links_are_same_origin_deduped_and_labelled() {
        let base = url::Url::parse("https://example.com/docs/").unwrap();
        let html = r#"<html><body>
            <a href="guide.html">Getting Started</a>
            <a href="guide.html">Getting Started (again)</a>
            <a href="/api/auth">Authentication</a>
            <a href="https://other.com/x">Offsite</a>
            <a href="mailto:a@example.com">Mail</a>
            <a href="https://example.com:8443/admin">Other port</a>
            <a href="/bare"></a>
        </body></html>"#;

        let kids = links_from_html(html, &base, &args());
        let urls: Vec<&str> = kids.iter().map(|c| c.node.target_ref()).collect();

        assert!(urls.contains(&"https://example.com/docs/guide.html"));
        assert!(urls.contains(&"https://example.com/api/auth"));
        assert_eq!(
            urls.iter().filter(|u| u.ends_with("guide.html")).count(),
            1,
            "duplicate links collapse"
        );
        assert!(!urls.iter().any(|u| u.contains("other.com")), "offsite");
        assert!(!urls.iter().any(|u| u.contains("8443")), "other port");
        assert!(!urls.iter().any(|u| u.starts_with("mailto")), "mailto");

        // A link with no text falls back to its path so the row is never blank.
        let bare = kids.iter().find(|c| c.node.target().ends_with("/bare"));
        assert_eq!(bare.map(|c| c.label.as_str()), Some("/bare"));
    }
}
