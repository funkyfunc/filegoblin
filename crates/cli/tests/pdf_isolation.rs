//! A hostile PDF fails alone. Each PDF converts in a worker process, so a file
//! that overflows the parser's stack, inflates past the memory limit or never
//! finishes costs its own file — an ordinary per-file error — and the rest of
//! the run converts as usual.
//!
//! The hostile files are built here, at test time:
//! - nesting 100,000 levels deep inside an LZW stream, which the nesting
//!   pre-scan does not decode (an abort of the whole process without workers);
//! - a 2 GiB Flate bomb behind a second Flate layer, which the pre-scan peels
//!   only once (so the test's time goes to the parser, not the pre-scan);
//! - a "never-ending" page: one text-showing operator with a million kerned
//!   glyphs, quadratic work that takes the release build over half an hour.

use assert_cmd::Command;
use serde_json::Value;
use std::io::Write;
use std::path::Path;

fn cmd() -> Command {
    let mut c = Command::cargo_bin("filegoblin").unwrap();
    // Deterministic whatever models the machine has.
    c.args(["--vision", "off"]);
    c
}

/// A one-page PDF whose content stream is `data`, with `filter` (a PDF
/// `/Filter` value, or empty) in the stream's dictionary.
fn one_page_pdf(data: &[u8], filter: &str) -> Vec<u8> {
    let content = [
        format!("<< /Length {} {filter} >>\nstream\n", data.len()).into_bytes(),
        data.to_vec(),
        b"\nendstream".to_vec(),
    ]
    .concat();
    let objects: [&[u8]; 5] = [
        b"<< /Type /Catalog /Pages 2 0 R >>",
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents 5 0 R \
          /Resources << /Font << /F1 4 0 R >> >> >>",
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>",
        &content,
    ];
    let mut pdf = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (i, object) in objects.iter().enumerate() {
        offsets.push(pdf.len());
        pdf.extend(format!("{} 0 obj\n", i + 1).bytes());
        pdf.extend(*object);
        pdf.extend(b"\nendobj\n");
    }
    let xref = pdf.len();
    pdf.extend(format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).bytes());
    for offset in offsets {
        pdf.extend(format!("{offset:010} 00000 n \n").bytes());
    }
    pdf.extend(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            objects.len() + 1
        )
        .bytes(),
    );
    pdf
}

fn good_pdf() -> Vec<u8> {
    one_page_pdf(
        b"BT /F1 24 Tf 72 700 Td (Goblins hoard good text.) Tj ET",
        "",
    )
}

/// `data` as LZW of nothing but literal 9-bit codes, with a Clear code every
/// 250 so the code width never grows.
fn lzw_literal(data: &[u8]) -> Vec<u8> {
    const CLEAR: u32 = 256;
    const END: u32 = 257;
    let mut codes = Vec::new();
    for run in data.chunks(250) {
        codes.push(CLEAR);
        codes.extend(run.iter().map(|&b| u32::from(b)));
    }
    codes.push(END);
    let (mut bits, mut held, mut out) = (0u32, 0u32, Vec::new());
    for code in codes {
        bits = (bits << 9) | code;
        held += 9;
        while held >= 8 {
            held -= 8;
            out.push((bits >> held) as u8);
        }
        bits &= (1 << held) - 1;
    }
    if held > 0 {
        out.push((bits << (8 - held)) as u8);
    }
    out
}

fn nested_in_lzw_pdf() -> Vec<u8> {
    let depth = 100_000;
    let content = [
        b"BT /F1 12 Tf 72 700 Td ".to_vec(),
        vec![b'['; depth],
        b"(x)".to_vec(),
        vec![b']'; depth],
        b" TJ ET".to_vec(),
    ]
    .concat();
    one_page_pdf(&lzw_literal(&content), "/Filter /LZWDecode")
}

