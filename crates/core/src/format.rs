//! Central format detection. Routing resolves a [`Format`] for every input —
//! extension first (fast, matches historical behaviour), then a magic-byte
//! fallback for renamed or extension-less files — so filegoblin can be pointed at
//! anything and still dispatch to the right parser. `label()` is the coarse type
//! string surfaced in the `--json` machine contract.

use std::path::Path;

/// A coarse content category used both to dispatch a parser and to label output.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Pdf,
    Docx,
    Spreadsheet,
    Pptx,
    Epub,
    Jupyter,
    Email,
    Rtf,
    Html,
    Code,
    Image,
    Archive,
    Markdown,
    Text,
}

impl Format {
    /// The stable string used in `--json` output.
    pub fn label(&self) -> &'static str {
        match self {
            Format::Pdf => "pdf",
            Format::Docx => "docx",
            Format::Spreadsheet => "spreadsheet",
            Format::Pptx => "pptx",
            Format::Epub => "epub",
            Format::Jupyter => "jupyter",
            Format::Email => "email",
            Format::Rtf => "rtf",
            Format::Html => "html",
            Format::Code => "code",
            Format::Image => "image",
            Format::Archive => "archive",
            Format::Markdown => "markdown",
            Format::Text => "text",
        }
    }
}

/// Maps a lower-cased file extension to a [`Format`], if recognised.
fn from_extension(ext: &str) -> Option<Format> {
    Some(match ext {
        "pdf" => Format::Pdf,
        "docx" => Format::Docx,
        "xlsx" | "xls" | "ods" | "csv" => Format::Spreadsheet,
        "pptx" => Format::Pptx,
        "epub" => Format::Epub,
        "ipynb" => Format::Jupyter,
        "eml" | "mbox" => Format::Email,
        "rtf" => Format::Rtf,
        "html" | "htm" => Format::Html,
        "rs" | "js" | "ts" | "py" | "go" | "c" | "cpp" => Format::Code,
        "png" | "jpg" | "jpeg" | "webp" => Format::Image,
        "zip" | "gz" | "tgz" => Format::Archive,
        "md" | "markdown" => Format::Markdown,
        _ => return None,
    })
}

/// Detects a format from the first bytes of a file, for inputs whose extension is
/// missing or unrecognised. Returns `None` when nothing matches (caller falls back
/// to `Text`).
fn from_magic(header: &[u8]) -> Option<Format> {
    if header.starts_with(b"%PDF") {
        return Some(Format::Pdf);
    }
    if header.starts_with(b"{\\rtf") {
        return Some(Format::Rtf);
    }
    // Zip family (OOXML/EPUB/plain zip). We can't disambiguate the container here;
    // the extension refines it, and an extension-less zip is treated as an archive.
    if header.starts_with(b"PK\x03\x04") || header.starts_with(b"PK\x05\x06") {
        return Some(Format::Archive);
    }
    if header.starts_with(&[0x1f, 0x8b]) {
        return Some(Format::Archive); // gzip
    }
    if header.starts_with(&[0x89, b'P', b'N', b'G']) || header.starts_with(&[0xff, 0xd8, 0xff]) {
        return Some(Format::Image); // PNG / JPEG
    }
    if header.starts_with(b"RIFF") && header.len() >= 12 && &header[8..12] == b"WEBP" {
        return Some(Format::Image);
    }
    // RFC822-ish email headers at the very start of the stream.
    if header.starts_with(b"From ")
        || header.starts_with(b"Return-Path:")
        || header.starts_with(b"Received:")
        || header.starts_with(b"MIME-Version:")
    {
        return Some(Format::Email);
    }
    // HTML documents without an extension.
    let prefix: Vec<u8> = header
        .iter()
        .take(64)
        .map(|b| b.to_ascii_lowercase())
        .collect();
    if prefix.starts_with(b"<!doctype html") || prefix.starts_with(b"<html") {
        return Some(Format::Html);
    }
    None
}

/// Resolves the [`Format`] for an input: extension first, then a magic-byte
/// fallback, then plaintext. An unknown binary resolves to `Text` too — the
/// plaintext read will fail honestly (surfacing an error) rather than mis-routing
/// it to a parser that would mangle it.
pub fn detect(path: &Path, header: &[u8]) -> Format {
    if let Some(ext) = path.extension().and_then(|e| e.to_str())
        && let Some(fmt) = from_extension(&ext.to_lowercase())
    {
        return fmt;
    }
    from_magic(header).unwrap_or(Format::Text)
}

/// Reads up to `n` leading bytes of a file for magic-byte detection.
pub fn read_header(path: &Path, n: usize) -> Vec<u8> {
    use std::io::Read;
    let mut buf = vec![0u8; n];
    if let Ok(mut f) = std::fs::File::open(path) {
        let read = f.read(&mut buf).unwrap_or(0);
        buf.truncate(read);
    } else {
        buf.clear();
    }
    buf
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extension_wins() {
        assert_eq!(detect(Path::new("a.pdf"), b""), Format::Pdf);
        assert_eq!(detect(Path::new("a.eml"), b""), Format::Email);
        assert_eq!(detect(Path::new("a.rs"), b""), Format::Code);
        assert_eq!(detect(Path::new("a.unknownext"), b"hello"), Format::Text);
    }

    #[test]
    fn magic_fallback_for_extensionless() {
        assert_eq!(detect(Path::new("blob"), b"%PDF-1.7"), Format::Pdf);
        assert_eq!(detect(Path::new("blob"), b"{\\rtf1"), Format::Rtf);
        assert_eq!(
            detect(Path::new("blob"), b"PK\x03\x04rest"),
            Format::Archive
        );
        assert_eq!(
            detect(Path::new("blob"), b"Received: from mx"),
            Format::Email
        );
        assert_eq!(
            detect(Path::new("blob"), b"<!DOCTYPE html><html>"),
            Format::Html
        );
    }

    #[test]
    fn renamed_file_detected_by_magic() {
        // A PDF renamed to .bin still routes to the PDF parser.
        assert_eq!(detect(Path::new("report.bin"), b"%PDF-1.4"), Format::Pdf);
    }

    #[test]
    fn plain_text_default() {
        assert_eq!(detect(Path::new("notes"), b"just some words"), Format::Text);
    }
}
