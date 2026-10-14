use assert_cmd::Command;
use serde_json::Value;
use std::fs;
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

fn gobble(args: &[&str]) -> String {
    let output = cmd().args(args).assert().success();
    String::from_utf8(output.get_output().stdout.clone()).unwrap()
}

#[test]
fn test_format_plain_strips_markdown_punctuation() {
    let temp_dir = tempfile::tempdir().unwrap();
    let sample_file = temp_dir.path().join("sample.md");

    let md_content = r#"# Main Architecture

## Core Pipeline Overview

The pipeline processes documents in **Rust** with high efficiency.
See [documentation link](https://filegoblin.rs/docs) for details.

Here is an inline formula: $E = mc^2$, and display formula:
$$\sum_{i=1}^n x_i = X$$

| Module | Purpose | Status |
|:---|:---|:---|
| Parser | Ingestion | Active |
| Chunker | Semantic splits | Complete |

```rust
fn run() {
    println!("Fast!");
}
```
"#;
    fs::write(&sample_file, md_content).unwrap();

    let out = gobble(&[sample_file.to_str().unwrap(), "--format", "plain", "-q"]);

    // Headings must not have '#'
    assert!(out.contains("Main Architecture"));
    assert!(out.contains("Core Pipeline Overview"));
    assert!(!out.contains("# Main Architecture"));
    assert!(!out.contains("## Core Pipeline Overview"));

    // Bold/italic/links stripped
    assert!(out.contains("Rust with high efficiency."));
    assert!(!out.contains("**Rust**"));
    // Web links keep their address: references are content.
    assert!(out.contains("See documentation link (https://filegoblin.rs/docs) for details."));
    assert!(!out.contains("[documentation link](https://filegoblin.rs/docs)"));

    // Math delimiters stripped
    assert!(out.contains("E = mc^2"));
    assert!(!out.contains("$E = mc^2$"));
    assert!(out.contains(r"\sum_{i=1}^n x_i = X"));
    assert!(!out.contains("$$"));

    // Table must be space-aligned without pipe characters
    assert!(!out.contains('|'));
    assert!(!out.contains("|:---|:---|:---|"));
    assert!(out.contains("Module"));
    assert!(out.contains("Parser"));
    assert!(out.contains("Chunker"));

    // Code fences stripped, code content preserved
    assert!(!out.contains("```"));
    assert!(out.contains("fn run() {"));
    assert!(out.contains("println!(\"Fast!\");"));
}

#[test]
fn test_format_json_unchunked_matches_schema() {
    let temp_dir = tempfile::tempdir().unwrap();
    let sample_file = temp_dir.path().join("doc.md");
    fs::write(&sample_file, "# Document\n\nSome body text.").unwrap();

    let out = gobble(&[sample_file.to_str().unwrap(), "--format", "json", "-q"]);

    let val: Value = serde_json::from_str(&out).expect("Valid JSON array");
    assert!(val.is_array());
    let arr = val.as_array().unwrap();
    assert_eq!(arr.len(), 1);

    let doc = &arr[0];
    assert!(doc.get("path").is_some());
    assert!(doc.get("format").is_some());
    assert!(doc.get("tokens").is_some());
    assert!(doc.get("content").is_some());
}

#[test]
fn test_format_json_chunked_matches_schema() {
    let temp_dir = tempfile::tempdir().unwrap();
    let sample_file = temp_dir.path().join("nested.md");

    let content = r#"# Section 1: Introduction

First paragraph of text detailing introduction.

## Subsection 1.1: Background

Here is extensive background narrative information.

## Subsection 1.2: Architecture

Details on the modular components and design decisions.
"#;
    fs::write(&sample_file, content).unwrap();

    let out = gobble(&[
        sample_file.to_str().unwrap(),
        "--format",
        "json",
        "--chunk-size",
        "20",
        "-q",
    ]);

    let val: Value = serde_json::from_str(&out).expect("Valid chunk JSON array");
    assert!(val.is_array());
    let chunks = val.as_array().unwrap();
    assert!(chunks.len() >= 2);

    for (i, chunk) in chunks.iter().enumerate() {
        assert_eq!(chunk["chunk_index"].as_u64().unwrap(), i as u64);
        assert_eq!(chunk["total_chunks"].as_u64().unwrap(), chunks.len() as u64);
        assert!(chunk.get("file").is_some());
        assert!(chunk.get("breadcrumbs").is_some());
        assert!(chunk["breadcrumbs"].is_array());
        assert!(chunk.get("content").is_some());
        assert!(chunk.get("tokens").is_some());
    }

    // Verify breadcrumbs reflect hierarchy
    let has_subsection = chunks.iter().any(|c| {
        let b = c["breadcrumbs"].as_array().unwrap();
        b.iter()
            .any(|s| s.as_str().unwrap_or("").contains("Subsection 1.1"))
    });
    assert!(
        has_subsection,
        "Expected breadcrumbs to contain Subsection 1.1"
    );
}

#[test]
fn test_large_table_context_preservation_across_chunks() {
    let temp_dir = tempfile::tempdir().unwrap();
    let sample_file = temp_dir.path().join("large_table.md");

    let mut md = String::from(
        "# Financial Data\n\n| Year | Revenue | Operating Income | Net Margin |\n|---|---|---|---|\n",
    );
    for y in 2000..=2025 {
        md.push_str(&format!(
            "| {} | ${}M | ${}M | {:.1}% |\n",
            y,
            y * 10,
            y * 2,
            15.0 + (y % 5) as f64
        ));
    }
    fs::write(&sample_file, md).unwrap();

    let out = gobble(&[
        sample_file.to_str().unwrap(),
        "--format",
        "markdown",
        "--chunk-size",
        "30",
        "-q",
    ]);

    // Split output into chunks by divider
    let chunks: Vec<&str> = out.split("\n\n---\n\n").collect();
    assert!(
        chunks.len() >= 3,
        "Table must split into multiple chunks, got {}",
        chunks.len()
    );

    // Every continuation chunk containing table rows MUST replicate the table header & separator
    for (i, chunk) in chunks.iter().enumerate() {
        assert!(
            chunk.contains("| Year | Revenue | Operating Income | Net Margin |"),
            "Chunk {} missing table header: {}",
            i,
            chunk
        );
        assert!(
            chunk.contains("|---|---|---|---|"),
            "Chunk {} missing separator: {}",
            i,
            chunk
        );
    }
}

#[test]
fn test_heading_breadcrumbs_in_markdown_chunks() {
    let temp_dir = tempfile::tempdir().unwrap();
    let sample_file = temp_dir.path().join("hierarchy.md");

    let md = r#"# Chapter One

Introduction text.

## Section A

Paragraph in section A.

### Subsection Alpha

Paragraph in alpha.

## Section B

Paragraph in section B.
"#;
    fs::write(&sample_file, md).unwrap();

    let out = gobble(&[
        sample_file.to_str().unwrap(),
        "--format",
        "markdown",
        "--chunk-size",
        "15",
        "-q",
    ]);

    assert!(out.contains("<!-- Context: Chapter One -->"));
    assert!(out.contains("<!-- Context: Chapter One > Section A -->"));
    assert!(out.contains("<!-- Context: Chapter One > Section A > Subsection Alpha -->"));
    assert!(out.contains("<!-- Context: Chapter One > Section B -->"));
}
