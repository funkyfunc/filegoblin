pub mod cli;
pub mod compressor;
pub mod cost;
pub mod curation;
pub mod flavors;
pub mod format;
pub mod git;
pub mod net;
pub mod parsers;
pub mod pipeline;
pub mod privacy_shield;
pub mod scrub_config;
pub mod walk;

use crate::parsers::gobble::Gobble;

use anyhow::{Context, Result};
use colored::Colorize;
use std::path::Path;
#[cfg(feature = "web")]
use url::Url;

/// filegoblin Core
/// We keep logic in lib.rs to ensure the application is deeply testable
/// independently from the `clap` CLI layer.
#[allow(clippy::too_many_arguments)]
pub mod formatters;
pub mod routing;

/// Outcome of a run, used by the CLI to choose an exit code.
pub struct GobbleOutcome {
    /// Number of Privacy Shield redactions performed (0 when `--scrub` is off).
    pub redactions: usize,
    /// Number of non-empty content documents produced (excludes the tree map),
    /// counted on both the buffered and the streaming paths. The CLI exits
    /// non-zero when this is 0 so a silent no-op can never look like success.
    pub produced: usize,
}

/// Runs the full ingest → format pipeline and reports what happened, so the caller
/// can honour `--scrub-fail` and the `--json` "nothing extracted" exit code.
pub fn gobble_app(
    targets: &[String],
    flavor: &flavors::Flavor,
    args: &crate::cli::Cli,
) -> Result<GobbleOutcome> {
    if targets.is_empty() && !args.clipboard {
        anyhow::bail!("No targets to gobble: pass at least one path/URL or --clipboard");
    }

    let (tx, rx) = std::sync::mpsc::sync_channel(100);

    let targets_clone = targets.to_vec();
    let args_clone = args.clone();
    // Keep the handle: the producer's error (e.g. "recompile with --features web")
    // must reach the user, never be silently discarded.
    let producer =
        std::thread::spawn(move || routing::route_targets(&targets_clone, &args_clone, tx));

    let display_name = match targets {
        [] => "clipboard".to_string(),
        [one] => one.clone(),
        [a, b] => format!("{} & {}", a, b),
        [first, rest @ ..] => format!("{} and {} others", first, rest.len()),
    };

    // One shared Privacy Shield for the whole run, so redactions are configured
    // consistently and the audit tally aggregates across every document (both the
    // buffered and the streaming paths).
    let shield = if args.scrub {
        Some(std::sync::Arc::new(build_shield(args)?))
    } else {
        None
    };

    let produced = if requires_buffering(args) {
        let raw_documents: Vec<crate::pipeline::GobbleDocument> = rx.into_iter().collect();
        let pruned_docs =
            crate::pipeline::stages::run_curation_stage(raw_documents, args, targets.len())?;
        let final_docs =
            crate::pipeline::stages::run_privacy_stage(pruned_docs, args, shield.as_deref())?;
        let (compressed_docs, pre_compression_tokens) =
            crate::pipeline::stages::run_compression_stage(final_docs, args)?;
        let produced = compressed_docs.iter().filter(|d| d.is_content()).count();
        formatters::format_and_output(
            compressed_docs,
            pre_compression_tokens,
            flavor,
            &display_name,
            args,
        )?;
        produced
    } else {
        // True Out-of-Core Stream Processing: process documents one by one.
        let shield_stream = shield.clone();
        let stream = rx.into_iter().map(move |mut doc| {
            if let Some(s) = &shield_stream {
                doc.content = s.redact(&doc.content);
            }
            if let Some(level) = &args.compress {
                doc.content = crate::compressor::CompressionPipeline::new(level, Some(&doc.path))
                    .process(&doc.content);
            }
            doc
        });
        formatters::stream_and_output(stream, flavor, &display_name, args)?
    };

    // The channel is fully drained by now, so the producer has finished; surface
    // any routing failure (unreachable URL, missing feature, clipboard error).
    producer
        .join()
        .map_err(|_| anyhow::anyhow!("The ingestion thread panicked"))??;

    // Emit the audit report (never the values) and return the outcome.
    let redactions = if let Some(s) = &shield {
        if !args.quiet {
            print_scrub_report(&s.counts());
        }
        s.total()
    } else {
        0
    };
    Ok(GobbleOutcome {
        redactions,
        produced,
    })
}

