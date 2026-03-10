pub mod cli;
pub mod flavors;
pub mod parsers;
pub mod privacy_shield;
pub mod compressor;
pub mod curation;

use crate::parsers::gobble::Gobble;
use anyhow::{Context, Result};
use colored::Colorize;
use std::path::Path;
use url::Url;
use std::io::Read;

// The pulldown-cmark imports are needed for pipeline processing
use pulldown_cmark::{Parser, Event, Tag, TagEnd};
use pulldown_cmark_to_cmark::cmark;

/// filegoblin Core
/// We keep logic in lib.rs to ensure the application is deeply testable
/// independently from the `clap` CLI layer.
#[allow(clippy::too_many_arguments)]
pub fn gobble_app(
    targets: &[String],
    flavor: &flavors::Flavor,
    args: &crate::cli::Cli,
) -> Result<()> {
    if args.twitter_login {
        crate::parsers::twitter::handle_twitter_login()?;
        return Ok(());
    }

    if targets.is_empty() {
        return Ok(());
    }

    let display_name = if targets.len() == 1 {
        targets[0].clone()
    } else if targets.len() == 2 {
        format!("{} & {}", targets[0], targets[1])
    } else {
        format!("{} and {} others", targets[0], targets.len() - 1)
    };
    
    let mut raw_pairs: Vec<(String, String)> = Vec::new();

    for target in targets {
        if target == "-" {
            // Read all bytes from stdin
            let mut buffer = String::new();
            if let Err(e) = std::io::stdin().read_to_string(&mut buffer) {
                eprintln!("{} Error reading stdin: {}", "⚠️".yellow(), e);
            }
            if !buffer.is_empty() {
                raw_pairs.push(("stdin".to_string(), buffer));
            }
            continue;
        }

        if let Ok(url) = Url::parse(target) {
            if url.scheme() == "http" || url.scheme() == "https" {
                if !args.quiet {
                    eprintln!("🌐 Sniffing the network for files: {}", url);
                }
                if args.horde {
                    raw_pairs.extend(crate::parsers::crawler::crawl_web(&url, args)?);
                } else if url.domain().map_or(false, |d| d.ends_with("github.com")) {
                    if !args.quiet {
                        eprintln!("{}", "🐙 Cloning GitHub repository for deep ingestion...".truecolor(0, 200, 255));
                    }
                    let temp_dir = std::env::temp_dir().join(format!("filegoblin_gh_{}", std::time::UNIX_EPOCH.elapsed().unwrap().as_nanos()));
                    std::fs::create_dir_all(&temp_dir)?;
                    if let Err(e) = parsers::github::clone_github_repo(target, &temp_dir) {
                        let _ = std::fs::remove_dir_all(&temp_dir);
                        anyhow::bail!("Failed to clone GitHub repo: {}", e);
                    }
                    let mut repo_args = args.clone();
                    repo_args.horde = true; // Force horde mode for a repository clone
                    match gobble_local(&temp_dir.to_string_lossy(), &repo_args) {
                        Ok(local_pairs) => {
                            raw_pairs.extend(local_pairs);
                        }
                        Err(e) => {
                            let _ = std::fs::remove_dir_all(&temp_dir);
                            anyhow::bail!("Failed to process cloned repo: {}", e);
                        }
                    }
                    let _ = std::fs::remove_dir_all(&temp_dir);
                } else if url.domain().map_or(false, |d| d.ends_with("youtube.com") || d.ends_with("youtu.be")) {
                    if !args.quiet {
                        eprintln!("▶️ Diverting to YouTubeGobbler to extract video transcripts...");
                    }
                    let youtube = parsers::youtube::YouTubeGobbler::new();
                    let text = youtube.gobble_str(target, args)?;
                    raw_pairs.push((target.to_string(), text));
                } else if url.domain().map_or(false, |d| d.ends_with("twitter.com") || d.ends_with("x.com")) {
                    if !args.quiet {
                        eprintln!("🐦 Diverting to TwitterGobbler for deep context extraction...");
                    }
                    let twitter = parsers::twitter::TwitterGobbler { flavor: flavor.clone() };
                    let text = twitter.gobble_str(target, args)?;
                    raw_pairs.push((target.to_string(), text));
                } else {
                    let html_content = fetch_url(&url, args.quiet)?;
                    let text =
                        parsers::web::WebGobbler { extract_full: args.full }.gobble_str(&html_content, args)?;
                    raw_pairs.push((target.to_string(), text));
                }
            } else {
                if !args.quiet {
                    eprintln!("📁 Sniffing for files at local path: {}", target);
                }
                raw_pairs.extend(gobble_local(target, args)?);
            }
        } else {
            if !args.quiet {
                eprintln!("📁 Sniffing for files at local path: {}", target);
            }
            raw_pairs.extend(gobble_local(target, args)?);
        }
    }

    // Execute Curation Intelligence pipelines before generic formatting steps (RAG/BM25 & Auto-Pruning)
    let (searched_pairs, search_scores) = if let Some(query) = args.search.as_deref() {
        if !args.quiet {
            eprintln!(
                "{}",
                format!("🧠 Searching {} files in the hoard for '{}' (Semantic Priority)...", raw_pairs.len(), query).truecolor(0, 200, 255)
            );
        }
        let scored_results = curation::semantic_search(raw_pairs, query, 3).context("Semantic Search failed")?;
        let scores: std::collections::HashMap<String, f32> = scored_results.iter().map(|(s, p, _)| (p.clone(), *s)).collect();
        let pairs: Vec<(String, String)> = scored_results.into_iter().map(|(_, p, c)| (p, c)).collect();
        (pairs, Some(scores))
    } else {
        (raw_pairs, None)
    };

    let pruned_pairs = if let Some(budget) = args.max_tokens {
        if !args.quiet {
            eprintln!(
                "{}",
                format!("✂️ Enforcing rigid context budget ({} tokens)...", budget).truecolor(255, 99, 71)
            );
        }
        let (kept_pairs, initial_tokens, kept_budget) = curation::enforce_budget(searched_pairs, budget, !args.quiet);
        if !args.quiet {
            eprintln!(
                "{}",
                format!("⚔️ Pruned from {} baseline tokens down to {} tokens (fit {}/{} target files).", initial_tokens, kept_budget, kept_pairs.len(), targets.len()).truecolor(255, 99, 71)
            );
        }
        kept_pairs
    } else {
        searched_pairs
    };

    // Apply Privacy Shield if --scrub is requested
    let final_pairs = if args.scrub {
        if !args.quiet {
            eprintln!(
                "{}",
                "🛡️ Scrubbed the secrets...".truecolor(255, 191, 0)
            );
        }
        let shield =
            privacy_shield::PrivacyShield::init().context("Failed to initialize Privacy Shield")?;
        pruned_pairs
            .into_iter()
            .map(|(p, c)| (p, shield.redact(&c)))
            .collect()
    } else {
        pruned_pairs
    };

    // Calculate baseline tokens BEFORE compression for savings metric
    let pre_compression_tokens: usize = final_pairs
        .iter()
        .map(|(path, content)| crate::compressor::heuristic::estimate_tokens(content, path))
        .sum();

    // Apply Compression Pipeline if --compress is flag
    let compressed_pairs = if let Some(level) = args.compress.as_ref() {
        if !args.quiet {
            eprintln!("{}", format!("🗜️ Shrinking the loot (Level: {:?})...", level).truecolor(0, 200, 255));
        }
        final_pairs.into_iter().map(|(path, content)| {
             // Avoid double-compressing the _tree.md as the ASCII structure breaks easily
            if path == "_tree.md" {
                 return (path, content);
            }

            let mut mapped_events = Vec::new();
            let parser = Parser::new(&content);
            let mut current_lang = None;
            
            // Build the default pipeline for prose/mixed content
            let default_pipeline = crate::compressor::CompressionPipeline::new(level, None);

            for event in parser {
                match event {
                    Event::Start(Tag::CodeBlock(pulldown_cmark::CodeBlockKind::Fenced(ref lang))) => {
                        current_lang = Some(lang.to_string());
                        mapped_events.push(event.clone());
                    }
                    Event::End(TagEnd::CodeBlock) => {
                        current_lang = None;
                        mapped_events.push(event.clone());
                    }
                    Event::Text(ref text) => {
                         // Apply code-block specific compression or default prose compression
                         let transformed = if let Some(lang) = &current_lang {
                              let pipeline = crate::compressor::CompressionPipeline::new(level, Some(lang));
                              pipeline.process(text)
                         } else {
                              default_pipeline.process(text)
                         };
                         mapped_events.push(Event::Text(pulldown_cmark::CowStr::Boxed(transformed.into_boxed_str())));
                    }
                    _ => {
                        mapped_events.push(event.clone());
                    }
                }
            }
            
            let mut minified_markdown = String::with_capacity(content.len());
            cmark(mapped_events.into_iter(), &mut minified_markdown).unwrap();
            
            // Always run Level 1 folder globally on the final markdown output String to clean up layout markers
            let final_pipeline = crate::compressor::CompressionPipeline::new(level, None);
            let fully_compressed = final_pipeline.process(&minified_markdown);
            (path, fully_compressed)
        }).collect()
    } else {
        final_pairs
    };

    if args.split {
        let root_dir_name = display_name
            .replace("https://", "")
            .replace("http://", "")
            .replace("/", "_")
            .replace(" ", "_")
            .replace("&", "and");
        let root_dir = args.write.clone().unwrap_or_else(|| format!("{}_gobbled", root_dir_name));
        std::fs::create_dir_all(&root_dir)?;

        if !args.quiet {
            eprintln!(
                "{}",
                format!("💾 Stashing the split loot into directory: ./{}", root_dir).truecolor(0, 255, 100)
            );
        }

        let mut post_compression_tokens = 0;
        let mut total_chars = 0;

        for (path, content) in compressed_pairs.iter() {
            // Safe pathing logic for urls and relative filepaths
            let safe_path = if let Ok(u) = Url::parse(path) {
                let mut p = u.path().trim_start_matches('/').to_string();
                if p.is_empty() {
                    p = "index".to_string();
                }
                if p.ends_with('/') {
                    p.push_str("index");
                }
                p.replace("..", "").replace(":", "_")
            } else {
                path.replace("..", "")
            };

            let file_name = if safe_path.ends_with(".md") {
                safe_path
            } else {
                format!("{}.md", safe_path)
            };
            let file_path = std::path::Path::new(&root_dir).join(file_name);

            if let Some(parent) = file_path.parent() {
                std::fs::create_dir_all(parent)?;
            }

            let flavored = flavors::format_output(flavor, path, content);
            std::fs::write(&file_path, &flavored)?;

            if !args.quiet {
                eprintln!("  ↳ Spat out {}", file_path.display());
            }

            total_chars += flavored.len();
            post_compression_tokens += crate::compressor::heuristic::estimate_tokens(&flavored, path);
        }

        if args.tokens {
            if args.quiet {
                eprintln!("{}", post_compression_tokens);
            } else {
                eprintln!(
                    "{}",
                    format!(
                        "📊 Total Output Length: {} chars (~{} tokens)",
                        total_chars, post_compression_tokens
                    )
                    .truecolor(255, 191, 0)
                );
            }
        }

        if args.open {
            if !args.quiet {
                eprintln!("{}", format!("🚪 Opening directory: ./{}", root_dir).truecolor(0, 200, 255));
            }
            if let Err(e) = open::that(&root_dir) {
                eprintln!("{} Failed to open directory: {}", "⚠️".yellow(), e);
            }
        }
    } else if args.json {
        // Build strictly structured JSON array format matching our (String, String) pairs
        #[derive(serde::Serialize)]
        struct FileNode {
            path: String,
            content: String,
        }

        let mut out = Vec::new();
        for (p, c) in compressed_pairs {
            out.push(FileNode {
                path: p,
                content: c,
            });
        }
        let serialized = serde_json::to_string_pretty(&out)?;

        if let Some(file_path) = args.write.as_deref() {
            std::fs::write(file_path, &serialized)?;
            if !args.quiet {
                eprintln!("{}", format!("💾 Saved JSON output to {}", file_path).truecolor(0, 255, 100));
            }
        } else {
            // Print strictly the JSON to standard out
            println!("{}", serialized);
        }

        if args.copy {
            let mut clipboard = arboard::Clipboard::new()?;
            clipboard.set_text(serialized)?;
            if !args.quiet {
                eprintln!("{}", "📋 Stashed the JSON loot in your clipboard!".truecolor(0, 255, 100));
            }
        }
    } else {
        // Parse token threshold if chunk is enabled
        let token_threshold: Option<usize> = args.chunk.as_deref().and_then(|c| {
            let c = c.trim().to_lowercase();
            if c.ends_with('k') {
                c.trim_end_matches('k').parse::<f64>().ok().map(|n| (n * 1_000.0) as usize)
            } else if c.ends_with('m') {
                c.trim_end_matches('m').parse::<f64>().ok().map(|n| (n * 1_000_000.0) as usize)
            } else {
                c.parse::<usize>().ok()
            }
        });

        if let Some(threshold) = token_threshold {
            if !args.quiet {
                eprintln!("{}", format!("🍰 Chunking output at ~{} tokens per file", threshold).truecolor(0, 200, 255));
            }
            
            let mut part_number = 1;
            let mut current_combined = String::new();
            let mut current_tokens = 0;
            let mut post_compression_tokens = 0;
            let mut total_chars = 0;
            let file_count = compressed_pairs.len();
            
            for (i, (path, content)) in compressed_pairs.into_iter().enumerate() {
                let file_tokens = crate::compressor::heuristic::estimate_tokens(&content, &path);
                post_compression_tokens += file_tokens;
                
                // If adding this file pushes us over the threshold AND we already have content in this chunk
                if current_tokens + file_tokens > threshold && !current_combined.is_empty() {
                    // Flush current chunk
                    let output = flavors::format_output(flavor, &display_name, &current_combined);
                    total_chars += output.len();
                    
                    let part_file_name = args.write
                        .as_deref()
                        .map(|o| format!("{}.part{}.md", o.trim_end_matches(".md"), part_number))
                        .unwrap_or_else(|| format!("gobbled.part{}.md", part_number));
                        
                    std::fs::write(&part_file_name, &output)?;
                    if !args.quiet {
                        eprintln!("{}", format!("  ↳ Baked {}", part_file_name).truecolor(0, 255, 100));
                    }
                    
                    // Reset for next chunk
                    part_number += 1;
                    current_combined.clear();
                    current_tokens = 0;
                }
                
                // Append current file to the chunk
                if path == "_tree.md" {
                    current_combined.push_str(&content);
                } else {
                    current_combined.push_str(&format!("// --- FILE_START: {} ---\n", path));
                    current_combined.push_str(&content);
                    current_combined.push_str("\n\n");
                }
                current_tokens += file_tokens;
                
                // If it's the last file, flush the remaining chunk
                if i == file_count - 1 && !current_combined.is_empty() {
                    let output = flavors::format_output(flavor, &display_name, &current_combined);
                    total_chars += output.len();
                    
                    let part_file_name = args.write
                        .as_deref()
                        .map(|o| format!("{}.part{}.md", o.trim_end_matches(".md"), part_number))
                        .unwrap_or_else(|| format!("gobbled.part{}.md", part_number));
                        
                    std::fs::write(&part_file_name, &output)?;
                    if !args.quiet {
                        eprintln!("{}", format!("  ↳ Baked {}", part_file_name).truecolor(0, 255, 100));
                    }
                }
            }
            
            // Print chunking summary
            if args.tokens {
                if args.quiet {
                    eprintln!("{}", post_compression_tokens);
                } else {
                    eprintln!(
                        "{}",
                        format!(
                            "📊 Output Length: {} chars (~{} tokens across {} parts)",
                            total_chars, post_compression_tokens, part_number
                        )
                        .truecolor(255, 191, 0)
                    );
                }
            }
            
            let tokens_saved = if args.compress.is_some() && pre_compression_tokens > post_compression_tokens {
                 pre_compression_tokens - post_compression_tokens
            } else {
                 0
            };

            // The Full Belch (Summary Table)
            if !args.quiet {
                eprintln!("\n{}", "╭──────────────────────────────────────╮".truecolor(139, 69, 19));
                eprintln!("{} {}", "│".truecolor(139, 69, 19), "        THE FULL BELCH (SUMMARY)       ".truecolor(167, 255, 0).bold());
                eprintln!("{}", "├──────────────────────────────────────┤".truecolor(139, 69, 19));
                eprintln!("{} {:<37} {}", "│".truecolor(139, 69, 19), format!("📁 Files Gobbled: {}", file_count), "│".truecolor(139, 69, 19));
                eprintln!("{} {:<37} {}", "│".truecolor(139, 69, 19), format!("📏 Total Characters: {}", total_chars), "│".truecolor(139, 69, 19));
                eprintln!("{} {:<37} {}", "│".truecolor(139, 69, 19), format!("🪙 Estimated Tokens: {}", post_compression_tokens), "│".truecolor(139, 69, 19));
                if tokens_saved > 0 {
                    eprintln!("{} {:<37} {}", "│".truecolor(139, 69, 19), format!("📉 Tokens Saved:   {} 📉", tokens_saved).truecolor(0, 255, 150), "│".truecolor(139, 69, 19));
                }
                eprintln!("{} {:<37} {}", "│".truecolor(139, 69, 19), format!("🍰 Parts Baked: {}", part_number), "│".truecolor(139, 69, 19));
                eprintln!("{}", "╰──────────────────────────────────────╯".truecolor(139, 69, 19));
            }
            return Ok(());
        }

        // Original Unchunked Pipeline
        let mut combined = String::new();
        let file_count = compressed_pairs.len();
        let mut post_compression_tokens = 0;

        // Build per-file token counts for manifest
        let mut file_token_list: Vec<(String, usize)> = Vec::new();
        
        for (path, content) in &compressed_pairs {
            let toks = crate::compressor::heuristic::estimate_tokens(content, path);
            file_token_list.push((path.clone(), toks));
            post_compression_tokens += toks;
        }

        // --tokens-only: print just the count to stdout and skip all content
        if args.tokens_only {
            println!("{}", post_compression_tokens);
            return Ok(());
        }

        // --manifest: prepend a table of contents
        if args.manifest && file_token_list.len() > 1 {
            combined.push_str("| # | File | Tokens |\n");
            combined.push_str("|---|------|--------|\n");
            for (i, (path, toks)) in file_token_list.iter().enumerate() {
                if path != "_tree.md" {
                    combined.push_str(&format!("| {} | {} | {} |\n", i + 1, path, toks));
                }
            }
            combined.push_str("\n");
        }

        for (path, content) in compressed_pairs {
            if path == "_tree.md" {
                combined.push_str(&content);
            } else {
                // Inject relevance score annotation if search was used
                if let Some(ref scores) = search_scores {
                    if let Some(score) = scores.get(&path) {
                        combined.push_str(&format!("// --- FILE_START: {} --- (relevance: {:.2})\n", path, score));
                    } else {
                        combined.push_str(&format!("// --- FILE_START: {} ---\n", path));
                    }
                } else {
                    combined.push_str(&format!("// --- FILE_START: {} ---\n", path));
                }
                combined.push_str(&content);
                combined.push_str("\n\n");
            }
        }

        let output = flavors::format_output(flavor, &display_name, &combined);
        let total_chars = output.len();

        if args.tokens {
            if args.quiet {
                eprintln!("tokens: {}", post_compression_tokens);
            } else {
                eprintln!(
                    "{}",
                    format!(
                        "📊 tokens: {} (~{} chars)",
                        post_compression_tokens, total_chars
                    )
                    .truecolor(255, 191, 0)
                );
            }
        }

        if let Some(file_path) = args.write.as_deref() {
            std::fs::write(file_path, &output)?;
            if !args.quiet {
                eprintln!("{}", format!("💾 Saved output to {}", file_path).truecolor(0, 255, 100));
            }
        } else {
            // The actual markdown output
            println!("\n---\n{}", output);
        }

        let tokens_saved = if args.compress.is_some() && pre_compression_tokens > post_compression_tokens {
             pre_compression_tokens - post_compression_tokens
        } else {
             0
        };

        // The Full Belch (Summary Table)
        if !args.quiet {
            eprintln!("\n{}", "╭──────────────────────────────────────╮".truecolor(139, 69, 19));
            eprintln!("{} {}", "│".truecolor(139, 69, 19), "        THE FULL BELCH (SUMMARY)       ".truecolor(167, 255, 0).bold());
            eprintln!("{}", "├──────────────────────────────────────┤".truecolor(139, 69, 19));
            eprintln!("{} {:<37} {}", "│".truecolor(139, 69, 19), format!("📁 Files Gobbled: {}", file_count), "│".truecolor(139, 69, 19));
            eprintln!("{} {:<37} {}", "│".truecolor(139, 69, 19), format!("📏 Total Characters: {}", total_chars), "│".truecolor(139, 69, 19));
            eprintln!("{} {:<37} {}", "│".truecolor(139, 69, 19), format!("🪙 Estimated Tokens: {}", post_compression_tokens), "│".truecolor(139, 69, 19));
            if tokens_saved > 0 {
                eprintln!("{} {:<37} {}", "│".truecolor(139, 69, 19), format!("📉 Tokens Saved:   {} 📉", tokens_saved).truecolor(0, 255, 150), "│".truecolor(139, 69, 19));
            }
            eprintln!("{}", "╰──────────────────────────────────────╯".truecolor(139, 69, 19));
        }

        if args.open {
            let file_to_open = if let Some(file_path) = args.write.as_deref() {
                // --write was specified — open that file directly
                std::path::PathBuf::from(file_path)
            } else {
                // No --write — write to a temp file and open that
                let file_prefix = display_name
                    .replace("https://", "")
                    .replace("http://", "")
                    .replace("/", "_")
                    .replace("..", "")
                    .replace(":", "_")
                    .replace(" ", "_")
                    .replace("&", "and");
                let temp_dir = std::env::temp_dir();
                let temp_file = temp_dir.join(format!("{}_gobbled.md", file_prefix));
                std::fs::write(&temp_file, &output)?;
                temp_file
            };

            if !args.quiet {
                eprintln!("{}", format!("🚪 Kicked open: {}", file_to_open.display()).truecolor(0, 200, 255));
            }
            if let Err(e) = open::that(&file_to_open) {
                eprintln!("{} Failed to open file: {}", "⚠️".yellow(), e);
            }
        }

        if args.copy {
            let mut clipboard = arboard::Clipboard::new()?;
            clipboard.set_text(output)?;
            if !args.quiet {
                eprintln!("{}", "📋 Stashed the loot in your clipboard!".truecolor(0, 255, 100));
            }
        }
    }

    Ok(())
}

