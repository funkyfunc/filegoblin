//! MCP (Model Context Protocol) stdio server: newline-delimited JSON-RPC 2.0.
//!
//! Hand-rolled rather than the `rmcp` SDK: a tools-only stdio server needs
//! ~5 methods, and the SDK drags in tokio/tracing/schemars. Spec:
//! <https://modelcontextprotocol.io/specification/2025-06-18/basic/transports>
//! Stdout carries ONLY protocol messages (one JSON object per line); all
//! diagnostics anywhere in filegoblin already go to stderr.

use anyhow::Result;
use serde_json::{Value, json};

/// Protocol revisions we can speak, newest first. If the client requests one of
/// these we echo it; otherwise we answer with our newest and let the client
/// decide (per spec, it disconnects only if it can't accept that).
/// `2025-03-26` is deliberately absent: that revision made JSON-RPC batching
/// mandatory, and we reject batches. Advertising it would promise something we
/// do not implement.
const SUPPORTED_VERSIONS: &[&str] = &["2025-11-25", "2025-06-18", "2024-11-05"];

/// Ceiling on a single tool result. Conversions land straight in a model's
/// context window, and `convert` on a large repository can otherwise return
/// tens of megabytes.
const MAX_TOOL_RESULT_BYTES: usize = 1024 * 1024;

/// How many conversions may run at once. Beyond this, a `tools/call` runs on
/// the read loop as it always did — the point is that `ping` and `tools/list`
/// stay answerable, not unbounded parallelism.
const MAX_CONCURRENT_CONVERSIONS: usize = 4;

/// Shared stdout. Every response is written under this lock so two workers can
/// never interleave halves of a line into the protocol stream.
type SharedOut = std::sync::Arc<std::sync::Mutex<std::io::Stdout>>;

/// Runs the server until stdin closes. A broken stdout pipe (client died) is a
/// clean exit, not an error.
///
/// `tools/call` runs on a worker thread; everything else is answered inline.
/// The loop used to be strictly sequential, so a large PDF or repository
/// conversion blocked `ping` — and a client that pings on a timer concludes the
/// server is dead. Only conversions are slow, so only they need to move off the
/// read loop, which keeps the handshake ordering exactly as it was.
pub fn serve() -> Result<()> {
    use std::io::BufRead;
    let stdin = std::io::stdin();
    let mut stdin = stdin.lock();
    let stdout: SharedOut = std::sync::Arc::new(std::sync::Mutex::new(std::io::stdout()));

    // Ids the client has cancelled. Per spec a cancelled request MUST NOT get a
    // response, so a worker that finishes after cancellation drops its result.
    let cancelled: std::sync::Arc<std::sync::Mutex<std::collections::HashSet<String>>> =
        std::sync::Arc::new(std::sync::Mutex::new(std::collections::HashSet::new()));
    let in_flight = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let mut workers: Vec<std::thread::JoinHandle<()>> = Vec::new();

    // Read raw bytes, not `lines()`: one invalid UTF-8 byte from a buggy client
    // used to end the whole session with "stream did not contain valid UTF-8".
    // A bad line deserves -32700, not a dead transport.
    let mut buf: Vec<u8> = Vec::new();
    loop {
        buf.clear();
        match stdin.read_until(b'\n', &mut buf) {
            Ok(0) => break, // EOF
            Ok(_) => {}
            Err(e) => {
                join_workers(workers);
                return Err(e.into());
            }
        }
        let line = String::from_utf8_lossy(&buf).into_owned();
        if line.trim().is_empty() {
            continue;
        }

        // Record cancellations before anything else so a worker still running
        // sees them.
        if let Some(id) = cancelled_request_id(&line) {
            cancelled
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .insert(id);
            continue;
        }

        // Slow path: a conversion, if we have capacity for another.
        if let Some((id, params)) = slow_tools_call(&line)
            && in_flight.load(std::sync::atomic::Ordering::SeqCst) < MAX_CONCURRENT_CONVERSIONS
        {
            in_flight.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let stdout = stdout.clone();
            let cancelled = cancelled.clone();
            let in_flight = in_flight.clone();
            workers.push(std::thread::spawn(move || {
                let response = handle_tools_call(&id, &params);
                in_flight.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
                let key = id.to_string();
                if cancelled
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .contains(&key)
                {
                    return; // cancelled: the spec forbids answering
                }
                let _ = write_line(&stdout, &response);
            }));
            // Reap finished workers so the vector can't grow without bound.
            workers.retain(|h| !h.is_finished());
            continue;
        }

        if let Some(response) = handle_line(&line)
            && let Err(e) = write_line(&stdout, &response)
        {
            join_workers(workers);
            return if e.kind() == std::io::ErrorKind::BrokenPipe {
                Ok(()) // client hung up
            } else {
                Err(e.into())
            };
        }
    }

    // Finish anything still converting before exiting, so a response is never
    // lost to EOF.
    join_workers(workers);
    Ok(())
}

