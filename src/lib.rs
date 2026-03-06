pub mod cli;
pub mod flavors;
pub mod parsers;
pub mod privacy_shield;
pub mod compressor;

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
    compress: Option<&crate::cli::CompressionLevel>,
    full: bool,
    horde: bool,
    split: bool,
    chunk: Option<&str>,
    out_file: Option<&str>,
    tokens: bool,
    quiet: bool,
    json: bool,
    scrub: bool,
    copy_clipboard: bool,
    open_explorer: bool,
    plugin: Option<&str>,
) -> Result<()> {
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
            if !quiet {
                eprintln!("📥 Sniffing stream from stdin...");
            }
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
                if !quiet {
                    eprintln!("🌐 Sniffing the network for files: {}", url);
                }
                if horde {
                    raw_pairs.extend(crate::parsers::crawler::crawl_web(&url, full)?);
                } else if url.domain().map_or(false, |d| d.ends_with("twitter.com") || d.ends_with("x.com")) {
                    if !quiet {
                        eprintln!("🐦 Diverting to TwitterGobbler for deep context extraction...");
                    }
                    let twitter = parsers::twitter::TwitterGobbler { flavor: flavor.clone() };
                    let text = twitter.gobble_str(target)?;
                    raw_pairs.push((target.to_string(), text));
                } else {
                    let html_content = fetch_url(&url, quiet)?;
                    let text =
                        parsers::web::WebGobbler { extract_full: full }.gobble_str(&html_content)?;
                    raw_pairs.push((target.to_string(), text));
                }
            } else {
                if !quiet {
                    eprintln!("📁 Sniffing for files at local path: {}", target);
                }
                raw_pairs.extend(gobble_local(target, full, horde, plugin)?);
            }
        } else {
            if !quiet {
                eprintln!("📁 Sniffing for files at local path: {}", target);
            }
            raw_pairs.extend(gobble_local(target, full, horde, plugin)?);
        }
    }

    // Apply Privacy Shield if --scrub is requested
    let final_pairs = if scrub {
        if !quiet {
            eprintln!(
                "{}",
                "🛡️ Scrubbed the secrets...".truecolor(255, 191, 0)
            );
        }
        let shield =
            privacy_shield::PrivacyShield::init().context("Failed to initialize Privacy Shield")?;
        raw_pairs
            .into_iter()
            .map(|(p, c)| (p, shield.redact(&c)))
            .collect()
    } else {
        raw_pairs
    };

    // Calculate baseline tokens BEFORE compression for savings metric
    let pre_compression_tokens: usize = final_pairs
        .iter()
        .map(|(path, content)| crate::compressor::heuristic::estimate_tokens(content, path))
        .sum();

    // Apply Compression Pipeline if --compress is flag
    let compressed_pairs = if let Some(level) = compress {
        if !quiet {
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

    if split {
        let root_dir_name = display_name
            .replace("https://", "")
            .replace("http://", "")
            .replace("/", "_")
            .replace(" ", "_")
            .replace("&", "and");
        let root_dir = out_file.map(|d| d.to_string()).unwrap_or_else(|| format!("{}_gobbled", root_dir_name));
        std::fs::create_dir_all(&root_dir)?;

        if !quiet {
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

            if !quiet {
                eprintln!("  ↳ Spat out {}", file_path.display());
            }

            total_chars += flavored.len();
            post_compression_tokens += crate::compressor::heuristic::estimate_tokens(&flavored, path);
        }

        if tokens && !quiet {
            eprintln!(
                "{}",
                format!(
                    "📊 Total Output Length: {} chars (~{} tokens)",
                    total_chars, post_compression_tokens
                )
                .truecolor(255, 191, 0)
            );
        }

        if open_explorer {
            if !quiet {
                eprintln!("{}", format!("🚪 Opening directory: ./{}", root_dir).truecolor(0, 200, 255));
            }
            if let Err(e) = open::that(&root_dir) {
                eprintln!("{} Failed to open directory: {}", "⚠️".yellow(), e);
            }
        }
    } else if json {
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

        if let Some(file_path) = out_file {
            std::fs::write(file_path, &serialized)?;
            if !quiet {
                eprintln!("{}", format!("💾 Saved JSON output to {}", file_path).truecolor(0, 255, 100));
            }
        } else {
            // Print strictly the JSON to standard out
            println!("{}", serialized);
        }

        if copy_clipboard {
            let mut clipboard = arboard::Clipboard::new()?;
            clipboard.set_text(serialized)?;
            if !quiet {
                eprintln!("{}", "📋 Stashed the JSON loot in your clipboard!".truecolor(0, 255, 100));
            }
        }
    } else {
        // Parse token threshold if chunk is enabled
        let token_threshold: Option<usize> = chunk.and_then(|c| {
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
            if !quiet {
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
                    
                    let part_file_name = out_file
                        .map(|o| format!("{}.part{}.md", o.trim_end_matches(".md"), part_number))
                        .unwrap_or_else(|| format!("gobbled.part{}.md", part_number));
                        
                    std::fs::write(&part_file_name, &output)?;
                    if !quiet {
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
                    
                    let part_file_name = out_file
                        .map(|o| format!("{}.part{}.md", o.trim_end_matches(".md"), part_number))
                        .unwrap_or_else(|| format!("gobbled.part{}.md", part_number));
                        
                    std::fs::write(&part_file_name, &output)?;
                    if !quiet {
                        eprintln!("{}", format!("  ↳ Baked {}", part_file_name).truecolor(0, 255, 100));
                    }
                }
            }
            
            // Print chunking summary
            if tokens && !quiet {
                eprintln!(
                    "{}",
                    format!(
                        "📊 Output Length: {} chars (~{} tokens across {} parts)",
                        total_chars, post_compression_tokens, part_number
                    )
                    .truecolor(255, 191, 0)
                );
            }
            
            let tokens_saved = if compress.is_some() && pre_compression_tokens > post_compression_tokens {
                 pre_compression_tokens - post_compression_tokens
            } else {
                 0
            };

            // The Full Belch (Summary Table)
            if !quiet {
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
        
        for (path, content) in compressed_pairs {
            if path == "_tree.md" {
                combined.push_str(&content);
            } else {
                combined.push_str(&format!("// --- FILE_START: {} ---\n", path));
                combined.push_str(&content);
                combined.push_str("\n\n");
            }
            post_compression_tokens += crate::compressor::heuristic::estimate_tokens(&content, &path);
        }

        let output = flavors::format_output(flavor, &display_name, &combined);
        let total_chars = output.len();

        if tokens && !quiet {
            eprintln!(
                "{}",
                format!(
                    "📊 Output Length: {} chars (~{} tokens)",
                    total_chars, post_compression_tokens
                )
                .truecolor(255, 191, 0)
            );
        }

        if let Some(file_path) = out_file {
            std::fs::write(file_path, &output)?;
            if !quiet {
                eprintln!("{}", format!("💾 Saved output to {}", file_path).truecolor(0, 255, 100));
            }
        } else {
            // The actual markdown output
            println!("\n---\n{}", output);
        }

        let tokens_saved = if compress.is_some() && pre_compression_tokens > post_compression_tokens {
             pre_compression_tokens - post_compression_tokens
        } else {
             0
        };

        // The Full Belch (Summary Table)
        if !quiet {
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

        if open_explorer {
            let file_prefix = display_name
                .replace("https://", "")
                .replace("http://", "")
                .replace("/", "_")
                .replace("..", "")
                .replace(":", "_")
                .replace(" ", "_")
                .replace("&", "and");
            
            // Write to the OS temporary directory to prevent accidental local file overwrites
            let temp_dir = std::env::temp_dir();
            let temp_file = temp_dir.join(format!("{}_gobbled.md", file_prefix));
            
            std::fs::write(&temp_file, &output)?;
            
            if !quiet {
                eprintln!("{}", format!("🚪 Kicked open temporary file: {}", temp_file.display()).truecolor(0, 200, 255));
            }
            if let Err(e) = open::that(&temp_file) {
                 eprintln!("{} Failed to open temporary file: {}", "⚠️".yellow(), e);
            }
        }

        if copy_clipboard {
            let mut clipboard = arboard::Clipboard::new()?;
            clipboard.set_text(output)?;
            if !quiet {
                eprintln!("{}", "📋 Stashed the loot in your clipboard!".truecolor(0, 255, 100));
            }
        }
    }

    Ok(())
}

fn gobble_local(target: &str, full: bool, horde: bool, plugin_override: Option<&str>) -> Result<Vec<(String, String)>> {
    let mut files = Vec::new();

    if !horde {
        files.push((target.to_string(), route_and_gobble(target, full, plugin_override)?));
        return Ok(files);
    }

    let walker = ignore::WalkBuilder::new(target).build();
    let root = std::path::Path::new(target);

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
    for file_path in files_to_process {
        if let Ok(rel_path) = file_path.strip_prefix(if root.is_file() {
            root.parent().unwrap_or(root)
        } else {
            root
        }) {
            let rel_str = rel_path.to_string_lossy().into_owned();

            let content = match route_and_gobble(file_path.to_str().unwrap(), full, plugin_override) {
                Ok(c) => c,
                Err(e) => format!("Error summarizing file: {}", e),
            };

            files.push((rel_str, content));
        }
    }

    Ok(files)
}

fn route_and_gobble(path_str: &str, full: bool, plugin_override: Option<&str>) -> Result<String> {
    let path = Path::new(path_str);
    let extension = path
        .extension()
        .and_then(|s| s.to_str())
        .map(|s| s.to_lowercase())
        .unwrap_or_default();

    // EXPLICIT OVERRIDE: If the user passed `--plugin <NAME>`, force execution through that WASM component.
    if let Some(plugin_name) = plugin_override {
        if let Some(plugin_path) = parsers::wasm::WasmGobbler::sniff(plugin_name) {
            match (parsers::wasm::WasmGobbler { wasm_path: plugin_path }).gobble(path) {
                Ok(markdown) => return Ok(markdown),
                Err(e) => anyhow::bail!("Explicit WASM Plugin '{}' failed: {}", plugin_name, e),
            }
        } else {
            anyhow::bail!("Requested explicit --plugin '{}' could not be found in ~/.filegoblin/plugins/ or ./plugins/", plugin_name);
        }
    }

    match extension.as_str() {
        "pdf" => parsers::pdf::PdfGobbler.gobble(path),
        "docx" => parsers::office::OfficeGobbler.gobble(path),
        "xlsx" | "xls" | "ods" | "csv" => parsers::sheet::SheetGobbler.gobble(path),
        "pptx" => parsers::powerpoint::PptxGobbler.gobble(path),
        "html" | "htm" => parsers::web::WebGobbler { extract_full: full }.gobble(path),
        "rs" | "js" | "py" | "ts" | "go" | "c" | "cpp" => parsers::code::CodeGobbler.gobble(path),
        _ => {
            // Priority 1: Check if a user provided a dynamic WASM plugin for this extension
            #[allow(clippy::collapsible_if)]
            if let Some(plugin_path) = parsers::wasm::WasmGobbler::sniff(&extension) {
                if let Ok(markdown) = (parsers::wasm::WasmGobbler { wasm_path: plugin_path }).gobble(path) {
                    return Ok(markdown);
                }
            }

            // Priority 2: Fall back to core heuristics
            // If it's an image, pass to ocr. If text, just read.
            if ["png", "jpg", "jpeg", "webp"].contains(&extension.as_str()) {
                parsers::ocr::OcrGobbler.gobble(path)
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

        let parsed_content = crate::parsers::web::WebGobbler {
            extract_full: false,
        }
        .gobble_str(&html_content)
        .unwrap();
        assert!(parsed_content.contains("Goblin network testing!"));
    }

    #[test]
    fn test_route_and_gobble_unknown_extension() {
        // Fallback for an unknown extension (.xyz)
        let test_file = "dummy.xyz";
        std::fs::write(test_file, "Plaintext fallback text").unwrap();
        let res = route_and_gobble(test_file, false, None).unwrap();
        assert_eq!(res, "Plaintext fallback text");
        std::fs::remove_file(test_file).ok();
    }
}
