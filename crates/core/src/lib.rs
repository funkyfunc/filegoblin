pub mod cli;
pub mod compressor;
pub mod cost;
pub mod curation;
pub mod flavors;
pub mod parsers;
pub mod pipeline;
pub mod privacy_shield;

use crate::parsers::gobble::Gobble;

use anyhow::{Context, Result};
use colored::Colorize;
use std::path::Path;

/// filegoblin Core
/// We keep logic in lib.rs to ensure the application is deeply testable
/// independently from the `clap` CLI layer.
#[allow(clippy::too_many_arguments)]
pub mod formatters;
pub mod routing;

pub fn gobble_app(
    targets: &[String],
    flavor: &flavors::Flavor,
    args: &crate::cli::Cli,
) -> Result<()> {
    let (tx, rx) = std::sync::mpsc::sync_channel(100);

    let targets_clone = targets.to_vec();
    let args_clone = args.clone();
    std::thread::spawn(move || {
        let _ = routing::route_targets(&targets_clone, &args_clone, tx);
    });

    let display_name = if targets.len() == 1 {
        targets[0].clone()
    } else if targets.len() == 2 {
        format!("{} & {}", targets[0], targets[1])
    } else if args.clipboard && targets.is_empty() {
        "clipboard".to_string()
    } else {
        format!("{} and {} others", targets[0], targets.len() - 1)
    };

    // If --search is used, we must load everything into memory to build the RAG index
    if args.search.is_some() {
        let raw_documents: Vec<crate::pipeline::GobbleDocument> = rx.into_iter().collect();
        let pruned_docs =
            crate::pipeline::stages::run_curation_stage(raw_documents, args, targets.len())?;
        let final_docs = crate::pipeline::stages::run_privacy_stage(pruned_docs, args)?;
        let (compressed_docs, pre_compression_tokens) =
            crate::pipeline::stages::run_compression_stage(final_docs, args)?;
        formatters::format_and_output(
            compressed_docs,
            pre_compression_tokens,
            flavor,
            &display_name,
            args,
        )
    } else {
        // True Out-of-Core Stream Processing
        // For streaming, formatters must process documents one by one.
        let shield = if args.scrub {
            crate::privacy_shield::PrivacyShield::init().ok()
        } else {
            None
        };
        let stream = rx.into_iter().map(move |mut doc| {
            if let Some(s) = &shield {
                doc.content = s.redact(&doc.content);
            }
            if let Some(level) = &args.compress {
                doc.content = crate::compressor::CompressionPipeline::new(level, Some(&doc.path))
                    .process(&doc.content);
            }
            doc
        });
        formatters::stream_and_output(stream, flavor, &display_name, args)
    }
}

