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

/// A Latin-1, semicolon-delimited export (Excel in most of Europe). It used to
/// print only the header and "(N malformed row(s) skipped)", exiting 0.
#[test]
fn golden_csv_latin1_semicolon() {
    insta::assert_snapshot!(gobble(&["test_assets/latin1_semicolon.csv", "-q"]));
}

/// Pre-2007 binaries. The `.doc` is written by macOS's Word 97 exporter, the
/// `.ppt` and `.xls` by `test_assets/generate_legacy_office.py`.
#[test]
fn golden_doc() {
    insta::assert_snapshot!(gobble(&["test_assets/legacy_report.doc", "-q"]));
}

#[test]
fn golden_ppt() {
    insta::assert_snapshot!(gobble(&["test_assets/legacy_deck.ppt", "-q"]));
}

#[test]
fn golden_xls() {
    insta::assert_snapshot!(gobble(&["test_assets/legacy_ledger.xls", "-q"]));
}

/// Files Word and PowerPoint saved themselves, from Apache POI's test data
/// (Apache-2.0; `test_assets/apache_poi/PROVENANCE.txt` says what each holds).
#[test]
fn golden_apache_poi_samples() {
    for name in [
        "Lists.doc",
        "footnote.doc",
        "hyperlink.doc",
        "simple-table.doc",
        "basic_test_ppt_file.ppt",
        "42474-1.ppt",
        "54111.ppt",
    ] {
        let out = gobble(&[&format!("test_assets/apache_poi/{name}"), "-q"]);
        let snapshot = format!("apache_poi_{}", name.replace(['.', '-'], "_"));
        insta::assert_snapshot!(snapshot, out);
    }
}

// --- Encodings, containers, and page variants --------------------------------

/// Body of a conversion without its `FILE_START` breadcrumb, so the same
/// document under different names can be compared.
fn body(output: &str) -> String {
    output.lines().skip(1).collect::<Vec<_>>().join("\n")
}

#[test]
fn legacy_encoded_text_is_decoded() {
    let dir = tempfile::tempdir().unwrap();
    let latin1 = dir.path().join("latin1.txt");
    std::fs::write(&latin1, b"caf\xe9 notes\n").unwrap();
    assert!(gobble(&[latin1.to_str().unwrap(), "-q"]).contains("café notes"));

    let utf16 = dir.path().join("utf16.txt");
    let mut bytes = vec![0xFF, 0xFE];
    for unit in "hello utf16 wörld\n".encode_utf16() {
        bytes.extend_from_slice(&unit.to_le_bytes());
    }
    std::fs::write(&utf16, bytes).unwrap();
    assert!(gobble(&[utf16.to_str().unwrap(), "-q"]).contains("hello utf16 wörld"));
}

/// The legacy-encoding fallback must not turn binary into "text".
#[test]
fn unknown_binary_still_exits_4() {
    let dir = tempfile::tempdir().unwrap();
    let blob = dir.path().join("blob.xyz");
    std::fs::write(&blob, b"\x7fELF\x02\x01\x01\x00\x00\x00\xe9\xff\x10\x00").unwrap();
    cmd()
        .args([blob.to_str().unwrap()])
        .assert()
        .code(4)
        .stderr(contains("binary"));
}

#[test]
fn empty_csv_exits_4() {
    let file = tempfile::Builder::new().suffix(".csv").tempfile().unwrap();
    cmd()
        .args([file.path().to_str().unwrap(), "-q"])
        .assert()
        .code(4);
}

/// `.docm`/`.dotx` and a renamed `.docx` used to hit the zip magic and come
/// out as an archive of raw XML parts.
#[test]
fn word_variants_and_renamed_documents_convert_like_docx() {
    let expected = body(&gobble(&["test_assets/quarterly_report.docx", "-q"]));
    let dir = tempfile::tempdir().unwrap();
    for name in ["report.docm", "report.dotx", "report.bin", "report"] {
        let path = dir.path().join(name);
        std::fs::copy(repo_root().join("test_assets/quarterly_report.docx"), &path).unwrap();
        let out = gobble(&[path.to_str().unwrap(), "-q"]);
        assert_eq!(body(&out), expected, "{name}");
    }
}

#[test]
fn macro_enabled_presentation_converts_like_pptx() {
    let expected = body(&gobble(&["test_assets/dummy.pptx", "-q"]));
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("deck.pptm");
    std::fs::copy(repo_root().join("test_assets/dummy.pptx"), &path).unwrap();
    assert_eq!(body(&gobble(&[path.to_str().unwrap(), "-q"])), expected);
}

/// An OLE2 file is read by what its streams hold, so a renamed or
/// extension-less legacy file converts exactly like the original.
#[test]
fn legacy_office_files_convert_whatever_their_name() {
    let dir = tempfile::tempdir().unwrap();
    for fixture in ["legacy_report.doc", "legacy_deck.ppt", "legacy_ledger.xls"] {
        let source = format!("test_assets/{fixture}");
        let expected = body(&gobble(&[&source, "-q"]));
        for name in [
            "no_extension",
            "renamed.bin",
            "swapped.doc",
            "swapped.ppt",
            "swapped.xls",
        ] {
            let path = dir.path().join(name);
            std::fs::copy(repo_root().join(&source), &path).unwrap();
            let out = gobble(&[path.to_str().unwrap(), "-q"]);
            assert_eq!(body(&out), expected, "{fixture} as {name}");
        }
    }
}

