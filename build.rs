use std::fs::{self, File};
use std::io::Write;
use std::path::Path;

const WASM_URL: &str = "https://unpkg.com/tesseract.js-core@6.1.2/tesseract-core-simd.wasm";
const WASM_FILE: &str = "assets/tesseract-core-simd.wasm";

fn main() {
    println!("cargo:rerun-if-changed=build.rs");

    // We only rerun if the asset is missing
    println!("cargo:rerun-if-changed={}", WASM_FILE);

    let asset_path = Path::new(WASM_FILE);

    // Ensure assets directory exists
    if let Some(parent) = asset_path.parent()
        && !parent.exists()
    {
        let _ = fs::create_dir_all(parent);
    }

    if !asset_path.exists() {
        println!("cargo:warning=filegoblin: OCR WASM brains missing. Fetching from web...");
        match fetch_wasm() {
            Ok(bytes) => {
                if let Ok(mut file) = File::create(asset_path) {
                    let _ = file.write_all(&bytes);
                    println!("cargo:warning=filegoblin: OCR Brains successfully deposited!");
                }
            }
            Err(e) => {
                // Warning, but don't fail the build to allow local offline dev
                println!(
                    "cargo:warning=filegoblin: Failed to fetch OCR WASM: {}. Falling back to gristly mocks.",
                    e
                );
                // Create a dummy empty file so `include_bytes!` doesn't panic on compilation
                if let Ok(mut file) = File::create(asset_path) {
                    let _ = file.write_all(b"");
                }
            }
        }
    }
}

fn fetch_wasm() -> Result<Vec<u8>, reqwest::Error> {
    let client = reqwest::blocking::Client::new();
    let resp = client.get(WASM_URL).send()?;
    let bytes = resp.bytes()?;
    Ok(bytes.to_vec())
}
