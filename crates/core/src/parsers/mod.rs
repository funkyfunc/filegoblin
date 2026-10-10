pub mod archive;
pub mod code;

/// Resolves a `quick_xml` `Event::GeneralRef` (`&lt;`, `&amp;`, `&#233;`, …).
///
/// quick-xml 0.39 emits entity references as their **own events**, separate
/// from `Event::Text`. A walker that only handles `Text` therefore drops every
/// escaped character silently — e.g. an Atom `<content>` carrying escaped HTML
/// arrives as `pHello/p` with the angle brackets gone. Every XML parser here
/// routes `GeneralRef` through this.
pub(crate) fn resolve_xml_entity(e: &quick_xml::events::BytesRef) -> Option<String> {
    if let Ok(Some(c)) = e.resolve_char_ref() {
        return Some(c.to_string()); // numeric: &#233; / &#x30;
    }
    let name = String::from_utf8_lossy(e);
    quick_xml::escape::resolve_predefined_entity(&name).map(str::to_string)
}

/// Cap on a single decompressed zip member read into memory.
///
/// `.docx`/`.pptx`/`.odt`/`.epub` are all zip containers, so the bomb guard in
/// `archive.rs` is not the only place that inflates attacker-controlled data —
/// a 10 MB `.pptx` whose `slide1.xml` expands to 50 GB used to be read straight
/// into a `String`. Every container parser reads members through
/// [`read_zip_entry_to_string`] instead of `read_to_string`.
pub(crate) const MAX_ZIP_ENTRY_BYTES: u64 = 256 * 1024 * 1024; // 256 MiB

/// Reads one zip member to a `String`, refusing to inflate past
/// [`MAX_ZIP_ENTRY_BYTES`].
///
/// Bounds the read with `take(cap + 1)` and checks what actually arrived: the
/// entry's declared size is attacker-controlled metadata a bomb simply lies
/// about, so it is never trusted.
pub(crate) fn read_zip_entry_to_string<R: std::io::Read>(
    entry: &mut R,
    name: &str,
) -> anyhow::Result<String> {
    use std::io::Read as _;
    let mut buf = Vec::new();
    entry
        .take(MAX_ZIP_ENTRY_BYTES + 1)
        .read_to_end(&mut buf)
        .map_err(|e| anyhow::anyhow!("Failed to read `{}` from the container: {}", name, e))?;
    if buf.len() as u64 > MAX_ZIP_ENTRY_BYTES {
        anyhow::bail!(
            "`{}` inflates past the {} MiB limit (bomb guard)",
            name,
            MAX_ZIP_ENTRY_BYTES / (1024 * 1024)
        );
    }
    String::from_utf8(buf)
        .map_err(|_| anyhow::anyhow!("`{}` is not valid UTF-8 (corrupt container?)", name))
}

/// Builds a markdown pipe table from cells fed in row order.
///
/// Five parsers (docx, pptx, ODF, spreadsheets, and the HTML extractor) each
/// grew their own copy of "join cells with pipes, emit a separator after row
/// 0", and each copy had a different bug: one dropped self-closing empty cells
/// so values shifted a column left, another emitted rows wider than the header,
/// none escaped cell text the same way. Rows are buffered so every row can be
/// padded to a single width before rendering.
#[derive(Default)]
pub(crate) struct TableBuilder {
    rows: Vec<Vec<String>>,
    current: Vec<String>,
}

impl TableBuilder {
    /// Appends `text` as `repeat` identical cells (formats that carry a
    /// "repeat this cell N times" attribute expand through this).
    pub(crate) fn push_cell(&mut self, text: &str, repeat: usize) {
        let cell = Self::escape(text);
        for _ in 0..repeat {
            self.current.push(cell.clone());
        }
    }

    /// Closes the current row. Trailing empty cells are writer padding rather
    /// than data, so they are dropped; short rows are padded back out at render
    /// time against the real table width.
    pub(crate) fn end_row(&mut self) {
        while self.current.last().is_some_and(|c| c.is_empty()) {
            self.current.pop();
        }
        if !self.current.is_empty() {
            self.rows.push(std::mem::take(&mut self.current));
        }
        self.current.clear();
    }

