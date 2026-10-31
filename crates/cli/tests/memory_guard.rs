//! The CLI's memory ceiling (`FILEGOBLIN_MEMORY_MB`), end to end.

use std::io::Read;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// A process over its ceiling stops with exit 5 and says why. MCP mode with
/// stdin held open waits forever otherwise, so only the guard can end it.
#[test]
fn a_process_over_its_ceiling_stops_with_a_message() {
    let mut child = Command::new(assert_cmd::cargo::cargo_bin("filegoblin"))
        .arg("--mcp")
        .env("FILEGOBLIN_MEMORY_MB", "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let _held_open = child.stdin.take();
    let start = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if start.elapsed() > Duration::from_secs(20) {
            child.kill().unwrap();
            panic!("the memory guard never stopped the process");
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    let mut stderr = String::new();
    child
        .stderr
        .take()
        .unwrap()
        .read_to_string(&mut stderr)
        .unwrap();
    assert_eq!(status.code(), Some(5), "{stderr}");
    assert!(stderr.contains("more than its 1 MB limit"), "{stderr}");
}

#[test]
fn a_malformed_ceiling_is_a_usage_error() {
    let out = assert_cmd::Command::cargo_bin("filegoblin")
        .unwrap()
        .args(["Cargo.toml", "-q"])
        .env("FILEGOBLIN_MEMORY_MB", "lots")
        .assert()
        .code(2);
    let stderr = String::from_utf8_lossy(&out.get_output().stderr).into_owned();
    assert!(
        stderr.contains("FILEGOBLIN_MEMORY_MB must be a whole number"),
        "{stderr}"
    );
}

/// `--memory` is the whole run's budget, and the CLI's ceiling is half of it:
/// piped stdin (read whole, up to 1 GB) passes 128 MB long before it ends.
#[test]
fn memory_sets_the_runs_budget() {
    use std::io::Write;
    let mut child = Command::new(assert_cmd::cargo::cargo_bin("filegoblin"))
        .args(["--memory", "256M", "-q"])
        .env_remove("FILEGOBLIN_MEMORY_MB")
        .env_remove("FILEGOBLIN_MEMORY")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    // Feeds stdin until the child is gone (a broken pipe) or 1 GB is sent.
    let feeder = std::thread::spawn(move || {
        let chunk = vec![b'a'; 1 << 20];
        for _ in 0..1024 {
            if stdin.write_all(&chunk).is_err() {
                break;
            }
        }
    });
    let start = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if start.elapsed() > Duration::from_secs(30) {
            child.kill().unwrap();
            panic!("--memory never stopped the process");
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    feeder.join().unwrap();
    let mut stderr = String::new();
    child
        .stderr
        .take()
        .unwrap()
        .read_to_string(&mut stderr)
        .unwrap();
    assert_eq!(status.code(), Some(5), "{stderr}");
    assert!(
        stderr.contains("more than its 128 MB limit (half of --memory)"),
        "{stderr}"
    );
}

#[test]
fn a_malformed_memory_size_is_a_usage_error() {
    for bad in ["lots", "100M"] {
        let out = assert_cmd::Command::cargo_bin("filegoblin")
            .unwrap()
            .args(["Cargo.toml", "-q", "--memory", bad])
            .assert()
            .code(2);
        let stderr = String::from_utf8_lossy(&out.get_output().stderr).into_owned();
        assert!(stderr.contains("--memory"), "{bad}: {stderr}");
    }
}
