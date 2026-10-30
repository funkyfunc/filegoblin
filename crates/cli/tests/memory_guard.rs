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