/// Whether an output mode needs the full corpus in memory (so it can't stream).
///
/// `--search` builds a RAG index and `--max-tokens` ranks documents against each other;
/// `--json` emits a deterministic, sorted array with per-file metadata; and
/// `--copy`/`--open`/`--cost`/`--tokens-only`/`--summary`/`--manifest` all operate on the
/// complete output (whole-payload clipboard, grand totals, table of contents). Everything
/// else streams out-of-core.
fn requires_buffering(args: &crate::cli::Cli) -> bool {
    args.json
        || args.search.is_some()
        || args.max_tokens.is_some()
        || args.copy
        || args.open
        || args.cost
        || args.tokens_only
        || args.summary
        || args.manifest
}

/// Constructs a Privacy Shield from the CLI flags: redaction style, an optional
/// JSON config (allowlist + custom patterns), and any `--scrub-allow` values.
fn build_shield(args: &crate::cli::Cli) -> Result<crate::privacy_shield::PrivacyShield> {
    let config = crate::scrub_config::ScrubConfig::discover(args.scrub_config.as_deref())?;

    let mut allowlist = args.scrub_allow.clone();
    let mut custom: Vec<(String, String)> = Vec::new();
    if let Some(cfg) = config {
        allowlist.extend(cfg.allowlist);
        custom = cfg
            .patterns
            .into_iter()
            .map(|p| (p.name, p.regex))
            .collect();
    }

    crate::privacy_shield::PrivacyShield::with_config(args.scrub_style, allowlist, &custom)
}

/// Prints a typed, value-free tally of what the Privacy Shield redacted.
fn print_scrub_report(counts: &std::collections::BTreeMap<String, usize>) {
    if counts.is_empty() {
        eprintln!(
            "{}",
            "🛡️ Privacy Shield: nothing sensitive found.".truecolor(255, 191, 0)
        );
        return;
    }
    let total: usize = counts.values().sum();
    let breakdown = counts
        .iter()
        .map(|(kind, n)| format!("{}×{}", n, kind))
        .collect::<Vec<_>>()
        .join(", ");
    eprintln!(
        "{}",
        format!(
            "🛡️ Privacy Shield redacted {} item(s): {}",
            total, breakdown
        )
        .truecolor(255, 191, 0)
    );
}

