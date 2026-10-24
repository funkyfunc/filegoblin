//! PDF conversion in child processes ("workers").
//!
//! Some PDFs cannot be survived in-process. pdf_oxide parses nested arrays and
//! dictionaries recursively with no depth limit, so nesting the pre-scan in
//! `pdf.rs` cannot see (inside LZW or ASCII85 streams, or filter chains)
//! overflows the stack — and a stack overflow aborts the whole process, where
//! no `catch_unwind` reaches it. pdf_oxide caps each inflated stream at
//! 256 MB, but pages decode in parallel and any number of pages can share one
//! bomb: a 15 KB file of 64 such pages took 12.9 GB. And a page can take
//! hours: one line of a million kerned glyphs is quadratic work.
//!
//! So the CLI converts PDFs in workers: copies of its own binary started with
//! [`WORKER_FLAG`], reading requests on stdin and writing replies on stdout. A
//! worker converts one PDF at a time and lives for the whole run, so the
//! vision models load once per worker rather than once per file. When a worker
//! dies (a stack overflow, any other crash), goes over its memory allowance
//! (it measures itself and exits with a status of its own) or finishes no page
//! for the stall timeout (the parent kills it), that one file fails with the
//! usual per-file error and the next PDF gets a fresh worker.
//!
//! Library users of `filegoblin-core` convert in-process unless they call
//! [`enable`] — and hand their own binary's [`WORKER_FLAG`] runs to [`serve`],
//! as the CLI's `main` does. `FILEGOBLIN_PDF_WORKER=0` turns workers off for
//! debugging.
//!
//! The wire format, both ways: frames of a tag byte, a little-endian u64
//! length and that many bytes.

use crate::parsers::gobble::ParseOptions;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::io::{BufRead, BufReader, ErrorKind, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, OnceLock};
use std::time::{Duration, Instant};

/// The first argument of a worker. The CLI checks for it before anything else.
pub const WORKER_FLAG: &str = "--pdf-worker";

/// How a worker over its memory allowance exits.
const OUT_OF_MEMORY: i32 = 86;

/// Windows' exit status for a process that overflowed its stack.
const STATUS_STACK_OVERFLOW: i32 = 0xC000_00FD_u32 as i32;

const DEFAULT_STALL_SECS: u64 = 120;

const TIMEOUT_ENV: &str = "FILEGOBLIN_PDF_TIMEOUT";
const MEMORY_ENV: &str = "FILEGOBLIN_PDF_MEMORY_MB";
const WORKERS_ENV: &str = "FILEGOBLIN_PDF_WORKERS";

// Frame tags.
const REQUEST: u8 = b'R';
const PROGRESS: u8 = b'P';
const CONVERTED: u8 = b'O';
const FAILED: u8 = b'E';

/// What one PDF may cost a worker.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Limits {
    /// The longest a worker may go without finishing a page.
    stall: Duration,
    /// The most memory a worker may use.
    memory_mb: u64,
}

impl Limits {
    fn from_env() -> Result<Self> {
        Ok(Self {
            stall: Duration::from_secs(env_number(TIMEOUT_ENV, DEFAULT_STALL_SECS)?),
            memory_mb: env_number(MEMORY_ENV, default_memory_mb())?,
        })
    }
}

/// Half the machine's memory (8 GiB where that cannot be told): above what
/// real PDFs take (with the vision models, the PDFs in `test_assets` peak at
/// 1.7 GB in the median and 6.4 GB at most), while a bomb is stopped with half
/// the memory still free.
fn default_memory_mb() -> u64 {
    physical_memory().map_or(8192, |bytes| (bytes >> 20) / 2)
}

fn env_number(name: &str, default: u64) -> Result<u64> {
    parse_number(name, std::env::var(name).ok().as_deref(), default)
}

/// A positive whole number from the environment; anything else set there is
/// an error rather than a silently ignored setting.
fn parse_number(name: &str, value: Option<&str>, default: u64) -> Result<u64> {
    let Some(value) = value else {
        return Ok(default);
    };
    value
        .trim()
        .parse::<u64>()
        .ok()
        .filter(|n| *n > 0)
        .with_context(|| format!("{name} must be a whole number above 0, not {value:?}"))
}

