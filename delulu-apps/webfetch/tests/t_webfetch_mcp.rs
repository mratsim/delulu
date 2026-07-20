//! MCP integration tests for delulu-webfetch-mcp (stdio transport).
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
//! The MCP server does NOT exit on stdin EOF — it runs until killed.
//! Each test spawns the server, sends JSON-RPC requests, reads the
//! response, then drops stdin and kills the child. A 15s timeout on
//! all async operations prevents hangs.
#![cfg(test)]
#![cfg(feature = "mcp")]

mod mcp_helpers;
use mcp_helpers::*;

use anyhow::{Context, Result};
use serde_json::{json, Value};
use std::sync::Once;
use std::time::Duration;
use tokio::io::AsyncWriteExt;

const TIMEOUT: Duration = Duration::from_secs(15);

fn init_tracing() {
    static INIT: Once = Once::new();
    INIT.call_once(|| {
        tracing_subscriber::fmt()
            .with_env_filter("info")
            .init();
    });
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_mcp_server_starts_stdio() -> Result<()> {
    init_tracing();
    let path = find_binary()?;

    let (child, mut stdin, mut stdout) = spawn_stdio_server(&path).await?;

    // Send initialize request manually so we can validate the full response.
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

    let response = read_json_response(&mut stdout, TIMEOUT).await?;

    assert!(
        response.get("result").is_some(),
        "Init response should have 'result' key: {}",
        response
    );
    assert_eq!(
        response["jsonrpc"], "2.0",
        "jsonrpc should be 2.0: {}",
        response
    );
    assert!(
        response["result"].get("serverInfo").is_some(),
        "result should have serverInfo: {}",
        response
    );
    assert!(
        response["result"]["serverInfo"].get("name").is_some(),
        "serverInfo should have name key: {}",
        response
    );

    // Send initialized notification
    let initialized = b"{\"jsonrpc\":\"2.0\",\"method\":\"notifications/initialized\"}\n";
    tokio::time::timeout(TIMEOUT, stdin.write_all(initialized))
        .await
        .map_err(|_| anyhow::anyhow!("timeout writing initialized notification"))?
        .context("failed to write initialized notification")?;

    drop(stdin);
    drop(child);

    Ok(())
}

#[tokio::test]
async fn test_mcp_help_output() -> Result<()> {
    init_tracing();
    let path = find_binary()?;

    let output = std::process::Command::new(&path)
        .arg("--help")
        .output()?;

    assert!(output.status.success(), "Help should succeed");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("webfetch-mcp"),
        "Help should show binary name"
    );
    assert!(stdout.contains("stdio"), "Help should show stdio command");
    assert!(stdout.contains("http"), "Help should show http command");

    Ok(())
}

#[tokio::test]
async fn test_mcp_version_output() -> Result<()> {
    init_tracing();
    let path = find_binary()?;

    let output = std::process::Command::new(&path)
        .arg("--version")
        .output()?;

    // Binary does not support --version; output goes to stderr as an error message.
    // Test that we get a non-empty response (the error message about unknown flag).
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!stderr.is_empty(), "expected --version output on stderr");

    Ok(())
}

#[tokio::test]
async fn test_mcp_tools_list() -> Result<()> {
    init_tracing();
    let path = find_binary()?;

    let (child, mut stdin, mut stdout) = spawn_stdio_server(&path).await?;

    mcp_initialize(&mut stdin, &mut stdout)
        .await
        .context("MCP initialize failed")?;

    // Send tools/list request
    let list_req = b"{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"tools/list\"}\n";
    tokio::time::timeout(TIMEOUT, stdin.write_all(list_req))
        .await
        .map_err(|_| anyhow::anyhow!("timeout writing tools/list request"))?
        .context("failed to write tools/list request")?;

    let response = read_json_response(&mut stdout, TIMEOUT).await?;

    let tools = response["result"]["tools"]
        .as_array()
        .context("tools should be an array")?;
    let tool_names: Vec<&str> = tools
        .iter()
        .filter_map(|t| t["name"].as_str())
        .collect();
    assert!(
        tool_names.contains(&"webfetch"),
        "tools/list should contain webfetch tool, got: {:?}",
        tool_names
    );
    assert!(
        tool_names.contains(&"webfetch_raw"),
        "tools/list should contain webfetch_raw tool, got: {:?}",
        tool_names
    );

    drop(stdin);
    drop(child);

    Ok(())
}