pub fn gobble_local(
    target: &str,
    args: &crate::cli::Cli,
) -> Result<Vec<crate::pipeline::GobbleDocument>> {
    use crate::pipeline::GobbleDocument;
    let mut files: Vec<GobbleDocument> = Vec::new();
    let root = std::path::Path::new(target);

    // ARCHIVE INTERCEPTION
    if root.is_file() {
        let ext = root
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_lowercase();
        if ext == "zip" || ext == "gz" || ext == "tgz" {
            let archive_gobbler = crate::parsers::archive::ArchiveGobbler;
            return archive_gobbler.extract_archive(root, args);
        }
    }

    if !root.is_dir() || !args.horde {
        let (format, result) = route_and_gobble(target, args);
        let bytes = std::fs::metadata(target).ok().map(|m| m.len());
        match result {
            Ok(content) => {
                files.push(GobbleDocument::extracted(
                    target.to_string(),
                    content,
                    format.label(),
                    bytes,
                ));
            }
            Err(e) => {
                eprintln!("{} Error summarizing {}: {}", "⚠️".yellow(), target, e);
                files.push(GobbleDocument::failed(
                    target.to_string(),
                    format.label(),
                    e.to_string(),
                ));
            }
        }
        return Ok(files);
    }

    // 1. Walk the tree (respects .gitignore) → ASCII map + file list.
    let (tree_out, mut files_to_process) = crate::walk::walk_tree(target, args.depth);
    files.push(GobbleDocument::extracted(
        "_tree.md".to_string(),
        tree_out,
        "tree",
        None,
    ));

    // Note: --max-tokens budgeting is enforced globally in the pipeline
    // (crate::curation::enforce_budget), which needs the full corpus in memory to
    // rank documents against each other. It is not applied here during the walk.

    // 2. Never ingest secrets, no matter what the flags say.
    let skipped = crate::walk::filter_radioactive(&mut files_to_process);
    if skipped > 0 && !args.quiet {
        eprintln!(
            "{}",
            format!("🛡️ SKIPPED {} RADIOACTIVE FILE(S)", skipped).truecolor(255, 0, 0)
        );
    }

    // 3. Apply --include / --exclude glob filters.
    crate::walk::apply_glob_filters(
        &mut files_to_process,
        &args.include,
        &args.exclude,
        args.quiet,
    );

    // 4. Apply --git-diff (restrict to changed files vs a ref).
    if let Some(ref git_ref) = args.git_diff {
        match crate::git::changed_files(root, git_ref) {
            Ok(changed) => {
                let before = files_to_process.len();
                files_to_process.retain(|p| {
                    p.strip_prefix(root)
                        .map(|rel| {
                            changed
                                .iter()
                                .any(|cf| rel.to_string_lossy() == cf.as_str())
                        })
                        .unwrap_or(false)
                });
                if !args.quiet {
                    eprintln!(
                        "{}",
                        format!(
                            "🔀 Git diff mode (vs {}): {} changed files found (from {} total)",
                            git_ref,
                            files_to_process.len(),
                            before
                        )
                        .truecolor(0, 200, 255)
                    );
                }
            }
            Err(e) => eprintln!("{} {}", "⚠️".yellow(), e),
        }
    }

    // 5. Parse every surviving file in parallel.
    files.extend(parse_files(&files_to_process, root, args));
    Ok(files)
}

/// Parses each file (in parallel) into a `GobbleDocument`, honoring `--diff-format`.
/// A parse failure is logged AND recorded as a document with `error` set, so it can
/// surface as an in-band `{path, error}` record in `--json` instead of vanishing.
fn parse_files(
    paths: &[std::path::PathBuf],
    root: &Path,
    args: &crate::cli::Cli,
) -> Vec<crate::pipeline::GobbleDocument> {
    use crate::pipeline::GobbleDocument;
    use rayon::prelude::*;
    let strip_base = if root.is_file() {
        root.parent().unwrap_or(root)
    } else {
        root
    };

    paths
        .par_iter()
        .filter_map(|file_path| {
            let rel_str = file_path
                .strip_prefix(strip_base)
                .ok()?
                .to_string_lossy()
                .into_owned();
            let bytes = std::fs::metadata(file_path).ok().map(|m| m.len());

            // --diff-format short-circuits parsing with git's unified diff.
            if args.diff_format && args.git_diff.is_some() {
                let diff =
                    crate::git::file_diff(root, args.git_diff.as_deref().unwrap(), file_path);
                return diff.map(|c| GobbleDocument::extracted(rel_str, c, "diff", bytes));
            }

            let (format, result) = route_and_gobble(&file_path.to_string_lossy(), args);
            Some(match result {
                Ok(content) => GobbleDocument::extracted(rel_str, content, format.label(), bytes),
                Err(e) => {
                    eprintln!("{} Error summarizing {}: {}", "⚠️".yellow(), rel_str, e);
                    GobbleDocument::failed(rel_str, format.label(), e.to_string())
                }
            })
        })
        .collect()
}