static POOL: OnceLock<Pool> = OnceLock::new();

/// From now on, convert PDFs in worker processes (this binary, re-run with
/// [`WORKER_FLAG`]). The CLI calls this at startup. Does nothing when
/// `FILEGOBLIN_PDF_WORKER` is `0` or `off`; fails on a malformed limit.
pub fn enable() -> Result<()> {
    if std::env::var("FILEGOBLIN_PDF_WORKER")
        .is_ok_and(|v| v == "0" || v.eq_ignore_ascii_case("off"))
    {
        return Ok(());
    }
    let limits = Limits::from_env()?;
    let max = match std::env::var(WORKERS_ENV) {
        Ok(v) => parse_number(WORKERS_ENV, Some(&v), 1)? as usize,
        Err(_) => default_workers(),
    };
    // Without a path to this binary there is nothing to start: an error at
    // the first PDF (which then converts in-process) says so.
    let exe = std::env::current_exe().unwrap_or_default();
    let _ = POOL.set(Pool::new(exe, limits, max));
    Ok(())
}

/// The workers, when [`enable`] has been called in this process.
pub(crate) fn pool() -> Option<&'static Pool> {
    POOL.get()
}

/// Two, or one on a single core. Each worker holds its own copy of the vision
/// models: on 45 one-page PDFs with `--vision smart`, one worker took 140 s,
/// two 110 s and four 102 s, peaking at 5.4, 8.5 and 9.6 GB in all.
fn default_workers() -> usize {
    std::thread::available_parallelism()
        .map_or(1, |n| n.get())
        .min(2)
}

/// Pages converted in this process. A worker reports each change to its
/// parent: proof that it is not stuck.
static PAGES_DONE: AtomicU64 = AtomicU64::new(0);

