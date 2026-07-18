pub mod cli;
pub mod compressor;
pub mod cost;
pub mod curation;
pub mod flavors;
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
    /// Number of non-empty content documents produced (excludes the tree map).
    /// Only meaningful on the buffered path, which includes every `--json` run;
    /// the streaming path reports 0 and callers only consult this for `--json`.
    pub produced: usize,
}

/// Runs the full ingest → format pipeline and reports what happened, so the caller
/// can honour `--scrub-fail` and the `--json` "nothing extracted" exit code.
pub fn gobble_app(
    targets: &[String],
    flavor: &flavors::Flavor,
    args: &crate::cli::Cli,
) -> Result<GobbleOutcome> {
    let (tx, rx) = std::sync::mpsc::sync_channel(100);

    let targets_clone = targets.to_vec();
    let args_clone = args.clone();
    std::thread::spawn(move || {
        let _ = routing::route_targets(&targets_clone, &args_clone, tx);
    });

    let display_name = if targets.len() == 1 {
        targets[0].clone()
    } else if targets.len() == 2 {
        format!("{} & {}", targets[0], targets[1])
    } else if args.clipboard && targets.is_empty() {
        "clipboard".to_string()
    } else {
        format!("{} and {} others", targets[0], targets.len() - 1)
    };

    // One shared Privacy Shield for the whole run, so redactions are configured
    // consistently and the audit tally aggregates across every document (both the
    // buffered and the streaming paths).
    let shield = if args.scrub {
        Some(std::sync::Arc::new(build_shield(args)?))
    } else {
        None
    };

    // Some operations need the full corpus in memory before we can format:
    //  - --search builds a RAG index; --max-tokens ranks documents against each
    //    other to enforce a global budget;
    //  - --copy / --open / --cost / --tokens-only / --summary / --manifest all
    //    operate on the complete output (whole-payload clipboard, totals, TOC);
    //  - --json is a machine contract: buffering lets us emit a deterministic,
    //    alphabetically-sorted array with per-file metadata.
    // Everything else streams out-of-core.
    let needs_buffer = args.json
        || args.search.is_some()
        || args.max_tokens.is_some()
        || args.copy
        || args.open
        || args.cost
        || args.tokens_only
        || args.summary
        || args.manifest;

    let produced = if needs_buffer {
        let raw_documents: Vec<crate::pipeline::GobbleDocument> = rx.into_iter().collect();
        let pruned_docs =
            crate::pipeline::stages::run_curation_stage(raw_documents, args, targets.len())?;
        let final_docs =
            crate::pipeline::stages::run_privacy_stage(pruned_docs, args, shield.as_deref())?;
        let (compressed_docs, pre_compression_tokens) =
            crate::pipeline::stages::run_compression_stage(final_docs, args)?;
        let produced = compressed_docs
            .iter()
            .filter(|d| d.path != "_tree.md" && !d.content.trim().is_empty())
            .count();
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
        formatters::stream_and_output(stream, flavor, &display_name, args)?;
        0
    };

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

pub fn gobble_local(target: &str, args: &crate::cli::Cli) -> Result<Vec<(String, String)>> {
    let mut files = Vec::new();
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
        match route_and_gobble(target, args) {
            Ok(content) => files.push((target.to_string(), content)),
            Err(e) => eprintln!("{} Error summarizing {}: {}", "⚠️".yellow(), target, e),
        }
        return Ok(files);
    }

    // 1. Walk the tree (respects .gitignore) → ASCII map + file list.
    let (tree_out, mut files_to_process) = crate::walk::walk_tree(target, args.depth);
    files.push(("_tree.md".to_string(), tree_out));

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

/// Parses each file (in parallel) into a `(relative_path, content)` pair, honoring
/// `--diff-format`. Errors are logged and the file is skipped.
fn parse_files(
    paths: &[std::path::PathBuf],
    root: &Path,
    args: &crate::cli::Cli,
) -> Vec<(String, String)> {
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

            let content = if args.diff_format && args.git_diff.is_some() {
                crate::git::file_diff(root, args.git_diff.as_deref().unwrap(), file_path)
            } else {
                match route_and_gobble(&file_path.to_string_lossy(), args) {
                    Ok(c) => Some(c),
                    Err(e) => {
                        eprintln!("{} Error summarizing {}: {}", "⚠️".yellow(), rel_str, e);
                        None
                    }
                }
            };

            content.map(|c| (rel_str, c))
        })
        .collect()
}

