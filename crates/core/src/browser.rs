//! `--browser`: render JS-heavy pages with the user's ALREADY-INSTALLED
//! Chromium-family browser via `--headless=new --dump-dom`. No bundled browser,
//! no CDP crate — Chrome's built-in headless command handler already implements
//! the load-wait + virtual-time settle loop, and since Chrome 136 attaching to
//! a user's running (logged-in) browser is impossible anyway.
//!
//! Known limitations (documented, accepted): fresh temp profile = no cookies /
//! logged-in sessions; shadow-DOM content is not serialized; per-URL launch
//! cost ~1s.

use anyhow::{Context, Result};
use std::path::PathBuf;
use std::process::{Command, Stdio};

/// Reaching `load` is bounded by Chrome's own --timeout; the settle phase by
/// --virtual-time-budget; this is the outer wall-clock kill for the known
/// new-headless hang bugs.
const LOAD_TIMEOUT_MS: u32 = 15_000;
const SETTLE_BUDGET_MS: u32 = 5_000;
const WALL_CLOCK_KILL_MS: u64 = (LOAD_TIMEOUT_MS + SETTLE_BUDGET_MS + 10_000) as u64;
/// Ceiling on a rendered DOM. The static fetch path enforces
/// `net::MAX_RESPONSE_BYTES`; the render path buffered without any bound, and a
/// page that builds a huge DOM in JS could accumulate until the wall clock.
const MAX_DOM_BYTES: usize = 64 * 1024 * 1024;

/// Finds an installed Chromium-family browser. Env overrides first, then the
/// per-OS locations used by chrome-launcher/Lighthouse.
pub fn find_browser() -> Option<PathBuf> {
    for var in ["FILEGOBLIN_BROWSER", "CHROME_PATH", "CHROME"] {
        if let Ok(p) = std::env::var(var) {
            let p = PathBuf::from(p);
            if p.exists() {
                return Some(p);
            }
            // Falling through silently made a typo in the override invisible:
            // the user set it, filegoblin ignored it, and any later failure
            // pointed at the page instead.
            eprintln!(
                "⚠️  ${} points at a path that does not exist: {} — falling back to auto-discovery",
                var,
                p.display()
            );
        }
    }

    #[cfg(target_os = "macos")]
    {
        let apps = [
            "Google Chrome.app/Contents/MacOS/Google Chrome",
            "Chromium.app/Contents/MacOS/Chromium",
            "Microsoft Edge.app/Contents/MacOS/Microsoft Edge",
            "Brave Browser.app/Contents/MacOS/Brave Browser",
            "Vivaldi.app/Contents/MacOS/Vivaldi",
            "Google Chrome Canary.app/Contents/MacOS/Google Chrome Canary",
        ];
        let mut roots = vec![PathBuf::from("/Applications")];
        if let Some(home) = home::home_dir() {
            roots.push(home.join("Applications"));
        }
        for root in roots {
            for app in apps {
                let p = root.join(app);
                if p.exists() {
                    return Some(p);
                }
            }
        }
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let names = [
            "chrome-headless-shell",
            "google-chrome-stable",
            "google-chrome",
            "chromium",
            "chromium-browser",
            "brave-browser",
            "microsoft-edge-stable",
            "microsoft-edge",
        ];
        let path = std::env::var_os("PATH").unwrap_or_default();
        for dir in std::env::split_paths(&path) {
            for name in names {
                let p = dir.join(name);
                if p.exists() {
                    return Some(p);
                }
            }
        }
        let snap = PathBuf::from("/snap/bin/chromium");
        if snap.exists() {
            return Some(snap);
        }
    }

    #[cfg(target_os = "windows")]
    {
        let candidates = [
            ("LOCALAPPDATA", r"Google\Chrome\Application\chrome.exe"),
            ("ProgramFiles", r"Google\Chrome\Application\chrome.exe"),
            ("ProgramFiles(x86)", r"Google\Chrome\Application\chrome.exe"),
            (
                "ProgramFiles(x86)",
                r"Microsoft\Edge\Application\msedge.exe",
            ),
            ("ProgramFiles", r"Microsoft\Edge\Application\msedge.exe"),
            (
                "LOCALAPPDATA",
                r"BraveSoftware\Brave-Browser\Application\brave.exe",
            ),
        ];
        for (var, rel) in candidates {
            if let Ok(base) = std::env::var(var) {
                let p = PathBuf::from(base).join(rel);
                if p.exists() {
                    return Some(p);
                }
            }
        }
    }

    None
}

