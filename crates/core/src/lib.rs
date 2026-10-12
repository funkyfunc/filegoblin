pub mod browser;
pub mod cache;
pub mod cli;
pub mod compressor;
pub mod curation;
pub mod explore;
pub mod format;
pub mod git;
pub mod mcp;
pub mod net;
pub mod parsers;
pub mod pipeline;
pub mod privacy_shield;
pub mod scrub_config;
#[cfg(feature = "vision")]
pub mod vision;
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
/// can honour `--scrub-fail` and the "nothing extracted" exit code.
pub fn gobble_app(targets: &[String], args: &crate::cli::Cli) -> Result<GobbleOutcome> {
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
                let language = crate::compressor::language_for_path(&doc.path);
                doc.content = crate::compressor::CompressionPipeline::new(level, language)
                    .process(&doc.content);
            }
            doc
        });
        formatters::stream_and_output(stream, &display_name, args)?
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

/// Runs the full ingest pipeline for the given targets and returns the combined
/// markdown as a String instead of writing to stdout. Used by the MCP server
/// (whose stdout carries only protocol messages).
///
/// Runs the **same** curation → privacy → compression stages as [`gobble_app`].
/// It previously took `&Cli` and honoured none of them, so `--scrub` over MCP
/// returned unredacted PII with no error — the one path where the caller is a
/// model rather than a human who might notice.
///
/// If nothing at all was extracted the first error (or a generic message) is
/// returned so a no-op can't look like success; partial failures are appended
/// as a visible note rather than dropped.
pub fn gobble_to_string(targets: &[String], args: &crate::cli::Cli) -> Result<String> {
    if targets.is_empty() {
        anyhow::bail!("No targets to gobble");
    }
    let (tx, rx) = std::sync::mpsc::sync_channel(100);
    let targets_clone = targets.to_vec();
    let args_clone = args.clone();
    let producer =
        std::thread::spawn(move || routing::route_targets(&targets_clone, &args_clone, tx));
    let docs: Vec<crate::pipeline::GobbleDocument> = rx.into_iter().collect();
    producer
        .join()
        .map_err(|_| anyhow::anyhow!("The ingestion thread panicked"))??;

    // Record failures before the stages run so they can be reported in-band.
    let failures: Vec<String> = docs
        .iter()
        .filter_map(|d| d.error.as_ref().map(|e| format!("{}: {}", d.path, e)))
        .collect();

    let has_content = docs
        .iter()
        .any(|d| d.error.is_none() && !d.content.trim().is_empty() && d.path != "_tree.md");
    if !has_content {
        if let Some(err) = docs.iter().find_map(|d| d.error.as_deref()) {
            anyhow::bail!("{}", err);
        }
        anyhow::bail!("Nothing could be extracted from {}", targets.join(", "));
    }

    let shield = if args.scrub {
        Some(build_shield(args)?)
    } else {
        None
    };
    let pruned = crate::pipeline::stages::run_curation_stage(docs, args, targets.len())?;
    let scrubbed = crate::pipeline::stages::run_privacy_stage(pruned, args, shield.as_ref())?;
    let (processed, _pre_compression_tokens) =
        crate::pipeline::stages::run_compression_stage(scrubbed, args)?;

    let content: Vec<&crate::pipeline::GobbleDocument> = processed
        .iter()
        .filter(|d| d.error.is_none() && !d.content.trim().is_empty())
        .collect();

    // Bare content for a single document, FILE_START breadcrumbs (tree map
    // first) for bundles. Note this deliberately differs from the stdout paths,
    // which always emit the breadcrumb: a lone file handed to a model reads
    // better without one.
    let single = content.len() == 1;
    let mut out = String::new();
    for doc in content {
        if single && doc.path != "_tree.md" {
            out.push_str(doc.content.trim());
        } else {
            out.push_str(&crate::formatters::render_block(doc));
        }
    }

    // A partial success that looks total is the failure mode this whole
    // codebase is built to avoid; the model gets told what it did not receive.
    if !failures.is_empty() {
        out.push_str(&format!(
            "\n\n> [{} item(s) failed to convert and are missing from this output: {}]",
            failures.len(),
            failures.join("; ")
        ));
    }

    Ok(out.trim().to_string())
}

