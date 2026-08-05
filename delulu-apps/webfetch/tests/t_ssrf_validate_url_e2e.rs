//! SSRF validation e2e tests for delulu-webfetch-mcp (stdio transport).
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
//!
//! # Test Pattern
//!
//! Test-first SSRF string contract (INV-007): these two exact rejection
//! strings are captured from the binary's `validate_url` implementation and
//! pinned here byte-for-byte. The MCP
//! server is spawned over stdio WITHOUT `--expose-local-networks`; the
//! `webfetch` tool must reject a loopback URL with the detailed private-IP
//! message and a non-resolvable `.invalid` domain with the generic message.
//! The server does NOT exit on stdin EOF — it runs until killed. Each test
//! spawns the server, sends JSON-RPC requests, reads the response, then drops
//! stdin and kills the child. A 30s timeout on all async operations prevents
//! hangs.
#![cfg(test)]
#![cfg(feature = "mcp")]

mod mcp_helpers;
use mcp_helpers::*;

use anyhow::{Context, Result};
use serde_json::json;
use std::time::Duration;

const TIMEOUT: Duration = Duration::from_secs(30);

/// Exact private-IP rejection string (INV-007) — copied byte-for-byte from
/// `main_mcp.rs::validate_url` (pre-move).
const DETAILED: &str = "URL resolves to a private IP address which is blocked by default. Use --expose-local-networks to allow fetching from local/private networks.";

/// Exact generic rejection string (INV-007) — copied byte-for-byte from
/// `main_mcp.rs::validate_url` (pre-move).
const GENERIC: &str = "DNS resolution failed";

/// Call the `webfetch` tool with the given URL and return the tool's text
/// payload. A tool `Err(String)` is surfaced by rmcp as
/// `result.content[0].text` with `isError: true`; JSON-RPC-level errors are
/// read from `error.message` as a fallback.
async fn call_webfetch_tool(
    stdin: &mut tokio::process::ChildStdin,
    stdout: &mut tokio::process::ChildStdout,
    url: &str,
) -> Result<String> {
    send_tool_call(stdin, "webfetch", json!({"url": url}))
        .await
        .context("failed to send webfetch tool call")?;
    let response = read_json_response(stdout, TIMEOUT, Some(2)).await?;
    if let Some(text) = response["result"]["content"][0]["text"].as_str() {
        return Ok(text.to_string());
    }
    if let Some(message) = response["error"]["message"].as_str() {
        return Ok(message.to_string());
    }
    anyhow::bail!("unexpected response shape: {}", response)
}

#[tokio::test]
async fn test_webfetch_blocks_loopback_with_detailed_message() -> Result<()> {
    let path = find_binary()?;
    let (child, mut stdin, mut stdout) = spawn_stdio_server(&path).await?;

    mcp_initialize(&mut stdin, &mut stdout)
        .await
        .context("MCP initialize failed")?;

    let message = call_webfetch_tool(&mut stdin, &mut stdout, "http://127.0.0.1/x")
        .await
        .context("webfetch tool call failed")?;

    assert_eq!(
        message, DETAILED,
        "loopback URL must be rejected with the exact INV-007 detailed string"
    );

    drop(stdin);
    drop(child);

    Ok(())
}

#[tokio::test]
async fn test_webfetch_nonexistent_domain_gets_generic_message() -> Result<()> {
    let path = find_binary()?;
    let (child, mut stdin, mut stdout) = spawn_stdio_server(&path).await?;

    mcp_initialize(&mut stdin, &mut stdout)
        .await
        .context("MCP initialize failed")?;

    let message = call_webfetch_tool(&mut stdin, &mut stdout, "http://nonexistent.invalid/x")
        .await
        .context("webfetch tool call failed")?;

    assert_eq!(
        message, GENERIC,
        "non-resolvable domain must be rejected with the exact INV-007 generic string"
    );

    drop(stdin);
    drop(child);

    Ok(())
}