/// Detects a file's [`Format`] and dispatches to the matching parser. Returns the
/// detected format alongside the parse result so callers can attach it to the
/// document (even on failure). Detection is extension-first with a magic-byte
/// fallback, so renamed / extension-less files still route correctly.
#[allow(unused_variables)]
fn route_and_gobble(
    path_str: &str,
    args: &crate::cli::Cli,
) -> (crate::format::Format, Result<String>) {
    use crate::format::Format;
    let path = Path::new(path_str);
    let extension = path
        .extension()
        .and_then(|s| s.to_str())
        .map(|s| s.to_lowercase())
        .unwrap_or_default();

    let header = crate::format::read_header(path, 512);
    let format = crate::format::detect(path, &header);
    let opts = crate::parsers::gobble::ParseOptions::from(args);

    // EXPLICIT OVERRIDE: `--plugin <NAME>` forces execution through that WASM component.
    if let Some(plugin_name) = args.plugin.as_deref() {
        return (format, run_explicit_plugin(plugin_name, path, &opts));
    }

    let result = match format {
        Format::Pdf => parsers::pdf::PdfGobbler.gobble(path, &opts),
        Format::Docx => parsers::office::OfficeGobbler.gobble(path, &opts),
        Format::Spreadsheet => parsers::sheet::SheetGobbler.gobble(path, &opts),
        Format::Pptx => parsers::powerpoint::PptxGobbler.gobble(path, &opts),
        Format::Jupyter => parsers::jupyter::JupyterGobbler.gobble(path, &opts),
        Format::Email => parsers::email::EmailGobbler.gobble(path, &opts),
        Format::Rtf => parsers::rtf::RtfGobbler.gobble(path, &opts),
        Format::Code => parsers::code::CodeGobbler.gobble(path, &opts),
        Format::Epub => parsers::epub::EpubGobbler.gobble(path, &opts),
        Format::Html => parsers::html::HtmlGobbler {
            extract_full: opts.full,
            ..Default::default()
        }
        .gobble(path, &opts),
        Format::Image => {
            #[cfg(feature = "ocr")]
            {
                parsers::ocr::OcrGobbler.gobble(path, &opts)
            }
            #[cfg(not(feature = "ocr"))]
            {
                Err(anyhow::anyhow!(
                    "OCR extraction requires the 'ocr' feature. Recompile with --features ocr."
                ))
            }
        }
        // Markdown, plain text, unknown, and (inline) archives fall through to a
        // WASM plugin (if one is installed for the extension) or a plaintext read.
        Format::Markdown | Format::Text | Format::Archive => {
            #[cfg(feature = "wasm")]
            if let Some(plugin_path) = parsers::wasm::WasmGobbler::sniff(&extension, false)
                && let Ok(markdown) = (parsers::wasm::WasmGobbler {
                    wasm_path: plugin_path,
                })
                .gobble(path, &opts)
            {
                return (format, Ok(markdown));
            }
            std::fs::read_to_string(path).context("Failed to read file as plaintext")
        }
    };

    (format, result)
}

/// Runs an explicitly requested WASM plugin (`--plugin`). Allowed to use the local
/// `./plugins` dir since it is an opt-in.
#[allow(unused_variables)]
fn run_explicit_plugin(
    plugin_name: &str,
    path: &Path,
    opts: &crate::parsers::gobble::ParseOptions,
) -> Result<String> {
    #[cfg(feature = "wasm")]
    {
        if let Some(plugin_path) = parsers::wasm::WasmGobbler::sniff(plugin_name, true) {
            (parsers::wasm::WasmGobbler {
                wasm_path: plugin_path,
            })
            .gobble(path, opts)
            .map_err(|e| anyhow::anyhow!("Explicit WASM Plugin '{}' failed: {}", plugin_name, e))
        } else {
            anyhow::bail!(
                "Requested explicit --plugin '{}' could not be found in ~/.filegoblin/plugins/ or ./plugins/",
                plugin_name
            )
        }
    }
    #[cfg(not(feature = "wasm"))]
    {
        anyhow::bail!(
            "Explicit WASM Plugins require the 'wasm' feature. Recompile with --features wasm."
        )
    }
}

