//!  Delulu All-MCP — shared subprocess MCP test harness
//!
//!  Copyright (C) 2026  Mamy Ratsimbazafy
//!
//!  This program is free software: you can redistribute it and/or modify
//!  it under the terms of the GNU Affero General Public License as published by
//!  the Free Software Foundation, either version 3 of the License, or
//!  (at your option) any later version.
//!
//!  This program is distributed in the hope that it will be useful,
//!  but WITHOUT ANY WARRANTY; without even the implied warranty of
//!  MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
//!  GNU Affero General Public License for more details.
//!
//!  You should have received a copy of the GNU Affero General Public License
//!  along with this program.  If not, see <http://www.gnu.org/licenses/>.
//!

//! Shared subprocess harness for the all-mcp e2e suites.
//!
//! Modeled on the webfetch `tests/mcp_helpers.rs` pattern, but **release-pinned**:
//! [`find_binary`] resolves binaries only under `target/release`.
//! Reusing a stale `debug` binary would falsify the tool-name matrix, so this
//! copy deliberately differs from the debug-first travel/webfetch helpers.
//!
//! Provides `find_binary`, `spawn_stdio_server`, `stream_stderr_to_console`,
//! `mcp_initialize`, `send_tool_call`, `read_json_response`, and `list_tools`.
//!
//! Top-level `tests/*.rs` files are auto-discovered, so this helper needs no
//! `[[test]]` declaration in `Cargo.toml`.

#![allow(dead_code)] // shared harness: not every helper is used by every test target

use anyhow::{Context, Result};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStderr, ChildStdin, ChildStdout, Command};
use tokio::task::JoinHandle;

const TIMEOUT: Duration = Duration::from_secs(30);

/// Locate a delulu MCP binary by name, pinned to `target/release`.
///
/// # Pre
/// `bin_name` is the stem of a release binary, e.g. `"delulu-all-mcp"`.
///
/// # Post
/// Returns the path `target/release/<bin_name>` if the file exists.
///
/// # Panic-if
/// Returns `Err` if the release binary is missing, with a message telling the
/// developer to run `cargo build --release`.
pub fn find_binary(bin_name: &str) -> Result<PathBuf> {
    let manifest_dir = PathBuf::from(
        std::env::var("CARGO_MANIFEST_DIR")
            .map_err(|e| anyhow::anyhow!("CARGO_MANIFEST_DIR not set: {}", e))?,
    );
    let workspace_root = manifest_dir
        .parent()
        .and_then(|p| p.parent())
        .ok_or_else(|| anyhow::anyhow!("Could not determine workspace root"))?;

    let path = workspace_root.join("target/release").join(bin_name);
    if path.exists() {
        return Ok(path);
    }
    anyhow::bail!(
        "Binary {} not found at {:?}. Run 'cargo build --release --features mcp' first (a stale debug binary would falsify the tool-name matrix).",
        bin_name,
        path
    )
}

/// Spawn the MCP server with `stdio` transport, returning the child process
/// and its piped stdin/stdout.
///
/// # Pre
/// `path` points at a built MCP server binary.
///
/// # Post
/// The subprocess is running with `stdio`, `http` not used; stdin/stdout are
/// piped and stderr is forwarded to the console.
///
/// # Panic-if
/// Returns `Err` on spawn failure.
/// Spawn the MCP server with `stdio` transport plus extra leading CLI args,
/// returning the child process and its piped stdin/stdout.
///
/// # Pre
/// `path` points at a built MCP server binary; `extra_args` are flags
/// placed before the `stdio` subcommand (e.g. `--expose-local-networks`).
///
/// # Post
/// The subprocess is running with the given args followed by `stdio`;
/// stdin/stdout are piped and stderr is forwarded to the console.
///
/// # Panic-if
/// Returns `Err` on spawn failure.
pub async fn spawn_stdio_server_with_args(
    path: &Path,
    extra_args: &[&str],
) -> Result<(Child, ChildStdin, ChildStdout)> {
    let mut cmd = Command::new(path);
    cmd.args(extra_args)
        .arg("stdio")
        .stdout(Stdio::piped())
        .stdin(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let mut child = cmd.spawn()?;
    let stderr = child.stderr.take().unwrap();
    let stdout = child.stdout.take().unwrap();
    let stdin = child.stdin.take().unwrap();
    std::mem::drop(stream_stderr_to_console(stderr));
    Ok((child, stdin, stdout))
}
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
    std::mem::drop(stream_stderr_to_console(stderr));
    Ok((child, stdin, stdout))
}

/// Spawn a tokio task that reads stderr line by line and forwards to `eprintln!`.
///
/// # Pre
/// `stderr` is the piped stderr of a spawned child.
///
/// # Post
/// Stderr is streamed to the console until the pipe closes or a read error.
///
/// # Panic-if
/// Never.
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

