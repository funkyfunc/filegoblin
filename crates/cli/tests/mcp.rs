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
        format!(
            r#"{{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{{"name":"convert","arguments":{{"source":"{}"}}}}}}"#,
            repo_root.join("test_assets/dummy.csv").display()
        ),
        r#"{"jsonrpc":"2.0","id":4,"method":"resources/list"}"#.to_string(),
        r#"{"jsonrpc":"2.0","id":5,"method":"ping"}"#.to_string(),
        "this is not json".to_string(),
        r#"{"jsonrpc":"2.0","id":6,"method":"tools/call","params":{"name":"convert","arguments":{"source":"/definitely/not/a/real/file.pdf"}}}"#.to_string(),
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
}
