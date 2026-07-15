pub mod archive;
pub mod code;
#[cfg(feature = "web")]
pub mod crawler;
#[cfg(feature = "web")]
pub mod epub;
#[cfg(feature = "web")]
pub mod github;
pub mod gobble;
pub mod jupyter;
#[cfg(feature = "ocr")]
pub mod ocr;
pub mod office;
pub mod pdf;
pub mod powerpoint;
pub mod sheet;
#[cfg(feature = "wasm")]
pub mod wasm;
#[cfg(feature = "web")]
pub mod web;

// Re-export common traits so `main.rs` doesn't need to dig
pub use self::code::CodeGobbler;
pub use self::office::OfficeGobbler;