pub fn gobble_local(target: &str, args: &crate::cli::Cli) -> Result<Vec<(String, String)>> {
    let mut files = Vec::new();
    let root = std::path::Path::new(target);

    // ARCHIVE INTERCEPTION
    if root.is_file() {
        let ext = root
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_lowercase();
        if ext == "zip" || ext == "gz" || ext == "tgz" {
            let archive_gobbler = crate::parsers::archive::ArchiveGobbler;
            return archive_gobbler.extract_archive(root, args);
        }
    }

    if !root.is_dir() || !args.horde {
        match route_and_gobble(target, args) {
            Ok(content) => files.push((target.to_string(), content)),
            Err(e) => eprintln!("{} Error summarizing {}: {}", "⚠️".yellow(), target, e),
        }
        return Ok(files);
    }

    let mut walk_builder = ignore::WalkBuilder::new(target);
    if let Some(max_depth) = args.depth {
        walk_builder.max_depth(Some(max_depth));
    }
    let walker = walk_builder.build();

    let mut tree_out = String::new();
    tree_out.push_str("```tree\n.");

    let mut files_to_process = Vec::new();

    for result in walker {
        let entry = match result {
            Ok(e) => e,
            Err(_) => continue,
        };

        let path = entry.path();
        let depth = entry.depth();

        let prefix = " ".repeat(depth * 2);
        let name = path.file_name().unwrap_or_default().to_string_lossy();
        if path.is_dir() {
            if depth > 0 {
                tree_out.push_str(&format!("\n{}├── {}/", prefix, name));
            }
        } else {
            tree_out.push_str(&format!("\n{}├── {}", prefix, name));
            files_to_process.push(path.to_path_buf());
        }
    }
    tree_out.push_str("\n```\n\n");
    files.push(("_tree.md".to_string(), tree_out));

    // PASS 1: PageRank Graphing for --max-tokens cutoff
    if let Some(budget) = args.max_tokens {
        if !args.quiet {
            eprintln!(
                "{}",
                "🕸️  Building Dependency Graph for PageRank pruning...".truecolor(255, 99, 71)
            );
        }
        let mut docs_for_pagerank = Vec::new();
        let mut total_tokens = 0;

        for path in &files_to_process {
            if let Ok(content) = std::fs::read_to_string(path) {
                let tokens = crate::compressor::heuristic::estimate_tokens(
                    &content,
                    &path.to_string_lossy(),
                );
                total_tokens += tokens;
                let path_str = path.to_string_lossy().to_string();
                docs_for_pagerank.push((path_str, content, tokens));
            }
        }

        if total_tokens > budget {
            let pr_input: Vec<(&String, &String)> =
                docs_for_pagerank.iter().map(|(p, c, _)| (p, c)).collect();
            let ranks = crate::curation::pagerank::compute_pagerank(&pr_input);

            docs_for_pagerank.sort_by(|(path_a, _, _), (path_b, _, _)| {
                let rank_a = ranks.get(path_a).unwrap_or(&0.0);
                let rank_b = ranks.get(path_b).unwrap_or(&0.0);
                // Sort descending by rank
                rank_b
                    .partial_cmp(rank_a)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });

            let mut kept_budget = 0;
            let mut kept_paths = std::collections::HashSet::new();
            for (path, _, tokens) in docs_for_pagerank {
                if kept_budget + tokens <= budget {
                    kept_paths.insert(path);
                    kept_budget += tokens;
                }
            }
            let before = files_to_process.len();
            files_to_process.retain(|p| kept_paths.contains(&p.to_string_lossy().to_string()));
            if !args.quiet {
                eprintln!(
                    "{}",
                    format!(
                        "⚔️ PageRank pruned from {} to {} files.",
                        before,
                        files_to_process.len()
                    )
                    .truecolor(255, 99, 71)
                );
            }
        }
    }

    // Process all collected files

    // HARDCODED Security & Privacy Exclusion
    let mut skipped = 0;
    files_to_process.retain(|p| {
        let name = p.file_name().unwrap_or_default().to_string_lossy();
        let path_str = p.to_string_lossy();
        let is_radioactive = matches!(
            name.as_ref(),
            ".env"
                | ".env.local"
                | ".env.development"
                | ".env.test"
                | ".env.production"
                | "id_rsa"
                | "id_ed25519"
                | "credentials.json"
        ) || name.ends_with(".pem")
            || path_str.contains(".aws/credentials")
            || path_str.contains(".ssh/");

        if is_radioactive {
            skipped += 1;
        }

        !is_radioactive
    });

    if skipped > 0 && !args.quiet {
        use colored::Colorize;
        eprintln!(
            "{}",
            format!("🛡️ SKIPPED {} RADIOACTIVE FILE(S)", skipped).truecolor(255, 0, 0)
        );
    }
    // Apply --include glob filtering if specified
    if !args.include.is_empty() {
        let before_count = files_to_process.len();
        files_to_process.retain(|p| {
            let name = p.file_name().unwrap_or_default().to_string_lossy();
            args.include.iter().any(|pattern| {
                // Support both "*.rs" style and plain ".rs" style patterns
                if let Some(ext_pattern) = pattern.strip_prefix("*.") {
                    name.ends_with(&format!(".{}", ext_pattern))
                } else if pattern.starts_with('.') {
                    name.ends_with(pattern)
                } else {
                    name.contains(pattern.as_str())
                }
            })
        });
        if !args.quiet {
            eprintln!(
                "{}",
                format!(
                    "🎯 Filtered horde from {} to {} files matching: {}",
                    before_count,
                    files_to_process.len(),
                    args.include.join(", ")
                )
                .truecolor(0, 200, 255)
            );
        }
    }

    // Apply --exclude glob filtering if specified
    if !args.exclude.is_empty() {
        let before_count = files_to_process.len();
        files_to_process.retain(|p| {
            let name = p.file_name().unwrap_or_default().to_string_lossy();
            !args.exclude.iter().any(|pattern| {
                if let Some(ext_pattern) = pattern.strip_prefix("*.") {
                    name.ends_with(&format!(".{}", ext_pattern))
                } else if pattern.starts_with('.') {
                    name.ends_with(pattern)
                } else if pattern.contains('*') {
                    // Simple wildcard matching: *test* style
                    let parts: Vec<&str> = pattern.split('*').collect();
                    parts
                        .iter()
                        .all(|part| part.is_empty() || name.contains(part))
                } else {
                    name.contains(pattern.as_str())
                }
            })
        });
        if !args.quiet {
            eprintln!(
                "{}",
                format!(
                    "🚫 Excluded {} files matching: {}",
                    before_count - files_to_process.len(),
                    args.exclude.join(", ")
                )
                .truecolor(255, 99, 71)
            );
        }
    }

    // Apply --git-diff filtering if specified
    if let Some(ref git_ref) = args.git_diff {
        let git_result = std::process::Command::new("git")
            .args(["diff", "--name-only", git_ref])
            .current_dir(root)
            .output();

        match git_result {
            Ok(output) if output.status.success() => {
                let changed_files: Vec<String> = String::from_utf8_lossy(&output.stdout)
                    .lines()
                    .map(|l| l.to_string())
                    .collect();
                let before_count = files_to_process.len();
                files_to_process.retain(|p| {
                    if let Ok(rel) = p.strip_prefix(root) {
                        let rel_str = rel.to_string_lossy();
                        changed_files.iter().any(|cf| rel_str == cf.as_str())
                    } else {
                        false
                    }
                });
                if !args.quiet {
                    eprintln!(
                        "{}",
                        format!(
                            "🔀 Git diff mode (vs {}): {} changed files found (from {} total)",
                            git_ref,
                            files_to_process.len(),
                            before_count
                        )
                        .truecolor(0, 200, 255)
                    );
                }
            }
            Ok(output) => {
                let stderr_msg = String::from_utf8_lossy(&output.stderr);
                eprintln!("{} git diff failed: {}", "⚠️".yellow(), stderr_msg.trim());
            }
            Err(e) => {
                eprintln!(
                    "{} Could not run git (is it installed?): {}",
                    "⚠️".yellow(),
                    e
                );
            }
        }
    }

    use rayon::prelude::*;
    let parsed_files: Vec<(String, String)> = files_to_process
        .into_par_iter()
        .filter_map(|file_path| {
            if let Ok(rel_path) = file_path.strip_prefix(if root.is_file() {
                root.parent().unwrap_or(root)
            } else {
                root
            }) {
                let rel_str = rel_path.to_string_lossy().into_owned();

                // --diff-format: use unified diff output instead of full file content
                let content = if args.diff_format {
                    if let Some(ref git_ref) = args.git_diff {
                        let diff_result = std::process::Command::new("git")
                            .args(["diff", git_ref, "--", file_path.to_str().unwrap()])
                            .current_dir(root)
                            .output();
                        match diff_result {
                            Ok(output) if output.status.success() => {
                                let diff_text = String::from_utf8_lossy(&output.stdout).to_string();
                                if diff_text.trim().is_empty() {
                                    None
                                } else {
                                    Some(diff_text)
                                }
                            }
                            _ => None,
                        }
                    } else {
                        match route_and_gobble(file_path.to_str().unwrap(), args) {
                            Ok(c) => Some(c),
                            Err(e) => {
                                eprintln!("{} Error summarizing {}: {}", "⚠️".yellow(), rel_str, e);
                                None
                            }
                        }
                    }
                } else {
                    match route_and_gobble(file_path.to_str().unwrap(), args) {
                        Ok(c) => Some(c),
                        Err(e) => {
                            eprintln!("{} Error summarizing {}: {}", "⚠️".yellow(), rel_str, e);
                            None
                        }
                    }
                };

                content.map(|c| (rel_str, c))
            } else {
                None
            }
        })
        .collect();

    files.extend(parsed_files);

    Ok(files)
}