/// Records a finished page (stage 1 and stage 3 of the PDF pipeline both
/// count).
pub(crate) fn page_done() {
    PAGES_DONE.fetch_add(1, Ordering::Relaxed);
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

// --- The parent's side ------------------------------------------------------

/// Workers, started on demand up to `max` at once.
pub(crate) struct Pool {
    exe: PathBuf,
    limits: Limits,
    max: usize,
    state: Mutex<PoolState>,
    freed: Condvar,
    /// Lines printed once per run however many workers print them.
    printed: Mutex<std::collections::HashSet<String>>,
}

struct PoolState {
    idle: Vec<Worker>,
    /// Workers in existence, idle or busy.
    alive: usize,
}

impl Pool {
    fn new(exe: PathBuf, limits: Limits, max: usize) -> Self {
        Pool {
            exe,
            limits,
            max: max.max(1),
            state: Mutex::new(PoolState {
                idle: Vec::new(),
                alive: 0,
            }),
            freed: Condvar::new(),
            printed: Mutex::default(),
        }
    }

    /// Converts one PDF in a worker. A worker that cannot be started leaves
    /// the file to this process, with a warning.
    pub(crate) fn convert(&'static self, path: &Path, opts: &ParseOptions) -> Result<String> {
        let mut worker = match self.take() {
            Ok(worker) => worker,
            Err(e) => {
                static WARNED: std::sync::Once = std::sync::Once::new();
                if !opts.quiet {
                    WARNED.call_once(|| {
                        eprintln!(
                            "filegoblin: could not start a PDF worker ({e:#}); converting PDFs \
                             in this process instead"
                        )
                    });
                }
                return super::pdf::gobble_in_process(path, opts);
            }
        };
        match worker.convert(path, opts, self.limits) {
            Outcome::Answered(result) => {
                self.give_back(worker);
                result
            }
            Outcome::Lost(error) => {
                drop(worker);
                lock(&self.state).alive -= 1;
                self.freed.notify_one();
                Err(error)
            }
        }
    }

    /// An idle worker, a new one, or (with `max` busy) the next one freed.
    fn take(&'static self) -> Result<Worker> {
        let mut state = lock(&self.state);
        loop {
            if let Some(mut worker) = state.idle.pop() {
                // Gone while idle: nothing of this file's doing.
                if matches!(worker.child.try_wait(), Ok(None)) {
                    return Ok(worker);
                }
                state.alive -= 1;
                continue;
            }
            if state.alive < self.max {
                state.alive += 1;
                drop(state);
                return Worker::spawn(self).inspect_err(|_| {
                    lock(&self.state).alive -= 1;
                    self.freed.notify_one();
                });
            }
            state = self.freed.wait(state).unwrap_or_else(|e| e.into_inner());
        }
    }

    fn give_back(&self, worker: Worker) {
        lock(&self.state).idle.push(worker);
        self.freed.notify_one();
    }

    /// Prints a worker's stderr line, once per run when it is a once-per-run
    /// hint (which each worker would otherwise print).
    fn print(&self, line: &[u8]) {
        let text = String::from_utf8_lossy(line);
        if super::pdf::ONCE_PER_RUN.iter().any(|p| text.starts_with(p))
            && !lock(&self.printed).insert(text.into_owned())
        {
            return;
        }
        let mut err = std::io::stderr().lock();
        let _ = err.write_all(line);
        let _ = err.write_all(b"\n");
    }
}

/// How a request ended.
enum Outcome {
    /// The worker replied (the PDF converted, or failed the ordinary way) and
    /// can take another.
    Answered(Result<String>),
    /// The worker died or was stopped; the error is the file's.
    Lost(anyhow::Error),
}

struct Worker {
    child: Child,
    requests: ChildStdin,
    replies: Receiver<(u8, Vec<u8>)>,
    stderr: Arc<StderrState>,
    /// Disconnects when the worker's stderr closes.
    stderr_closed: Receiver<()>,
}

/// A worker's stderr as the parent passes it on.
#[derive(Default)]
struct StderrState {
    /// The current request is `--quiet`.
    quiet: AtomicBool,
    /// Lines of the runtime's crash report, held back: the parent turns them
    /// into the file's error.
    crash: Mutex<Vec<String>>,
}

impl Worker {
    fn spawn(pool: &'static Pool) -> Result<Worker> {
        let mut child = Command::new(&pool.exe)
            .arg(WORKER_FLAG)
            .env(MEMORY_ENV, pool.limits.memory_mb.to_string())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .with_context(|| format!("running {}", pool.exe.display()))?;
        let requests = child.stdin.take().context("no stdin pipe")?;
        let stdout = child.stdout.take().context("no stdout pipe")?;
        let stderr = child.stderr.take().context("no stderr pipe")?;

        let (replies_tx, replies) = std::sync::mpsc::channel();
        let max_reply = pool.limits.memory_mb << 20;
        std::thread::Builder::new()
            .name("filegoblin-pdf-replies".into())
            .spawn(move || {
                let mut stdout = BufReader::new(stdout);
                while let Ok(Some(frame)) = read_frame(&mut stdout, max_reply) {
                    if replies_tx.send(frame).is_err() {
                        break;
                    }
                }
            })?;

        let state = Arc::new(StderrState::default());
        let (closed_tx, stderr_closed) = std::sync::mpsc::channel();
        let shared = Arc::clone(&state);
        std::thread::Builder::new()
            .name("filegoblin-pdf-stderr".into())
            .spawn(move || pass_on_stderr(stderr, &shared, pool, closed_tx))?;

        Ok(Worker {
            child,
            requests,
            replies,
            stderr: state,
            stderr_closed,
        })
    }

    fn convert(&mut self, path: &Path, opts: &ParseOptions, limits: Limits) -> Outcome {
        self.stderr.quiet.store(opts.quiet, Ordering::Relaxed);
        let request = Request {
            path: to_wire(path),
            opts: opts.clone(),
            cache: crate::cache::enabled(),
        };
        let sent = serde_json::to_vec(&request)
            .map_err(std::io::Error::other)
            .and_then(|body| write_frame(&mut self.requests, REQUEST, &body));
        if sent.is_err() {
            return Outcome::Lost(self.why_lost(limits));
        }
        let mut last_progress = Instant::now();
        loop {
            let wait = limits.stall.saturating_sub(last_progress.elapsed());
            match self.replies.recv_timeout(wait) {
                Ok((PROGRESS, _)) => last_progress = Instant::now(),
                Ok((CONVERTED, body)) => {
                    return Outcome::Answered(
                        String::from_utf8(body).context("a PDF worker sent invalid UTF-8"),
                    );
                }
                Ok((FAILED, body)) => return Outcome::Answered(Err(decode_error(&body))),
                Ok((tag, _)) => {
                    return Outcome::Lost(anyhow::anyhow!(
                        "Mischievous PDF Error: the PDF worker sent a garbled reply (tag {tag})"
                    ));
                }
                Err(RecvTimeoutError::Timeout) => {
                    let _ = self.child.kill();
                    return Outcome::Lost(anyhow::anyhow!(
                        "Mischievous PDF Error: no progress on this PDF for {} s, so it was \
                         stopped (a stuck or hostile file); {TIMEOUT_ENV} sets a longer wait",
                        limits.stall.as_secs()
                    ));
                }
                Err(RecvTimeoutError::Disconnected) => {
                    return Outcome::Lost(self.why_lost(limits));
                }
            }
        }
    }

    /// The file's error once the worker has gone.
    fn why_lost(&mut self, limits: Limits) -> anyhow::Error {
        // A worker whose pipe closed has already ended, and a kill does not
        // change how; one still running (it sent something unreadable) must
        // not be waited on forever.
        let _ = self.child.kill();
        let status = match self.child.wait() {
            Ok(status) => status,
            Err(e) => {
                return anyhow::anyhow!("Mischievous PDF Error: the PDF worker vanished: {e}");
            }
        };
        // The crash report arrives on stderr; give it a moment to.
        let _ = self.stderr_closed.recv_timeout(Duration::from_secs(1));
        let crash = lock(&self.stderr.crash).join("\n");
        why_lost(status, &crash, limits)
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        if matches!(self.child.try_wait(), Ok(None)) {
            let _ = self.child.kill();
        }
        let _ = self.child.wait();
    }
}

/// The error for a worker that ended with `status`, having printed `crash`
/// (the runtime's report, if any).
fn why_lost(status: ExitStatus, crash: &str, limits: Limits) -> anyhow::Error {
    if status.code() == Some(OUT_OF_MEMORY) || crash.contains("memory allocation of") {
        anyhow::anyhow!(
            "Mischievous PDF Error: converting this PDF took more than {} MB of memory, so it \
             was stopped (a decompression bomb or other hostile file?); {MEMORY_ENV} sets the \
             limit",
            limits.memory_mb
        )
    } else if crash.contains("has overflowed its stack")
        || status.code() == Some(STATUS_STACK_OVERFLOW)
    {
        anyhow::anyhow!(
            "Mischievous PDF Error: the PDF parser overflowed its stack on this file (objects \
             nested too deep: a malformed or hostile PDF)"
        )
    } else {
        anyhow::anyhow!("Mischievous PDF Error: the PDF parser crashed on this file ({status})")
    }
}

/// Lines Rust's runtime prints as a process dies.
fn is_crash_report(line: &str) -> bool {
    line.contains("has overflowed its stack")
        || line.starts_with("fatal runtime error")
        || (line.starts_with("memory allocation of") && line.ends_with("failed"))
}

/// Passes a worker's stderr on to ours, line by line, until it closes.
fn pass_on_stderr(stderr: impl Read, state: &StderrState, pool: &Pool, _closed: Sender<()>) {
    for line in BufReader::new(stderr).split(b'\n') {
        let Ok(line) = line else { break };
        let text = String::from_utf8_lossy(&line);
        if is_crash_report(&text) {
            lock(&state.crash).push(text.into_owned());
        } else if !text.trim().is_empty() && !state.quiet.load(Ordering::Relaxed) {
            pool.print(&line);
        }
    }
}

// --- The worker's side ------------------------------------------------------

/// Runs this process as a worker: converts the PDFs its parent sends until the
/// parent closes stdin — and at once, even mid-conversion, if it goes away.
pub fn serve() -> Result<()> {
    let memory = env_number(MEMORY_ENV, default_memory_mb())?;
    let (tx, requests) = std::sync::mpsc::channel();
    std::thread::Builder::new()
        .name("filegoblin-pdf-requests".into())
        .spawn(move || {
            let mut stdin = std::io::stdin().lock();
            while let Ok(Some(frame)) = read_frame(&mut stdin, 64 << 20) {
                if tx.send(frame).is_err() {
                    break;
                }
            }
            std::process::exit(0);
        })?;
    std::thread::Builder::new()
        .name("filegoblin-pdf-watch".into())
        .spawn(move || watch(memory << 20))?;

    for (tag, body) in requests {
        anyhow::ensure!(
            tag == REQUEST,
            "a PDF worker got a frame it does not know ({tag})"
        );
        let request: Request = serde_json::from_slice(&body).context("a garbled PDF request")?;
        crate::cache::set_enabled(request.cache);
        let (tag, body) =
            match super::pdf::gobble_in_process(&from_wire(request.path), &request.opts) {
                Ok(markdown) => (CONVERTED, markdown.into_bytes()),
                Err(e) => (FAILED, encode_error(&e)),
            };
        write_frame(&mut std::io::stdout().lock(), tag, &body)?;
    }
    Ok(())
}

/// Every 25 ms: exit once over the memory limit, and tell the parent about
/// pages finished since last time.
fn watch(limit: u64) {
    let mut reported = 0;
    loop {
        std::thread::sleep(Duration::from_millis(25));
        if footprint().is_some_and(|used| used > limit) {
            std::process::exit(OUT_OF_MEMORY);
        }
        let done = PAGES_DONE.load(Ordering::Relaxed);
        if done != reported {
            reported = done;
            if write_frame(&mut std::io::stdout().lock(), PROGRESS, &[]).is_err() {
                std::process::exit(0);
            }
        }
    }
}

/// The memory this process holds: private pages, whether resident, compressed
/// or swapped where the OS says. `None` where it cannot be told, which leaves
/// the worker without a memory limit.
#[cfg(target_os = "macos")]
fn footprint() -> Option<u64> {
    // SAFETY: a zeroed rusage_info_v2 is a valid value, and proc_pid_rusage
    // writes exactly one of them for RUSAGE_INFO_V2.
    unsafe {
        let mut info: libc::rusage_info_v2 = std::mem::zeroed();
        let buffer = (&raw mut info).cast::<libc::rusage_info_t>();
        (libc::proc_pid_rusage(libc::getpid(), libc::RUSAGE_INFO_V2, buffer) == 0)
            .then_some(info.ri_phys_footprint)
    }
}

#[cfg(target_os = "linux")]
fn footprint() -> Option<u64> {
    // Resident pages that are not file-backed: statm's resident minus shared.
    let statm = std::fs::read_to_string("/proc/self/statm").ok()?;
    let mut fields = statm.split_whitespace().skip(1);
    let resident: u64 = fields.next()?.parse().ok()?;
    let shared: u64 = fields.next()?.parse().ok()?;
    // SAFETY: sysconf has no preconditions.
    let page = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
    Some(resident.saturating_sub(shared) * u64::try_from(page).ok()?)
}

#[cfg(windows)]
fn footprint() -> Option<u64> {
    use windows::Win32::System::ProcessStatus::{GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS};
    use windows::Win32::System::Threading::GetCurrentProcess;
    let mut counters = PROCESS_MEMORY_COUNTERS::default();
    let size = std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32;
    // SAFETY: GetCurrentProcess returns a pseudo-handle that needs no closing,
    // and `size` is the size of the struct the counters are written into.
    unsafe { GetProcessMemoryInfo(GetCurrentProcess(), &mut counters, size) }.ok()?;
    // Private committed memory.
    Some(counters.PagefileUsage as u64)
}

#[cfg(not(any(target_os = "macos", target_os = "linux", windows)))]
fn footprint() -> Option<u64> {
    None
}

/// The machine's installed memory.
#[cfg(unix)]
fn physical_memory() -> Option<u64> {
    // SAFETY: sysconf has no preconditions.
    let (pages, page) = unsafe {
        (
            libc::sysconf(libc::_SC_PHYS_PAGES),
            libc::sysconf(libc::_SC_PAGESIZE),
        )
    };
    Some(u64::try_from(pages).ok()? * u64::try_from(page).ok()?)
}

#[cfg(windows)]
fn physical_memory() -> Option<u64> {
    use windows::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
    let mut status = MEMORYSTATUSEX {
        dwLength: std::mem::size_of::<MEMORYSTATUSEX>() as u32,
        ..Default::default()
    };
    // SAFETY: `dwLength` is the size of the struct the status is written into.
    unsafe { GlobalMemoryStatusEx(&mut status) }.ok()?;
    Some(status.ullTotalPhys)
}

#[cfg(not(any(unix, windows)))]
fn physical_memory() -> Option<u64> {
    None
}

// --- The wire ---------------------------------------------------------------

#[derive(Serialize, Deserialize)]
struct Request {
    path: WirePath,
    opts: ParseOptions,
    /// Whether this run uses the cache (no `--no-cache`).
    cache: bool,
}

/// A path in its raw OS encoding, so no file name is beyond a worker.
#[cfg(unix)]
type WirePath = Vec<u8>;
#[cfg(windows)]
type WirePath = Vec<u16>;
#[cfg(not(any(unix, windows)))]
type WirePath = String;

#[cfg(unix)]
fn to_wire(path: &Path) -> WirePath {
    use std::os::unix::ffi::OsStrExt;
    path.as_os_str().as_bytes().to_vec()
}

#[cfg(unix)]
fn from_wire(path: WirePath) -> PathBuf {
    use std::os::unix::ffi::OsStringExt;
    std::ffi::OsString::from_vec(path).into()
}

#[cfg(windows)]
fn to_wire(path: &Path) -> WirePath {
    use std::os::windows::ffi::OsStrExt;
    path.as_os_str().encode_wide().collect()
}

#[cfg(windows)]
fn from_wire(path: WirePath) -> PathBuf {
    use std::os::windows::ffi::OsStringExt;
    std::ffi::OsString::from_wide(&path).into()
}

#[cfg(not(any(unix, windows)))]
fn to_wire(path: &Path) -> WirePath {
    path.to_string_lossy().into_owned()
}

#[cfg(not(any(unix, windows)))]
fn from_wire(path: WirePath) -> PathBuf {
    path.into()
}

/// An error's chain of messages, so the parent can rebuild it exactly.
fn encode_error(e: &anyhow::Error) -> Vec<u8> {
    let chain: Vec<String> = e.chain().map(|cause| cause.to_string()).collect();
    serde_json::to_vec(&chain).unwrap_or_default()
}

fn decode_error(body: &[u8]) -> anyhow::Error {
    let chain: Vec<String> = serde_json::from_slice(body).unwrap_or_default();
    let mut messages = chain.into_iter().rev();
    let root = anyhow::anyhow!(
        messages
            .next()
            .unwrap_or_else(|| "the PDF worker failed without saying why".into())
    );
    messages.fold(root, |error, context| error.context(context))
}

fn write_frame(w: &mut impl Write, tag: u8, body: &[u8]) -> std::io::Result<()> {
    w.write_all(&[tag])?;
    w.write_all(&(body.len() as u64).to_le_bytes())?;
    w.write_all(body)?;
    w.flush()
}

/// The next frame, or `None` where the stream ends between frames. A frame
/// longer than `max` is an error, not an allocation.
fn read_frame(r: &mut impl Read, max: u64) -> std::io::Result<Option<(u8, Vec<u8>)>> {
    let mut tag = [0u8; 1];
    match r.read_exact(&mut tag) {
        Err(e) if e.kind() == ErrorKind::UnexpectedEof => return Ok(None),
        other => other?,
    }
    let mut len = [0u8; 8];
    r.read_exact(&mut len)?;
    let len = u64::from_le_bytes(len);
    if len > max {
        return Err(std::io::Error::new(
            ErrorKind::InvalidData,
            format!("a {len}-byte frame"),
        ));
    }
    let mut body = vec![0u8; len as usize];
    r.read_exact(&mut body)?;
    Ok(Some((tag[0], body)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_round_trip_and_end_cleanly() {
        let mut wire = Vec::new();
        write_frame(&mut wire, CONVERTED, b"# Hoard\n").unwrap();
        write_frame(&mut wire, PROGRESS, &[]).unwrap();
        let mut r = wire.as_slice();
        assert_eq!(
            read_frame(&mut r, 1 << 20).unwrap(),
            Some((CONVERTED, b"# Hoard\n".to_vec()))
        );
        assert_eq!(
            read_frame(&mut r, 1 << 20).unwrap(),
            Some((PROGRESS, vec![]))
        );
        assert_eq!(read_frame(&mut r, 1 << 20).unwrap(), None);
        // Cut short inside a frame: an error, not a clean end.
        assert!(read_frame(&mut &wire[..5], 1 << 20).is_err());
    }

    #[test]
    fn an_oversized_frame_is_refused_before_allocating() {
        let mut wire = vec![CONVERTED];
        wire.extend(u64::MAX.to_le_bytes());
        assert!(read_frame(&mut wire.as_slice(), 1 << 20).is_err());
    }

    #[test]
    fn errors_cross_with_their_context_chain() {
        let original = anyhow::anyhow!("pdf_oxide: bad xref").context("Mischievous PDF Error: x");
        let rebuilt = decode_error(&encode_error(&original));
        assert_eq!(rebuilt.to_string(), original.to_string());
        assert_eq!(format!("{rebuilt:#}"), format!("{original:#}"));
    }

    #[cfg(unix)]
    #[test]
    fn a_path_that_is_not_utf8_crosses_intact() {
        use std::os::unix::ffi::OsStrExt;
        let path = Path::new(std::ffi::OsStr::from_bytes(b"/tmp/caf\xe9.pdf"));
        let request = Request {
            path: to_wire(path),
            opts: ParseOptions::default(),
            cache: false,
        };
        let back: Request = serde_json::from_slice(&serde_json::to_vec(&request).unwrap()).unwrap();
        assert_eq!(from_wire(back.path), path);
    }

    #[test]
    fn limits_from_the_environment_must_be_positive_numbers() {
        assert_eq!(parse_number("X", None, 7).unwrap(), 7);
        assert_eq!(parse_number("X", Some(" 30 "), 7).unwrap(), 30);
        for bad in ["", "0", "-1", "1.5", "lots"] {
            let e = parse_number("X", Some(bad), 7).unwrap_err().to_string();
            assert!(e.contains("X must be a whole number"), "{e}");
        }
    }

    #[cfg(unix)]
    #[test]
    fn a_lost_worker_is_explained() {
        use std::os::unix::process::ExitStatusExt;
        let limits = Limits {
            stall: Duration::from_secs(1),
            memory_mb: 512,
        };
        let abort = ExitStatus::from_raw(6); // killed by SIGABRT
        let overflow = "thread 'filegoblin-pdf-3' has overflowed its stack";
        let msg = why_lost(abort, overflow, limits).to_string();
        assert!(msg.contains("overflowed its stack"), "{msg}");

        let msg = why_lost(ExitStatus::from_raw(OUT_OF_MEMORY << 8), "", limits).to_string();
        assert!(msg.contains("more than 512 MB"), "{msg}");

        let msg = why_lost(ExitStatus::from_raw(11), "", limits).to_string();
        assert!(msg.contains("crashed") && msg.contains("11"), "{msg}");
    }

    #[test]
    fn the_runtime_crash_report_is_recognized() {
        assert!(is_crash_report(
            "thread 'filegoblin-pdf-0' has overflowed its stack"
        ));
        assert!(is_crash_report(
            "fatal runtime error: stack overflow, aborting"
        ));
        assert!(is_crash_report(
            "memory allocation of 4294967296 bytes failed"
        ));
        assert!(!is_crash_report(
            "filegoblin: 3 of 9 pages read with the vision models"
        ));
    }

    #[test]
    fn this_process_can_measure_its_memory_and_the_machines() {
        if cfg!(any(target_os = "macos", target_os = "linux", windows)) {
            let used = footprint().expect("a footprint on this platform");
            let installed = physical_memory().expect("the installed memory");
            assert!(0 < used && used < installed, "{used} of {installed}");
            assert_eq!(default_memory_mb(), (installed >> 20) / 2);
        }
    }
}