/// Send `initialize` and `notifications/initialized` to the MCP server.
///
/// # Pre
/// `stdin`/`stdout` belong to a freshly spawned MCP server.
///
/// # Post
/// The server has completed protocol negotiation; a JSON-RPC `initialize`
/// response with `id: 1` was consumed.
///
/// # Panic-if
/// Returns `Err` on timeout or invalid response.
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

/// Send a `tools/list` request and return the sorted tool names.
///
/// # Pre
/// The server has been initialized (or the request will (re)initialize if not
/// already done).
///
/// # Post
/// Returns the sorted list of tool names from `result.tools[].name`.
///
/// # Panic-if
/// Returns `Err` on invalid response or missing `result.tools`.
pub async fn list_tools(
    stdin: &mut ChildStdin,
    stdout: &mut ChildStdout,
    initialized: &mut bool,
) -> Result<Vec<String>> {
    if !*initialized {
        mcp_initialize(stdin, stdout).await?;
        *initialized = true;
    }
    let list = json!({
        "jsonrpc": "2.0",
        "id": 100,
        "method": "tools/list",
        "params": {}
    });
    let mut list_str = list.to_string();
    list_str.push('\n');
    tokio::time::timeout(TIMEOUT, stdin.write_all(list_str.as_bytes()))
        .await
        .map_err(|_| anyhow::anyhow!("timeout writing tools/list request"))?
        .context("failed to write tools/list request")?;

    let response = read_json_response(stdout, TIMEOUT, Some(100)).await?;
    let tools = response
        .get("result")
        .and_then(|r| r.get("tools"))
        .and_then(|t| t.as_array())
        .ok_or_else(|| anyhow::anyhow!("tools/list response missing result.tools: {}", response))?;
    let mut names: Vec<String> = tools
        .iter()
        .filter_map(|t| t.get("name").and_then(|n| n.as_str()).map(String::from))
        .collect();
    names.sort();
    Ok(names)
}

/// Send a `tools/list` request and return the full tool entries.
///
/// # Pre
/// The server has been initialized (or the request will (re)initialize if not
/// already done).
///
/// # Post
/// Returns the `result.tools` array of full tool objects (each carrying
/// `name`, `description`, and `inputSchema`).
///
/// # Panic-if
/// Returns `Err` on invalid response or missing `result.tools`.
pub async fn list_tools_entries(
    stdin: &mut ChildStdin,
    stdout: &mut ChildStdout,
    initialized: &mut bool,
) -> Result<Vec<Value>> {
    if !*initialized {
        mcp_initialize(stdin, stdout).await?;
        *initialized = true;
    }
    let list = json!({
        "jsonrpc": "2.0",
        "id": 100,
        "method": "tools/list",
        "params": {}
    });
    let mut list_str = list.to_string();
    list_str.push('\n');
    tokio::time::timeout(TIMEOUT, stdin.write_all(list_str.as_bytes()))
        .await
        .map_err(|_| anyhow::anyhow!("timeout writing tools/list request"))?
        .context("failed to write tools/list request")?;

    let response = read_json_response(stdout, TIMEOUT, Some(100)).await?;
    let tools = response
        .get("result")
        .and_then(|r| r.get("tools"))
        .and_then(|t| t.as_array())
        .ok_or_else(|| {
            anyhow::anyhow!("tools/list response missing result.tools: {}", response)
        })?;
    Ok(tools.clone())
}

/// Send a `tools/call` JSON-RPC request with `"id": 2`.
///
/// # Pre
/// `name` is a tool exposed by the server.
///
/// # Post
/// The request is written to the pipe.
///
/// # Panic-if
/// Returns `Err` on write timeout or error.
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
/// # Pre
/// `stdout` is the piped stdout of a spawned MCP server.
///
/// # Post
/// Returns the first response line matching `expected_id` (or any response with
/// a `result`/`error` when `expected_id` is `None`); notifications are skipped.
///
/// # Panic-if
/// Returns `Err` on EOF, read error, or timeout.
pub async fn read_json_response(
    stdout: &mut ChildStdout,
    timeout: Duration,
    expected_id: Option<u64>,
) -> Result<Value> {
    let mut buf = [0u8; 4096];
    let mut line_buf = String::new();

    loop {
        line_buf.clear();
        // Read one byte at a time to build a newline-delimited line.
        loop {
            let read_result = tokio::time::timeout(timeout, stdout.read(&mut buf[..1])).await;
            match read_result {
                Ok(Ok(0)) => break, // EOF
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

        if let Ok(response) = serde_json::from_str::<Value>(&line_buf)
            && let Some(obj) = response.as_object()
        {
            // Skip notifications (no "id" field).
            if !obj.contains_key("id") {
                continue;
            }
            // If expected_id is set, skip responses that don't match.
            if let Some(eid) = expected_id
                && obj.get("id").and_then(|v| v.as_u64()) != Some(eid)
            {
                continue;
            }
            if obj.contains_key("result") || obj.contains_key("error") {
                return Ok(response);
            }
        }
    }
}