#[cfg(feature = "web")]
fn fetch_url(url: &Url, quiet: bool) -> Result<String> {
    use std::io::Read;

    // reqwest respects HTTP_PROXY / HTTPS_PROXY automatically.
    if let Ok(proxy) = std::env::var("HTTPS_PROXY").or_else(|_| std::env::var("HTTP_PROXY"))
        && !quiet
    {
        eprintln!("🔒 Corporate proxy detected: routing through {}", proxy);
    }

    let client = crate::net::blocking_client()?;

    let resp = client
        .get(url.clone())
        .send()
        .context("Mischievous network error: Failed to reach the URL")?;

    // Reject oversized bodies up front when the server declares a length.
    if let Some(len) = resp.content_length()
        && len > crate::net::MAX_RESPONSE_BYTES
    {
        anyhow::bail!(
            "Response body is {} bytes, exceeding the {} MiB cap",
            len,
            crate::net::MAX_RESPONSE_BYTES / (1024 * 1024)
        );
    }

    let content_type = resp
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned);

    // Cap chunked/undeclared bodies by reading at most MAX_RESPONSE_BYTES + 1.
    let mut buf = Vec::new();
    resp.take(crate::net::MAX_RESPONSE_BYTES + 1)
        .read_to_end(&mut buf)
        .context("Mischievous network error: Failed to read URL body")?;
    if buf.len() as u64 > crate::net::MAX_RESPONSE_BYTES {
        anyhow::bail!(
            "Response body exceeds the {} MiB cap",
            crate::net::MAX_RESPONSE_BYTES / (1024 * 1024)
        );
    }

    Ok(decode_html_bytes(&buf, content_type.as_deref()))
}

/// Decodes a fetched page's bytes: honours the `Content-Type` charset when
/// declared, falls back to a `<meta charset>` sniff of the head, then UTF-8.
/// Decoding is lossy rather than fatal — a latin-1 page must never make the
/// whole fetch fail.
#[cfg(feature = "web")]
fn decode_html_bytes(buf: &[u8], content_type: Option<&str>) -> String {
    let from_header = content_type.and_then(charset_of);
    let charset = from_header.or_else(|| {
        let head = String::from_utf8_lossy(&buf[..buf.len().min(2048)]).to_lowercase();
        charset_of(&head)
    });
    let encoding = charset
        .and_then(|c| encoding_rs::Encoding::for_label(c.as_bytes()))
        .unwrap_or(encoding_rs::UTF_8);
    encoding.decode(buf).0.into_owned()
}

/// Pulls the value out of the first `charset=...` in a header or HTML head.
#[cfg(feature = "web")]
fn charset_of(s: &str) -> Option<String> {
    let idx = s.find("charset=")? + "charset=".len();
    let value: String = s[idx..]
        .trim_start_matches(['"', '\''])
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
        .collect();
    (!value.is_empty()).then_some(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[test]
    #[cfg(feature = "web")]
    fn test_fetch_url_and_gobble() {
        use std::io::Write;
        use std::net::TcpListener;
        use std::thread;

        // Start a dummy local server to test the network engine without an external dependency
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let url_str = format!("http://127.0.0.1:{}/", port);

        thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                use std::io::Read;
                let mut buf = [0; 1024];
                let _ = stream.read(&mut buf);

                let body = b"<html><body><article>Goblin network testing!</article></body></html>";
                let header = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                let _ = stream.write_all(header.as_bytes());
                let _ = stream.write_all(body);
            }
        });

        let parsed_url = Url::parse(&url_str).unwrap();
        let html_content = fetch_url(&parsed_url, true).unwrap();
        assert!(html_content.contains("Goblin network testing!"));

        let args = crate::cli::Cli::parse_from(["filegoblin", "dummy"]);
        let parsed_content = crate::parsers::html::HtmlGobbler::default()
            .gobble_str(&html_content, &(&args).into())
            .unwrap();
        assert!(parsed_content.contains("Goblin network testing!"));
    }

    #[test]
    fn test_route_and_gobble_unknown_extension() {
        // Fallback for an unknown extension (.xyz)
        let test_file = "dummy.xyz";
        std::fs::write(test_file, "Plaintext fallback text").unwrap();
        let args = crate::cli::Cli::parse_from(["filegoblin", "dummy"]);
        let (format, res) = route_and_gobble(test_file, &args);
        assert_eq!(res.unwrap(), "Plaintext fallback text");
        assert_eq!(format, crate::format::Format::Text);
        std::fs::remove_file(test_file).ok();
    }
}
