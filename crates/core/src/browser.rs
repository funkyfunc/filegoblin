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

/// Finds an installed Chromium-family browser. Env overrides first, then the
/// per-OS locations used by chrome-launcher/Lighthouse.
pub fn find_browser() -> Option<PathBuf> {
    for var in ["FILEGOBLIN_BROWSER", "CHROME_PATH", "CHROME"] {
        if let Ok(p) = std::env::var(var) {
            let p = PathBuf::from(p);
            if p.exists() {
                return Some(p);
            }
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
    .stderr(Stdio::null());

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
    let reader = std::thread::spawn(move || {
        use std::io::Read;
        let mut chunk = [0u8; 65536];
        while let Ok(n) = stdout_pipe.read(&mut chunk) {
            if n == 0 {
                break;
            }
            sink.lock().unwrap().extend_from_slice(&chunk[..n]);
        }
    });

    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(WALL_CLOCK_KILL_MS);
    let mut stable_since: Option<(usize, std::time::Instant)> = None;
    loop {
        if child.try_wait()?.is_some() {
            break; // clean exit
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
            let _ = child.kill();
            let _ = child.wait();
            break; // whatever was collected gets validated below
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }

    let _ = reader.join();
    let bytes = std::mem::take(&mut *collected.lock().unwrap());
    let dom = String::from_utf8_lossy(&bytes).into_owned();
    let trimmed = dom.trim_start().to_lowercase();
    // Chrome's exit code is unreliable; validate the output instead.
    if dom.trim().len() < 40 || !(trimmed.starts_with("<!doctype") || trimmed.starts_with("<html"))
    {
        anyhow::bail!(
            "Browser produced no usable DOM for {} (page may block headless rendering)",
            url
        );
    }
    Ok(dom)
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
            eprintln!("no browser installed; skipping");
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