/// zlib data that inflates to `mib` MiB of spaces, built in milliseconds: a
/// MiB compressed with a full flush (which resets the compressor, so each MiB
/// after the first compresses to the same bytes) repeated, then the final
/// block and the Adler-32 of the whole.
fn zlib_spaces(mib: u64) -> Vec<u8> {
    use flate2::{Compress, Compression, FlushCompress};
    let spaces = vec![b' '; 1 << 20];
    let mut z = Compress::new(Compression::best(), true);
    let mut first = Vec::with_capacity(1 << 20);
    z.compress_vec(&spaces, &mut first, FlushCompress::Full)
        .unwrap();
    let mut repeat = Vec::with_capacity(1 << 20);
    z.compress_vec(&spaces, &mut repeat, FlushCompress::Full)
        .unwrap();
    let mut end = Vec::with_capacity(64);
    z.compress_vec(&[], &mut end, FlushCompress::Finish)
        .unwrap();
    assert_eq!(z.total_in(), 2 << 20, "the compressor took both MiB");

    let mut out = first;
    for _ in 1..mib {
        out.extend(&repeat);
    }
    // The final block, then the checksum of `mib` MiB of spaces: Adler-32 of
    // n bytes of value v is a = 1 + n·v and b = n + v·n(n+1)/2, mod 65521.
    out.extend(&end[..end.len() - 4]);
    let (n, v) = (u128::from(mib) << 20, u128::from(b' '));
    let a = (1 + n * v) % 65521;
    let b = (n + v * n * (n + 1) / 2) % 65521;
    out.extend(((b << 16 | a) as u32).to_be_bytes());
    out
}

fn flate_bomb_pdf() -> Vec<u8> {
    let mut outer = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::best());
    outer.write_all(&zlib_spaces(2048)).unwrap();
    one_page_pdf(
        &outer.finish().unwrap(),
        "/Filter [/FlateDecode /FlateDecode]",
    )
}

fn never_ending_pdf() -> Vec<u8> {
    let mut content = b"BT /F1 1 Tf 0 400 Td [".to_vec();
    let mut seed = 7u32;
    for _ in 0..1_000_000 {
        seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12_345);
        content.extend(format!("(a)-{}", 1 + (seed >> 16) % 900).bytes());
    }
    content.extend(b"] TJ ET");
    one_page_pdf(&content, "")
}

/// `--json` records by path: `(content, error)`.
fn records(stdout: &[u8]) -> Vec<(String, Option<String>, Option<String>)> {
    let parsed: Value = serde_json::from_slice(stdout).expect("stdout is JSON");
    parsed
        .as_array()
        .expect("an array of records")
        .iter()
        .map(|r| {
            let text = |key: &str| r[key].as_str().map(str::to_string);
            (text("path").unwrap(), text("content"), text("error"))
        })
        .collect()
}

fn error_for<'a>(records: &'a [(String, Option<String>, Option<String>)], file: &str) -> &'a str {
    records
        .iter()
        .find(|(path, ..)| path.ends_with(file))
        .and_then(|(_, _, error)| error.as_deref())
        .unwrap_or_else(|| panic!("no error recorded for {file}: {records:?}"))
}

fn content_of<'a>(records: &'a [(String, Option<String>, Option<String>)], file: &str) -> &'a str {
    records
        .iter()
        .find(|(path, ..)| path.ends_with(file))
        .and_then(|(_, content, _)| content.as_deref())
        .unwrap_or_else(|| panic!("no content for {file}: {records:?}"))
}

fn write(dir: &Path, name: &str, bytes: &[u8]) -> String {
    let path = dir.join(name);
    std::fs::write(&path, bytes).unwrap();
    path.to_str().unwrap().to_string()
}

#[test]
fn hostile_pdfs_fail_alone_in_a_directory_run() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "nested.pdf", &nested_in_lzw_pdf());
    write(dir.path(), "bomb.pdf", &flate_bomb_pdf());
    write(dir.path(), "good.pdf", &good_pdf());
    write(dir.path(), "notes.md", b"# Notes\n\nThe hoard is safe.\n");

    let output = cmd()
        .args([dir.path().to_str().unwrap(), "--horde", "--json", "-q"])
        // Low, so the bomb is stopped early.
        .env("FILEGOBLIN_PDF_MEMORY_MB", "256")
        .assert()
        .success();
    let records = records(&output.get_output().stdout);

    let nested = error_for(&records, "nested.pdf");
    assert!(nested.contains("overflowed its stack"), "{nested}");
    let bomb = error_for(&records, "bomb.pdf");
    assert!(bomb.contains("more than 256 MB of memory"), "{bomb}");
    assert!(content_of(&records, "good.pdf").contains("Goblins hoard good text."));
    assert!(content_of(&records, "notes.md").contains("The hoard is safe."));
}

