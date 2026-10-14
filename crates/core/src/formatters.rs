use crate::pipeline::GobbleDocument;
use anyhow::Result;
use colored::Colorize;
use url::Url;

pub mod plain;

/// Renders one document as an output block: the tree map bare, everything else
/// behind a `FILE_START` breadcrumb.
///
/// This framing is the shape every downstream consumer parses, and it had grown
/// five separate implementations across this file and `lib.rs` — differing in
/// trailing newlines and in whether they carried the relevance score. Any change
/// to the format had to be made in all five, and a miss would have gone
/// unnoticed until someone diffed two output modes.
pub(crate) fn render_block(doc: &GobbleDocument) -> String {
    if doc.path == "_tree.md" {
        return doc.content.clone();
    }
    match doc.relevance_score {
        Some(score) => format!(
            "// --- FILE_START: {} --- (relevance: {:.2})\n{}\n\n",
            doc.path, score, doc.content
        ),
        None => format!("// --- FILE_START: {} ---\n{}\n\n", doc.path, doc.content),
    }
}

/// The format label for a document in the `--json` contract: the format detected
/// at extraction time, or a magic/extension re-detection as a fallback.
fn format_label(doc: &GobbleDocument) -> String {
    if let Some(f) = &doc.format {
        return f.clone();
    }
    crate::format::detect(std::path::Path::new(&doc.path), doc.content.as_bytes())
        .label()
        .to_string()
}

/// Parses a `--chunk` value (`100k`, `2m`, `5000`) into a token threshold.
fn parse_chunk_threshold(chunk: Option<&str>) -> Option<usize> {
    let c = chunk?.trim().to_lowercase();
    if let Some(n) = c.strip_suffix('k') {
        n.parse::<f64>().ok().map(|n| (n * 1_000.0) as usize)
    } else if let Some(n) = c.strip_suffix('m') {
        n.parse::<f64>().ok().map(|n| (n * 1_000_000.0) as usize)
    } else {
        c.parse::<usize>().ok()
    }
}

/// A relative-path / URL sanitizer for `--split`, mapping a document path to a
/// safe on-disk `.md` file name **relative to the split root**.
///
/// This has to produce a genuinely relative path, not merely a tidier one:
/// `Path::join` with an absolute argument *replaces* the root, so
/// `filegoblin /etc/hosts --split` used to target `/etc/hosts.md` outside the
/// split directory entirely. Stripping `..` textually also isn't enough —
/// `"../x"` becomes `"/x"`, which is absolute again.
fn sanitize_split_path(path: &str) -> String {
    let safe = if let Ok(u) = Url::parse(path) {
        let mut p = u.path().trim_start_matches('/').to_string();
        if p.is_empty() {
            p = "index".to_string();
        }
        if p.ends_with('/') {
            p.push_str("index");
        }
        // Distinct pages that differ only in their query string
        // (`?page=1`, `?page=2`) otherwise collide on one file, last write
        // winning, during a --horde --split crawl.
        if let Some(q) = u.query().filter(|q| !q.is_empty()) {
            p.push('_');
            p.push_str(q);
        }
        p
    } else {
        path.to_string()
    };

    // Drop traversal and root components rather than rewriting them, then
    // re-join with a plain separator.
    let cleaned: Vec<String> = safe
        .split(['/', '\\'])
        .filter(|seg| !seg.is_empty() && *seg != "." && *seg != "..")
        .map(|seg| {
            seg.chars()
                .map(|c| match c {
                    ':' | '?' | '*' | '"' | '<' | '>' | '|' | '\0' => '_',
                    c => c,
                })
                .collect::<String>()
        })
        // A Windows drive-relative prefix ("C") is fine as a directory name.
        .collect();

    let joined = cleaned.join("/");
    let joined = if joined.is_empty() {
        "document".to_string()
    } else {
        joined
    };
    if joined.ends_with(".md") {
        joined
    } else {
        format!("{}.md", joined)
    }
}

/// The root directory name for `--split`, derived from the display name.
fn sanitize_root_dir_name(display_name: &str) -> String {
    display_name
        .replace("https://", "")
        .replace("http://", "")
        .replace(['/', ' '], "_")
        .replace('&', "and")
}