#[allow(unused_variables)]
fn route_and_gobble(path_str: &str, args: &crate::cli::Cli) -> Result<String> {
    let path = Path::new(path_str);
    let extension = path
        .extension()
        .and_then(|s| s.to_str())
        .map(|s| s.to_lowercase())
        .unwrap_or_default();

    // EXPLICIT OVERRIDE: If the user passed `--plugin <NAME>`, force execution through that WASM component.
    if let Some(plugin_name) = args.plugin.as_deref() {
        #[cfg(feature = "wasm")]
        {
            if let Some(plugin_path) = parsers::wasm::WasmGobbler::sniff(plugin_name) {
                match (parsers::wasm::WasmGobbler {
                    wasm_path: plugin_path,
                })
                .gobble(path, args)
                {
                    Ok(markdown) => return Ok(markdown),
                    Err(e) => anyhow::bail!("Explicit WASM Plugin '{}' failed: {}", plugin_name, e),
                }
            } else {
                anyhow::bail!(
                    "Requested explicit --plugin '{}' could not be found in ~/.filegoblin/plugins/ or ./plugins/",
                    plugin_name
                );
            }
        }
        #[cfg(not(feature = "wasm"))]
        {
            anyhow::bail!(
                "Explicit WASM Plugins require the 'wasm' feature. Recompile with --features wasm."
            );
        }
    }

    match extension.as_str() {
        "epub" => {
            #[cfg(feature = "web")]
            {
                parsers::epub::EpubGobbler.gobble(path, args)
            }
            #[cfg(not(feature = "web"))]
            {
                anyhow::bail!(
                    "Parsing EPUB files requires the 'web' feature. Recompile with --features web."
                );
            }
        }
        "ipynb" => parsers::jupyter::JupyterGobbler.gobble(path, args),
        "pdf" => parsers::pdf::PdfGobbler.gobble(path, args),
        "docx" => parsers::office::OfficeGobbler.gobble(path, args),
        "xlsx" | "xls" | "ods" | "csv" => parsers::sheet::SheetGobbler.gobble(path, args),
        "pptx" => parsers::powerpoint::PptxGobbler.gobble(path, args),
        "html" | "htm" => {
            #[cfg(feature = "web")]
            {
                parsers::web::WebGobbler {
                    extract_full: args.full,
                }
                .gobble(path, args)
            }
            #[cfg(not(feature = "web"))]
            {
                anyhow::bail!(
                    "Parsing HTML files requires the 'web' feature. Recompile with --features web, or use --fallback."
                );
            }
        }
        "rs" | "js" | "py" | "ts" | "go" | "c" | "cpp" => {
            parsers::code::CodeGobbler.gobble(path, args)
        }
        _ => {
            // Priority 1: Check if a user provided a dynamic WASM plugin for this extension
            #[allow(clippy::collapsible_if)]
            #[cfg(feature = "wasm")]
            if let Some(plugin_path) = parsers::wasm::WasmGobbler::sniff(&extension) {
                if let Ok(markdown) = (parsers::wasm::WasmGobbler {
                    wasm_path: plugin_path,
                })
                .gobble(path, args)
                {
                    return Ok(markdown);
                }
            }

            // Priority 2: Fall back to core heuristics
            // If it's an image, pass to ocr. If text, just read.
            if ["png", "jpg", "jpeg", "webp"].contains(&extension.as_str()) {
                #[cfg(feature = "ocr")]
                {
                    parsers::ocr::OcrGobbler.gobble(path, args)
                }
                #[cfg(not(feature = "ocr"))]
                {
                    anyhow::bail!(
                        "OCR extraction requires the 'ocr' feature. Recompile with --features ocr."
                    );
                }
            } else {
                std::fs::read_to_string(path).context("Failed to read file as plaintext")
            }
        }
    }
}