    /// Renders and resets. Row 0 becomes the header; every row is padded to the
    /// widest row so no cell can land under the wrong column.
    pub(crate) fn render(&mut self) -> String {
        let width = self.rows.iter().map(|r| r.len()).max().unwrap_or(0);
        if width == 0 {
            self.rows.clear();
            return String::new();
        }
        let mut out = String::new();
        for (i, row) in self.rows.iter().enumerate() {
            let mut cells: Vec<&str> = row.iter().map(String::as_str).collect();
            cells.resize(width, "");
            out.push_str(&format!("| {} |\n", cells.join(" | ")));
            if i == 0 {
                out.push_str(&format!("|{}\n", "---|".repeat(width)));
            }
        }
        self.rows.clear();
        out
    }

    /// A pipe inside a cell would end the cell; a newline would end the row.
    fn escape(text: &str) -> String {
        text.split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .replace('|', "\\|")
    }
}

#[cfg(test)]
mod table_tests {
    use super::TableBuilder;

    #[test]
    fn pads_short_rows_and_escapes_cells() {
        let mut t = TableBuilder::default();
        t.push_cell("A", 1);
        t.push_cell("B", 1);
        t.push_cell("C", 1);
        t.end_row();
        t.push_cell("only", 1);
        t.end_row();
        t.push_cell("a|b", 1);
        t.push_cell("multi\nline", 1);
        t.end_row();
        let out = t.render();
        assert!(out.contains("| A | B | C |"), "{out}");
        assert!(out.contains("|---|---|---|"), "{out}");
        assert!(out.contains("| only |  |  |"), "{out}");
        assert!(out.contains("| a\\|b | multi line |  |"), "{out}");
        assert!(t.render().is_empty(), "render must reset the builder");
    }

    #[test]
    fn repeat_expands_and_trailing_filler_is_dropped() {
        let mut t = TableBuilder::default();
        t.push_cell("x", 1);
        t.push_cell("d", 3);
        t.push_cell("", 4096);
        t.end_row();
        assert_eq!(t.render().lines().next().unwrap(), "| x | d | d | d |");
    }
}

/// The first `max_bytes` of `s`, truncated **down to a char boundary**.
///
/// `&s[..s.len().min(n)]` panics whenever a multi-byte character straddles byte
/// `n`, which is the common case for any non-ASCII document: sniffing the head
/// of a fetched page or the head of OCR output would abort the process. Every
/// prefix-slice of user data goes through this.
pub(crate) fn str_prefix(s: &str, max_bytes: usize) -> &str {
    if s.len() <= max_bytes {
        return s;
    }
    let mut end = max_bytes;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

#[cfg(test)]
mod prefix_tests {
    use super::str_prefix;

    #[test]
    fn prefix_never_panics_on_a_split_multibyte_char() {
        // 'é' is two bytes and straddles the cut at 1024.
        let s = format!("{}é{}", "a".repeat(1023), "b".repeat(200));
        assert_eq!(str_prefix(&s, 1024).len(), 1023);
        // A cut landing inside a 3-byte char walks all the way back.
        assert_eq!(str_prefix("日本語", 4), "日");
        assert_eq!(str_prefix("日本語", 3), "日");
        assert_eq!(str_prefix("日本語", 2), "");
        // Shorter-than-max and exact-fit are returned whole.
        assert_eq!(str_prefix("abc", 10), "abc");
        assert_eq!(str_prefix("abc", 3), "abc");
    }
}

#[cfg(feature = "web")]
pub mod crawler;
pub mod email;
pub mod epub;
pub mod feed;
#[cfg(feature = "web")]
pub mod github;
pub mod gobble;
pub mod html;
pub mod jupyter;
pub mod ocr;
pub mod office;
pub mod omml;
pub mod opendocument;
pub mod openxml_chart;
pub mod pdf;
pub mod powerpoint;
pub mod rtf;
pub mod sheet;
pub mod subtitle;
pub mod transcribe;
pub mod transcript;
#[cfg(feature = "wasm")]
pub mod wasm;

// Re-export common traits so `main.rs` doesn't need to dig
pub use self::code::CodeGobbler;
pub use self::office::OfficeGobbler;