/// Prints the "Full Belch" summary table to stderr (no-op when quiet). `parts` is
/// `Some(n)` only for chunked output.
fn print_belch(
    quiet: bool,
    file_count: usize,
    total_chars: usize,
    tokens: usize,
    tokens_saved: usize,
    parts: Option<usize>,
) {
    if quiet {
        return;
    }
    let bar = "│".truecolor(139, 69, 19);
    eprintln!(
        "\n{}",
        "╭──────────────────────────────────────╮".truecolor(139, 69, 19)
    );
    eprintln!(
        "{} {}",
        bar,
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
        bar,
        format!("📁 Files Gobbled: {}", file_count),
        bar
    );
    eprintln!(
        "{} {:<37} {}",
        bar,
        format!("📏 Total Characters: {}", total_chars),
        bar
    );
    eprintln!(
        "{} {:<37} {}",
        bar,
        format!("🪙 Estimated Tokens: {}", tokens),
        bar
    );
    if tokens_saved > 0 {
        eprintln!(
            "{} {:<37} {}",
            bar,
            format!("📉 Tokens Saved:   {} 📉", tokens_saved).truecolor(0, 255, 150),
            bar
        );
    }
    if let Some(parts) = parts {
        eprintln!(
            "{} {:<37} {}",
            bar,
            format!("🍰 Parts Baked: {}", parts),
            bar
        );
    }
    eprintln!(
        "{}",
        "╰──────────────────────────────────────╯".truecolor(139, 69, 19)
    );
}