fn join_workers(workers: Vec<std::thread::JoinHandle<()>>) {
    for w in workers {
        let _ = w.join();
    }
}

fn write_line(stdout: &SharedOut, response: &Value) -> std::io::Result<()> {
    use std::io::Write;
    let mut out = stdout.lock().unwrap_or_else(|e| e.into_inner());
    writeln!(out, "{}", response)?;
    out.flush()
}

/// The request id from a `notifications/cancelled`, if this line is one.
fn cancelled_request_id(line: &str) -> Option<String> {
    let msg: Value = serde_json::from_str(line).ok()?;
    if msg.get("method")?.as_str()? != "notifications/cancelled" {
        return None;
    }
    Some(msg.get("params")?.get("requestId")?.to_string())
}

/// `(id, params)` when this line is a `tools/call` — the only method slow
/// enough to be worth a thread.
fn slow_tools_call(line: &str) -> Option<(Value, Value)> {
    let msg: Value = serde_json::from_str(line).ok()?;
    if msg.get("method")?.as_str()? != "tools/call" {
        return None;
    }
    let id = msg.get("id")?.clone();
    Some((id, msg.get("params").cloned().unwrap_or(Value::Null)))
}

/// Dispatches one wire line. `None` means no reply (notifications, or replies
/// we must not send).
fn handle_line(line: &str) -> Option<Value> {
    let msg: Value = match serde_json::from_str(line) {
        Ok(v) => v,
        Err(_) => return Some(rpc_error(&Value::Null, -32700, "Parse error")),
    };
    if msg.is_array() {
        // JSON-RPC batching was removed from MCP in the 2025-06-18 revision.
        return Some(rpc_error(&Value::Null, -32600, "Batching is not supported"));
    }

    // No `method` means this is a *response* (a peer answering us), not a
    // request. We never send requests, so this should not arrive — but
    // answering it with -32601 would be us replying to a reply, which the spec
    // forbids outright.
    let method = msg.get("method").and_then(Value::as_str)?;
    let id = msg.get("id").cloned();
    let params = msg.get("params").cloned().unwrap_or(Value::Null);

    let Some(id) = id else {
        // A notification (initialized, cancelled, …): never answered.
        return None;
    };

    Some(match method {
        "initialize" => handle_initialize(&id, &params),
        "ping" => rpc_result(&id, json!({})),
        "tools/list" => handle_tools_list(&id),
        "tools/call" => handle_tools_call(&id, &params),
        _ => rpc_error(&id, -32601, &format!("Method not found: {}", method)),
    })
}

