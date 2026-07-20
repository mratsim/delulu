//! Shared MCP test helpers for streaming subprocess output.
//!
//! Modeled on the travel-search `mcp_helpers.rs` pattern.
//!
//! Copyright (C) 2026  Mamy Ratsimbazafy
//!
//! This program is free software: you can redistribute it and/or modify
//! it under the terms of the GNU Affero General Public License as published by
//! the Free Software Foundation, either version 3 of the License, or
//! (at your option) any later version.
//!
//! This program is distributed in the hope that it will be useful,
//! but WITHOUT ANY WARRANTY; without even the implied warranty of
//! MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
//! GNU Affero General Public License for more details.
//!
//! You should have received a copy of the GNU Affero General Public License
//! along with this program.  If not, see <http://www.gnu.org/licenses/>.

use anyhow::{Context, Result};
use serde_json::{Value, json};
use std::path::Path;
use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStderr, ChildStdin, ChildStdout, Command};
use tokio::task::JoinHandle;

const TIMEOUT: Duration = Duration::from_secs(30);

/// Locate the `delulu-webfetch-mcp` binary using `CARGO_MANIFEST_DIR`.
///
/// Prefers `target/debug/` over `target/release/`.
pub fn find_binary() -> Result<PathBuf> {
    let manifest_dir = PathBuf::from(
        std::env::var("CARGO_MANIFEST_DIR")
            .map_err(|e| anyhow::anyhow!("CARGO_MANIFEST_DIR not set: {}", e))?,
    );
    let workspace_root = manifest_dir
        .parent()
        .and_then(|p| p.parent())
        .ok_or_else(|| anyhow::anyhow!("Could not determine workspace root"))?;

    let paths = [
        workspace_root.join("target/debug/delulu-webfetch-mcp"),
        workspace_root.join("target/release/delulu-webfetch-mcp"),
    ];

    for path in &paths {
        if path.exists() {
            return Ok(path.to_path_buf());
        }
    }
    // Fallback: check PATH via `which`
    if let Ok(output) = std::process::Command::new("which")
        .arg("delulu-webfetch-mcp")
        .output()
    {
        if output.status.success() {
            let path_str = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !path_str.is_empty() {
                return Ok(PathBuf::from(path_str));
            }
        }
    }
    anyhow::bail!(
        "Binary not found. Run 'cargo build -p delulu-webfetch --features mcp' first. Searched: {:?}",
        paths
    )
}

/// Spawn the MCP server with `stdio` transport, returning the child process
/// and its piped stdin/stdout.
///
/// Stderr is forwarded to the console via `stream_stderr_to_console`.
/// Uses `kill_on_drop(true)` so the child is killed when `child` is dropped.
pub async fn spawn_stdio_server(path: &Path) -> Result<(Child, ChildStdin, ChildStdout)> {
    let mut child = Command::new(path)
        .arg("stdio")
        .stdout(Stdio::piped())
        .stdin(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()?;
    let stderr = child.stderr.take().unwrap();
    let stdout = child.stdout.take().unwrap();
    let stdin = child.stdin.take().unwrap();
    let _ = stream_stderr_to_console(stderr);
    Ok((child, stdin, stdout))
}

/// Spawn a tokio task that reads stderr line by line and forwards to `eprintln!`.
///
/// Uses `BufReader` and `AsyncBufReadExt::read_line` for line-by-line reading.
pub fn stream_stderr_to_console(stderr: ChildStderr) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut reader = BufReader::new(stderr);
        let mut line = String::new();
        loop {
            match reader.read_line(&mut line).await {
                Ok(0) => break,
                Ok(_) => {
                    eprint!("{}", line);
                    line.clear();
                }
                Err(e) => {
                    eprintln!("[mcp_helpers] stderr read error: {e}");
                    break;
                }
            }
        }
    })
}

