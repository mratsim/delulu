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

//! MCP integration tests for delulu-websearch-mcp (stdio transport).
//!
//! # Test Pattern
//!
//! The MCP server does NOT exit on stdin EOF — it runs until killed.
//! Each test spawns the server, sends JSON-RPC requests, reads the
//! response, then drops stdin and kills the child. A 30s timeout on
//! all async operations prevents hangs.
//!
//! Phase 1: Smoke tests (non-live) — server starts, help, version, tools/list.
//! Phase 3: E2E orchestrator tests (#[ignore], live) — spawn Python test scripts.
#![cfg(test)]
#![cfg(feature = "mcp")]

mod mcp_helpers;
use mcp_helpers::*;

use anyhow::{Context, Result};
use serde_json::json;
use std::sync::Once;
use std::time::Duration;
use tokio::io::AsyncWriteExt;

const TIMEOUT: Duration = Duration::from_secs(30);

fn init_tracing() {
    static INIT: Once = Once::new();
    INIT.call_once(|| {
        tracing_subscriber::fmt().with_env_filter("info").init();
    });
}

/// Find a Python interpreter that can run the MCP test scripts.
///
/// Prefers `uv` when available (handles pyproject.toml in tests/),
/// then `python3`, then `python`.
/// Returns the command name and any prefix args needed before the script path.
fn find_python() -> (&'static str, Vec<&'static str>) {
    if std::process::Command::new("uv")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
    {
        (
            "uv",
            vec!["run", "--directory", "tests/manual/end-to-end", "python3"],
        )
    } else if std::process::Command::new("python3")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
    {
        ("python3", vec![])
    } else {
        ("python", vec![])
    }
}

// ---------------------------------------------------------------------------
// Phase 1: Smoke tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_mcp_help_output() -> Result<()> {
    init_tracing();
    let path = find_binary()?;

    let output = std::process::Command::new(&path).arg("--help").output()?;

    assert!(output.status.success(), "Help should succeed");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("stdio"), "Help should show stdio command");
    assert!(stdout.contains("http"), "Help should show http command");

    Ok(())
}

#[tokio::test]
async fn test_mcp_version_output() -> Result<()> {
    init_tracing();
    let path = find_binary()?;

    // Uses separate Command::output() with Stdio::piped() for both streams.
    let output = std::process::Command::new(&path)
        .arg("--version")
        .output()?;

    // Combined stdout+stderr should be non-empty.
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        !combined.is_empty(),
        "expected --version output on stdout or stderr"
    );

    Ok(())
}

#[tokio::test]
async fn test_mcp_server_starts_stdio() -> Result<()> {
    init_tracing();
    let path = find_binary()?;

    let (_child, mut stdin, mut stdout) = spawn_stdio_server(&path).await?;

    // Send initialize request manually so we can validate the full response.
    let init = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "protocolVersion": PROTOCOL_VERSION,
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

    let response = read_json_response(&mut stdout, TIMEOUT, Some(1)).await?;

    // Validate JSON-RPC response structure
    assert_eq!(
        response["jsonrpc"], "2.0",
        "jsonrpc should be 2.0: {}",
        response
    );
    assert_eq!(response["id"], 1, "id should be 1: {}", response);
    let server_name = response["result"]["serverInfo"]["name"]
        .as_str()
        .context("result.serverInfo.name should be a non-empty string")?;
    assert!(
        !server_name.is_empty(),
        "serverInfo.name should be non-empty"
    );
    assert_eq!(
        response["result"]["protocolVersion"], PROTOCOL_VERSION,
        "protocolVersion should match PROTOCOL_VERSION"
    );

    // Send initialized notification
    let initialized = b"{\"jsonrpc\":\"2.0\",\"method\":\"notifications/initialized\"}\n";
    tokio::time::timeout(TIMEOUT, stdin.write_all(initialized))
        .await
        .map_err(|_| anyhow::anyhow!("timeout writing initialized notification"))?
        .context("failed to write initialized notification")?;

    Ok(())
}

