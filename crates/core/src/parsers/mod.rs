pub mod archive;
pub mod code;
#[cfg(feature = "web")]
pub mod crawler;
pub mod email;
pub mod epub;
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
#[cfg(feature = "wasm")]
pub mod wasm;

// Re-export common traits so `main.rs` doesn't need to dig
pub use self::code::CodeGobbler;
pub use self::office::OfficeGobbler;
