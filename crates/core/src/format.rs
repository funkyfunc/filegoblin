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
    /// Pre-2007 Word and PowerPoint (`.doc`/`.ppt` and their template and
    /// show variants), and any OLE2 compound file without a known extension.
    /// The parser tells Word, PowerPoint and Excel apart by the container's
    /// streams. `.xls` stays `Spreadsheet`; the sheet parser hands compound
    /// files back to the same reader.
    LegacyOffice,
    Epub,
    /// RSS 2.0 / Atom / RDF syndication feeds.
    Feed,
    /// SubRip / WebVTT subtitle files — rendered as a transcript.
    Subtitle,
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
            Format::Feed => "feed",
            Format::Subtitle => "subtitle",
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
        // The macro-enabled (`m`) and template (`t`) variants of each OOXML
        // format are the same package with a different content type; without
        // them a `.docm` hit the zip magic and came out as an archive of raw
        // XML parts.
        "docx" | "docm" | "dotx" | "dotm" => Format::Docx,
        "xlsx" | "xlsm" | "xltx" | "xltm" | "xlsb" | "xlam" | "xls" | "ods" | "csv" | "tsv" => {
            Format::Spreadsheet
        }
        "pptx" | "pptm" | "potx" | "potm" | "ppsx" | "ppsm" => Format::Pptx,
        "odt" | "odp" => Format::OpenDocument,
        "doc" | "dot" | "ppt" | "pps" | "pot" => Format::LegacyOffice,
        "epub" => Format::Epub,
        "rss" | "atom" => Format::Feed,
        "srt" | "vtt" => Format::Subtitle,
        "ipynb" => Format::Jupyter,
        "eml" | "mbox" => Format::Email,
        "rtf" => Format::Rtf,
        "html" | "htm" => Format::Html,
        // A short list here isn't cosmetic: anything missing is labelled
        // `text` in the --json contract and skips code-specific handling
        // (`--extract symbols`, comment stripping under --compress).
        "rs" | "js" | "mjs" | "cjs" | "jsx" | "ts" | "tsx" | "py" | "pyi" | "go" | "c" | "h"
        | "cpp" | "cc" | "cxx" | "hpp" | "java" | "kt" | "kts" | "rb" | "swift" | "cs" | "php"
        | "scala" | "sh" | "bash" | "zsh" | "fish" | "ps1" | "lua" | "pl" | "r" | "sql" | "vim"
        | "ex" | "exs" | "erl" | "hs" | "ml" | "clj" | "cljs" | "dart" | "zig" | "nim" | "jl"
        | "tf" | "proto" | "gradle" | "cmake" => Format::Code,
        "png" | "jpg" | "jpeg" | "webp" | "heic" | "heif" | "avif" | "tiff" | "tif" | "gif" => {
            Format::Image
        }
        "mp3" | "m4a" | "wav" | "flac" | "ogg" | "oga" | "aac" | "opus" | "caf" => Format::Audio,
        "mp4" | "m4v" | "mov" | "webm" | "mkv" => Format::Video,
        "zip" | "gz" | "tgz" | "tar" => Format::Archive,
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
    // POSIX/GNU tar: no magic at offset 0, but every ustar header carries
    // `ustar` at offset 257.
    if header.get(257..262) == Some(b"ustar".as_slice()) {
        return Some(Format::Archive);
    }
    // OLE compound file (legacy .doc/.ppt/.xls, or a .msg). The legacy parser
    // looks inside to see which it is.
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
    if header.starts_with(b"caff")
        || header.starts_with(b"ID3")
        || header.starts_with(b"OggS")
        || header.starts_with(b"fLaC")
    {
        return Some(Format::Audio);
    }
    if header.len() >= 12 && &header[4..8] == b"ftyp" {
        // ISO base media family — the major brand decides. HEIC/HEIF/AVIF
        // (every iPhone photo) share the ftyp container with mp4/mov, so
        // brand-checking is the only way to tell a still image from a video.
        let brand = &header[8..12];
        return Some(match brand {
            b"heic" | b"heix" | b"heim" | b"heis" | b"hevc" | b"hevx" | b"mif1" | b"msf1"
            | b"avif" | b"avis" => Format::Image,
            b"M4A " | b"M4B " => Format::Audio,
            _ => Format::Video,
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
    // Text formats with no byte-level magic: sniff a larger window.
    let text = String::from_utf8_lossy(&header[..header.len().min(1024)]);
    if crate::parsers::feed::looks_like_feed(&text) {
        return Some(Format::Feed);
    }
    if crate::parsers::subtitle::looks_like_subtitles(&text) {
        return Some(Format::Subtitle);
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
    match from_magic(header) {
        // A zip might be a renamed Office document: look inside before
        // treating it as an archive to extract.
        Some(Format::Archive) if header.starts_with(b"PK") => {
            ooxml_package_kind(path).unwrap_or(Format::Archive)
        }
        Some(fmt) => fmt,
        None => Format::Text,
    }
}

/// Classifies a zip as a Word, Excel or PowerPoint package from its central
/// directory, or `None` when it is not one (or cannot be read).
///
/// Both halves of the test matter: every OOXML package has a
/// `[Content_Types].xml` part, but so do NuGet and XPS packages; and a plain
/// zip can contain a folder that happens to be called `ppt/`. Only the
/// combination is an Office document.
fn ooxml_package_kind(path: &Path) -> Option<Format> {
    let file = std::fs::File::open(path).ok()?;
    let archive = zip::ZipArchive::new(std::io::BufReader::new(file)).ok()?;
    let mut has_content_types = false;
    let mut kind = None;
    for name in archive.file_names() {
        if name == "[Content_Types].xml" {
            has_content_types = true;
        } else if kind.is_none() {
            kind = if name.starts_with("word/") {
                Some(Format::Docx)
            } else if name.starts_with("xl/") {
                Some(Format::Spreadsheet)
            } else if name.starts_with("ppt/") {
                Some(Format::Pptx)
            } else {
                None
            };
        }
        if has_content_types && kind.is_some() {
            return kind;
        }
    }
    None
}

/// How many leading bytes callers should read for [`detect`].
///
/// Byte-level magic needs only ~64, but the text sniffers look at a 1024-byte
/// window — and callers were passing 512, so a feed with more than 512 bytes of
/// XML prolog or comments before `<rss` was never recognised. Reading a little
/// extra costs nothing and keeps the window the sniffers expect.
pub const SNIFF_BYTES: usize = 2048;

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
mod extension_coverage_tests {
    use super::*;

    /// The code list was seven entries, so ordinary source files were labelled
    /// `text` in the --json contract and skipped code-specific handling.
    #[test]
    fn common_languages_are_recognised_as_code() {
        for name in [
            "App.java",
            "main.rb",
            "View.swift",
            "Model.kt",
            "service.cs",
            "index.php",
            "deploy.sh",
            "query.sql",
            "handler.ex",
            "lib.hs",
            "main.zig",
            "infra.tf",
            "api.proto",
        ] {
            assert_eq!(
                detect(Path::new(name), b""),
                Format::Code,
                "{name} should be Code"
            );
        }
    }

    /// A feed with a long prolog needs more than the 512 bytes callers used to
    /// read for the 1024-byte text sniff window to see the root element.
    #[test]
    fn sniff_window_covers_a_long_xml_prolog() {
        let mut doc = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
        doc.push_str(&format!("<!-- {} -->\n", "padding ".repeat(80)));
        doc.push_str("<rss version=\"2.0\"><channel><title>T</title></channel></rss>");
        assert!(doc.len() > 512, "fixture must exceed the old window");

        let header: Vec<u8> = doc.bytes().take(SNIFF_BYTES).collect();
        assert_eq!(detect(Path::new("feed_no_ext"), &header), Format::Feed);
    }
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
        for name in ["a.doc", "a.dot", "a.ppt", "a.pps", "a.pot", "A.DOC"] {
            assert_eq!(detect(Path::new(name), b""), Format::LegacyOffice, "{name}");
        }
        let ole = [0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1, 0x00];
        assert_eq!(detect(Path::new("mystery"), &ole), Format::LegacyOffice);
        // A legacy .xls is still labelled a spreadsheet.
        assert_eq!(detect(Path::new("a.xls"), &ole), Format::Spreadsheet);
    }

    /// Regression: HEIC/HEIF (every iPhone photo) shares the `ftyp` container
    /// with mp4/mov, so brand-blind sniffing sent stills to the transcriber.
    #[test]
    fn ftyp_brands_split_images_audio_and_video() {
        let ftyp = |brand: &[u8]| {
            let mut h = vec![0, 0, 0, 0x28];
            h.extend_from_slice(b"ftyp");
            h.extend_from_slice(brand);
            h.extend_from_slice(b"\0\0\0\0mif1MiHE");
            h
        };
        assert_eq!(detect(Path::new("photo"), &ftyp(b"heic")), Format::Image);
        assert_eq!(detect(Path::new("photo"), &ftyp(b"mif1")), Format::Image);
        assert_eq!(detect(Path::new("photo"), &ftyp(b"avif")), Format::Image);
        assert_eq!(detect(Path::new("song"), &ftyp(b"M4A ")), Format::Audio);
        assert_eq!(detect(Path::new("clip"), &ftyp(b"isom")), Format::Video);
        assert_eq!(detect(Path::new("clip"), &ftyp(b"qt  ")), Format::Video);
        // Extension still wins over the brand.
        assert_eq!(detect(Path::new("a.heic"), b""), Format::Image);
        assert_eq!(detect(Path::new("a.heif"), b""), Format::Image);
    }

    /// Regression: `.docm`/`.xlsm`/`.pptm` and the template variants hit the
    /// zip magic and were dumped as an archive of raw XML parts.
    #[test]
    fn ooxml_variants_route_to_their_office_parser() {
        for ext in ["docx", "docm", "dotx", "dotm", "DOCM"] {
            assert_eq!(detect(Path::new(&format!("a.{ext}")), b""), Format::Docx);
        }
        for ext in ["xlsx", "xlsm", "xltx", "xltm", "xlsb", "xlam", "tsv"] {
            assert_eq!(
                detect(Path::new(&format!("a.{ext}")), b""),
                Format::Spreadsheet,
                "{ext}"
            );
        }
        for ext in ["pptx", "pptm", "potx", "potm", "ppsx", "ppsm"] {
            assert_eq!(detect(Path::new(&format!("a.{ext}")), b""), Format::Pptx);
        }
    }

    /// Regression: a plain `.tar` was read as "plaintext" and failed.
    #[test]
    fn tar_by_extension_and_ustar_magic() {
        assert_eq!(detect(Path::new("a.tar"), b""), Format::Archive);
        let mut header = vec![0u8; 512];
        header[..5].copy_from_slice(b"d/x.t");
        header[257..263].copy_from_slice(b"ustar\0");
        assert_eq!(detect(Path::new("backup"), &header), Format::Archive);
        // GNU tar writes `ustar  \0` — same five leading bytes.
        header[257..265].copy_from_slice(b"ustar  \0");
        assert_eq!(detect(Path::new("backup.bin"), &header), Format::Archive);
    }

    fn write_zip(suffix: &str, names: &[&str]) -> tempfile::NamedTempFile {
        use std::io::Write;
        let mut file = tempfile::Builder::new().suffix(suffix).tempfile().unwrap();
        {
            let mut zip = zip::ZipWriter::new(file.as_file_mut());
            for name in names {
                zip.start_file(*name, zip::write::SimpleFileOptions::default())
                    .unwrap();
                zip.write_all(b"<x/>").unwrap();
            }
            zip.finish().unwrap();
        }
        file
    }

    fn detect_file(file: &tempfile::NamedTempFile) -> Format {
        let header = read_header(file.path(), SNIFF_BYTES);
        detect(file.path(), &header)
    }

    /// A renamed (or extension-less) Office document is a zip; look inside it
    /// instead of extracting it as an archive.
    #[test]
    fn renamed_ooxml_is_recognised_from_its_zip_entries() {
        let docx = write_zip(".bin", &["[Content_Types].xml", "word/document.xml"]);
        assert_eq!(detect_file(&docx), Format::Docx);
        let xlsx = write_zip("", &["[Content_Types].xml", "xl/workbook.xml"]);
        assert_eq!(detect_file(&xlsx), Format::Spreadsheet);
        let pptx = write_zip(".dat", &["ppt/presentation.xml", "[Content_Types].xml"]);
        assert_eq!(detect_file(&pptx), Format::Pptx);
    }

    #[test]
    fn plain_zips_stay_archives() {
        let plain = write_zip(".bin", &["notes/readme.md", "src/main.rs"]);
        assert_eq!(detect_file(&plain), Format::Archive);
        // A folder that happens to be called `ppt/`, without the OPC content
        // types part, is not a presentation.
        let folder = write_zip(".bin", &["ppt/slides.md"]);
        assert_eq!(detect_file(&folder), Format::Archive);
        // NuGet/XPS packages carry `[Content_Types].xml` but no Office parts.
        let nupkg = write_zip(".bin", &["[Content_Types].xml", "lib/net8.0/a.dll"]);
        assert_eq!(detect_file(&nupkg), Format::Archive);
        // An explicit `.zip` is extracted even when it holds a document.
        let zipped_doc = write_zip(".zip", &["[Content_Types].xml", "word/document.xml"]);
        assert_eq!(detect_file(&zipped_doc), Format::Archive);
    }

    #[test]
    fn opendocument_by_extension() {
        assert_eq!(detect(Path::new("a.odt"), b""), Format::OpenDocument);
        assert_eq!(detect(Path::new("a.odp"), b""), Format::OpenDocument);
        assert_eq!(detect(Path::new("a.ods"), b""), Format::Spreadsheet);
    }
}
