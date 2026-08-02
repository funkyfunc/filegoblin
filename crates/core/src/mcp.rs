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
const SUPPORTED_VERSIONS: &[&str] = &["2025-11-25", "2025-06-18", "2025-03-26", "2024-11-05"];

/// Runs the server until stdin closes. A broken stdout pipe (client died) is a
/// clean exit, not an error.
pub fn serve() -> Result<()> {
    use std::io::{BufRead, Write};
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();

    for line in stdin.lock().lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        if let Some(response) = handle_line(&line) {
            let write = writeln!(stdout, "{}", response).and_then(|_| stdout.flush());
            if let Err(e) = write {
                if e.kind() == std::io::ErrorKind::BrokenPipe {
                    return Ok(());
                }
                return Err(e.into());
            }
        }
    }
    Ok(())
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

    let method = msg.get("method").and_then(Value::as_str).unwrap_or("");
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
            json!({ "content": [{ "type": "text", "text": markdown }], "isError": false }),
        ),
        Err(e) => rpc_result(
            id,
            json!({ "content": [{ "type": "text", "text": format!("Conversion failed: {:#}", e) }], "isError": true }),
        ),
    }
}

/// Runs one conversion with quiet defaults. Directories get horde mode; URLs
/// never do (an unbounded crawl inside a tool call would run forever).
fn convert_source(source: &str) -> Result<String> {
    use clap::Parser;
    let mut args = crate::cli::Cli::parse_from(["filegoblin", "--quiet", source]);
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
}
