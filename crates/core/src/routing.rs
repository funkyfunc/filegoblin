#![allow(clippy::ptr_arg)]
use anyhow::{Context, Result};
use colored::Colorize;
use std::io::Read;
use url::Url;

pub fn route_targets(
    targets: &[String],
    args: &crate::cli::Cli,
    tx: std::sync::mpsc::SyncSender<crate::pipeline::GobbleDocument>,
) -> Result<()> {
    if args.clipboard {
        if !args.quiet {
            eprintln!("📋 Reading from system clipboard...");
        }
        let mut clipboard =
            arboard::Clipboard::new().context("Failed to access system clipboard")?;
        let text = clipboard
            .get_text()
            .context("Failed to read text from clipboard")?;
        if !text.is_empty() {
            let _ = tx.send(crate::pipeline::GobbleDocument::extracted(
                "clipboard".to_string(),
                text,
                "text",
                None,
            ));
        }
    }

    if targets.is_empty() {
        return Ok(());
    }

    for target in targets {
        if target == "-" {
            route_stdin(&tx);
        } else if let Ok(url) = Url::parse(target) {
            if url.scheme() == "http" || url.scheme() == "https" {
                route_url_target(&url, target, args, &tx)?;
            } else {
                route_local_target(target, args, &tx);
            }
        } else {
            route_local_target(target, args, &tx);
        }
    }

    Ok(())
}

fn route_stdin(tx: &std::sync::mpsc::SyncSender<crate::pipeline::GobbleDocument>) {
    let mut buffer = String::new();
    if let Err(e) = std::io::stdin().read_to_string(&mut buffer) {
        eprintln!("{} Error reading stdin: {}", "⚠️".yellow(), e);
    }
    if !buffer.is_empty() {
        let _ = tx.send(crate::pipeline::GobbleDocument::extracted(
            "stdin".to_string(),
            buffer,
            "text",
            None,
        ));
    }
}

#[allow(unreachable_code, unused_variables)]
fn route_url_target(
    url: &Url,
    target: &str,
    args: &crate::cli::Cli,
    tx: &std::sync::mpsc::SyncSender<crate::pipeline::GobbleDocument>,
) -> Result<()> {
    if !args.quiet {
        eprintln!("🌐 Sniffing the network for files: {}", url);
    }
    if args.horde {
        #[cfg(feature = "web")]
        {
            match crate::parsers::crawler::crawl_web(url, args) {
                Ok(pairs) => {
                    for (p, c) in pairs {
                        let _ = tx.send(crate::pipeline::GobbleDocument::new(p, c));
                    }
                }
                Err(e) => {
                    eprintln!("{} Horde crawl failed for {}: {}", "⚠️".yellow(), url, e)
                }
            }
        }
        #[cfg(not(feature = "web"))]
        {
            anyhow::bail!(
                "Horde mode crawling requires the 'web' feature. Recompile with --features web."
            );
        }
    } else if is_github_repo_url(url) {
        if !args.quiet {
            eprintln!(
                "{}",
                "🐙 Cloning GitHub repository for deep ingestion...".truecolor(0, 200, 255)
            );
        }
        #[cfg(feature = "web")]
        {
            let temp_dir = tempfile::tempdir()?;
            if let Err(e) = crate::parsers::github::clone_github_repo(target, temp_dir.path()) {
                eprintln!("{} Failed to clone GitHub repo: {}", "⚠️".yellow(), e);
                return Ok(());
            }
            let mut repo_args = args.clone();
            repo_args.horde = true; // Force horde mode for a repository clone
            match crate::gobble_local(&temp_dir.path().to_string_lossy(), &repo_args) {
                Ok(docs) => {
                    for doc in docs {
                        let _ = tx.send(doc);
                    }
                }
                Err(e) => {
                    eprintln!("{} Failed to process cloned repo: {}", "⚠️".yellow(), e);
                }
            }
        }
        #[cfg(not(feature = "web"))]
        {
            anyhow::bail!(
                "GitHub cloning requires the 'web' feature. Recompile with --features web."
            );
        }
    } else {
        #[cfg(feature = "web")]
        {
            match crate::fetch_url(url, args.quiet) {
                Ok(body) => route_fetched_body(body, url, target, args, tx),
                Err(e) => {
                    eprintln!("{} Failed to fetch URL {}: {}", "⚠️".yellow(), url, e)
                }
            }
        }
        #[cfg(not(feature = "web"))]
        {
            anyhow::bail!(
                "Web extraction requires the 'web' feature. Recompile with --features web."
            );
        }
    }
    Ok(())
}

/// True only for an actual repository page on github.com (`github.com/owner/repo`).
/// `api.github.com`, `raw.githubusercontent.com`, gists, and bare `github.com`
/// are ordinary fetches, not clones.
fn is_github_repo_url(url: &Url) -> bool {
    let repo_host = matches!(url.domain(), Some("github.com") | Some("www.github.com"));
    if !repo_host {
        return false;
    }
    let segments: Vec<&str> = url
        .path_segments()
        .map(|s| s.filter(|p| !p.is_empty()).collect())
        .unwrap_or_default();
    segments.len() >= 2
}

