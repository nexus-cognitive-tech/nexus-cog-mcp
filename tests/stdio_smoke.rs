//! End-to-end smoke test for the stdio MCP transport.
//!
//! Spawns `nexus-cog-mcp-server`, performs the initialize handshake,
//! emits the mandatory `notifications/initialized`, then lists tools
//! and calls `cortex_explain`. The test uses a temporary workspace
//! (kept alive for the duration of the test via [`tempfile::TempDir::keep`])
//! so it does not interfere with any default DB.
//!
//! ## Framing
//!
//! The rmcp 2.2 stdio transport speaks **newline-delimited JSON**
//! (one JSON-RPC frame per line). Earlier revisions of this test
//! used the LSP-style `Content-Length: N\r\n\r\n<body>` framing
//! borrowed from the MCP HTTP transport, which the rmcp stdio
//! transport silently drops as `unparsable incoming message` and
//! then hangs because no initialize handshake ever lands.

use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use serde_json::{json, Value};

struct StdioClient {
    stdin: ChildStdin,
    reader: BufReader<ChildStdout>,
}

impl StdioClient {
    fn spawn(binary: PathBuf, workspace: PathBuf) -> (Self, Child) {
        let mut child = Command::new(binary)
            .env("NEXUS_COG_MCP_DEFAULT_WORKSPACE", workspace)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn nexus-cog-mcp-server");

        let stdin = child.stdin.take().expect("stdin");
        let stdout = child.stdout.take().expect("stdout");
        (
            Self {
                stdin,
                reader: BufReader::new(stdout),
            },
            child,
        )
    }

    /// Write one JSON-RPC frame as a single line followed by `\n`.
    ///
    /// `id` is `None` for notifications (frames that carry no
    /// response — `notifications/initialized`, cancelled requests,
    /// etc.). Notifications are mandatory in MCP 2024-11-05: the
    /// server will not service `tools/list` until it has received
    /// the `notifications/initialized` frame following the
    /// `initialize` handshake.
    fn send(&mut self, method: &str, params: Value, id: Option<i64>) {
        let mut msg = json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
        });
        if let Some(id) = id {
            msg.as_object_mut()
                .expect("object")
                .insert("id".into(), json!(id));
        }
        let mut line = msg.to_string();
        line.push('\n');
        self.stdin
            .write_all(line.as_bytes())
            .expect("write frame");
        self.stdin.flush().expect("flush stdin");
    }

    /// Read until we see a JSON-RPC response with `id == expected_id`.
    ///
    /// Notifications and other server-pushed frames are skipped so
    /// callers can iterate request / response pairs linearly. Any
    /// JSON-RPC error frame (carrying `error` instead of `result`)
    /// causes a panic with the full body — easier to diagnose than
    /// a silent hang on the next read.
    fn recv_response(&mut self, expected_id: i64) -> Value {
        loop {
            let frame = self.recv_frame();
            if frame.get("id").is_none() {
                // Notification — skip.
                continue;
            }
            let id = frame["id"].as_i64().unwrap_or(-1);
            assert_eq!(
                id, expected_id,
                "unexpected response id; frame={frame}"
            );
            if let Some(err) = frame.get("error") {
                panic!("server returned JSON-RPC error for id={expected_id}: {err}");
            }
            return frame;
        }
    }

    fn recv_frame(&mut self) -> Value {
        let mut line = String::new();
        loop {
            line.clear();
            let n = self
                .reader
                .read_line(&mut line)
                .expect("read line from server");
            if n == 0 {
                panic!("server closed stdout before sending a frame");
            }
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            return serde_json::from_str(trimmed)
                .unwrap_or_else(|e| panic!("invalid JSON from server: {e}; line={trimmed:?}"));
        }
    }
}

#[test]
fn stdio_handshake_and_tool_call() {
    let binary = PathBuf::from(env!("CARGO_BIN_EXE_nexus-cog-mcp-server"));
    let tmp = tempfile::tempdir().expect("tempdir");
    let workspace = tmp.keep();

    let (mut client, mut child) = StdioClient::spawn(binary, workspace);

    // 1. initialize (request / response)
    client.send(
        "initialize",
        json!({
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": { "name": "test", "version": "0.1" }
        }),
        Some(1),
    );
    let init = client.recv_response(1);
    assert_eq!(
        init["result"]["protocolVersion"],
        "2024-11-05",
        "initialize failed: {init}"
    );

    // 2. notifications/initialized — mandatory per MCP 2024-11-05.
    //    Without it the rmcp server refuses to service tools/list.
    client.send("notifications/initialized", json!({}), None);

    // 3. tools/list
    client.send("tools/list", json!({}), Some(2));
    let list = client.recv_response(2);
    let tools = list["result"]["tools"].as_array().expect("tools array");
    let names: Vec<&str> = tools
        .iter()
        .map(|t| t["name"].as_str().expect("tool name"))
        .collect();
    assert!(
        names.contains(&"cortex_explain"),
        "cortex_explain missing: {names:?}"
    );
    assert!(
        names.contains(&"intel_store"),
        "intel_store missing: {names:?}"
    );

    // 4. tools/call — cortex_explain (no params)
    client.send(
        "tools/call",
        json!({
            "name": "cortex_explain",
            "arguments": {}
        }),
        Some(3),
    );
    let call = client.recv_response(3);
    let content = call["result"]["content"]
        .as_array()
        .expect("content array");
    assert!(!content.is_empty(), "tool returned empty content: {call}");

    // 5. clean shutdown. Drop stdin to send EOF, then poll for the
    //    child to exit; if it does not exit on its own (rmcp stdio
    //    servers do not always react to EOF promptly), force-kill
    //    and reap. Using `wait` without a timeout would hang the
    //    test forever if the server is wedged on its read loop.
    drop(client.stdin);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) => {
                if std::time::Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
            Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                break;
            }
        }
    }
}
