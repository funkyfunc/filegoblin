use crate::parsers::gobble::Gobble;
use anyhow::Result;
use std::path::Path;

// Embed the Tesseract WASM directly into the executable at compile time!
// If build.rs failed offline, it may be a dummy 0-byte file.
const TESSERACT_CORE_WASM: &[u8] = include_bytes!("../../assets/tesseract-core-simd.wasm");

pub struct OcrGobbler;

impl Gobble for OcrGobbler {
    fn gobble(&self, _path: &Path) -> Result<String> {
        // MOCK IMPLEMENTATION: Awaiting tesseract-wasm / wasmtime integration
        
        // Ensure that our build pipeline actually downloaded the brains.
        if TESSERACT_CORE_WASM.is_empty() {
             anyhow::bail!("This one is too gristly! I need a password to chew on it. (Offline Mode: No OCR Brains)")
        }

        anyhow::bail!("OCR Brains loaded! But the WASM bindings remain to be forged.")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ocr_extraction() {
        let gobbler = OcrGobbler;
        // Simulating passing an image or scanned pdf path
        let p = Path::new("dummy_scan.png");
        let result = gobbler.gobble(p);
        
        // Assert mock extraction returns our vibe-spec error
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Brains loaded"));
    }
}