pub fn gobble_local(
    target: &str,
    args: &crate::cli::Cli,
) -> Result<Vec<(String, String)>> {
    let mut files = Vec::new();
    let root = std::path::Path::new(target);

    if !root.is_dir() || !args.horde {
        let content = route_and_gobble(target, args)?;
        files.push((target.to_string(), content));
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

    // Process all collected files
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
                format!("🎯 Filtered horde from {} to {} files matching: {}", before_count, files_to_process.len(), args.include.join(", ")).truecolor(0, 200, 255)
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
                    parts.iter().all(|part| part.is_empty() || name.contains(part))
                } else {
                    name.contains(pattern.as_str())
                }
            })
        });
        if !args.quiet {
            eprintln!(
                "{}",
                format!("🚫 Excluded {} files matching: {}", before_count - files_to_process.len(), args.exclude.join(", ")).truecolor(255, 99, 71)
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
                        format!("🔀 Git diff mode (vs {}): {} changed files found (from {} total)", git_ref, files_to_process.len(), before_count).truecolor(0, 200, 255)
                    );
                }
            }
            Ok(output) => {
                let stderr_msg = String::from_utf8_lossy(&output.stderr);
                eprintln!("{} git diff failed: {}", "⚠️".yellow(), stderr_msg.trim());
            }
            Err(e) => {
                eprintln!("{} Could not run git (is it installed?): {}", "⚠️".yellow(), e);
            }
        }
    }

    for file_path in files_to_process {
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
                            if diff_text.trim().is_empty() { None } else { Some(diff_text) }
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

            if let Some(c) = content {
                files.push((rel_str, c));
            }
        }
    }

    Ok(files)
}