fn handle_initialize(id: &Value, params: &Value) -> Value {
    let requested = params
        .get("protocolVersion")
        .and_then(Value::as_str)
        .unwrap_or("");
    let version = if SUPPORTED_VERSIONS.contains(&requested) {
        requested
    } else {
        SUPPORTED_VERSIONS[0]
    };
    rpc_result(
        id,
        json!({
            "protocolVersion": version,
            "capabilities": { "tools": {} },
            "serverInfo": {
                "name": "filegoblin",
                "title": "filegoblin",
                "version": env!("CARGO_PKG_VERSION"),
            },
            "instructions": "Convert any file path or URL (PDF, Office, HTML, images, archives, repos, …) to clean, LLM-ready markdown with the `convert` tool.",
        }),
    )
}

fn handle_tools_list(id: &Value) -> Value {
    rpc_result(
        id,
        json!({
            "tools": [{
                "name": "convert",
                "description": "Convert a file path, directory, or URL to clean markdown. Handles PDF (incl. scanned, via OCR), Word, Excel/CSV, PowerPoint, OpenDocument, EPUB, email, images, archives, source trees, and web pages.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "source": {
                            "type": "string",
                            "description": "Absolute file path, directory, or http(s) URL to convert"
                        }
                    },
                    "required": ["source"]
                }
            }]
        }),
    )
}

fn handle_tools_call(id: &Value, params: &Value) -> Value {
    let name = params.get("name").and_then(Value::as_str).unwrap_or("");
    if name != "convert" {
        return rpc_error(id, -32602, &format!("Unknown tool: {}", name));
    }
    let Some(source) = params
        .get("arguments")
        .and_then(|a| a.get("source"))
        .and_then(Value::as_str)
    else {
        return rpc_error(id, -32602, "Missing required argument: source");
    };

    // Tool execution failures are reported in-band (isError:true) so the model
    // sees them; only protocol misuse gets JSON-RPC errors.
    match convert_source(source) {
        Ok(markdown) => rpc_result(
            id,
            json!({ "content": [{ "type": "text", "text": truncate_result(markdown) }], "isError": false }),
        ),
        Err(e) => rpc_result(
            id,
            json!({ "content": [{ "type": "text", "text": format!("Conversion failed: {:#}", e) }], "isError": true }),
        ),
    }
}

/// Bounds a tool result, marking the cut so the model knows it is partial.
fn truncate_result(mut markdown: String) -> String {
    if markdown.len() <= MAX_TOOL_RESULT_BYTES {
        return markdown;
    }
    let cut = crate::parsers::str_prefix(&markdown, MAX_TOOL_RESULT_BYTES).len();
    markdown.truncate(cut);
    markdown.push_str(&format!(
        "\n\n> [Output truncated at {} MiB. Convert a narrower target, or run the \
         filegoblin CLI directly with --split to get the whole thing.]",
        MAX_TOOL_RESULT_BYTES / (1024 * 1024)
    ));
    markdown
}

/// Rejects sources that must never reach the argument parser.
///
/// `source` is model-supplied. Splicing it in as bare argv let a value of
/// `--help` print clap's help text to **stdout** — corrupting the JSON-RPC
/// stream — and then `exit(0)` mid-request, and a value of `-` made the server
/// read its own protocol stdin to EOF and deadlock.
fn validate_source(source: &str) -> Result<()> {
    if source.trim().is_empty() {
        anyhow::bail!("`source` is empty");
    }
    if source == "-" {
        anyhow::bail!("`-` (stdin) is not a valid source for the MCP server");
    }
    if source.starts_with('-') {
        anyhow::bail!(
            "`source` must be a file path, directory, or http(s) URL — got something that \
             looks like a flag: {source}"
        );
    }
    if source.contains('\0') {
        anyhow::bail!("`source` contains a NUL byte");
    }
    Ok(())
}

/// Runs one conversion with quiet defaults. Directories get horde mode; URLs
/// never do (an unbounded crawl inside a tool call would run forever).
fn convert_source(source: &str) -> Result<String> {
    use clap::Parser;
    validate_source(source)?;
    // `--` so a source is always positional, and `try_parse_from` so a parse
    // failure is a returned error rather than clap calling `exit()` on us.
    let mut args = crate::cli::Cli::try_parse_from(["filegoblin", "--quiet", "--", source])
        .map_err(|e| anyhow::anyhow!("Could not accept that source: {}", e.kind()))?;
    if std::path::Path::new(source).is_dir() {
        args.horde = true;
    }
    crate::gobble_to_string(&[source.to_string()], &args)
}