#[allow(unused_variables)]
fn route_and_gobble(path_str: &str, args: &crate::cli::Cli) -> Result<String> {
    let path = Path::new(path_str);
    let extension = path
        .extension()
        .and_then(|s| s.to_str())
        .map(|s| s.to_lowercase())
        .unwrap_or_default();

    // EXPLICIT OVERRIDE: If the user passed `--plugin <NAME>`, force execution through that WASM component.
    if let Some(plugin_name) = args.plugin.as_deref() {
        #[cfg(feature = "wasm")]
        {
            // Explicit --plugin is an opt-in, so we allow the local ./plugins dir here.
            if let Some(plugin_path) = parsers::wasm::WasmGobbler::sniff(plugin_name, true) {
                match (parsers::wasm::WasmGobbler {
                    wasm_path: plugin_path,
                })
                .gobble(path, args)
                {
                    Ok(markdown) => return Ok(markdown),
                    Err(e) => anyhow::bail!("Explicit WASM Plugin '{}' failed: {}", plugin_name, e),
                }
            } else {
                anyhow::bail!(
                    "Requested explicit --plugin '{}' could not be found in ~/.filegoblin/plugins/ or ./plugins/",
                    plugin_name
                );
            }
        }
        #[cfg(not(feature = "wasm"))]
        {
            anyhow::bail!(
                "Explicit WASM Plugins require the 'wasm' feature. Recompile with --features wasm."
            );
        }
    }

    match extension.as_str() {
        "epub" => {
            #[cfg(feature = "web")]
            {
                parsers::epub::EpubGobbler.gobble(path, args)
            }
            #[cfg(not(feature = "web"))]
            {
                anyhow::bail!(
                    "Parsing EPUB files requires the 'web' feature. Recompile with --features web."
                );
            }
        }
        "ipynb" => parsers::jupyter::JupyterGobbler.gobble(path, args),
        "pdf" => parsers::pdf::PdfGobbler.gobble(path, args),
        "docx" => parsers::office::OfficeGobbler.gobble(path, args),
        "xlsx" | "xls" | "ods" | "csv" => parsers::sheet::SheetGobbler.gobble(path, args),
        "pptx" => parsers::powerpoint::PptxGobbler.gobble(path, args),
        "eml" | "mbox" => parsers::email::EmailGobbler.gobble(path, args),
        "rtf" => parsers::rtf::RtfGobbler.gobble(path, args),
        "html" | "htm" => {
            #[cfg(feature = "web")]
            {
                parsers::web::WebGobbler {
                    extract_full: args.full,
                }
                .gobble(path, args)
            }
            #[cfg(not(feature = "web"))]
            {
                anyhow::bail!(
                    "Parsing HTML files requires the 'web' feature. Recompile with --features web, or use --fallback."
                );
            }
        }
        "rs" | "js" | "py" | "ts" | "go" | "c" | "cpp" => {
            parsers::code::CodeGobbler.gobble(path, args)
        }
        _ => {
            // Priority 1: Check if a user installed a dynamic WASM plugin for this
            // extension. Implicit dispatch only trusts ~/.filegoblin/plugins (never
            // the current directory) so a hostile CWD cannot auto-run its plugins.
            #[allow(clippy::collapsible_if)]
            #[cfg(feature = "wasm")]
            if let Some(plugin_path) = parsers::wasm::WasmGobbler::sniff(&extension, false) {
                if let Ok(markdown) = (parsers::wasm::WasmGobbler {
                    wasm_path: plugin_path,
                })
                .gobble(path, args)
                {
                    return Ok(markdown);
                }
            }

            // Priority 2: Fall back to core heuristics
            // If it's an image, pass to ocr. If text, just read.
            if ["png", "jpg", "jpeg", "webp"].contains(&extension.as_str()) {
                #[cfg(feature = "ocr")]
                {
                    parsers::ocr::OcrGobbler.gobble(path, args)
                }
                #[cfg(not(feature = "ocr"))]
                {
                    anyhow::bail!(
                        "OCR extraction requires the 'ocr' feature. Recompile with --features ocr."
                    );
                }
            } else {
                std::fs::read_to_string(path).context("Failed to read file as plaintext")
            }
        }
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

    String::from_utf8(buf).context("Mischievous decoding error: Failed to read URL body as UTF-8")
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
        let parsed_content = crate::parsers::web::WebGobbler {
            extract_full: false,
        }
        .gobble_str(&html_content, &args)
        .unwrap();
        assert!(parsed_content.contains("Goblin network testing!"));
    }

    #[test]
    fn test_route_and_gobble_unknown_extension() {
        // Fallback for an unknown extension (.xyz)
        let test_file = "dummy.xyz";
        std::fs::write(test_file, "Plaintext fallback text").unwrap();
        let args = crate::cli::Cli::parse_from(["filegoblin", "dummy"]);
        let res = route_and_gobble(test_file, &args).unwrap();
        assert_eq!(res, "Plaintext fallback text");
        std::fs::remove_file(test_file).ok();
    }
}