/// Send `initialize` request and `notifications/initialized` to the MCP server.
///
/// Uses protocol version `2025-03-26` with `"id": 1`.
/// All async operations use 30-second timeouts.
pub async fn mcp_initialize(stdin: &mut ChildStdin, stdout: &mut ChildStdout) -> Result<()> {
    let init = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "protocolVersion": "2025-03-26",
            "capabilities": {},
            "clientInfo": {"name": "test-client", "version": "1.0"}
        }
    });
    let mut init_str = init.to_string();
    init_str.push('\n');

    tokio::time::timeout(TIMEOUT, stdin.write_all(init_str.as_bytes()))
        .await
        .map_err(|_| anyhow::anyhow!("timeout writing init request"))?
        .context("failed to write init request")?;

    let response = read_json_response(stdout, TIMEOUT, Some(1)).await?;
    assert_eq!(
        response["jsonrpc"], "2.0",
        "Should get JSON-RPC init response with jsonrpc 2.0: {}",
        response
    );

    let initialized = b"{\"jsonrpc\":\"2.0\",\"method\":\"notifications/initialized\"}\n";
    tokio::time::timeout(TIMEOUT, stdin.write_all(initialized))
        .await
        .map_err(|_| anyhow::anyhow!("timeout writing initialized notification"))?
        .context("failed to write initialized notification")?;

    Ok(())
}

/// Send a `tools/call` JSON-RPC request with `"id": 2`.
///
/// Uses a 30-second timeout on the underlying write to prevent hangs
/// when the OS pipe buffer is full or the server is unresponsive.
pub async fn send_tool_call(stdin: &mut ChildStdin, name: &str, args: Value) -> Result<()> {
    let call = json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "tools/call",
        "params": {"name": name, "arguments": args}
    });
    let mut call_str = call.to_string();
    call_str.push('\n');
    tokio::time::timeout(TIMEOUT, stdin.write_all(call_str.as_bytes()))
        .await
        .map_err(|_| anyhow::anyhow!("timeout sending tool call"))?
        .context("failed to write tool call")?;
    Ok(())
}

/// Read a complete JSON-RPC response from stdout incrementally.
///
/// Parses newline-delimited JSON. Ignores notifications (no `id` field).
/// If `expected_id` is `Some(n)`, only returns a response whose `id` matches.
/// Returns `Err(anyhow!("timeout reading response after {timeout}s"))` on timeout.
pub async fn read_json_response(
    stdout: &mut ChildStdout,
    timeout: Duration,
    expected_id: Option<u64>,
) -> Result<Value> {
    let mut buf = [0u8; 4096];
    let mut line_buf = String::new();

    loop {
        line_buf.clear();
        // Read one byte at a time to build a line (newline-delimited JSON)
        loop {
            let read_result = tokio::time::timeout(timeout, stdout.read(&mut buf[..1])).await;
            match read_result {
                Ok(Ok(0)) => break, // EOF
                Ok(Ok(n)) if n == 0 => break,
                Ok(Ok(_)) => {
                    let ch = buf[0] as char;
                    if ch == '\n' {
                        break; // end of line
                    }
                    line_buf.push(ch);
                }
                Ok(Err(e)) => anyhow::bail!("read error: {e}"),
                Err(_) => anyhow::bail!("timeout reading response after {}s", timeout.as_secs()),
            }
        }

        if line_buf.is_empty() {
            anyhow::bail!("Stdout closed without receiving a valid JSON-RPC response");
        }

        if let Ok(response) = serde_json::from_str::<Value>(&line_buf) {
            if let Some(obj) = response.as_object() {
                // Skip notifications (no "id" field)
                if !obj.contains_key("id") {
                    continue;
                }
                // If expected_id is set, skip responses that don't match
                if let Some(eid) = expected_id {
                    if obj.get("id").and_then(|v| v.as_u64()) != Some(eid) {
                        continue;
                    }
                }
                if obj.contains_key("result") || obj.contains_key("error") {
                    return Ok(response);
                }
            }
        }
    }
}