fn rpc_result(id: &Value, result: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

fn rpc_error(id: &Value, code: i64, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initialize_echoes_known_version_and_caps() {
        let resp = handle_line(
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"t","version":"0"}}}"#,
        )
        .unwrap();
        assert_eq!(resp["result"]["protocolVersion"], "2025-06-18");
        assert!(resp["result"]["capabilities"]["tools"].is_object());
        assert_eq!(resp["result"]["serverInfo"]["name"], "filegoblin");
    }

    #[test]
    fn unknown_version_gets_our_newest() {
        let resp = handle_line(
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2099-01-01"}}"#,
        )
        .unwrap();
        assert_eq!(resp["result"]["protocolVersion"], SUPPORTED_VERSIONS[0]);
    }

    #[test]
    fn notifications_get_no_reply_and_unknown_methods_get_32601() {
        assert!(handle_line(r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#).is_none());
        let resp = handle_line(r#"{"jsonrpc":"2.0","id":7,"method":"resources/list"}"#).unwrap();
        assert_eq!(resp["error"]["code"], -32601);
        assert_eq!(resp["id"], 7);
    }

    #[test]
    fn garbage_and_batches_are_rejected_without_dying() {
        let resp = handle_line("not json at all").unwrap();
        assert_eq!(resp["error"]["code"], -32700);
        let resp = handle_line(r#"[{"jsonrpc":"2.0","id":1,"method":"ping"}]"#).unwrap();
        assert_eq!(resp["error"]["code"], -32600);
    }

    #[test]
    fn tools_call_unknown_tool_is_protocol_error() {
        let resp = handle_line(
            r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"nope","arguments":{}}}"#,
        )
        .unwrap();
        assert_eq!(resp["error"]["code"], -32602);
    }

    /// Regression: a `source` starting with `-` was spliced into argv, so
    /// `--help` printed to stdout (corrupting the protocol stream) and exited
    /// the process, and `-` made the server eat its own stdin.
    #[test]
    fn flag_like_sources_are_refused_in_band() {
        for source in ["--help", "--version", "-x", "-", ""] {
            let line = json!({
                "jsonrpc": "2.0",
                "id": 9,
                "method": "tools/call",
                "params": { "name": "convert", "arguments": { "source": source } }
            })
            .to_string();
            let resp = handle_line(&line).expect("must answer, not exit");
            assert_eq!(resp["id"], 9, "source {source:?}");
            assert_eq!(
                resp["result"]["isError"], true,
                "source {source:?} must be an in-band tool error, got: {resp}"
            );
        }
    }

    /// A peer's *response* (no `method`) must never be answered.
    #[test]
    fn responses_are_never_answered() {
        assert!(handle_line(r#"{"jsonrpc":"2.0","id":6,"result":{"ok":true}}"#).is_none());
        assert!(
            handle_line(r#"{"jsonrpc":"2.0","id":6,"error":{"code":-1,"message":"x"}}"#).is_none()
        );
    }

    /// We must not advertise a revision whose semantics we do not implement.
    #[test]
    fn batching_revision_is_not_advertised() {
        assert!(
            !SUPPORTED_VERSIONS.contains(&"2025-03-26"),
            "2025-03-26 made batching mandatory and we reject batches"
        );
    }

    #[test]
    fn oversized_results_are_truncated_with_a_marker() {
        let big = "x".repeat(MAX_TOOL_RESULT_BYTES + 5000);
        let out = truncate_result(big);
        assert!(out.len() < MAX_TOOL_RESULT_BYTES + 500);
        assert!(
            out.contains("Output truncated"),
            "got tail: {}",
            &out[out.len() - 200..]
        );
    }
}
