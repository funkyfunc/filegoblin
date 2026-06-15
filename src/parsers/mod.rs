pub mod code;
pub mod crawler;
pub mod credentials;
pub mod gemini_share;
pub mod github;
pub mod gobble;
pub mod google;
pub mod google_docs;
pub mod ocr;
pub mod office;
pub mod pdf;
pub mod powerpoint;
pub mod sheet;
pub mod twitter;
pub mod wasm;
pub mod web;
pub mod youtube;

// Re-export common traits so `main.rs` doesn't need to dig
pub use self::code::CodeGobbler;
pub use self::gemini_share::GeminiGobbler;
pub use self::google::GoogleGobbler;
pub use self::google_docs::GoogleDocsGobbler;
pub use self::office::OfficeGobbler;