#[tokio::test]
#[ignore] // network-dependent — requires live https://example.com
async fn test_mcp_webfetch_tool() -> Result<()> {
    init_tracing();
    let path = find_binary()?;

    let (child, mut stdin, mut stdout) = spawn_stdio_server(&path).await?;

    mcp_initialize(&mut stdin, &mut stdout)
        .await
        .context("MCP initialize failed")?;

    send_tool_call(&mut stdin, "webfetch", json!({"url": "https://example.com"}))
        .await
        .context("failed to send webfetch tool call")?;

    let response = read_json_response(&mut stdout, TIMEOUT).await?;

    let text = response["result"]["content"][0]["text"]
        .as_str()
        .context("response should have text content")?;
    assert!(
        text.starts_with("---"),
        "Response should start with YAML frontmatter (---), got: {}",
        &text[..text.len().min(100)]
    );

    drop(stdin);
    drop(child);

    Ok(())
}

#[tokio::test]
#[ignore] // network-dependent — requires live https://example.com
async fn test_mcp_webfetch_raw_tool() -> Result<()> {
    init_tracing();
    let path = find_binary()?;

    let (child, mut stdin, mut stdout) = spawn_stdio_server(&path).await?;

    mcp_initialize(&mut stdin, &mut stdout)
        .await
        .context("MCP initialize failed")?;

    send_tool_call(
        &mut stdin,
        "webfetch_raw",
        json!({"url": "https://example.com"}),
    )
    .await
    .context("failed to send webfetch_raw tool call")?;

    let response = read_json_response(&mut stdout, TIMEOUT).await?;

    let text = response["result"]["content"][0]["text"]
        .as_str()
        .context("response should have text content")?;
    let inner: Value =
        serde_json::from_str(text).context("response text should be valid JSON")?;
    let has_variant = match inner {
        Value::Object(obj) => {
            obj.contains_key("GenericHtml")
                || obj.contains_key("Reddit")
                || obj.contains_key("Discourse")
        }
        other => panic!(
            "Expected JSON object with ExtractionResult variant, got: {}",
            serde_json::to_string_pretty(&other).unwrap_or_default()
        ),
    };
    assert!(
        has_variant,
        "Response should contain an ExtractionResult variant key (GenericHtml, Reddit, or Discourse)"
    );

    drop(stdin);
    drop(child);

    Ok(())
}

#[tokio::test]
#[ignore] // network-dependent — requires live https://example.com
async fn test_mcp_webfetch_invalid_url() -> Result<()> {
    init_tracing();
    let path = find_binary()?;

    let (child, mut stdin, mut stdout) = spawn_stdio_server(&path).await?;

    mcp_initialize(&mut stdin, &mut stdout)
        .await
        .context("MCP initialize failed")?;

    send_tool_call(
        &mut stdin,
        "webfetch",
        json!({"url": "ftp://invalid.example.com"}),
    )
    .await
    .context("failed to send webfetch tool call")?;

    let response = read_json_response(&mut stdout, TIMEOUT).await?;

    let text = response["result"]["content"][0]["text"]
        .as_str()
        .context("response should have text content")?;
    assert!(
        text.contains("error: true"),
        "Response should contain 'error: true' in frontmatter, got: {}",
        &text[..text.len().min(200)]
    );

    drop(stdin);
    drop(child);

    Ok(())
}
