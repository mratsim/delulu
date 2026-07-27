//!  Delulu Web Search
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

//! Shared MCP test helpers for streaming subprocess output.
//!
//! Modeled on the webfetch `mcp_helpers.rs` pattern.
//! Key differences:
//! - NO `send_tool_call()` — not consumed by any test (all tool calls go through Python MCP SDK)
//! - NO `stream_stderr_to_console()` — use `Stdio::inherit()` on the Command instead of
//!   `Stdio::piped()` for stderr in smoke tests. Exception: `test_mcp_version_output` uses
//!   `Command::output()` with `Stdio::piped()` for both streams.
//! - NO `mcp_initialize()` — the MCP initialize handshake is inlined in each test that needs it.

use anyhow::Result;
use serde_json::Value;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;
use tokio::io::AsyncReadExt;
use tokio::process::{Child, ChildStdin, ChildStdout, Command};

/// MCP protocol version constant — single source of truth for all Rust tests.
/// Keep in sync with websearch_test_utils.py
pub const PROTOCOL_VERSION: &str = "2025-03-26";

/// Precondition: `CARGO_MANIFEST_DIR` is set (always true under `cargo test`).
///   Assumes `CARGO_MANIFEST_DIR` resolves to `delulu-apps/websearch/` and
///   navigates up 2 levels to the workspace root.
/// Postcondition: Returns a path to an existing executable binary, or Err.
/// Returns Err: Binary not found in any searched location.
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
        workspace_root.join("target/debug/delulu-websearch-mcp"),
        workspace_root.join("target/release/delulu-websearch-mcp"),
    ];

    for path in &paths {
        if path.exists() {
            let metadata = std::fs::metadata(path)
                .map_err(|e| anyhow::anyhow!(
                    "find_binary: failed to check metadata for {}: {}",
                    path.display(), e
                ))?;
            if metadata.permissions().mode() & 0o111 != 0 {
                return Ok(path.to_path_buf());
            }
            anyhow::bail!(
                "find_binary: {} exists but is not executable.\n  Run: chmod +x {}",
                path.display(), path.display()
            );
        }
    }
    // Fallback: check PATH via `which`
    if let Ok(output) = std::process::Command::new("which")
        .arg("delulu-websearch-mcp")
        .output()
        && output.status.success()
    {
        let path_str = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !path_str.is_empty() {
            return Ok(PathBuf::from(path_str));
        }
    }
    anyhow::bail!(
        "Binary not found. Run 'cargo build -p delulu-websearch --features mcp' first. Searched: {:?}",
        paths
    )
}

/// Precondition: `path` must be a valid executable binary (already verified by `find_binary()`).
/// Postcondition: Server process is running with stdin/stdout pipes connected.
///   - stdin: writable, used to send JSON-RPC requests
///   - stdout: readable, newline-delimited JSON responses
///   - stderr: inherited by the test process (via Stdio::inherit())
///   - kill_on_drop: true (prevents orphan processes)
/// Returns Err: `path` does not exist or is not executable.
/// Returns Err: Server process exits immediately.
pub async fn spawn_stdio_server(path: &Path) -> Result<(Child, ChildStdin, ChildStdout)> {
    let mut child = Command::new(path)
        .arg("stdio")
        .stdout(Stdio::piped())
        .stdin(Stdio::piped())
        .stderr(Stdio::inherit())
        .kill_on_drop(true)
        .spawn()?;
    let stdout = child.stdout.take().unwrap();
    let stdin = child.stdin.take().unwrap();
    Ok((child, stdin, stdout))
}

/// Precondition: `stdout` is from a spawned server with piped stdout.
///   `expected_id` must match a request previously sent via stdin.
/// Postcondition: Returns the JSON-RPC response with matching `id`, or times out.
///   - Notification messages (no `id` field) are skipped and discarded.
///   - Responses with non-matching `id` are NOT discarded (only the one matching `expected_id` is returned).
/// Returns Err: Timeout expires.
/// Returns Err: stdout stream ends unexpectedly (server crash).
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

        if let Ok(response) = serde_json::from_str::<Value>(&line_buf)
            && let Some(obj) = response.as_object()
        {
            // Skip notifications (no "id" field)
            if !obj.contains_key("id") {
                continue;
            }
            // If expected_id is set, skip responses that don't match
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
