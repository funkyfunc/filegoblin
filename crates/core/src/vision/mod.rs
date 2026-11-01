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
pub mod install;
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
/// pool), so a second page side by side gains little (5–9% on a scanned and
/// a math paper) for a gigabyte more at the peak (3.7 → 2.7 GB on the scan).
/// Two pages run at once only when the process may use 8 GB or more
/// ([`crate::memory::budget_mb`]); below that, one, so a worker held to 4 GB
/// still converts long documents. Page-level parallelism is kept for the
/// cheap geometric path.
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
            limit: if crate::memory::budget_mb() >= 8192 {
                2
            } else {
                1
            },
        })
    }

    /// Runs `f` once a permit is free, off rayon's pool.
    pub fn run<R: Send>(&self, f: impl FnOnce() -> R + Send) -> R {
        if rayon::current_thread_index().is_none() {
            let _permit = self.enter();
            return f();
        }
        std::thread::scope(|s| {
            // As deep as the PDF conversion threads' stacks: the page's
            // content is parsed (recursively) again on this thread.
            std::thread::Builder::new()
                .name("filegoblin-vision".into())
                .stack_size(64 << 20)
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

/// How many conversions on this machine may use the models at once.
pub const SLOTS_ENV: &str = "FILEGOBLIN_VISION_SLOTS";

/// One of the machine's vision slots, held while a conversion uses the models.
///
/// The memory budget bounds one run and cannot see the others. Nineteen
/// `--vision smart` runs started side by side each stayed under their own
/// limit (about 2 GB apiece) and together held 30 GB of a 24 GB Mac, until the
/// OS killed apps to make room. So a conversion takes a slot before it loads
/// the models: one per 8 GB of the machine's memory ([`SLOTS_ENV`] sets the
/// number), shared by every filegoblin process of this user through lock
/// files under `~/.filegoblin/slots`, and waits while all are taken. A killed
/// process gives its slot back, since the OS drops a dead process's locks.
/// The comparison harness's other tools take the same slots
/// (`scripts/alt_engine_worker.py`).
///
/// Threads of one process share its slot: the models are loaded once per
/// process, and [`PageGate`] bounds the pages inside it. The slot is given
/// back between conversions, so a PDF worker idle in its pool never keeps a
/// sibling waiting. Where locks cannot be taken (no home directory, or a
/// platform without `flock`) nothing waits.
pub struct MachineSlot(());

static HELD: std::sync::Mutex<(usize, Option<std::fs::File>)> = std::sync::Mutex::new((0, None));

impl MachineSlot {
    /// Waits for a slot, saying so once unless `quiet`.
    pub fn take(quiet: bool) -> MachineSlot {
        let mut held = HELD.lock().unwrap_or_else(|e| e.into_inner());
        if held.0 == 0 {
            held.1 = wait_for_slot(quiet);
        }
        held.0 += 1;
        MachineSlot(())
    }
}

impl Drop for MachineSlot {
    fn drop(&mut self) {
        let mut held = HELD.lock().unwrap_or_else(|e| e.into_inner());
        held.0 -= 1;
        if held.0 == 0 {
            // Closing the file releases its lock.
            held.1 = None;
        }
    }
}

/// One slot per 8 GB of the machine's memory, at least one.
fn slot_count() -> usize {
    let default = crate::memory::physical_memory().map_or(1, |bytes| (bytes >> 33).max(1));
    crate::memory::env_number(SLOTS_ENV, default).unwrap_or(default) as usize
}

fn wait_for_slot(quiet: bool) -> Option<std::fs::File> {
    let dir = home::home_dir()?.join(".filegoblin").join("slots");
    std::fs::create_dir_all(&dir).ok()?;
    let slots = slot_count();
    let started = std::time::Instant::now();
    let mut told = quiet;
    loop {
        let mut opened = false;
        for i in 0..slots {
            match try_slot(&dir.join(format!("vision-{i}.lock"))) {
                Slot::Taken(file) => return Some(file),
                Slot::Busy => opened = true,
                Slot::Unavailable => {}
            }
        }
        if !opened {
            return None;
        }
        if !told && started.elapsed() > std::time::Duration::from_secs(2) {
            eprintln!(
                "⏳ Waiting for other filegoblin runs to finish with the vision models \
                 ({slots} at a time on this machine; {SLOTS_ENV} sets it)."
            );
            told = true;
        }
        std::thread::sleep(std::time::Duration::from_millis(250));
    }
}

enum Slot {
    Taken(std::fs::File),
    Busy,
    Unavailable,
}

#[cfg(unix)]
fn try_slot(path: &std::path::Path) -> Slot {
    use std::os::fd::AsRawFd;
    let Ok(file) = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)
    else {
        return Slot::Unavailable;
    };
    // SAFETY: flock on a descriptor `file` owns; the lock lives as long as it.
    if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0 {
        Slot::Taken(file)
    } else {
        Slot::Busy
    }
}

#[cfg(not(unix))]
fn try_slot(_path: &std::path::Path) -> Slot {
    Slot::Unavailable
}

/// True unless vision is switched off with `FILEGOBLIN_VISION=0` or the
/// layout weights cannot be found. Loads nothing, so a conversion can take its
/// [`MachineSlot`] before [`enabled`] loads the models.
pub fn wanted() -> bool {
    !std::env::var("FILEGOBLIN_VISION").is_ok_and(|v| v == "0" || v.eq_ignore_ascii_case("off"))
        && models::available(models::LAYOUT)
}

/// True unless vision is switched off with `FILEGOBLIN_VISION=0` or the
/// layout weights cannot be found.
///
/// Loads every model up front, on the calling thread. That is deliberate:
/// rten runs graph optimization on a thread pool while loading, and a rayon
/// worker that waits on it steals other page jobs — which, if loading were
/// lazy, would wait on the very `OnceLock` that worker is initializing.
pub fn enabled() -> bool {
    if !wanted() {
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

#[cfg(test)]
mod tests {
    use super::*;

    /// A slot one holder has is busy to every other, in this process or
    /// another (flock locks belong to the open file), and free once closed.
    #[cfg(unix)]
    #[test]
    fn a_slot_is_held_until_its_holder_lets_go() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("vision-0.lock");
        let Slot::Taken(first) = try_slot(&path) else {
            panic!("a fresh slot is free");
        };
        assert!(matches!(try_slot(&path), Slot::Busy));
        drop(first);
        assert!(matches!(try_slot(&path), Slot::Taken(_)));
        assert!(matches!(
            try_slot(&dir.path().join("missing").join("vision-0.lock")),
            Slot::Unavailable
        ));
    }

    #[test]
    fn the_machine_gets_a_slot_per_8_gb() {
        let n = slot_count();
        assert!(n >= 1);
        if std::env::var_os(SLOTS_ENV).is_none() {
            if let Some(bytes) = crate::memory::physical_memory() {
                assert_eq!(n as u64, (bytes >> 33).max(1));
            }
        }
    }
}
