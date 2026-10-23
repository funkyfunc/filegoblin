//! Decoding of plain-text inputs (text, markdown, source code, CSV, subtitles,
//! stdin) whose encoding nobody declared.
//!
//! `std::fs::read_to_string` only accepts UTF-8, so a Latin-1 `notes.txt` or a
//! UTF-16 file saved by Notepad failed outright, and a Latin-1 CSV lost every
//! row containing an accented letter. The order here is the usual one for
//! undeclared text:
//!
//! 1. a byte-order mark names the encoding (UTF-8, UTF-16LE, UTF-16BE);
//! 2. otherwise valid UTF-8 is UTF-8 (the common case costs no copy);
//! 3. otherwise Windows-1252, the superset of Latin-1 that legacy Windows and
//!    Excel exports actually use.
//!
//! Step 3 decodes any byte sequence at all, so it would happily turn a binary
//! file into mojibake that looks like content. Anything decoded by a fallback
//! is therefore checked for binary bytes first, and refused if it has them:
//! an unknown binary must still fail loudly rather than emit garbage.

use anyhow::{Context, Result};
use encoding_rs::{Encoding, UTF_8, WINDOWS_1252};
use std::path::Path;

/// Reads a file and decodes it to text (see the module docs for the order).
pub fn read_text_file(path: &Path) -> Result<String> {
    let bytes = std::fs::read(path).context("Failed to read file as plaintext")?;
    decode_text(bytes)
}

/// Decodes undeclared text: BOM, then UTF-8, then Windows-1252. Errors when the
/// bytes are binary rather than text in some legacy encoding.
pub fn decode_text(bytes: Vec<u8>) -> Result<String> {
    if let Some((encoding, bom_len)) = Encoding::for_bom(&bytes) {
        if encoding == UTF_8 {
            // A UTF-8 BOM is a hint, not a guarantee: the bytes after it still
            // go through the normal UTF-8 → Windows-1252 path.
            return decode_text(bytes[bom_len..].to_vec());
        }
        let (text, _) = encoding.decode_without_bom_handling(&bytes[bom_len..]);
        return refuse_binary(text.into_owned(), encoding.name());
    }
    match String::from_utf8(bytes) {
        Ok(text) => Ok(text),
        Err(not_utf8) => {
            let bytes = not_utf8.into_bytes();
            let (text, _) = WINDOWS_1252.decode_without_bom_handling(&bytes);
            refuse_binary(text.into_owned(), WINDOWS_1252.name())
        }
    }
}

/// Text decoded by a fallback encoding must not carry NULs, and only a trace
/// of control characters: real text in any 8-bit or UTF-16 encoding has
/// neither, while almost every binary format has both.
fn refuse_binary(text: String, encoding: &str) -> Result<String> {
    let mut chars = 0usize;
    let mut controls = 0usize;
    for c in text.chars() {
        chars += 1;
        if c == '\0' {
            anyhow::bail!(
                "The goblin sniffed binary bytes, not text (NUL bytes found): no parser \
                 recognises this file's format"
            );
        }
        // Tab, newlines, form feed and the ANSI escape all turn up in real
        // text files; anything else in the C0/C1 control ranges does not.
        if c.is_control() && !matches!(c, '\t' | '\n' | '\r' | '\x0c' | '\x1b') {
            controls += 1;
        }
    }
    if controls * 100 > chars {
        anyhow::bail!(
            "The goblin sniffed binary bytes, not text ({} of {} characters are control \
             codes when read as {}): no parser recognises this file's format",
            controls,
            chars,
            encoding
        );
    }
    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utf8_passes_through_untouched() {
        assert_eq!(
            decode_text("café ☕".as_bytes().to_vec()).unwrap(),
            "café ☕"
        );
        assert_eq!(decode_text(Vec::new()).unwrap(), "");
    }

    #[test]
    fn utf8_bom_is_stripped() {
        let mut bytes = vec![0xEF, 0xBB, 0xBF];
        bytes.extend_from_slice("name,city".as_bytes());
        assert_eq!(decode_text(bytes).unwrap(), "name,city");
    }

    /// Regression: a Latin-1 file failed with "Failed to read file as plaintext".
    #[test]
    fn latin1_falls_back_to_windows_1252() {
        // "café notes" with é as the single Latin-1 byte 0xE9.
        let bytes = b"caf\xe9 notes\n".to_vec();
        assert_eq!(decode_text(bytes).unwrap(), "café notes\n");
        // Windows-1252's extra punctuation (curly quotes, euro sign) too.
        let bytes = b"\x93quoted\x94 \x80 5".to_vec();
        assert_eq!(decode_text(bytes).unwrap(), "\u{201c}quoted\u{201d} € 5");
    }

    /// Regression: Notepad's UTF-16 output failed the UTF-8-only read.
    #[test]
    fn utf16_with_a_bom_is_decoded() {
        let mut le = vec![0xFF, 0xFE];
        for unit in "hello utf16 wörld\n".encode_utf16() {
            le.extend_from_slice(&unit.to_le_bytes());
        }
        assert_eq!(decode_text(le).unwrap(), "hello utf16 wörld\n");

        let mut be = vec![0xFE, 0xFF];
        for unit in "big endian".encode_utf16() {
            be.extend_from_slice(&unit.to_be_bytes());
        }
        assert_eq!(decode_text(be).unwrap(), "big endian");
    }

    /// The fallback must not turn binary files into plausible-looking text.
    #[test]
    fn binary_is_refused_not_decoded() {
        let png_like = b"\x89PNG\r\n\x1a\n\x00\x00\x00\rIHDR\x00\x00".to_vec();
        let err = decode_text(png_like).unwrap_err().to_string();
        assert!(err.contains("binary"), "got: {err}");

        // No NULs, but dense control bytes (compressed/random data).
        let noise: Vec<u8> = (0u8..=255).filter(|b| *b != 0).cycle().take(600).collect();
        assert!(decode_text(noise).is_err());

        // A UTF-16 BOM in front of binary data is not a free pass.
        let mut fake = vec![0xFF, 0xFE];
        fake.extend_from_slice(&[0u8; 64]);
        assert!(decode_text(fake).is_err());
    }
}
