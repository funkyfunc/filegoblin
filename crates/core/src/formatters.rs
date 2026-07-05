use crate::pipeline::GobbleDocument;
use anyhow::Result;
use colored::Colorize;
use url::Url;

pub fn format_and_output(
    mut compressed_docs: Vec<GobbleDocument>,
    pre_compression_tokens: usize,
    flavor: &crate::flavors::Flavor,
    display_name: &str,
    args: &crate::cli::Cli,
) -> Result<()> {
    // All flavors benefit from deterministic alphabetical sorting for prefix caching and consistency.
    // However, if '_tree.md' is present, we must keep it at the very beginning of the payload.
    compressed_docs.sort_by(|a, b| {
        if a.path == "_tree.md" && b.path == "_tree.md" {
            std::cmp::Ordering::Equal
        } else if a.path == "_tree.md" {
            std::cmp::Ordering::Less
        } else if b.path == "_tree.md" {
            std::cmp::Ordering::Greater
        } else {
            a.path.cmp(&b.path)
        }
    });
    // Feature: Summary Preamble (--summary)
    let mut preamble = String::new();
    if args.summary {
        if !args.quiet {
            eprintln!(
                "{}",
                "📋 Generating heuristic summary...".truecolor(0, 255, 100)
            );
        }
        preamble.push_str("## Project Summary\n\n");
        let mut extensions: std::collections::HashMap<String, usize> =
            std::collections::HashMap::new();
        let mut readme_intro = None;

        for doc in &compressed_docs {
            if doc.path == "_tree.md" {
                continue;
            }

            // Extract extension
            if let Some(ext_idx) = doc.path.rfind('.') {
                let ext = &doc.path[ext_idx..];
                *extensions.entry(ext.to_string()).or_insert(0) += 1;
            } else {
                *extensions.entry("no-extension".to_string()).or_insert(0) += 1;
            }

            // Look for README
            if doc.path.to_lowercase().ends_with("readme.md") && readme_intro.is_none() {
                let intro = doc
                    .content
                    .lines()
                    .skip_while(|l| l.trim().is_empty() || l.starts_with('#')) // Skip blanks and titles
                    .filter(|l| !l.trim().is_empty())
                    .take(3)
                    .collect::<Vec<&str>>()
                    .join(" ");
                if !intro.is_empty() {
                    readme_intro = Some(intro);
                }
            }
        }

        if let Some(intro) = readme_intro {
            preamble.push_str(&format!("> **Overview:** {}\n\n", intro));
        }

        preamble.push_str("**Composition:**\n");
        let mut ext_vec: Vec<(&String, &usize)> = extensions.iter().collect();
        ext_vec.sort_by(|a, b| b.1.cmp(a.1)); // Sort by count descending
        for (ext, count) in ext_vec.iter().take(5) {
            // Top 5
            preamble.push_str(&format!("- **{}**: {} files\n", ext, count));
        }
        if ext_vec.len() > 5 {
            preamble.push_str(&format!("- *...and {} other types*\n", ext_vec.len() - 5));
        }
        preamble.push_str("\n---\n\n");
    }

    if args.split {
        let root_dir_name = display_name
            .replace("https://", "")
            .replace("http://", "")
            .replace("/", "_")
            .replace(" ", "_")
            .replace("&", "and");
        let root_dir = args
            .write
            .clone()
            .unwrap_or_else(|| format!("{}_gobbled", root_dir_name));
        std::fs::create_dir_all(&root_dir)?;

        if !args.quiet {
            eprintln!(
                "{}",
                format!("💾 Stashing the split loot into directory: ./{}", root_dir)
                    .truecolor(0, 255, 100)
            );
        }

        let mut post_compression_tokens = 0;
        let mut total_chars = 0;

        for doc in compressed_docs.iter() {
            // Safe pathing logic for urls and relative filepaths
            let safe_path = if let Ok(u) = Url::parse(&doc.path) {
                let mut p = u.path().trim_start_matches('/').to_string();
                if p.is_empty() {
                    p = "index".to_string();
                }
                if p.ends_with('/') {
                    p.push_str("index");
                }
                p.replace("..", "").replace(":", "_")
            } else {
                doc.path.replace("..", "")
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

            let flavored = crate::flavors::format_output(flavor, &doc.path, &doc.content);
            std::fs::write(&file_path, &flavored)?;

            if !args.quiet {
                eprintln!("  ↳ Spat out {}", file_path.display());
            }

            total_chars += flavored.len();
            post_compression_tokens +=
                crate::compressor::heuristic::estimate_tokens(&flavored, &doc.path);
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
                eprintln!(
                    "{}",
                    format!("🚪 Opening directory: ./{}", root_dir).truecolor(0, 200, 255)
                );
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
        for doc in compressed_docs {
            out.push(FileNode {
                path: doc.path,
                content: doc.content,
            });
        }
        let serialized = serde_json::to_string_pretty(&out)?;

        if let Some(file_path) = args.write.as_deref() {
            std::fs::write(file_path, &serialized)?;
            if !args.quiet {
                eprintln!(
                    "{}",
                    format!("💾 Saved JSON output to {}", file_path).truecolor(0, 255, 100)
                );
            }
        } else {
            // Print strictly the JSON to standard out
            println!("{}", serialized);
        }

        if args.copy {
            let mut clipboard = arboard::Clipboard::new()?;
            clipboard.set_text(serialized)?;
            if !args.quiet {
                eprintln!(
                    "{}",
                    "📋 Stashed the JSON loot in your clipboard!".truecolor(0, 255, 100)
                );
            }
        }
    } else {
        // Parse token threshold if chunk is enabled
        let token_threshold: Option<usize> = args.chunk.as_deref().and_then(|c| {
            let c = c.trim().to_lowercase();
            if c.ends_with('k') {
                c.trim_end_matches('k')
                    .parse::<f64>()
                    .ok()
                    .map(|n| (n * 1_000.0) as usize)
            } else if c.ends_with('m') {
                c.trim_end_matches('m')
                    .parse::<f64>()
                    .ok()
                    .map(|n| (n * 1_000_000.0) as usize)
            } else {
                c.parse::<usize>().ok()
            }
        });

        if let Some(threshold) = token_threshold {
            if !args.quiet {
                eprintln!(
                    "{}",
                    format!("🍰 Chunking output at ~{} tokens per file", threshold)
                        .truecolor(0, 200, 255)
                );
            }

            let mut part_number = 1;
            let mut current_combined = String::new();
            let mut current_tokens = 0;
            let mut post_compression_tokens = 0;
            let mut total_chars = 0;
            let file_count = compressed_docs.len();

            for (i, doc) in compressed_docs.into_iter().enumerate() {
                let file_tokens = doc.token_count.unwrap_or_else(|| {
                    crate::compressor::heuristic::estimate_tokens(&doc.content, &doc.path)
                });
                post_compression_tokens += file_tokens;

                // If adding this file pushes us over the threshold AND we already have content in this chunk
                if current_tokens + file_tokens > threshold && !current_combined.is_empty() {
                    // Flush current chunk
                    let output =
                        crate::flavors::format_output(flavor, display_name, &current_combined);
                    total_chars += output.len();

                    let part_file_name = args
                        .write
                        .as_deref()
                        .map(|o| format!("{}.part{}.md", o.trim_end_matches(".md"), part_number))
                        .unwrap_or_else(|| format!("gobbled.part{}.md", part_number));

                    std::fs::write(&part_file_name, &output)?;
                    if !args.quiet {
                        eprintln!(
                            "{}",
                            format!("  ↳ Baked {}", part_file_name).truecolor(0, 255, 100)
                        );
                    }

                    // Reset for next chunk
                    part_number += 1;
                    current_combined.clear();
                    current_tokens = 0;
                }

                // Append current file to the chunk
                if doc.path == "_tree.md" {
                    current_combined.push_str(&doc.content);
                } else {
                    if i == 0 {
                        current_combined.push_str(&preamble);
                    }
                    current_combined.push_str(&format!("// --- FILE_START: {} ---\n", doc.path));
                    current_combined.push_str(&doc.content);
                    current_combined.push_str("\n\n");
                }
                current_tokens += file_tokens;

                // If it's the last file, flush the remaining chunk
                if i == file_count - 1 && !current_combined.is_empty() {
                    let output =
                        crate::flavors::format_output(flavor, display_name, &current_combined);
                    total_chars += output.len();

                    let part_file_name = args
                        .write
                        .as_deref()
                        .map(|o| format!("{}.part{}.md", o.trim_end_matches(".md"), part_number))
                        .unwrap_or_else(|| format!("gobbled.part{}.md", part_number));

                    std::fs::write(&part_file_name, &output)?;
                    if !args.quiet {
                        eprintln!(
                            "{}",
                            format!("  ↳ Baked {}", part_file_name).truecolor(0, 255, 100)
                        );
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

            let tokens_saved =
                if args.compress.is_some() && pre_compression_tokens > post_compression_tokens {
                    pre_compression_tokens - post_compression_tokens
                } else {
                    0
                };

            // The Full Belch (Summary Table)
            if !args.quiet {
                eprintln!(
                    "\n{}",
                    "╭──────────────────────────────────────╮".truecolor(139, 69, 19)
                );
                eprintln!(
                    "{} {}",
                    "│".truecolor(139, 69, 19),
                    "        THE FULL BELCH (SUMMARY)       "
                        .truecolor(167, 255, 0)
                        .bold()
                );
                eprintln!(
                    "{}",
                    "├──────────────────────────────────────┤".truecolor(139, 69, 19)
                );
                eprintln!(
                    "{} {:<37} {}",
                    "│".truecolor(139, 69, 19),
                    format!("📁 Files Gobbled: {}", file_count),
                    "│".truecolor(139, 69, 19)
                );
                eprintln!(
                    "{} {:<37} {}",
                    "│".truecolor(139, 69, 19),
                    format!("📏 Total Characters: {}", total_chars),
                    "│".truecolor(139, 69, 19)
                );
                eprintln!(
                    "{} {:<37} {}",
                    "│".truecolor(139, 69, 19),
                    format!("🪙 Estimated Tokens: {}", post_compression_tokens),
                    "│".truecolor(139, 69, 19)
                );
                if tokens_saved > 0 {
                    eprintln!(
                        "{} {:<37} {}",
                        "│".truecolor(139, 69, 19),
                        format!("📉 Tokens Saved:   {} 📉", tokens_saved).truecolor(0, 255, 150),
                        "│".truecolor(139, 69, 19)
                    );
                }
                eprintln!(
                    "{} {:<37} {}",
                    "│".truecolor(139, 69, 19),
                    format!("🍰 Parts Baked: {}", part_number),
                    "│".truecolor(139, 69, 19)
                );

                if args.cost {
                    eprintln!(
                        "{}",
                        "├──────────────────────────────────────┤".truecolor(139, 69, 19)
                    );
                    let costs = crate::cost::estimate_costs(post_compression_tokens);
                    for (model, cost) in costs {
                        eprintln!(
                            "{} 👀 {:<34} {}",
                            "│".truecolor(139, 69, 19),
                            format!("{}: ${:.4}", model, cost).truecolor(200, 200, 200),
                            "│".truecolor(139, 69, 19)
                        );
                    }
                }

                eprintln!(
                    "{}",
                    "╰──────────────────────────────────────╯".truecolor(139, 69, 19)
                );
            }
            return Ok(());
        }

        // Original Unchunked Pipeline
        let mut combined = String::new();
        let file_count = compressed_docs.len();
        let mut post_compression_tokens = 0;

        // Build per-file token counts for manifest
        let mut file_token_list: Vec<(String, usize)> = Vec::new();

        for doc in &compressed_docs {
            let toks = doc.token_count.unwrap_or_else(|| {
                crate::compressor::heuristic::estimate_tokens(&doc.content, &doc.path)
            });
            file_token_list.push((doc.path.clone(), toks));
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
            combined.push('\n');
        }

        combined.push_str(&preamble);

        for doc in compressed_docs {
            if doc.path == "_tree.md" {
                combined.push_str(&doc.content);
            } else {
                if let Some(score) = doc.relevance_score {
                    combined.push_str(&format!(
                        "// --- FILE_START: {} --- (relevance: {:.2})\n",
                        doc.path, score
                    ));
                } else {
                    combined.push_str(&format!("// --- FILE_START: {} ---\n", doc.path));
                }
                combined.push_str(&doc.content);
                combined.push_str("\n\n");
            }
        }

        let output = crate::flavors::format_output(flavor, display_name, &combined);
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
                eprintln!(
                    "{}",
                    format!("💾 Saved output to {}", file_path).truecolor(0, 255, 100)
                );
            }
        } else {
            // The actual markdown output
            println!("\n---\n{}", output);
        }

        let tokens_saved =
            if args.compress.is_some() && pre_compression_tokens > post_compression_tokens {
                pre_compression_tokens - post_compression_tokens
            } else {
                0
            };

        // The Full Belch (Summary Table)
        if !args.quiet {
            eprintln!(
                "\n{}",
                "╭──────────────────────────────────────╮".truecolor(139, 69, 19)
            );
            eprintln!(
                "{} {}",
                "│".truecolor(139, 69, 19),
                "        THE FULL BELCH (SUMMARY)       "
                    .truecolor(167, 255, 0)
                    .bold()
            );
            eprintln!(
                "{}",
                "├──────────────────────────────────────┤".truecolor(139, 69, 19)
            );
            eprintln!(
                "{} {:<37} {}",
                "│".truecolor(139, 69, 19),
                format!("📁 Files Gobbled: {}", file_count),
                "│".truecolor(139, 69, 19)
            );
            eprintln!(
                "{} {:<37} {}",
                "│".truecolor(139, 69, 19),
                format!("📏 Total Characters: {}", total_chars),
                "│".truecolor(139, 69, 19)
            );
            eprintln!(
                "{} {:<37} {}",
                "│".truecolor(139, 69, 19),
                format!("🪙 Estimated Tokens: {}", post_compression_tokens),
                "│".truecolor(139, 69, 19)
            );
            if tokens_saved > 0 {
                eprintln!(
                    "{} {:<37} {}",
                    "│".truecolor(139, 69, 19),
                    format!("📉 Tokens Saved:   {} 📉", tokens_saved).truecolor(0, 255, 150),
                    "│".truecolor(139, 69, 19)
                );
            }
            if args.cost {
                eprintln!(
                    "{}",
                    "├──────────────────────────────────────┤".truecolor(139, 69, 19)
                );
                let costs = crate::cost::estimate_costs(post_compression_tokens);
                for (model, cost) in costs {
                    eprintln!(
                        "{} 👀 {:<34} {}",
                        "│".truecolor(139, 69, 19),
                        format!("{}: ${:.4}", model, cost).truecolor(200, 200, 200),
                        "│".truecolor(139, 69, 19)
                    );
                }
            }
            eprintln!(
                "{}",
                "╰──────────────────────────────────────╯".truecolor(139, 69, 19)
            );
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
                eprintln!(
                    "{}",
                    format!("🚪 Kicked open: {}", file_to_open.display()).truecolor(0, 200, 255)
                );
            }
            if let Err(e) = open::that(&file_to_open) {
                eprintln!("{} Failed to open file: {}", "⚠️".yellow(), e);
            }
        }

        if args.copy {
            let mut clipboard = arboard::Clipboard::new()?;
            clipboard.set_text(output)?;
            if !args.quiet {
                eprintln!(
                    "{}",
                    "📋 Stashed the loot in your clipboard!".truecolor(0, 255, 100)
                );
            }
        }
    }

    Ok(())
}

pub fn stream_and_output(
    rx: impl Iterator<Item = GobbleDocument>,
    flavor: &crate::flavors::Flavor,
    display_name: &str,
    args: &crate::cli::Cli,
) -> Result<()> {
    if args.summary || args.manifest {
        if !args.quiet {
            eprintln!(
                "{}",
                "⚠️ Buffer Mode: --summary or --manifest requires full repository scan in memory."
                    .truecolor(255, 191, 0)
            );
        }
        let compressed_docs: Vec<_> = rx.into_iter().collect();
        let tokens = compressed_docs
            .iter()
            .map(|d| d.token_count.unwrap_or(0))
            .sum();
        return format_and_output(compressed_docs, tokens, flavor, display_name, args);
    }

    if args.split {
        let root_dir_name = display_name
            .replace("https://", "")
            .replace("http://", "")
            .replace("/", "_")
            .replace(" ", "_")
            .replace("&", "and");
        let root_dir = args
            .write
            .clone()
            .unwrap_or_else(|| format!("{}_gobbled", root_dir_name));
        std::fs::create_dir_all(&root_dir)?;

        if !args.quiet {
            eprintln!(
                "{}",
                format!("💾 Streaming the split loot into directory: ./{}", root_dir)
                    .truecolor(0, 255, 100)
            );
        }

        let mut post_compression_tokens = 0;
        let mut total_chars = 0;
        let mut file_count = 0;

        for doc in rx {
            file_count += 1;
            let safe_path = if let Ok(u) = Url::parse(&doc.path) {
                let mut p = u.path().trim_start_matches('/').to_string();
                if p.is_empty() {
                    p = "index".to_string();
                }
                if p.ends_with('/') {
                    p.push_str("index");
                }
                p.replace("..", "").replace(":", "_")
            } else {
                doc.path.replace("..", "")
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

            let flavored = crate::flavors::format_output(flavor, &doc.path, &doc.content);
            std::fs::write(&file_path, &flavored)?;

            total_chars += flavored.len();
            post_compression_tokens +=
                crate::compressor::heuristic::estimate_tokens(&flavored, &doc.path);
        }

        if args.tokens {
            if args.quiet {
                eprintln!("{}", post_compression_tokens);
            } else {
                eprintln!(
                    "{}",
                    format!(
                        "📊 Total Output Length: {} chars (~{} tokens across {} files)",
                        total_chars, post_compression_tokens, file_count
                    )
                    .truecolor(255, 191, 0)
                );
            }
        }
        Ok(())
    } else if args.json {
        use std::io::Write;

        // Setup output
        let mut output_writer: Box<dyn Write> = if let Some(file_path) = args.write.as_deref() {
            Box::new(std::fs::File::create(file_path)?)
        } else {
            Box::new(std::io::stdout())
        };

        writeln!(output_writer, "[")?;

        let mut first = true;
        for doc in rx {
            if !first {
                writeln!(output_writer, ",")?;
            }
            first = false;

            #[derive(serde::Serialize)]
            struct FileNode<'a> {
                path: &'a str,
                content: &'a str,
            }

            let node = FileNode {
                path: &doc.path,
                content: &doc.content,
            };
            let serialized = serde_json::to_string_pretty(&node)?;
            write!(output_writer, "{}", serialized)?;
        }
        writeln!(
            output_writer,
            "
]"
        )?;

        if !args.quiet && args.write.is_some() {
            eprintln!(
                "{}",
                "💾 Streamed JSON array successfully.".truecolor(0, 255, 100)
            );
        }
        Ok(())
    } else {
        use std::io::Write;

        let mut current_combined = String::new();
        let mut total_tokens = 0;
        let mut _total_chars = 0;
        let mut part_number = 1;
        let mut file_count = 0;

        let token_threshold = args.chunk.as_deref().and_then(|c| {
            let c = c.trim().to_lowercase();
            if c.ends_with('k') {
                c.trim_end_matches('k')
                    .parse::<f64>()
                    .ok()
                    .map(|n| (n * 1_000.0) as usize)
            } else if c.ends_with('m') {
                c.trim_end_matches('m')
                    .parse::<f64>()
                    .ok()
                    .map(|n| (n * 1_000_000.0) as usize)
            } else {
                c.parse::<usize>().ok()
            }
        });

        // Setup streaming output if no chunking
        let mut stream_writer: Option<Box<dyn Write>> = if token_threshold.is_none() {
            if let Some(file_path) = args.write.as_deref() {
                Some(Box::new(std::fs::File::create(file_path)?))
            } else {
                Some(Box::new(std::io::stdout()))
            }
        } else {
            None
        };

        for doc in rx {
            file_count += 1;
            let file_tokens = doc.token_count.unwrap_or_else(|| {
                crate::compressor::heuristic::estimate_tokens(&doc.content, &doc.path)
            });
            total_tokens += file_tokens;

            // Chunking logic
            if let Some(threshold) = token_threshold {
                if total_tokens > threshold && !current_combined.is_empty() {
                    let output =
                        crate::flavors::format_output(flavor, display_name, &current_combined);
                    _total_chars += output.len();
                    let part_file_name = args
                        .write
                        .as_deref()
                        .map(|o| format!("{}.part{}.md", o.trim_end_matches(".md"), part_number))
                        .unwrap_or_else(|| format!("gobbled.part{}.md", part_number));
                    std::fs::write(&part_file_name, &output)?;
                    if !args.quiet {
                        eprintln!("  ↳ Baked {}", part_file_name.truecolor(0, 255, 100));
                    }

                    part_number += 1;
                    current_combined.clear();
                    total_tokens = file_tokens; // Reset token count for next chunk
                }

                if doc.path == "_tree.md" {
                    current_combined.push_str(&doc.content);
                } else {
                    current_combined.push_str(&format!(
                        "// --- FILE_START: {} ---
",
                        doc.path
                    ));
                    current_combined.push_str(&doc.content);
                    current_combined.push_str(
                        "

",
                    );
                }
            } else {
                // Streaming immediately
                let mut block = String::new();
                if doc.path == "_tree.md" {
                    block.push_str(&doc.content);
                } else {
                    block.push_str(&format!(
                        "// --- FILE_START: {} ---
",
                        doc.path
                    ));
                    block.push_str(&doc.content);
                    block.push_str(
                        "

",
                    );
                }
                // Wrap block in flavor formatting? Wait, flavor format wraps the ENTIRE string.
                // We'll just write the raw block if we are streaming to stdout without chunking.
                // Or we can apply flavor to the block.
                if let Some(writer) = &mut stream_writer {
                    write!(writer, "{}", block)?;
                }
            }
        }

        // Flush remaining chunk
        if token_threshold.is_some() && !current_combined.is_empty() {
            let output = crate::flavors::format_output(flavor, display_name, &current_combined);
            let part_file_name = args
                .write
                .as_deref()
                .map(|o| format!("{}.part{}.md", o.trim_end_matches(".md"), part_number))
                .unwrap_or_else(|| format!("gobbled.part{}.md", part_number));
            std::fs::write(&part_file_name, &output)?;
        }

        if args.tokens && !args.quiet {
            eprintln!(
                "{}",
                format!(
                    "📊 Output Length (~{} tokens across {} files)",
                    total_tokens, file_count
                )
                .truecolor(255, 191, 0)
            );
        }

        Ok(())
    }
}