/// Whether an output mode needs the full corpus in memory (so it can't stream).
///
/// `--search` builds a RAG index and `--max-tokens` ranks documents against each other;
/// `--json` emits a deterministic, sorted array with per-file metadata; and
/// `--copy`/`--open`/`--tokens-only`/`--summary`/`--manifest` all operate on the
/// complete output (whole-payload clipboard, grand totals, table of contents). Everything
/// else streams out-of-core.
fn requires_buffering(args: &crate::cli::Cli) -> bool {
    args.json
        || args.search.is_some()
        || args.max_tokens.is_some()
        || args.copy
        || args.open
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

    // ARCHIVE INTERCEPTION — by detected format, not just extension, so an
    // extension-less/renamed zip is extracted instead of read as "plaintext".
    if root.is_file() {
        let header = crate::format::read_header(root, crate::format::SNIFF_BYTES);
        if crate::format::detect(root, &header) == crate::format::Format::Archive {
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

    // 2. Apply --include / --exclude glob filters.
    //
    // filegoblin deliberately makes NO secrets-filtering promise: it converts
    // exactly what you point it at. Deciding what's sensitive is the user's
    // call — via --exclude globs, .gitignore (which the walker respects, and
    // is where real secrets usually already live), and --scrub for redaction.
    // A hardcoded blocklist that only applied on some code paths was a false
    // promise; see the 2026-07-30 ADR entry.
    crate::walk::apply_glob_filters(
        &mut files_to_process,
        &args.include,
        &args.exclude,
        args.quiet,
    )?;

    // 3. Apply --git-diff (restrict to changed files vs a ref).
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
            // Fatal, not a warning: leaving `files_to_process` unfiltered meant
            // a typo'd ref quietly ingested the *entire repository* — straight
            // to the clipboard with --copy, or into an LLM. The user asked for
            // a diff; giving them everything is the opposite of the request.
            Err(e) => anyhow::bail!(
                "Could not resolve --git-diff ref `{}`: {}. Refusing to fall back to the \
                 whole repository.",
                git_ref,
                e
            ),
        }
    }

    // 4. Parse every surviving file in parallel.
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
pub(crate) fn route_and_gobble(
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

    let header = crate::format::read_header(path, crate::format::SNIFF_BYTES);
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
        Format::OpenDocument => parsers::opendocument::OpenDocGobbler.gobble(path, &opts),
        Format::LegacyOffice => Err(anyhow::anyhow!(
            "This is a pre-2007 OLE Office file (.doc/.ppt/.msg). No pure-Rust parser \
             exists for it — re-save it as .docx/.pptx (or .eml) and gobble that instead."
        )),
        Format::Jupyter => parsers::jupyter::JupyterGobbler.gobble(path, &opts),
        Format::Email => parsers::email::EmailGobbler.gobble(path, &opts),
        Format::Rtf => parsers::rtf::RtfGobbler.gobble(path, &opts),
        Format::Code => parsers::code::CodeGobbler.gobble(path, &opts),
        Format::Epub => parsers::epub::EpubGobbler.gobble(path, &opts),
        Format::Feed => parsers::feed::FeedGobbler.gobble(path, &opts),
        Format::Subtitle => parsers::subtitle::SubtitleGobbler.gobble(path, &opts),
        Format::Html => parsers::html::HtmlGobbler {
            extract_full: opts.full,
            ..Default::default()
        }
        .gobble(path, &opts),
        // On macOS this always works (Apple Vision); elsewhere the gobbler
        // itself explains the missing `ocr` feature.
        Format::Image => parsers::ocr::OcrGobbler.gobble(path, &opts),
        Format::Audio | Format::Video => parsers::transcribe::TranscribeGobbler.gobble(path, &opts),
        // Archives reaching this point were found *inside* a directory walk
        // (a directly-targeted archive is intercepted and extracted upstream in
        // `gobble_local`). Reading one as plaintext would emit binary garbage.
        Format::Archive => Err(anyhow::anyhow!(
            "Archive found inside a directory walk; point filegoblin at it directly to extract it"
        )),
        // Markdown, plain text, and unknowns fall through to a WASM plugin
        // (if one is installed for the extension) or a plaintext read.
        Format::Markdown | Format::Text => {
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

/// A fetched HTTP body plus the media type the server declared for it, so the
/// caller can route by content instead of assuming everything is HTML.
#[cfg(feature = "web")]
pub(crate) struct FetchedBody {
    pub bytes: Vec<u8>,
    /// Lowercased media type without parameters (e.g. `text/html`), when declared.
    pub media_type: Option<String>,
    /// The raw Content-Type header (kept for charset decoding).
    pub content_type: Option<String>,
}

#[cfg(feature = "web")]
impl FetchedBody {
    /// Decodes the body as text, honouring the declared / sniffed charset.
    pub fn decode_text(&self) -> String {
        decode_html_bytes(&self.bytes, self.content_type.as_deref())
    }

    /// True when the body should go through the HTML extractor: the server says
    /// so, or the media type is vague/absent and the head of the body looks
    /// like an HTML document (servers mislabel constantly).
    pub fn looks_like_html(&self) -> bool {
        match self.media_type.as_deref() {
            Some(mt) if mt.contains("html") || mt.contains("xhtml") => true,
            Some("text/plain") | None => {
                let head = String::from_utf8_lossy(&self.bytes[..self.bytes.len().min(512)])
                    .to_lowercase();
                head.contains("<!doctype html") || head.contains("<html")
            }
            _ => false,
        }
    }

    /// True for non-HTML types we can pass through as text verbatim (markdown,
    /// plain text, JSON, XML, source code, …).
    pub fn is_textual(&self) -> bool {
        match self.media_type.as_deref() {
            Some(mt) => {
                mt.starts_with("text/")
                    || mt.ends_with("+json")
                    || mt.ends_with("+xml")
                    || matches!(
                        mt,
                        "application/json"
                            | "application/xml"
                            | "application/javascript"
                            | "application/x-yaml"
                            | "application/yaml"
                            | "application/toml"
                            | "application/markdown"
                    )
            }
            // No declared type: treat as text when it decodes without NULs.
            None => !self.bytes.contains(&0),
        }
    }
}

/// Wire format for the URL cache: two header lines (media type, content type —
/// header values cannot contain newlines) followed by the raw body bytes.
#[cfg(feature = "web")]
impl FetchedBody {
    fn to_cache_bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.bytes.len() + 128);
        out.extend_from_slice(CACHE_FORMAT_MAGIC);
        out.push(b'\n');
        out.extend_from_slice(self.media_type.as_deref().unwrap_or("").as_bytes());
        out.push(b'\n');
        out.extend_from_slice(self.content_type.as_deref().unwrap_or("").as_bytes());
        out.push(b'\n');
        out.extend_from_slice(&self.bytes);
        out
    }

    fn from_cache_bytes(raw: &[u8]) -> Option<Self> {
        // Entries written by an older filegoblin have no magic line. Treating
        // them as a miss is correct and free — the alternative, which this
        // format previously had no way to avoid, is parsing the first two lines
        // of the *body* as headers and serving a mangled document.
        let rest = raw
            .strip_prefix(CACHE_FORMAT_MAGIC)
            .and_then(|r| r.strip_prefix(b"\n"))?;

        let nl1 = rest.iter().position(|&b| b == b'\n')?;
        let nl2 = nl1 + 1 + rest[nl1 + 1..].iter().position(|&b| b == b'\n')?;
        let line = |s: &[u8]| {
            let v = String::from_utf8_lossy(s).into_owned();
            (!v.is_empty()).then_some(v)
        };
        Some(Self {
            media_type: line(&rest[..nl1]),
            content_type: line(&rest[nl1 + 1..nl2]),
            bytes: rest[nl2 + 1..].to_vec(),
        })
    }
}

/// Version marker for the URL-cache wire format. Bump the number whenever the
/// layout below changes, so old entries miss instead of mis-parsing.
#[cfg(feature = "web")]
const CACHE_FORMAT_MAGIC: &[u8] = b"filegoblin-cache-v1";

#[cfg(feature = "web")]
pub(crate) fn fetch_url(url: &Url, quiet: bool) -> Result<FetchedBody> {
    use std::io::Read;

    // Before anything else, including the cache: a blocked target must not be
    // reachable by seeding the cache either.
    crate::net::guard_url(url)?;

    if let Some(cached) = crate::cache::get_url(url.as_str())
        && let Some(body) = FetchedBody::from_cache_bytes(&cached)
    {
        if !quiet {
            eprintln!("♻️  Using cached fetch of {} (--no-cache to refetch)", url);
        }
        return Ok(body);
    }

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

    let media_type = content_type
        .as_deref()
        .map(|ct| ct.split(';').next().unwrap_or("").trim().to_lowercase())
        .filter(|mt| !mt.is_empty());

    let body = FetchedBody {
        bytes: buf,
        media_type,
        content_type,
    };
    crate::cache::put_url(url.as_str(), &body.to_cache_bytes());
    Ok(body)
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

        // The SSRF guard refuses loopback by default; this test *is* the
        // legitimate local-service case the opt-in exists for.
        let _lock = crate::net::LOCAL_URL_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        crate::net::set_allow_local_urls(true);

        let parsed_url = Url::parse(&url_str).unwrap();
        let body = fetch_url(&parsed_url, true);
        crate::net::set_allow_local_urls(false);
        let body = body.unwrap();
        assert!(body.looks_like_html());
        let html_content = body.decode_text();
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

#[cfg(all(test, feature = "web"))]
mod cache_format_tests {
    use super::FetchedBody;

    #[test]
    fn cache_entries_round_trip() {
        let body = FetchedBody {
            bytes: b"<html>hi</html>".to_vec(),
            media_type: Some("text/html".to_string()),
            content_type: Some("text/html; charset=utf-8".to_string()),
        };
        let back = FetchedBody::from_cache_bytes(&body.to_cache_bytes()).unwrap();
        assert_eq!(back.bytes, body.bytes);
        assert_eq!(back.media_type, body.media_type);
        assert_eq!(back.content_type, body.content_type);
    }

    /// Regression: with no version marker, an entry written by an older
    /// filegoblin had the first two lines of its *body* eaten as headers and
    /// the remainder served as the document.
    #[test]
    fn entries_without_the_version_marker_miss_rather_than_mis_parse() {
        let legacy = b"text/html\ntext/html; charset=utf-8\n<html>body</html>".to_vec();
        assert!(
            FetchedBody::from_cache_bytes(&legacy).is_none(),
            "an unversioned entry must be treated as a miss"
        );
        // Arbitrary garbage is a miss too, not a panic.
        assert!(FetchedBody::from_cache_bytes(b"").is_none());
        assert!(FetchedBody::from_cache_bytes(b"nonsense").is_none());
    }
}
