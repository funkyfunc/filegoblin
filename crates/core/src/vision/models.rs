//! Locating and loading model weights.
//!
//! With the `bundled-models` feature the weights are compiled into the binary
//! (`FILEGOBLIN_MODELS_DIR` at build time names the directory to embed), so a
//! single executable carries everything. Otherwise they are looked up, in
//! order, in the directories listed in `$FILEGOBLIN_MODELS` (separated like
//! `PATH`), a `models/` directory next to the executable,
//! `~/.filegoblin/models`, and `./models`. Each model is loaded once per
//! process and shared across threads.
//!
//! A model may also be stored in numbered pieces (`name.part1of3`, …), which
//! is how the PyPI and npm model packages ship weights under the registries'
//! per-file size limits. The pieces may sit in different directories; they
//! are joined in memory when the model loads.

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

/// The files a release ships (and `--install-models` installs).
pub const RELEASE_SET: [&str; 9] = [
    LAYOUT,
    MFR_ENCODER,
    MFR_DECODER_INIT,
    MFR_DECODER_STEP,
    MFR_TOKENIZER,
    TABLE_STRUCTURE,
    OCR_V6M_DET,
    OCR_V6M_REC,
    OCR_V6_DICT,
];

/// The directories searched for weights, in order.
pub fn candidate_dirs() -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = Vec::new();
    if let Some(list) = std::env::var_os("FILEGOBLIN_MODELS") {
        dirs.extend(std::env::split_paths(&list).filter(|p| !p.as_os_str().is_empty()));
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

/// Where a model's weights are on disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Located {
    /// One file.
    File(PathBuf),
    /// Numbered pieces, in order.
    Parts(Vec<PathBuf>),
}

/// Finds a model in `dirs`: a whole file in the first directory that has
/// one, otherwise a complete set of pieces gathered from all of them.
pub fn locate_in(name: &str, dirs: &[PathBuf]) -> Option<Located> {
    if let Some(p) = dirs.iter().map(|d| d.join(name)).find(|p| p.is_file()) {
        return Some(Located::File(p));
    }
    // `name.part{i}of{n}`
    let prefix = format!("{name}.part");
    let mut total: Option<usize> = None;
    let mut parts: std::collections::BTreeMap<usize, PathBuf> = std::collections::BTreeMap::new();
    for d in dirs {
        let Ok(entries) = std::fs::read_dir(d) else {
            continue;
        };
        for e in entries.flatten() {
            let fname = e.file_name();
            let Some(rest) = fname.to_str().and_then(|f| f.strip_prefix(&prefix)) else {
                continue;
            };
            let Some((i, n)) = rest.split_once("of") else {
                continue;
            };
            let (Ok(i), Ok(n)) = (i.parse::<usize>(), n.parse::<usize>()) else {
                continue;
            };
            if i == 0 || i > n || total.is_some_and(|t| t != n) {
                continue;
            }
            total = Some(n);
            parts.entry(i).or_insert_with(|| e.path());
        }
    }
    let n = total?;
    (parts.len() == n).then(|| Located::Parts(parts.into_values().collect()))
}

/// Where a model is, searching the standard directories.
pub fn locate(name: &str) -> Option<Located> {
    locate_in(name, &candidate_dirs())
}

/// Full path of a model stored as one file, if any search directory has it.
pub fn find(name: &str) -> Option<PathBuf> {
    match locate(name)? {
        Located::File(p) => Some(p),
        Located::Parts(_) => None,
    }
}

/// True when the model can be loaded: compiled in, or found on disk.
pub fn available(name: &str) -> bool {
    bundled(name).is_some() || locate(name).is_some()
}

fn read_located(loc: &Located) -> std::io::Result<Vec<u8>> {
    match loc {
        Located::File(p) => std::fs::read(p),
        Located::Parts(ps) => {
            let mut out = Vec::new();
            for p in ps {
                out.extend_from_slice(&std::fs::read(p)?);
            }
            Ok(out)
        }
    }
}

/// True when this binary carries the release model set itself.
pub fn bundled_build() -> bool {
    cfg!(feature = "bundled-models")
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
    String::from_utf8(read_located(&locate(name)?).ok()?).ok()
}

/// Loads an ONNX model with rten, or `None` (with a debug note) when the file
/// is missing or does not load.
pub fn load(name: &str) -> Option<rten::Model> {
    if let Some(b) = bundled(name) {
        return rten::Model::load_static_slice(b).ok();
    }
    let loc = locate(name)?;
    let loaded = match &loc {
        Located::File(p) => rten::Model::load_file(p).map_err(|e| e.to_string()),
        Located::Parts(_) => read_located(&loc)
            .map_err(|e| e.to_string())
            .and_then(|bytes| rten::Model::load(bytes).map_err(|e| e.to_string())),
    };
    match loaded {
        Ok(m) => Some(m),
        Err(e) => {
            if std::env::var_os("FILEGOBLIN_DEBUG").is_some() {
                eprintln!("filegoblin: could not load {name} ({loc:?}): {e}");
            }
            None
        }
    }
}

/// A human-readable account of where each release model comes from, for
/// `--models-status`.
pub fn status_report() -> String {
    let mut out = String::new();
    if bundled_build() {
        out.push_str("Models: built into this binary.\n");
    }
    out.push_str("Search directories, in order:\n");
    for d in candidate_dirs() {
        let mark = if d.is_dir() { "" } else { "  (not present)" };
        out.push_str(&format!("  {}{mark}\n", d.display()));
    }
    let mut missing = 0;
    out.push_str("Release models:\n");
    for name in RELEASE_SET {
        let line = if bundled(name).is_some() {
            "built in".to_string()
        } else {
            match locate(name) {
                Some(Located::File(p)) => p.display().to_string(),
                Some(Located::Parts(ps)) => format!(
                    "{} pieces, first in {}",
                    ps.len(),
                    ps[0]
                        .parent()
                        .map_or(String::new(), |d| d.display().to_string())
                ),
                None => {
                    missing += 1;
                    "MISSING".to_string()
                }
            }
        };
        out.push_str(&format!("  {name:<24} {line}\n"));
    }
    if missing == 0 {
        out.push_str("All release models found: `--vision smart` (the default) will use them.\n");
    } else {
        out.push_str(&format!(
            "{missing} missing. Run `filegoblin --install-models`, install the `filegoblin[models]` (pip) or `filegoblin-models` (npm) package, or use the filegoblin-full download.\n"
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pieces_are_found_across_directories_in_order() {
        let base = std::env::temp_dir().join(format!("fg-models-{}", std::process::id()));
        let (a, b) = (base.join("a"), base.join("b"));
        std::fs::create_dir_all(&a).unwrap();
        std::fs::create_dir_all(&b).unwrap();
        std::fs::write(a.join("m.onnx.part2of3"), b"BB").unwrap();
        std::fs::write(b.join("m.onnx.part1of3"), b"AA").unwrap();
        std::fs::write(b.join("whole.txt"), b"x").unwrap();
        let dirs = vec![a.clone(), b.clone()];
        // Incomplete: piece 3 is missing.
        assert_eq!(locate_in("m.onnx", &dirs), None);
        std::fs::write(a.join("m.onnx.part3of3"), b"CC").unwrap();
        let loc = locate_in("m.onnx", &dirs).unwrap();
        assert_eq!(read_located(&loc).unwrap(), b"AABBCC");
        assert_eq!(
            locate_in("whole.txt", &dirs),
            Some(Located::File(b.join("whole.txt")))
        );
        let _ = std::fs::remove_dir_all(&base);
    }
}
