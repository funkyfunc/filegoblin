//! End-to-end golden tests: run the real binary over real fixtures and snapshot
//! the markdown that comes out. The product IS its output — any change to
//! parsing or formatting shows up here as a reviewable snapshot diff
//! (`cargo insta review`, or set `INSTA_UPDATE=always` to accept).

use assert_cmd::Command;
use predicates::str::contains;
use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap()
}

fn cmd() -> Command {
    let mut c = Command::cargo_bin("filegoblin").unwrap();
    c.current_dir(repo_root());
    c
}

/// Runs the binary expecting success and returns stdout.
fn gobble(args: &[&str]) -> String {
    let output = cmd().args(args).assert().success();
    String::from_utf8(output.get_output().stdout.clone()).unwrap()
}

// --- One golden per format ---------------------------------------------------

#[test]
fn golden_pdf() {
    insta::assert_snapshot!(gobble(&["test_assets/financial_report.pdf", "-q"]));
}

#[test]
fn golden_docx() {
    insta::assert_snapshot!(gobble(&["test_assets/quarterly_report.docx", "-q"]));
}

#[test]
fn golden_csv() {
    insta::assert_snapshot!(gobble(&["test_assets/dummy.csv", "-q"]));
}

#[test]
fn golden_pptx() {
    insta::assert_snapshot!(gobble(&["test_assets/dummy.pptx", "-q"]));
}

#[test]
fn golden_jupyter() {
    insta::assert_snapshot!(gobble(&["test_assets/dummy.ipynb", "-q"]));
}

#[test]
fn golden_html_article() {
    insta::assert_snapshot!(gobble(&["test_assets/saved_article.html", "-q"]));
}

// --- Horde mode over a controlled directory ----------------------------------

#[test]
fn golden_horde_directory() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("notes.md"),
        "# Notes\n\nRemember the hoard.\n",
    )
    .unwrap();
    std::fs::create_dir(dir.path().join("src")).unwrap();
    std::fs::write(
        dir.path().join("src/lib.rs"),
        "/// Adds one.\npub fn add_one(x: i32) -> i32 {\n    x + 1\n}\n",
    )
    .unwrap();

    let output = cmd()
        .args([dir.path().to_str().unwrap(), "--horde", "-q"])
        .assert()
        .success();
    let stdout = String::from_utf8(output.get_output().stdout.clone()).unwrap();
    insta::assert_snapshot!(stdout);
}

// --- The agent-facing behavioral contract ------------------------------------

#[test]
fn exit_4_when_nothing_extracted() {
    let file = tempfile::Builder::new().suffix(".txt").tempfile().unwrap();
    cmd()
        .args([file.path().to_str().unwrap(), "-q"])
        .assert()
        .code(4);
}

#[test]
fn exit_4_for_missing_file() {
    cmd()
        .args(["does_not_exist_anywhere.md", "-q"])
        .assert()
        .code(4);
}

#[test]
fn invalid_extract_mode_is_a_hard_error() {
    cmd()
        .args(["Cargo.toml", "--extract", "everything"])
        .assert()
        .code(2)
        .stderr(contains("possible values"));
}

#[test]
fn bare_pipe_reads_stdin() {
    cmd()
        .args(["-q"])
        .write_stdin("hello from a pipe")
        .assert()
        .success()
        .stdout(contains("hello from a pipe"));
}

#[test]
fn explicit_path_ignores_open_stdin() {
    // An agent spawning `filegoblin <file>` with an idle stdin pipe must not
    // block on it or grow a phantom "-" target.
    let output = cmd()
        .args(["test_assets/dummy.csv", "-q"])
        .write_stdin("SHOULD NOT APPEAR")
        .assert()
        .success();
    let stdout = String::from_utf8(output.get_output().stdout.clone()).unwrap();
    assert!(!stdout.contains("SHOULD NOT APPEAR"));
    assert!(stdout.contains("// --- FILE_START: test_assets/dummy.csv ---"));
}

#[test]
fn json_contract_shape() {
    let output = cmd()
        .args(["test_assets/dummy.csv", "--json", "-q"])
        .assert()
        .success();
    let stdout = String::from_utf8(output.get_output().stdout.clone()).unwrap();
    let parsed: serde_json::Value =
        serde_json::from_str(&stdout).expect("stdout must be valid JSON");
    let records = parsed.as_array().expect("top level must be an array");
    assert_eq!(records.len(), 1);
    let rec = &records[0];
    assert_eq!(rec["path"], "test_assets/dummy.csv");
    assert!(rec["format"].is_string());
    assert!(rec["tokens"].is_number());
    assert!(
        rec["content"]
            .as_str()
            .unwrap()
            .contains("Headers: Name; Age; Role")
    );
}

#[test]
fn scrub_redacts_email_from_stdin() {
    cmd()
        .args(["-q", "--scrub"])
        .write_stdin("contact goblin@example.com for loot")
        .assert()
        .success()
        .stdout(contains("[EMAIL]"));
}