/// Renders `url` in the discovered browser and returns the post-JavaScript
/// serialized DOM. The caller is responsible for SSRF pre-validation — the
/// browser does NOT honour our redirect guards (which is one reason this is
/// strictly opt-in via `--browser`).
pub fn render_url(url: &url::Url, quiet: bool) -> Result<String> {
    let browser = find_browser().ok_or_else(|| {
        anyhow::anyhow!(
            "No Chromium-family browser found. Install Chrome/Chromium/Edge/Brave, \
             or point $FILEGOBLIN_BROWSER at one."
        )
    })?;
    if !quiet {
        eprintln!(
            "🕸️ Rendering with {} (headless)...",
            browser.file_name().unwrap_or_default().to_string_lossy()
        );
    }

    // Chrome refuses to share a live profile; a fresh temp dir also means no
    // cookies leak into the dump. RAII-cleaned.
    let profile = tempfile::tempdir().context("Failed to create temp browser profile")?;

    let mut cmd = Command::new(&browser);
    cmd.args([
        "--headless=new",
        "--disable-gpu",
        "--no-first-run",
        "--no-default-browser-check",
        "--disable-extensions",
        "--disable-background-networking",
        "--disable-component-update",
        "--disable-sync",
        "--disable-default-apps",
        "--disable-hang-monitor",
        "--disable-dev-shm-usage",
        "--mute-audio",
        "--hide-scrollbars",
        "--window-size=1280,2000",
        "--disable-breakpad",
        "--disable-logging",
        "--log-level=3",
        "--run-all-compositor-stages-before-draw",
    ])
    .arg(format!("--user-data-dir={}", profile.path().display()))
    .arg(format!("--user-agent={}", crate::net::user_agent()))
    .arg(format!("--timeout={}", LOAD_TIMEOUT_MS))
    .arg(format!("--virtual-time-budget={}", SETTLE_BUDGET_MS))
    .arg("--dump-dom")
    .arg(url.as_str())
    .stdin(Stdio::null())
    .stdout(Stdio::piped())
    // Captured, not discarded: with stderr thrown away, a missing shared
    // library, a refusal to start as root without --no-sandbox (the normal
    // Docker/CI case), and a wrong binary all collapsed into the same
    // "page may block headless rendering", which blames the page.
    .stderr(Stdio::piped());

    // NOTE: do NOT setpgid/process_group Chrome on macOS — detaching it from
    // the session breaks its helper-process handshake and it never exits.
    // kill() on the browser process suffices; helpers exit with it.

    let mut child = cmd.spawn().context("Failed to launch the browser")?;

    // Drain stdout on a thread into a shared buffer: a multi-MB DOM into a
    // 64KB pipe deadlocks a naive wait(), AND new headless Chrome sometimes
    // writes the full dump then hangs at shutdown (crbug 327458826) — so we
    // must be able to take the DOM without ever seeing the process exit.
    let collected = std::sync::Arc::new(std::sync::Mutex::new(Vec::<u8>::new()));
    let sink = collected.clone();
    let mut stdout_pipe = child.stdout.take().expect("stdout piped");
    let overflowed = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let overflow_flag = overflowed.clone();
    let reader = std::thread::spawn(move || {
        use std::io::Read;
        let mut chunk = [0u8; 65536];
        loop {
            // `while let Ok(n)` ended the loop on a transient EINTR and left the
            // partial buffer to pass validation as a successful render.
            let n = match stdout_pipe.read(&mut chunk) {
                Ok(0) => break,
                Ok(n) => n,
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(_) => break,
            };
            let mut buf = sink.lock().unwrap();
            // The static fetch path caps bodies at MAX_RESPONSE_BYTES; the
            // rendered DOM had no ceiling at all.
            if buf.len() + n > MAX_DOM_BYTES {
                overflow_flag.store(true, std::sync::atomic::Ordering::Relaxed);
                break;
            }
            buf.extend_from_slice(&chunk[..n]);
        }
    });

    // Chrome's stderr is drained on its own thread too: a full pipe would
    // otherwise block the browser before it finishes writing the DOM.
    let mut stderr_pipe = child.stderr.take().expect("stderr piped");
    let diagnostics = std::sync::Arc::new(std::sync::Mutex::new(Vec::<u8>::new()));
    let diag_sink = diagnostics.clone();
    let err_reader = std::thread::spawn(move || {
        use std::io::Read;
        let mut chunk = [0u8; 8192];
        loop {
            match stderr_pipe.read(&mut chunk) {
                Ok(0) => break,
                Ok(n) => {
                    let mut buf = diag_sink.lock().unwrap();
                    if buf.len() < 16 * 1024 {
                        buf.extend_from_slice(&chunk[..n]);
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(_) => break,
            }
        }
    });

    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(WALL_CLOCK_KILL_MS);
    let mut stable_since: Option<(usize, std::time::Instant)> = None;
    let mut timed_out = false;
    loop {
        if overflowed.load(std::sync::atomic::Ordering::Relaxed) {
            let _ = child.kill();
            let _ = child.wait();
            break;
        }
        // A `?` here used to leave Chrome running, the reader detached, and the
        // profile TempDir deleted out from under a live browser.
        match child.try_wait() {
            Ok(Some(_)) => break, // clean exit
            Ok(None) => {}
            Err(e) => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = reader.join();
                let _ = err_reader.join();
                return Err(anyhow::anyhow!(
                    "Failed while waiting on the browser: {}",
                    e
                ));
            }
        }
        // Dump-then-hang detection: output ends like a document and has been
        // quiet for a second → take it and put Chrome down ourselves.
        let (len, ends) = {
            let buf = collected.lock().unwrap();
            let tail: String = String::from_utf8_lossy(&buf[buf.len().saturating_sub(32)..])
                .trim_end()
                .to_lowercase();
            (buf.len(), tail.ends_with("</html>"))
        };
        if ends {
            match stable_since {
                Some((prev, at)) if prev == len => {
                    if at.elapsed() > std::time::Duration::from_millis(1000) {
                        let _ = child.kill();
                        let _ = child.wait();
                        break;
                    }
                }
                _ => stable_since = Some((len, std::time::Instant::now())),
            }
        }
        if std::time::Instant::now() > deadline {
            timed_out = true;
            let _ = child.kill();
            let _ = child.wait();
            break; // whatever was collected gets validated below
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }

    // Bounded, not `join()`. Chrome's helper processes inherit the stdout
    // pipe, so if one wedges after the browser is SIGKILLed the write end
    // never closes, `read()` never returns 0, and an unconditional join hangs
    // forever — *after* the wall-clock timer already fired. Detaching a stuck
    // reader leaks one thread; blocking here leaks the whole run.
    join_bounded(reader, std::time::Duration::from_secs(5));
    join_bounded(err_reader, std::time::Duration::from_secs(2));

    let bytes = std::mem::take(&mut *collected.lock().unwrap());
    let dom = String::from_utf8_lossy(&bytes).into_owned();
    let trimmed = dom.trim_start().to_lowercase();

    if overflowed.load(std::sync::atomic::Ordering::Relaxed) {
        anyhow::bail!(
            "Rendered DOM for {} exceeds the {} MiB limit",
            url,
            MAX_DOM_BYTES / (1024 * 1024)
        );
    }

    // Chrome's exit code is unreliable; validate the output instead.
    let looks_like_a_document = trimmed.starts_with("<!doctype")
        || trimmed.starts_with("<html")
        || trimmed.starts_with("<?xml");
    if dom.trim().len() < 40 || !looks_like_a_document {
        let stderr_tail = browser_diagnostics(&diagnostics);
        anyhow::bail!(
            "Browser produced no usable DOM for {}{}",
            url,
            if stderr_tail.is_empty() {
                " (page may block headless rendering)".to_string()
            } else {
                format!(" — Chrome said: {}", stderr_tail)
            }
        );
    }
    // A wall-clock kill mid-write leaves a document that still starts with
    // `<html`, so the shape check alone reported a truncated page as success.
    if timed_out && !trimmed.trim_end().ends_with("</html>") {
        anyhow::bail!(
            "Browser render of {} timed out after {}s with an incomplete DOM ({} bytes)",
            url,
            WALL_CLOCK_KILL_MS / 1000,
            dom.len()
        );
    }
    Ok(dom)
}

/// Waits up to `timeout` for a pipe-reader thread, then gives up and lets it
/// run detached rather than blocking the process forever.
fn join_bounded(handle: std::thread::JoinHandle<()>, timeout: std::time::Duration) {
    let deadline = std::time::Instant::now() + timeout;
    while !handle.is_finished() {
        if std::time::Instant::now() > deadline {
            return; // detached; the thread ends when its pipe finally closes
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    let _ = handle.join();
}

/// The last meaningful line or two of Chrome's stderr, for error messages.
fn browser_diagnostics(diagnostics: &std::sync::Mutex<Vec<u8>>) -> String {
    let raw = diagnostics.lock().unwrap();
    let text = String::from_utf8_lossy(&raw);
    let interesting: Vec<&str> = text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        // Chrome is chatty about DBus/GPU/Vulkan on every headless start;
        // those lines are noise, not the reason a render failed.
        .filter(|l| !l.contains("dbus") && !l.contains("GPU") && !l.contains("Vulkan"))
        .collect();
    interesting
        .iter()
        .rev()
        .take(2)
        .rev()
        .cloned()
        .collect::<Vec<_>>()
        .join("; ")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One sequential test: env-override discovery, then a real render of a
    /// JS-only page served locally (skipped when no browser is installed).
    /// Combined because parallel tests mutating FILEGOBLIN_BROWSER race.
    #[test]
    fn discovery_and_js_render() {
        // Env override wins (any path that certainly exists).
        unsafe { std::env::set_var("FILEGOBLIN_BROWSER", "/bin/ls") };
        assert_eq!(find_browser(), Some(PathBuf::from("/bin/ls")));
        unsafe { std::env::remove_var("FILEGOBLIN_BROWSER") };

        if find_browser().is_none() {
            // A silent `return` still reports "ok", so this looked like browser
            // coverage on machines that have none. CI sets
            // FILEGOBLIN_REQUIRE_BROWSER=1 to turn the skip into a failure.
            if std::env::var_os("FILEGOBLIN_REQUIRE_BROWSER").is_some() {
                panic!("FILEGOBLIN_REQUIRE_BROWSER is set but no browser was found");
            }
            eprintln!(
                "SKIPPING the --browser render assertions: no Chromium-family browser installed"
            );
            return;
        }
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            // Serve a page whose content exists ONLY after JS runs. One thread
            // per connection: Chrome opens speculative preconnect sockets that
            // never send a request, which would wedge a sequential accept loop.
            for stream in listener.incoming().flatten() {
                std::thread::spawn(move || {
                    let mut s = stream;
                    let _ = s.set_read_timeout(Some(std::time::Duration::from_secs(2)));
                    let mut buf = [0u8; 4096];
                    if s.read(&mut buf).unwrap_or(0) == 0 {
                        return; // preconnect socket, no request
                    }
                    let body = "<html><head><title>js page</title></head><body>\
                        <div id=app></div><script>document.getElementById('app').innerHTML = \
                        '<main><h1>Hydrated Title<\\/h1><p>Content born from JavaScript execution.<\\/p><\\/main>';\
                        </script></body></html>";
                    let resp = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                        body.len(),
                        body
                    );
                    let _ = s.write_all(resp.as_bytes());
                });
            }
        });
        let url = url::Url::parse(&format!("http://127.0.0.1:{}/", port)).unwrap();
        let dom = render_url(&url, true).expect("render should succeed");
        assert!(
            dom.contains("Content born from JavaScript execution"),
            "post-JS DOM expected"
        );
    }
}
