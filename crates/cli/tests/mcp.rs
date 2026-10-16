//! End-to-end MCP stdio test: drive the real binary through a full handshake
//! and assert the transport stays clean (every stdout line is a JSON object).

use std::io::Write;
use std::process::{Command, Stdio};

#[test]
fn mcp_handshake_and_convert() {
    let bin = assert_cmd::cargo::cargo_bin("filegoblin");
    let repo_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap();

    let mut child = Command::new(bin)
        .arg("--mcp")
        .current_dir(&repo_root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();

    // The server is a sequential line loop, so we can write the whole script
    // up front, close stdin, and read everything back.
    let script = [
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"test","version":"0"}}}"#.to_string(),
        r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#.to_string(),
        r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#.to_string(),
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": 3,
            "method": "tools/call",
            "params": {
                "name": "convert",
                "arguments": {
                    "source": repo_root.join("test_assets/dummy.csv").to_str().unwrap()
                }
            }
        })
        .to_string(),
        r#"{"jsonrpc":"2.0","id":4,"method":"resources/list"}"#.to_string(),
        r#"{"jsonrpc":"2.0","id":5,"method":"ping"}"#.to_string(),
        "this is not json".to_string(),
        r#"{"jsonrpc":"2.0","id":6,"method":"tools/call","params":{"name":"convert","arguments":{"source":"/definitely/not/a/real/file.pdf"}}}"#.to_string(),
        // A source that looks like a flag: this used to print clap's help to
        // *stdout*, destroying the protocol framing, and exit the process.
        r#"{"jsonrpc":"2.0","id":7,"method":"tools/call","params":{"name":"convert","arguments":{"source":"--help"}}}"#.to_string(),
        // `-` used to make the server read its own protocol stdin to EOF.
        r#"{"jsonrpc":"2.0","id":8,"method":"tools/call","params":{"name":"convert","arguments":{"source":"-"}}}"#.to_string(),
        // A peer *response* must never be answered.
        r#"{"jsonrpc":"2.0","id":99,"result":{"ok":true}}"#.to_string(),
        // Everything after the hostile inputs must still be served.
        r#"{"jsonrpc":"2.0","id":9,"method":"ping"}"#.to_string(),
    ]
    .join("\n");

    child
        .stdin
        .take()
        .unwrap()
        .write_all(script.as_bytes())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success(), "server must exit 0 on stdin EOF");

    let stdout = String::from_utf8(output.stdout).unwrap();
    let lines: Vec<serde_json::Value> = stdout
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| {
            serde_json::from_str(l).unwrap_or_else(|e| panic!("non-JSON on MCP stdout: {e}: {l:?}"))
        })
        .collect();

    let by_id = |id: i64| {
        lines
            .iter()
            .find(|v| v["id"] == id)
            .unwrap_or_else(|| panic!("no response with id {id}"))
    };

    assert_eq!(by_id(1)["result"]["protocolVersion"], "2025-06-18");
    assert_eq!(by_id(2)["result"]["tools"][0]["name"], "convert");
    // Real conversion: the CSV comes back as a markdown pipe table.
    let text = by_id(3)["result"]["content"][0]["text"].as_str().unwrap();
    assert!(text.contains("| Name | Age | Role |"), "got: {text}");
    assert_eq!(by_id(3)["result"]["isError"], false);
    // Unimplemented method → -32601, server keeps running.
    assert_eq!(by_id(4)["error"]["code"], -32601);
    assert!(by_id(5)["result"].is_object());
    // Garbage line → parse error with null id.
    assert!(lines.iter().any(|v| v["error"]["code"] == -32700));
    // Missing file → in-band tool error, not a protocol error.
    assert_eq!(by_id(6)["result"]["isError"], true);

    // Hostile sources are in-band errors, and the server survives them.
    assert_eq!(by_id(7)["result"]["isError"], true, "`--help` as a source");
    assert_eq!(by_id(8)["result"]["isError"], true, "`-` as a source");
    assert!(
        by_id(9)["result"].is_object(),
        "the server must still answer after hostile input"
    );

    // Exactly one response per request, and nothing else. Asserting only
    // by-id let a spurious reply — to the notification, or to the peer
    // response with id 99 — pass unnoticed.
    // 9 ids (1-9) + 1 parse error. The notification and the id-99 peer
    // response contribute nothing.
    assert_eq!(
        lines.len(),
        10,
        "expected 9 request responses + 1 parse error, got {}:\n{}",
        lines.len(),
        stdout
    );
    assert!(
        !lines.iter().any(|v| v["id"] == 99),
        "a JSON-RPC response must never be answered"
    );
}

/// Drives the server with a script and returns the response ids in the order
/// they were written.
fn run_script(lines: &[String]) -> Vec<serde_json::Value> {
    let bin = assert_cmd::cargo::cargo_bin("filegoblin");
    let repo_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap();
    let mut child = Command::new(bin)
        .arg("--mcp")
        .current_dir(&repo_root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(lines.join("\n").as_bytes())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str(l).expect("every stdout line must be JSON"))
        .collect()
}

/// Regression: the read loop was strictly sequential, so a large conversion
/// blocked `ping` — and a client that pings on a timer decides the server is
/// dead. Converting `crates/` takes far longer than answering a ping, so the
/// ordering here is not a close race.
#[test]
fn a_slow_conversion_does_not_block_other_requests() {
    let responses = run_script(&[
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"t","version":"0"}}}"#.to_string(),
        r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"convert","arguments":{"source":"crates"}}}"#.to_string(),
        r#"{"jsonrpc":"2.0","id":3,"method":"ping"}"#.to_string(),
    ]);

    let ids: Vec<i64> = responses.iter().filter_map(|r| r["id"].as_i64()).collect();
    assert!(ids.contains(&2), "the conversion must still be answered");
    let ping_at = ids.iter().position(|&i| i == 3).expect("ping answered");
    let convert_at = ids.iter().position(|&i| i == 2).expect("convert answered");
    assert!(
        ping_at < convert_at,
        "ping must not wait behind the conversion, got order {ids:?}"
    );
}

/// Per spec, a cancelled request MUST NOT receive a response.
#[test]
fn a_cancelled_request_gets_no_response() {
    let responses = run_script(&[
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"t","version":"0"}}}"#.to_string(),
        r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"convert","arguments":{"source":"crates"}}}"#.to_string(),
        r#"{"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":2,"reason":"user"}}"#.to_string(),
        r#"{"jsonrpc":"2.0","id":3,"method":"ping"}"#.to_string(),
    ]);

    let ids: Vec<i64> = responses.iter().filter_map(|r| r["id"].as_i64()).collect();
    assert!(
        !ids.contains(&2),
        "a cancelled request must not be answered, got {ids:?}"
    );
    assert!(ids.contains(&3), "the server must keep serving: {ids:?}");
}
