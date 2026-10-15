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
//! * [`ocr`] — PP-OCRv6 medium (Apache-2.0): text line detection and
//!   recognition, for pages that have no usable text layer.

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

/// Bounds how many pages run through the models at once.
///
/// Each model already spreads one page over every core (rten's own thread
/// pool), so running more pages side by side adds memory — a few hundred MB
/// of activations per page — without adding speed. Page-level parallelism is
/// kept for the cheap geometric path.
///
/// The gated work runs on a plain thread, never on a rayon worker. A rayon
/// worker that waits for rten's pool steals other jobs meanwhile; if it
/// stole another page it would block on this gate while still holding a
/// permit, and with every permit held that way the run deadlocks (seen on
/// multi-page scans). A plain thread waits without stealing, so a permit
/// holder only ever waits on rten.
pub struct PageGate {
    busy: std::sync::Mutex<usize>,
    freed: std::sync::Condvar,
    limit: usize,
}

/// Held while a page is inside the models.
struct PagePermit<'a>(&'a PageGate);

impl PageGate {
    pub fn global() -> &'static PageGate {
        static GATE: std::sync::OnceLock<PageGate> = std::sync::OnceLock::new();
        GATE.get_or_init(|| PageGate {
            busy: std::sync::Mutex::new(0),
            freed: std::sync::Condvar::new(),
            limit: 2,
        })
    }

    /// Runs `f` once a permit is free, off rayon's pool.
    pub fn run<R: Send>(&self, f: impl FnOnce() -> R + Send) -> R {
        if rayon::current_thread_index().is_none() {
            let _permit = self.enter();
            return f();
        }
        std::thread::scope(|s| {
            std::thread::Builder::new()
                .name("filegoblin-vision".into())
                .stack_size(8 << 20)
                .spawn_scoped(s, || {
                    let _permit = self.enter();
                    f()
                })
                .expect("spawn a vision thread")
                .join()
                .unwrap_or_else(|p| std::panic::resume_unwind(p))
        })
    }

    fn enter(&self) -> PagePermit<'_> {
        let mut busy = self.busy.lock().unwrap_or_else(|e| e.into_inner());
        while *busy >= self.limit {
            busy = self.freed.wait(busy).unwrap_or_else(|e| e.into_inner());
        }
        *busy += 1;
        PagePermit(self)
    }
}

impl Drop for PagePermit<'_> {
    fn drop(&mut self) {
        let mut busy = self.0.busy.lock().unwrap_or_else(|e| e.into_inner());
        *busy -= 1;
        self.0.freed.notify_one();
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
