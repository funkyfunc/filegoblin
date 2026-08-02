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
pub mod opendocument;
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
