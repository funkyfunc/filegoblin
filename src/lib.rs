pub mod flavors;
pub mod parsers;

use crate::parsers::gobble::Gobble;
use anyhow::{Context, Result};
use std::path::Path;
use url::Url;

/// filegoblin Core
/// We keep logic in lib.rs to ensure the application is deeply testable
/// independently from the `clap` CLI layer.
pub fn gobble_app(target: &str, flavor: &flavors::Flavor, full: bool) -> Result<()> {
    // Determine if the target is a URL or a Local Path
    let raw_content;
    let display_name;

    if let Ok(url) = Url::parse(target) {
        if url.scheme() == "http" || url.scheme() == "https" {
            println!("🌐 Gobbling network address: {}", url);
            display_name = target.to_string();
            let html_content = fetch_url(&url)?;
            raw_content = parsers::web::WebGobbler { extract_full: full }.gobble_str(&html_content)?;
        } else {
            println!("📁 Gobbling local path: {}", target);
            display_name = target.to_string();
            raw_content = route_and_gobble(target, full)?;
        }
    } else {
        println!("📁 Gobbling local path: {}", target);
        display_name = target.to_string();
        raw_content = route_and_gobble(target, full)?;
    }

    // Format output via the flavors engine
    let output = flavors::format_output(flavor, &display_name, &raw_content);

    // TEMPORARY: output to stdout as placeholder
    println!("\n---\n{}", output);

    Ok(())
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

fn fetch_url(url: &Url) -> Result<String> {
    // reqwest::blocking::Client automatically respects HTTP_PROXY and HTTPS_PROXY
    if let Ok(proxy) = std::env::var("HTTPS_PROXY").or_else(|_| std::env::var("HTTP_PROXY")) {
        println!("🔒 Corporate proxy detected: routing through {}", proxy);
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
        let html_content = fetch_url(&parsed_url).unwrap();
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