/// Routes a fetched URL body by what it actually is, not by assuming HTML:
/// - HTML (declared, or sniffed when the type is vague) → the DOM extractor
/// - other textual types (markdown, plain text, JSON, source, …) → passthrough
/// - binary documents (PDF, docx, epub, zip, …) → a temp file through the
///   normal magic-byte parser pipeline, so `filegoblin https://…/paper.pdf`
///   works exactly like a local file.
#[cfg(feature = "web")]
fn route_fetched_body(
    body: crate::FetchedBody,
    url: &Url,
    target: &str,
    args: &crate::cli::Cli,
    tx: &std::sync::mpsc::SyncSender<crate::pipeline::GobbleDocument>,
) {
    use crate::parsers::gobble::Gobble;

    if body.looks_like_html() {
        // With --browser, the user's installed Chrome renders the page so
        // JS-built content exists in the DOM. SSRF note: the browser does not
        // honour our redirect guards; the initial host was already validated
        // by the reqwest fetch above, and the mode is strictly opt-in.
        let html_content = if args.browser {
            match crate::browser::render_url(url, args.quiet) {
                Ok(dom) => dom,
                Err(e) => {
                    eprintln!(
                        "{} Browser rendering failed ({}); using the static fetch",
                        "⚠️".yellow(),
                        e
                    );
                    body.decode_text()
                }
            }
        } else {
            body.decode_text()
        };
        match (crate::parsers::html::HtmlGobbler {
            extract_full: args.full,
            source_url: Some(target.to_string()),
            ..Default::default()
        })
        .gobble_str(
            &html_content,
            &crate::parsers::gobble::ParseOptions::from(args),
        ) {
            Ok(text) => {
                let _ = tx.send(crate::pipeline::GobbleDocument::extracted(
                    target.to_string(),
                    text,
                    "html",
                    None,
                ));
            }
            Err(e) => {
                eprintln!(
                    "{} Failed to extract web content from {}: {}",
                    "⚠️".yellow(),
                    url,
                    e
                );
                // In-band failure record (was silently dropped before): the
                // JSON contract sees it, and produced=0 still exits 4.
                let _ = tx.send(crate::pipeline::GobbleDocument::failed(
                    target.to_string(),
                    "html",
                    format!("{:#}", e),
                ));
            }
        }
        return;
    }

    if body.is_textual() {
        let text = body.decode_text();

        // RSS/Atom: render the entry list instead of dumping raw XML. Detected
        // by declared type OR by the root element, since plenty of feeds are
        // served as text/xml (or even text/plain).
        let declared_feed = body
            .media_type
            .as_deref()
            .is_some_and(|mt| mt.contains("rss") || mt.contains("atom"));
        if declared_feed || crate::parsers::feed::looks_like_feed(&text) {
            match crate::parsers::feed::FeedGobbler
                .gobble_str(&text, &crate::parsers::gobble::ParseOptions::from(args))
            {
                Ok(md) => {
                    let _ = tx.send(crate::pipeline::GobbleDocument::extracted(
                        target.to_string(),
                        md,
                        "feed",
                        None,
                    ));
                    return;
                }
                // Not really a feed after all — fall through to passthrough.
                Err(e) if declared_feed => {
                    eprintln!("{} Feed parse failed for {}: {}", "⚠️".yellow(), url, e);
                }
                Err(_) => {}
            }
        }

        let label = match body.media_type.as_deref() {
            Some("text/markdown") | Some("application/markdown") => "markdown",
            Some("application/json") => "json",
            _ => "text",
        };
        let _ = tx.send(crate::pipeline::GobbleDocument::extracted(
            target.to_string(),
            text,
            label,
            None,
        ));
        return;
    }

    // Binary document: stage it in a temp file (keeping the URL's extension so
    // extension-first detection still helps) and run the normal parser routing.
    let ext = std::path::Path::new(url.path())
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| format!(".{}", e))
        .unwrap_or_default();
    let staged = tempfile::Builder::new()
        .prefix("gobble_url_")
        .suffix(&ext)
        .tempfile();
    let staged = match staged {
        Ok(mut f) => {
            use std::io::Write;
            if let Err(e) = f.write_all(&body.bytes) {
                eprintln!(
                    "{} Failed to stage {} for parsing: {}",
                    "⚠️".yellow(),
                    url,
                    e
                );
                return;
            }
            f
        }
        Err(e) => {
            eprintln!(
                "{} Failed to stage {} for parsing: {}",
                "⚠️".yellow(),
                url,
                e
            );
            return;
        }
    };

    let (format, result) = crate::route_and_gobble(&staged.path().to_string_lossy(), args);
    let doc = match result {
        Ok(content) => crate::pipeline::GobbleDocument::extracted(
            target.to_string(),
            content,
            format.label(),
            Some(body.bytes.len() as u64),
        ),
        Err(e) => {
            eprintln!("{} Failed to parse {}: {}", "⚠️".yellow(), url, e);
            crate::pipeline::GobbleDocument::failed(
                target.to_string(),
                format.label(),
                e.to_string(),
            )
        }
    };
    let _ = tx.send(doc);
}

fn route_local_target(
    target: &str,
    args: &crate::cli::Cli,
    tx: &std::sync::mpsc::SyncSender<crate::pipeline::GobbleDocument>,
) {
    if !args.quiet {
        eprintln!("📁 Sniffing for files at local path: {}", target);
    }
    match crate::gobble_local(target, args) {
        Ok(docs) => {
            for doc in docs {
                let _ = tx.send(doc);
            }
        }
        Err(e) => eprintln!("{} Failed to sniff path {}: {}", "⚠️".yellow(), target, e),
    }
}