/// A damaged legacy file fails on its own: exit 4 with a reason when it is
/// the only target, and no harm to the other files in the same run.
#[test]
fn damaged_legacy_office_files_fail_cleanly() {
    let dir = tempfile::tempdir().unwrap();
    let doc = std::fs::read(repo_root().join("test_assets/legacy_report.doc")).unwrap();
    let truncated = dir.path().join("truncated.doc");
    std::fs::write(&truncated, &doc[..600]).unwrap();
    cmd()
        .args([truncated.to_str().unwrap()])
        .assert()
        .code(4)
        .stderr(contains("truncated or damaged"));

    let fake = dir.path().join("fake.ppt");
    std::fs::write(&fake, b"definitely not a presentation").unwrap();
    cmd()
        .args([fake.to_str().unwrap()])
        .assert()
        .code(4)
        .stderr(contains("not a Word or PowerPoint binary"));

    let out = gobble(&[
        truncated.to_str().unwrap(),
        "test_assets/legacy_deck.ppt",
        "-q",
    ]);
    assert!(out.contains("Do not mention the crowns."), "{out}");
}

/// A minimal ustar archive: one 512-byte header per file, data padded to a
/// block, then two zero blocks.
fn ustar(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut out = Vec::new();
    for (name, data) in entries {
        let mut header = [0u8; 512];
        header[..name.len()].copy_from_slice(name.as_bytes());
        header[100..108].copy_from_slice(b"0000644\0");
        header[108..116].copy_from_slice(b"0000000\0");
        header[116..124].copy_from_slice(b"0000000\0");
        header[124..136].copy_from_slice(format!("{:011o}\0", data.len()).as_bytes());
        header[136..148].copy_from_slice(b"00000000000\0");
        header[156] = b'0';
        header[257..263].copy_from_slice(b"ustar\0");
        header[263..265].copy_from_slice(b"00");
        // The checksum is computed with its own field read as spaces.
        header[148..156].copy_from_slice(b"        ");
        let sum: u32 = header.iter().map(|&b| u32::from(b)).sum();
        header[148..156].copy_from_slice(format!("{:06o}\0 ", sum).as_bytes());
        out.extend_from_slice(&header);
        out.extend_from_slice(data);
        out.resize(out.len().div_ceil(512) * 512, 0);
    }
    out.resize(out.len() + 1024, 0);
    out
}

/// The README listed tar support, but a plain `.tar` failed as "plaintext".
#[test]
fn plain_tar_is_extracted() {
    let dir = tempfile::tempdir().unwrap();
    let tar = ustar(&[("hoard/notes.md", b"# Notes\n\nCount the coins.\n")]);
    for name in ["hoard.tar", "hoard_no_ext"] {
        let path = dir.path().join(name);
        std::fs::write(&path, &tar).unwrap();
        let out = gobble(&[path.to_str().unwrap(), "-q"]);
        assert!(out.contains("FILE_START: hoard/notes.md"), "{name}: {out}");
        assert!(out.contains("Count the coins."), "{name}: {out}");
    }
}

/// Every page of a multi-page TIFF is OCR'd, not just the first. OCR is built
/// in on macOS only (elsewhere it needs `--features ocr` and model weights).
#[cfg(target_os = "macos")]
#[test]
fn multipage_tiff_ocrs_every_page() {
    let out = gobble(&["test_assets/multipage_scan.tiff", "-q", "--no-cache"]);
    let page1 = out.find("## Page 1").expect(&out);
    let page2 = out.find("## Page 2").expect(&out);
    let ledger = out.find("LEDGER").expect(&out);
    let treasure = out.find("TREASURE").expect(&out);
    assert!(
        page1 < ledger && ledger < page2 && page2 < treasure,
        "{out}"
    );
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
            .contains("| Name | Age | Role |")
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

/// The `--scrub-fail` gate is a documented CI contract and had no test.
#[test]
fn scrub_fail_exits_3_when_something_was_redacted() {
    cmd()
        .args(["-q", "--scrub", "--scrub-fail"])
        .write_stdin("contact goblin@example.com for loot")
        .assert()
        .code(3);
}

#[test]
fn scrub_fail_exits_0_when_nothing_was_redacted() {
    cmd()
        .args(["-q", "--scrub", "--scrub-fail"])
        .write_stdin("nothing sensitive in this sentence at all")
        .assert()
        .success();
}

/// Flags that used to be accepted and then silently ignored must now be
/// rejected at parse time rather than producing surprising output.
#[test]
fn silently_ignored_flag_combinations_are_hard_errors() {
    for args in [
        vec!["Cargo.toml", "--json", "--tokens-only"],
        vec!["Cargo.toml", "--summary", "--chunk", "10k"],
        vec!["Cargo.toml", "--manifest", "--chunk", "10k"],
    ] {
        cmd()
            .args(&args)
            .assert()
            .code(2)
            .stderr(contains("cannot be used with"));
    }
}

/// A typo'd --chunk value used to disable chunking silently.
#[test]
fn invalid_chunk_value_is_a_hard_error() {
    cmd()
        .args(["Cargo.toml", "--chunk", "banana"])
        .assert()
        .code(2)
        .stderr(contains("token budget"));
}

/// A URL pointing at a private address is refused before any request.
#[test]
fn loopback_urls_are_refused() {
    cmd()
        .args(["http://127.0.0.1:9/", "-q"])
        .assert()
        .code(4)
        .stderr(contains("private or loopback"));
}

/// A bad --git-diff ref must not fall back to ingesting the whole repository.
#[test]
fn unknown_git_diff_ref_is_fatal() {
    cmd()
        .args([".", "--horde", "--git-diff", "no-such-ref-anywhere", "-q"])
        .assert()
        .failure()
        .stderr(contains("Refusing to fall back"));
}

/// An unparseable glob left an empty set that matched nothing.
#[test]
fn invalid_include_glob_is_reported() {
    cmd()
        .args([".", "--horde", "--include", "[unclosed", "-q"])
        .assert()
        .failure()
        .stderr(contains("Invalid pattern"));
}
