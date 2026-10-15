//! Locating and loading model weights.
//!
//! With the `bundled-models` feature the weights are compiled into the binary
//! (`FILEGOBLIN_MODELS_DIR` at build time names the directory to embed), so a
//! single executable carries everything. Otherwise they are looked up, in
//! order, in `$FILEGOBLIN_MODELS`, a `models/` directory next to the
//! executable, `~/.filegoblin/models`, and `./models`. Each model is loaded
//! once per process and shared across threads.

use std::path::PathBuf;

/// File names of the bundled models.
pub const LAYOUT: &str = "pp-doclayout-v3.onnx";
pub const MFR_ENCODER: &str = "mfr-encoder.onnx";
pub const MFR_DECODER_INIT: &str = "mfr-decoder-init.onnx";
pub const MFR_DECODER_STEP: &str = "mfr-decoder-step.onnx";
pub const MFR_TOKENIZER: &str = "mfr-tokenizer.json";
pub const TABLE_STRUCTURE: &str = "tatr-structure.onnx";
pub const OCR_DET: &str = "ppocr-v5-det.onnx";
pub const OCR_REC: &str = "ppocr-v5-rec.onnx";
pub const OCR_DICT: &str = "ppocr-v5-dict.txt";
pub const OCR_V6S_DET: &str = "ppocr-v6s-det.onnx";
pub const OCR_V6S_REC: &str = "ppocr-v6s-rec.onnx";
pub const OCR_V6M_DET: &str = "ppocr-v6m-det.onnx";
pub const OCR_V6M_REC: &str = "ppocr-v6m-rec.onnx";
pub const OCR_V6_DICT: &str = "ppocr-v6-dict.txt";

fn candidate_dirs() -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = Vec::new();
    if let Ok(d) = std::env::var("FILEGOBLIN_MODELS") {
        dirs.push(d.into());
    }
    if let Ok(exe) = std::env::current_exe()
        && let Some(dir) = exe.parent()
    {
        dirs.push(dir.join("models"));
    }
    if let Some(home) = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")) {
        dirs.push(PathBuf::from(home).join(".filegoblin").join("models"));
    }
    dirs.push("models".into());
    dirs
}

/// Full path of a model file, if any search directory has it.
pub fn find(name: &str) -> Option<PathBuf> {
    candidate_dirs()
        .into_iter()
        .map(|d| d.join(name))
        .find(|p| p.is_file())
}

#[cfg(feature = "bundled-models")]
macro_rules! bundle {
    ($($file:expr),* $(,)?) => {
        &[$(($file, include_bytes!(concat!(env!("FILEGOBLIN_MODELS_DIR"), "/", $file)) as &[u8])),*]
    };
}

#[cfg(feature = "bundled-models")]
static BUNDLED: &[(&str, &[u8])] = bundle!(
    "pp-doclayout-v3.onnx",
    "mfr-encoder.onnx",
    "mfr-decoder-init.onnx",
    "mfr-decoder-step.onnx",
    "mfr-tokenizer.json",
    "tatr-structure.onnx",
    "ppocr-v6m-det.onnx",
    "ppocr-v6m-rec.onnx",
    "ppocr-v6-dict.txt",
);

#[cfg(feature = "bundled-models")]
fn bundled(name: &str) -> Option<&'static [u8]> {
    BUNDLED.iter().find(|(n, _)| *n == name).map(|(_, b)| *b)
}

#[cfg(not(feature = "bundled-models"))]
fn bundled(_name: &str) -> Option<&'static [u8]> {
    None
}

/// A text asset (tokenizer, dictionary), from the binary or from disk.
pub fn read_text(name: &str) -> Option<String> {
    if let Some(b) = bundled(name) {
        return String::from_utf8(b.to_vec()).ok();
    }
    std::fs::read_to_string(find(name)?).ok()
}

/// Loads an ONNX model with rten, or `None` (with a debug note) when the file
/// is missing or does not load.
pub fn load(name: &str) -> Option<rten::Model> {
    if let Some(b) = bundled(name) {
        return rten::Model::load_static_slice(b).ok();
    }
    let path = find(name)?;
    match rten::Model::load_file(&path) {
        Ok(m) => Some(m),
        Err(e) => {
            if std::env::var_os("FILEGOBLIN_DEBUG").is_some() {
                eprintln!("filegoblin: could not load {}: {e}", path.display());
            }
            None
        }
    }
}
