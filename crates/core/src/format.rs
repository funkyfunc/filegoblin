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
    /// OpenDocument text/presentation (`.odt`/`.odp`). `.ods` routes to
    /// `Spreadsheet` (calamine reads it natively).
    OpenDocument,
    /// Pre-2007 OLE compound files (`.doc`/`.ppt`). No pure-Rust parser exists;
    /// detected so the user gets an actionable error instead of binary garbage.
    LegacyOffice,
    Epub,
    Jupyter,
    Email,
    Rtf,
    Html,
    Code,
    Image,
    /// Audio files (mp3, m4a, wav, flac, ogg, aac) — transcribed to text.
    Audio,
    /// Video containers (mp4, mov, webm, mkv) — audio track transcribed.
    Video,
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
            Format::OpenDocument => "opendocument",
            Format::LegacyOffice => "legacy-office",
            Format::Epub => "epub",
            Format::Jupyter => "jupyter",
            Format::Email => "email",
            Format::Rtf => "rtf",
            Format::Html => "html",
            Format::Code => "code",
            Format::Image => "image",
            Format::Audio => "audio",
            Format::Video => "video",
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
        "odt" | "odp" => Format::OpenDocument,
        "doc" | "ppt" => Format::LegacyOffice,
        "epub" => Format::Epub,
        "ipynb" => Format::Jupyter,
        "eml" | "mbox" => Format::Email,
        "rtf" => Format::Rtf,
        "html" | "htm" => Format::Html,
        "rs" | "js" | "ts" | "py" | "go" | "c" | "cpp" => Format::Code,
        "png" | "jpg" | "jpeg" | "webp" => Format::Image,
        "mp3" | "m4a" | "wav" | "flac" | "ogg" | "oga" | "aac" => Format::Audio,
        "mp4" | "m4v" | "mov" | "webm" | "mkv" => Format::Video,
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
    // OLE compound file (legacy .doc/.ppt/.xls/.msg). Better an actionable
    // error than the plaintext reader dumping binary garbage.
    if header.starts_with(&[0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1]) {
        return Some(Format::LegacyOffice);
    }
    if header.starts_with(&[0x89, b'P', b'N', b'G']) || header.starts_with(&[0xff, 0xd8, 0xff]) {
        return Some(Format::Image); // PNG / JPEG
    }
    if header.starts_with(b"RIFF") && header.len() >= 12 && &header[8..12] == b"WEBP" {
        return Some(Format::Image);
    }
    // Media containers (renamed/extension-less audio & video).
    if header.starts_with(b"RIFF") && header.len() >= 12 && &header[8..12] == b"WAVE" {
        return Some(Format::Audio);
    }
    if header.starts_with(b"ID3") || header.starts_with(b"OggS") || header.starts_with(b"fLaC") {
        return Some(Format::Audio);
    }
    if header.len() >= 12 && &header[4..8] == b"ftyp" {
        // ISO base media: M4A brand = audio, everything else (mp4/mov/m4v) video.
        return Some(if &header[8..11] == b"M4A" {
            Format::Audio
        } else {
            Format::Video
        });
    }
    if header.starts_with(&[0x1A, 0x45, 0xDF, 0xA3]) {
        return Some(Format::Video); // Matroska / WebM (EBML)
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

    #[test]
    fn legacy_office_by_extension_and_magic() {
        assert_eq!(detect(Path::new("a.doc"), b""), Format::LegacyOffice);
        assert_eq!(detect(Path::new("a.ppt"), b""), Format::LegacyOffice);
        let ole = [0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1, 0x00];
        assert_eq!(detect(Path::new("mystery"), &ole), Format::LegacyOffice);
        // Legacy .xls stays with calamine, which reads it natively.
        assert_eq!(detect(Path::new("a.xls"), &ole), Format::Spreadsheet);
    }

    #[test]
    fn opendocument_by_extension() {
        assert_eq!(detect(Path::new("a.odt"), b""), Format::OpenDocument);
        assert_eq!(detect(Path::new("a.odp"), b""), Format::OpenDocument);
        assert_eq!(detect(Path::new("a.ods"), b""), Format::Spreadsheet);
    }
}