#[test]
fn a_never_ending_pdf_is_stopped_and_the_next_one_converts() {
    let dir = tempfile::tempdir().unwrap();
    let endless = write(dir.path(), "endless.pdf", &never_ending_pdf());
    let good = write(dir.path(), "good.pdf", &good_pdf());

    let started = std::time::Instant::now();
    let output = cmd()
        .args([&endless, &good, "--json", "-q"])
        .env("FILEGOBLIN_PDF_TIMEOUT", "3")
        .assert()
        .success();
    let records = records(&output.get_output().stdout);

    let error = error_for(&records, "endless.pdf");
    assert!(error.contains("no progress on this PDF for 3 s"), "{error}");
    assert!(content_of(&records, "good.pdf").contains("Goblins hoard good text."));
    assert!(started.elapsed().as_secs() < 60, "{:?}", started.elapsed());
}

#[test]
fn the_next_pdf_after_a_crash_gets_a_fresh_worker() {
    let dir = tempfile::tempdir().unwrap();
    let nested = write(dir.path(), "nested.pdf", &nested_in_lzw_pdf());
    let good = write(dir.path(), "good.pdf", &good_pdf());

    // One at a time, in this order: crash, convert, crash, convert.
    let output = cmd()
        .args([&nested, &good, &nested, &good, "--json", "-q"])
        .assert()
        .success();
    let records = records(&output.get_output().stdout);
    assert_eq!(records.len(), 4, "{records:?}");
    let crashed = records
        .iter()
        .filter(|(_, _, e)| {
            e.as_deref()
                .is_some_and(|e| e.contains("overflowed its stack"))
        })
        .count();
    let converted = records
        .iter()
        .filter(|(_, c, _)| {
            c.as_deref()
                .is_some_and(|c| c.contains("Goblins hoard good text."))
        })
        .count();
    assert_eq!((crashed, converted), (2, 2), "{records:?}");
}

#[test]
fn a_lone_hostile_pdf_exits_4_with_its_reason_on_stderr() {
    let dir = tempfile::tempdir().unwrap();
    let nested = write(dir.path(), "nested.pdf", &nested_in_lzw_pdf());
    let output = cmd().arg(&nested).assert().code(4);
    let stderr = String::from_utf8_lossy(&output.get_output().stderr);
    assert!(stderr.contains("overflowed its stack"), "{stderr}");
    // The runtime's own crash report is turned into that message, not
    // passed through.
    assert!(!stderr.contains("fatal runtime error"), "{stderr}");
}

#[test]
fn workers_can_be_turned_off_for_debugging() {
    let dir = tempfile::tempdir().unwrap();
    let good = write(dir.path(), "good.pdf", &good_pdf());
    cmd()
        .args([&good, "-q"])
        .env("FILEGOBLIN_PDF_WORKER", "0")
        .assert()
        .success()
        .stdout(predicates::str::contains("Goblins hoard good text."));
}

#[test]
fn a_malformed_limit_is_a_usage_error() {
    cmd()
        .args(["Cargo.toml", "-q"])
        .env("FILEGOBLIN_PDF_TIMEOUT", "soon")
        .assert()
        .code(2)
        .stderr(predicates::str::contains(
            "FILEGOBLIN_PDF_TIMEOUT must be a whole number",
        ));
}

/// Writes the hostile files to `$FILEGOBLIN_HOSTILE_DIR`, for measuring them
/// against a release build:
/// `FILEGOBLIN_HOSTILE_DIR=/tmp/h cargo test --test pdf_isolation -- --ignored`
#[test]
#[ignore]
fn write_hostile_corpus() {
    let dir = std::env::var("FILEGOBLIN_HOSTILE_DIR").expect("FILEGOBLIN_HOSTILE_DIR");
    let dir = Path::new(&dir);
    std::fs::create_dir_all(dir).unwrap();
    write(dir, "nested.pdf", &nested_in_lzw_pdf());
    write(dir, "bomb.pdf", &flate_bomb_pdf());
    write(dir, "endless.pdf", &never_ending_pdf());
    write(dir, "good.pdf", &good_pdf());
}
