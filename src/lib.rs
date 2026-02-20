pub mod parsers;
pub mod flavors;

use anyhow::{Context, Result};
use std::path::Path;
use url::Url;
use crate::parsers::gobble::Gobble;

/// filegoblin Core
/// We keep logic in lib.rs to ensure the application is deeply testable
/// independently from the `clap` CLI layer.
pub fn gobble_app(target: &str, flavor: &flavors::Flavor) -> Result<()> {
    // Determine if the target is a URL or a Local Path
    let raw_content;
    let display_name;

    if let Ok(url) = Url::parse(target) {
        if url.scheme() == "http" || url.scheme() == "https" {
            println!("🌐 Gobbling network address: {}", url);
            display_name = target.to_string();
            let html_content = fetch_url(&url)?;
            raw_content = parsers::web::WebGobbler.gobble_str(&html_content)?; 
        } else {
            println!("📁 Gobbling local path: {}", target);
            display_name = target.to_string();
            raw_content = route_and_gobble(target)?;
        }
    } else {
        println!("📁 Gobbling local path: {}", target);
        display_name = target.to_string();
        raw_content = route_and_gobble(target)?;
    }

    // Format output via the flavors engine
    let output = flavors::format_output(flavor, &display_name, &raw_content);
    
    // TEMPORARY: output to stdout as placeholder
    println!("\n---\n{}", output);

    Ok(())
}

fn route_and_gobble(path_str: &str) -> Result<String> {
    let path = Path::new(path_str);
    let extension = path.extension()
        .and_then(|s| s.to_str())
        .map(|s| s.to_lowercase())
        .unwrap_or_default();

    match extension.as_str() {
        "pdf" => parsers::pdf::PdfGobbler.gobble(path),
        "docx" | "xlsx" => parsers::office::OfficeGobbler.gobble(path),
        "html" | "htm" => parsers::web::WebGobbler.gobble(path),
        "rs" | "js" | "py" | "ts" | "go" | "c" | "cpp" => parsers::code::CodeGobbler.gobble(path),
        _ => {
            // Default to OCR or raw text read for unknown formats
            // If it's an image, pass to ocr. If text, just read.
            if ["png", "jpg", "jpeg", "webp"].contains(&extension.as_str()) {
                parsers::ocr::OcrGobbler.gobble(path)
            } else {
                std::fs::read_to_string(path)
                    .context("Failed to read file as plaintext")
            }
        }
    }
}

fn fetch_url(url: &Url) -> Result<String> {
    let client = reqwest::blocking::Client::builder()
        .user_agent("filegoblin/1.5.0")
        .build()?;
        
    let resp = client.get(url.clone())
        .send()
        .context("Mischievous network error: Failed to reach the URL")?;
        
    let text = resp.text()
        .context("Mischievous decoding error: Failed to read URL body as UTF-8")?;
        
    Ok(text)
}
