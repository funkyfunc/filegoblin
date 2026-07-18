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
    } else if url.domain().is_some_and(|d| d.ends_with("github.com")) {
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
                Ok(html_content) => {
                    use crate::parsers::gobble::Gobble;
                    match (crate::parsers::html::HtmlGobbler {
                        extract_full: args.full,
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
                        Err(e) => eprintln!(
                            "{} Failed to extract web content from {}: {}",
                            "⚠️".yellow(),
                            url,
                            e
                        ),
                    }
                }
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
