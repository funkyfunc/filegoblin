pub mod flavors;
pub mod parsers;

use crate::parsers::gobble::Gobble;
use anyhow::{Context, Result};
use std::path::Path;
use url::Url;
use colored::Colorize;

/// filegoblin Core
/// We keep logic in lib.rs to ensure the application is deeply testable
/// independently from the `clap` CLI layer.
pub fn gobble_app(target: &str, flavor: &flavors::Flavor, full: bool, horde: bool, split: bool, tokens: bool, quiet: bool, json: bool) -> Result<()> {
    let display_name = target.to_string();
    let raw_pairs: Vec<(String, String)>;

    if let Ok(url) = Url::parse(target) {
        if url.scheme() == "http" || url.scheme() == "https" {
            if !quiet {
                eprintln!("🌐 Gobbling network address: {}", url);
            }
            if horde {
                raw_pairs = crate::parsers::crawler::crawl_web(&url, full)?;
            } else {
                let html_content = fetch_url(&url, quiet)?;
                let text = parsers::web::WebGobbler { extract_full: full }.gobble_str(&html_content)?;
                raw_pairs = vec![(target.to_string(), text)];
            }
        } else {
            if !quiet {
                eprintln!("📁 Gobbling local path: {}", target);
            }
            raw_pairs = gobble_local(target, full, horde)?;
        }
    } else {
        if !quiet {
            eprintln!("📁 Gobbling local path: {}", target);
        }
        raw_pairs = gobble_local(target, full, horde)?;
    }

    if split {
        let root_dir_name = target.replace("https://", "").replace("http://", "").replace("/", "_");
        let root_dir = format!("{}_gobbled", root_dir_name);
        std::fs::create_dir_all(&root_dir)?;
        
        if !quiet {
            eprintln!("{}", format!("💾 Splitting into directory: ./{}", root_dir).truecolor(0, 255, 100));
        }

        let mut total_tokens = 0;
        let mut total_chars = 0;

        for (path, content) in raw_pairs {
            // Safe pathing logic for urls and relative filepaths
            let safe_path = if let Ok(u) = Url::parse(&path) {
                let mut p = u.path().trim_start_matches('/').to_string();
                if p.is_empty() { p = "index".to_string(); }
                if p.ends_with('/') { p.push_str("index"); }
                p.replace("..", "").replace(":", "_")
            } else {
                path.replace("..", "")
            };
            
            let file_name = if safe_path.ends_with(".md") { safe_path } else { format!("{}.md", safe_path) };
            let file_path = std::path::Path::new(&root_dir).join(file_name);
            
            if let Some(parent) = file_path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            
            let flavored = flavors::format_output(flavor, &path, &content);
            std::fs::write(&file_path, &flavored)?;

            if !quiet {
                eprintln!("  ↳ Wrote {}", file_path.display());
            }
            
            total_chars += flavored.len();
            total_tokens += flavored.len() / 4;
        }

        if tokens && !quiet {
            eprintln!("{}", format!("📊 Total Output Length: {} chars (~{} tokens)", total_chars, total_tokens).truecolor(255, 191, 0));
        }
    } else {
        if json {
            // Build strictly structured JSON array format matching our (String, String) pairs
            #[derive(serde::Serialize)]
            struct FileNode {
                path: String,
                content: String,
            }
            
            let mut out = Vec::new();
            for (p, c) in raw_pairs {
                out.push(FileNode { path: p, content: c });
            }
            let serialized = serde_json::to_string_pretty(&out)?;
            
            // Print strictly the JSON to standard out
            println!("{}", serialized);
        } else {
            let mut combined = String::new();
            for (path, content) in raw_pairs {
                if path == "_tree.md" {
                    combined.push_str(&content);
                } else {
                    combined.push_str(&format!("// --- FILE_START: {} ---\n", path));
                    combined.push_str(&content);
                    combined.push_str("\n\n");
                }
            }

            let output = flavors::format_output(flavor, &display_name, &combined);

            if tokens && !quiet {
                let approx_tokens = output.len() / 4;
                eprintln!("{}", format!("📊 Output Length: {} chars (~{} tokens)", output.len(), approx_tokens).truecolor(255, 191, 0));
            }

            println!("\n---\n{}", output);
        }
    }

    Ok(())
}

fn gobble_local(target: &str, full: bool, horde: bool) -> Result<Vec<(String, String)>> {
    let mut files = Vec::new();

    if !horde {
        files.push((target.to_string(), route_and_gobble(target, full)?));
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
        if let Ok(rel_path) = file_path.strip_prefix(if root.is_file() { root.parent().unwrap_or(root) } else { root }) {
            let rel_str = rel_path.to_string_lossy().into_owned();
            
            let content = match route_and_gobble(file_path.to_str().unwrap(), full) {
                Ok(c) => c,
                Err(e) => format!("Error summarizing file: {}", e),
            };
            
            files.push((rel_str, content));
        }
    }

    Ok(files)
}

fn route_and_gobble(path_str: &str, full: bool) -> Result<String> {
    let path = Path::new(path_str);
    let extension = path
        .extension()
        .and_then(|s| s.to_str())
        .map(|s| s.to_lowercase())
        .unwrap_or_default();

    match extension.as_str() {
        "pdf" => parsers::pdf::PdfGobbler.gobble(path),
        "docx" | "xlsx" => parsers::office::OfficeGobbler.gobble(path),
        "html" | "htm" => parsers::web::WebGobbler { extract_full: full }.gobble(path),
        "rs" | "js" | "py" | "ts" | "go" | "c" | "cpp" => parsers::code::CodeGobbler.gobble(path),
        _ => {
            // Default to OCR or raw text read for unknown formats
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
    if let Ok(proxy) = std::env::var("HTTPS_PROXY").or_else(|_| std::env::var("HTTP_PROXY")) {
        if !quiet {
            eprintln!("🔒 Corporate proxy detected: routing through {}", proxy);
        }
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
                let header = format!("HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len());
                let _ = stream.write_all(header.as_bytes());
                let _ = stream.write_all(body);
            }
        });

        let parsed_url = Url::parse(&url_str).unwrap();
        let html_content = fetch_url(&parsed_url, true).unwrap();
        assert!(html_content.contains("Goblin network testing!"));

        let parsed_content = crate::parsers::web::WebGobbler { extract_full: false }.gobble_str(&html_content).unwrap();
        assert!(parsed_content.contains("Goblin network testing!"));
    }

    #[test]
    fn test_route_and_gobble_unknown_extension() {
        // Fallback for an unknown extension (.xyz)
        let test_file = "dummy.xyz";
        std::fs::write(test_file, "Plaintext fallback text").unwrap();
        let res = route_and_gobble(test_file, false).unwrap();
        assert_eq!(res, "Plaintext fallback text");
        std::fs::remove_file(test_file).ok();
    }
}