fn output_formatted_text(
    output: &str,
    post_compression_tokens: usize,
    chunk_count: usize,
    display_name: &str,
    args: &crate::cli::Cli,
) -> Result<()> {
    let output = format!("{}\n", output.trim());
    let total_chars = output.len();

    if args.tokens {
        if args.quiet {
            eprintln!("tokens: {}", post_compression_tokens);
        } else if chunk_count > 0 {
            eprintln!(
                "{}",
                format!(
                    "📊 tokens: {} (~{} chars across {} chunks)",
                    post_compression_tokens, total_chars, chunk_count
                )
                .truecolor(255, 191, 0)
            );
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
        println!("{}", output);
    }

    if args.open {
        let file_to_open = if let Some(file_path) = args.write.as_deref() {
            std::path::PathBuf::from(file_path)
        } else {
            let file_prefix = display_name
                .replace("https://", "")
                .replace("http://", "")
                .replace(['/', ':', ' '], "_")
                .replace("..", "")
                .replace('&', "and");
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

    Ok(())
}

fn format_semantic_chunks_and_output(
    docs: &[GobbleDocument],
    chunk_size: usize,
    display_name: &str,
    args: &crate::cli::Cli,
) -> Result<()> {
    let config = crate::chunking::ChunkConfig {
        chunk_size,
        chunk_overlap: args.chunk_overlap,
        strategy: args.chunk_strategy,
    };

    let mut all_chunks = Vec::new();
    for doc in docs {
        if doc.path == "_tree.md" {
            continue;
        }
        let chunks = crate::chunking::chunk_document(&doc.content, &doc.path, &config);
        all_chunks.extend(chunks);
    }

    let total_chunk_tokens: usize = all_chunks.iter().map(|c| c.tokens).sum();
    if args.tokens_only {
        println!("{}", total_chunk_tokens);
        return Ok(());
    }

    match args.output_format() {
        crate::cli::OutputFormat::Json => {
            let serialized = serde_json::to_string_pretty(&all_chunks)?;
            if let Some(file_path) = args.write.as_deref() {
                std::fs::write(file_path, &serialized)?;
                if !args.quiet {
                    eprintln!(
                        "{}",
                        format!("💾 Saved JSON output to {}", file_path).truecolor(0, 255, 100)
                    );
                }
            } else {
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
        }
        crate::cli::OutputFormat::Plain => {
            let mut chunk_texts = Vec::new();
            for chunk in &all_chunks {
                let plain_content = plain::markdown_to_plain(&chunk.content);
                let formatted = if !chunk.breadcrumbs.is_empty() {
                    format!(
                        "<!-- Context: {} -->\n\n{}",
                        chunk.breadcrumbs.join(" > "),
                        plain_content
                    )
                } else {
                    plain_content
                };
                chunk_texts.push(formatted);
            }
            let output = chunk_texts.join("\n\n---\n\n");
            output_formatted_text(
                &output,
                total_chunk_tokens,
                all_chunks.len(),
                display_name,
                args,
            )?;
        }
        crate::cli::OutputFormat::Markdown => {
            let mut chunk_texts = Vec::new();
            for chunk in &all_chunks {
                let formatted = if !chunk.breadcrumbs.is_empty() {
                    format!(
                        "<!-- Context: {} -->\n\n{}",
                        chunk.breadcrumbs.join(" > "),
                        chunk.content
                    )
                } else {
                    chunk.content.clone()
                };
                chunk_texts.push(formatted);
            }
            let output = chunk_texts.join("\n\n---\n\n");
            output_formatted_text(
                &output,
                total_chunk_tokens,
                all_chunks.len(),
                display_name,
                args,
            )?;
        }
    }

    Ok(())
}

pub fn format_and_output(
    mut compressed_docs: Vec<GobbleDocument>,
    pre_compression_tokens: usize,
    display_name: &str,
    args: &crate::cli::Cli,
) -> Result<()> {
    // Failed extractions are carried through only so `--json` can report them
    // in-band; every other output form drops them (no empty file blocks).
    if !args.json && args.output_format() != crate::cli::OutputFormat::Json {
        compressed_docs.retain(|d| d.error.is_none());
    }

    // Deterministic alphabetical sorting for prefix caching and consistency.
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

    if let Some(target_chunk_size) = args.chunk_size {
        return format_semantic_chunks_and_output(
            &compressed_docs,
            target_chunk_size,
            display_name,
            args,
        );
    }

    if args.split {
        let root_dir_name = sanitize_root_dir_name(display_name);
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
            let file_name = sanitize_split_path(&doc.path);
            let file_path = std::path::Path::new(&root_dir).join(file_name);

            if let Some(parent) = file_path.parent() {
                std::fs::create_dir_all(parent)?;
            }

            let flavored = format!("{}\n", doc.content.trim());
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
    } else if args.output_format() == crate::cli::OutputFormat::Json {
        // Stable machine contract: a deterministic (alphabetically sorted above) array
        // of records, each carrying the detected format and an LLM token estimate so an
        // agent can extract a file and budget it in one call.
        #[derive(serde::Serialize)]
        struct FileNode {
            path: String,
            format: String,
            #[serde(skip_serializing_if = "Option::is_none")]
            tokens: Option<usize>,
            #[serde(skip_serializing_if = "Option::is_none")]
            content: Option<String>,
            #[serde(skip_serializing_if = "Option::is_none")]
            error: Option<String>,
        }

        let mut out = Vec::new();
        for doc in compressed_docs {
            let format = format_label(&doc);
            if let Some(err) = doc.error {
                // In-band failure record: an agent sees the error, not a vanished file.
                out.push(FileNode {
                    path: doc.path,
                    format,
                    tokens: None,
                    content: None,
                    error: Some(err),
                });
            } else {
                let tokens = doc.tokens();
                out.push(FileNode {
                    path: doc.path,
                    format,
                    tokens: Some(tokens),
                    content: Some(doc.content),
                    error: None,
                });
            }
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
        let token_threshold = parse_chunk_threshold(args.chunk.as_deref());

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
                let file_tokens = doc.tokens();
                post_compression_tokens += file_tokens;

                // If adding this file pushes us over the threshold AND we already have content in this chunk
                if current_tokens + file_tokens > threshold && !current_combined.is_empty() {
                    // Flush current chunk
                    let output = format!("{}\n", current_combined.trim());
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
                if doc.path != "_tree.md" && i == 0 {
                    current_combined.push_str(&preamble);
                }
                current_combined.push_str(&render_block(&doc));
                current_tokens += file_tokens;

                // If it's the last file, flush the remaining chunk
                if i == file_count - 1 && !current_combined.is_empty() {
                    let output = format!("{}\n", current_combined.trim());
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
            print_belch(
                args.quiet,
                file_count,
                total_chars,
                post_compression_tokens,
                tokens_saved,
                Some(part_number),
            );
            return Ok(());
        }

        // Original Unchunked Pipeline
        let mut combined = String::new();
        let file_count = compressed_docs.len();
        let mut post_compression_tokens = 0;

        // Build per-file token counts for manifest
        let mut file_token_list: Vec<(String, usize)> = Vec::new();

        for doc in &compressed_docs {
            let toks = doc.tokens();
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
            // Number the rows we actually emit. Enumerating the full list and
            // then skipping `_tree.md` inside the loop meant the first real
            // file was numbered 2 whenever a tree map was present.
            for (i, (path, toks)) in file_token_list
                .iter()
                .filter(|(path, _)| path != "_tree.md")
                .enumerate()
            {
                {
                    combined.push_str(&format!("| {} | {} | {} |\n", i + 1, path, toks));
                }
            }
            combined.push('\n');
        }

        combined.push_str(&preamble);

        for doc in compressed_docs {
            combined.push_str(&render_block(&doc));
        }

        let output = if args.output_format() == crate::cli::OutputFormat::Plain {
            plain::markdown_to_plain(&combined)
        } else {
            combined
        };

        let tokens_saved =
            if args.compress.is_some() && pre_compression_tokens > post_compression_tokens {
                pre_compression_tokens - post_compression_tokens
            } else {
                0
            };

        // The Full Belch (Summary Table)
        print_belch(
            args.quiet,
            file_count,
            output.len(),
            post_compression_tokens,
            tokens_saved,
            None,
        );

        output_formatted_text(&output, post_compression_tokens, 0, display_name, args)?;
    }

    Ok(())
}

/// Streams documents straight to the output sink. Returns the number of
/// non-empty content documents emitted, so the caller can honour the
/// "nothing extracted → non-zero exit" contract on the streaming path too.
pub fn stream_and_output(
    rx: impl Iterator<Item = GobbleDocument>,
    display_name: &str,
    args: &crate::cli::Cli,
) -> Result<usize> {
    // Drop failed extractions from streamed output (they carry no content); the
    // `--json` machine contract, which reports them in-band, always buffers instead.
    let produced = std::cell::Cell::new(0usize);
    let rx = rx.filter(|d| d.error.is_none()).inspect(|d| {
        if d.is_content() {
            produced.set(produced.get() + 1);
        }
    });

    // NOTE: `--summary`, `--manifest`, and `--json` are all in
    // `requires_buffering`, so they never reach this function. The fallbacks
    // that used to live here (a buffered re-entry, and a *second*
    // streamed-JSON writer emitting a different schema — `path` + `content`
    // only, no `format`/`tokens`/`error`) were unreachable, and the JSON one
    // would have silently changed the machine contract the day anyone removed
    // `json` from that list. Deleted rather than left as a trap.
    debug_assert!(
        !(args.summary
            || args.manifest
            || args.output_format() == crate::cli::OutputFormat::Json
            || args.output_format() == crate::cli::OutputFormat::Plain
            || args.chunk_size.is_some()),
        "buffered-only modes must not reach stream_and_output"
    );

    if args.split {
        let root_dir_name = sanitize_root_dir_name(display_name);
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
            let file_name = sanitize_split_path(&doc.path);
            let file_path = std::path::Path::new(&root_dir).join(file_name);

            if let Some(parent) = file_path.parent() {
                std::fs::create_dir_all(parent)?;
            }

            let flavored = format!("{}\n", doc.content.trim());
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
        Ok(produced.get())
    } else {
        use std::io::Write;

        let mut current_combined = String::new();
        // `chunk_tokens` resets at every flush; `grand_total_tokens` never
        // does. Reusing one counter for both made the final "~N tokens across
        // M files" line under-report by everything already written out.
        let mut chunk_tokens = 0;
        let mut grand_total_tokens = 0;
        let mut total_chars = 0;
        let mut part_number = 1;
        let mut file_count = 0;

        let token_threshold = parse_chunk_threshold(args.chunk.as_deref());

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
            let file_tokens = doc.tokens();
            chunk_tokens += file_tokens;
            grand_total_tokens += file_tokens;

            // Chunking logic
            if let Some(threshold) = token_threshold {
                if chunk_tokens > threshold && !current_combined.is_empty() {
                    let output = format!("{}\n", current_combined.trim());
                    total_chars += output.len();
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
                    chunk_tokens = file_tokens; // this file opens the next chunk
                }

                current_combined.push_str(&render_block(&doc));
            } else if let Some(writer) = &mut stream_writer {
                // Streaming immediately
                write!(writer, "{}", render_block(&doc))?;
            }
        }

        // Flush remaining chunk
        if token_threshold.is_some() && !current_combined.is_empty() {
            let output = format!("{}\n", current_combined.trim());
            total_chars += output.len();
            let part_file_name = args
                .write
                .as_deref()
                .map(|o| format!("{}.part{}.md", o.trim_end_matches(".md"), part_number))
                .unwrap_or_else(|| format!("gobbled.part{}.md", part_number));
            std::fs::write(&part_file_name, &output)?;
            if !args.quiet {
                eprintln!("  ↳ Baked {}", part_file_name.truecolor(0, 255, 100));
            }
        }

        if args.tokens && !args.quiet {
            let chars = if total_chars > 0 {
                format!("{} chars, ", total_chars)
            } else {
                String::new()
            };
            eprintln!(
                "{}",
                format!(
                    "📊 Output Length ({}~{} tokens across {} files)",
                    chars, grand_total_tokens, file_count
                )
                .truecolor(255, 191, 0)
            );
        }

        Ok(produced.get())
    }
}