fn route_and_gobble(path_str: &str, args: &crate::cli::Cli) -> Result<String> {
    let path = Path::new(path_str);
    let extension = path
        .extension()
        .and_then(|s| s.to_str())
        .map(|s| s.to_lowercase())
        .unwrap_or_default();

    // EXPLICIT OVERRIDE: If the user passed `--plugin <NAME>`, force execution through that WASM component.
    if let Some(plugin_name) = args.plugin.as_deref() {
        if let Some(plugin_path) = parsers::wasm::WasmGobbler::sniff(plugin_name) {
            match (parsers::wasm::WasmGobbler { wasm_path: plugin_path }).gobble(path, args) {
                Ok(markdown) => return Ok(markdown),
                Err(e) => anyhow::bail!("Explicit WASM Plugin '{}' failed: {}", plugin_name, e),
            }
        } else {
            anyhow::bail!("Requested explicit --plugin '{}' could not be found in ~/.filegoblin/plugins/ or ./plugins/", plugin_name);
        }
    }

    match extension.as_str() {
        "pdf" => parsers::pdf::PdfGobbler.gobble(path, args),
        "docx" => parsers::office::OfficeGobbler.gobble(path, args),
        "xlsx" | "xls" | "ods" | "csv" => parsers::sheet::SheetGobbler.gobble(path, args),
        "pptx" => parsers::powerpoint::PptxGobbler.gobble(path, args),
        "html" | "htm" => parsers::web::WebGobbler { extract_full: args.full }.gobble(path, args),
        "rs" | "js" | "py" | "ts" | "go" | "c" | "cpp" => parsers::code::CodeGobbler.gobble(path, args),
        _ => {
            // Priority 1: Check if a user provided a dynamic WASM plugin for this extension
            #[allow(clippy::collapsible_if)]
            if let Some(plugin_path) = parsers::wasm::WasmGobbler::sniff(&extension) {
                if let Ok(markdown) = (parsers::wasm::WasmGobbler { wasm_path: plugin_path }).gobble(path, args) {
                    return Ok(markdown);
                }
            }

            // Priority 2: Fall back to core heuristics
            // If it's an image, pass to ocr. If text, just read.
            if ["png", "jpg", "jpeg", "webp"].contains(&extension.as_str()) {
                parsers::ocr::OcrGobbler.gobble(path, args)
            } else {
                std::fs::read_to_string(path).context("Failed to read file as plaintext")
            }
        }
    }
}

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

        let args = crate::cli::Cli::parse_from(&["filegoblin"]);
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
        let args = crate::cli::Cli::parse_from(&["filegoblin"]);
        let res = route_and_gobble(test_file, &args).unwrap();
        assert_eq!(res, "Plaintext fallback text");
        std::fs::remove_file(test_file).ok();
    }
}
