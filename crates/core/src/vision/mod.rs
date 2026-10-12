//! Bundled vision models for documents, run in-process on the CPU.
//!
//! Everything here is pure Rust: pages are rasterized with `hayro` and the
//! networks run on `rten`. No system libraries, no Python, no network and no
//! LLM at runtime. The weights are permissively licensed ONNX files that ship
//! next to the binary (see [`models`]).
//!
//! * [`layout`] — PP-DocLayoutV3 (Apache-2.0): page regions (titles,
//!   headings, paragraphs, tables, figures, captions, display *and inline*
//!   formulas, page headers and footers) plus a learned reading order.
//! * [`formula`] — pix2text MFR 1.5 (MIT): formula image → LaTeX.
//! * [`table`] — Table Transformer v1.1 (MIT, Microsoft): rows, columns,
//!   header rows and spanning cells of a table image.
//! * [`ocr`] — PP-OCRv5 (Apache-2.0): server text detection and line
//!   recognition, for pages that have no text layer.

pub mod formula;
pub mod layout;
pub mod models;
pub mod ocr;
pub mod render;
pub mod table;

/// Axis-aligned box in PDF user space (points, origin bottom-left, y up).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct UserBox {
    pub x0: f32,
    pub y0: f32,
    pub x1: f32,
    pub y1: f32,
}

impl UserBox {
    pub fn width(&self) -> f32 {
        (self.x1 - self.x0).max(0.0)
    }
    pub fn height(&self) -> f32 {
        (self.y1 - self.y0).max(0.0)
    }
    pub fn area(&self) -> f32 {
        self.width() * self.height()
    }
    pub fn contains_point(&self, x: f32, y: f32) -> bool {
        x >= self.x0 && x <= self.x1 && y >= self.y0 && y <= self.y1
    }
    pub fn intersection(&self, o: &UserBox) -> f32 {
        let w = (self.x1.min(o.x1) - self.x0.max(o.x0)).max(0.0);
        let h = (self.y1.min(o.y1) - self.y0.max(o.y0)).max(0.0);
        w * h
    }
    pub fn iou(&self, o: &UserBox) -> f32 {
        let i = self.intersection(o);
        let u = self.area() + o.area() - i;
        if u <= 0.0 { 0.0 } else { i / u }
    }
    pub fn union(&self, o: &UserBox) -> UserBox {
        UserBox {
            x0: self.x0.min(o.x0),
            y0: self.y0.min(o.y0),
            x1: self.x1.max(o.x1),
            y1: self.y1.max(o.y1),
        }
    }
}

/// True unless vision is switched off with `FILEGOBLIN_VISION=0` or the
/// layout weights cannot be found.
///
/// Loads every model up front, on the calling thread. That is deliberate:
/// rten runs graph optimization on a thread pool while loading, and a rayon
/// worker that waits on it steals other page jobs — which, if loading were
/// lazy, would wait on the very `OnceLock` that worker is initializing.
pub fn enabled() -> bool {
    if std::env::var("FILEGOBLIN_VISION").is_ok_and(|v| v == "0" || v.eq_ignore_ascii_case("off")) {
        return false;
    }
    if layout::model().is_none() {
        return false;
    }
    let _ = formula::available();
    let _ = table::model();
    let _ = ocr::available();
    true
}