#[tokio::test]
async fn test_mcp_tools_list() -> Result<()> {
    init_tracing();
    let path = find_binary()?;

    let (_child, mut stdin, mut stdout) = spawn_stdio_server(&path).await?;

    // Send initialize request
    let init = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "protocolVersion": PROTOCOL_VERSION,
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

    let _init_response = read_json_response(&mut stdout, TIMEOUT, Some(1)).await?;

    // Send initialized notification
    let initialized = b"{\"jsonrpc\":\"2.0\",\"method\":\"notifications/initialized\"}\n";
    tokio::time::timeout(TIMEOUT, stdin.write_all(initialized))
        .await
        .map_err(|_| anyhow::anyhow!("timeout writing initialized notification"))?
        .context("failed to write initialized notification")?;

    // Send tools/list request
    let list_req = b"{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"tools/list\"}\n";
    tokio::time::timeout(TIMEOUT, stdin.write_all(list_req))
        .await
        .map_err(|_| anyhow::anyhow!("timeout writing tools/list request"))?
        .context("failed to write tools/list request")?;

    let response = read_json_response(&mut stdout, TIMEOUT, Some(2)).await?;

    let tools = response["result"]["tools"]
        .as_array()
        .context("tools should be an array")?;
    let tool_names: Vec<&str> = tools.iter().filter_map(|t| t["name"].as_str()).collect();
    assert!(
        tool_names.contains(&"web_search"),
        "tools/list should contain web_search tool, got: {:?}",
        tool_names
    );
    assert!(
        tool_names.contains(&"web_search_next_page"),
        "tools/list should contain web_search_next_page tool, got: {:?}",
        tool_names
    );

    Ok(())
}

// ---------------------------------------------------------------------------
// Phase 3: E2E orchestrator tests (#[ignore] — live, require Python + network)
// ---------------------------------------------------------------------------

#[tokio::test]
#[ignore] // network-dependent — requires Python MCP SDK + live search engines
async fn test_mcp_e2e_stdio() -> Result<()> {
    init_tracing();

    let binary = find_binary()?;
    let binary_str = binary.to_string_lossy().to_string();

    let manifest = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let script = manifest.join("tests/manual/end-to-end/test_websearch_stdio.py");
    let script_str = script.to_string_lossy().to_string();

    // Defensive file-exists check
    if !script.exists() {
        eprintln!(
            "Python test file not found — run Phase 2/3 first: {}",
            script_str
        );
        return Ok(());
    }

    let (python, prefix_args) = find_python();
    let py_handle = tokio::task::spawn_blocking(move || {
        let mut cmd = std::process::Command::new(python);
        cmd.args(&prefix_args)
            .arg(&script_str)
            .arg(&binary_str)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());
        // Use process_group(0) for process tree cleanup on Linux
        #[cfg(target_os = "linux")]
        {
            use std::os::unix::process::CommandExt;
            cmd.process_group(0);
        }
        cmd.output()
    });

    let py_result = tokio::time::timeout(Duration::from_secs(60), py_handle)
        .await
        .context("Python test timed out after 60s")?
        .context("Python task panicked")?;

    let py_output = py_result?; // unwrap the inner io::Result

    let stdout = String::from_utf8_lossy(&py_output.stdout);
    let stderr = String::from_utf8_lossy(&py_output.stderr);
    print!("{}", stdout);
    if !stderr.is_empty() {
        eprint!("{}", stderr);
    }

    anyhow::ensure!(
        py_output.status.success(),
        "Python MCP stdio tests failed (exit: {:?})",
        py_output.status.code()
    );

    Ok(())
}

#[tokio::test]
#[ignore] // network-dependent — requires Python MCP SDK + live search engines
async fn test_mcp_e2e_http() -> Result<()> {
    init_tracing();

    let binary = find_binary()?;
    let binary_str = binary.to_string_lossy().to_string();

    let manifest = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let script = manifest.join("tests/manual/end-to-end/test_websearch_http.py");
    let script_str = script.to_string_lossy().to_string();

    // Defensive file-exists check
    if !script.exists() {
        eprintln!(
            "Python test file not found — run Phase 3 first: {}",
            script_str
        );
        return Ok(());
    }

    let (python, prefix_args) = find_python();
    let py_handle = tokio::task::spawn_blocking(move || {
        let mut cmd = std::process::Command::new(python);
        cmd.args(&prefix_args)
            .arg(&script_str)
            .arg(&binary_str)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());
        // Use process_group(0) for process tree cleanup on Linux
        #[cfg(target_os = "linux")]
        {
            use std::os::unix::process::CommandExt;
            cmd.process_group(0);
        }
        cmd.output()
    });

    let py_result = tokio::time::timeout(Duration::from_secs(60), py_handle)
        .await
        .context("Python test timed out after 60s")?
        .context("Python task panicked")?;

    let py_output = py_result?; // unwrap the inner io::Result

    let stdout = String::from_utf8_lossy(&py_output.stdout);
    let stderr = String::from_utf8_lossy(&py_output.stderr);
    print!("{}", stdout);
    if !stderr.is_empty() {
        eprint!("{}", stderr);
    }

    anyhow::ensure!(
        py_output.status.success(),
        "Python MCP HTTP tests failed (exit: {:?})",
        py_output.status.code()
    );

    Ok(())
}