#[cfg(feature = "web")]
fn fetch_url(url: &Url, quiet: bool) -> Result<String> {
    // reqwest::blocking::Client automatically respects HTTP_PROXY and HTTPS_PROXY
    if let Ok(proxy) = std::env::var("HTTPS_PROXY").or_else(|_| std::env::var("HTTP_PROXY"))
        && !quiet
    {
        eprintln!("🔒 Corporate proxy detected: routing through {}", proxy);
    }

    let client = reqwest::blocking::Client::builder()
        .user_agent("filegoblin/1.5.0")
        .build()?;

    let resp = client
        .get(url.clone())
        .send()
        .context("Mischievous network error: Failed to reach the URL")?;

    let text = resp
        .text()
        .context("Mischievous decoding error: Failed to read URL body as UTF-8")?;

    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;
    use std::io::Write;
    use std::net::TcpListener;
    use std::thread;

    #[test]
    #[cfg(feature = "web")]
    fn test_fetch_url_and_gobble() {
        // Start a dummy local server to test the network engine without an external dependency
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let url_str = format!("http://127.0.0.1:{}/", port);

        thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                use std::io::Read;
                let mut buf = [0; 1024];
                let _ = stream.read(&mut buf);

                let body = b"<html><body><article>Goblin network testing!</article></body></html>";
                let header = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                let _ = stream.write_all(header.as_bytes());
                let _ = stream.write_all(body);
            }
        });

        let parsed_url = Url::parse(&url_str).unwrap();
        let html_content = fetch_url(&parsed_url, true).unwrap();
        assert!(html_content.contains("Goblin network testing!"));

        let args = crate::cli::Cli::parse_from(["filegoblin", "dummy"]);
        let parsed_content = crate::parsers::web::WebGobbler {
            extract_full: false,
        }
        .gobble_str(&html_content, &args)
        .unwrap();
        assert!(parsed_content.contains("Goblin network testing!"));
    }

    #[test]
    fn test_route_and_gobble_unknown_extension() {
        // Fallback for an unknown extension (.xyz)
        let test_file = "dummy.xyz";
        std::fs::write(test_file, "Plaintext fallback text").unwrap();
        let args = crate::cli::Cli::parse_from(["filegoblin", "dummy"]);
        let res = route_and_gobble(test_file, &args).unwrap();
        assert_eq!(res, "Plaintext fallback text");
        std::fs::remove_file(test_file).ok();
    }
}
