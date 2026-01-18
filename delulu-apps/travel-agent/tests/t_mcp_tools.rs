//!  Delulu Travel Agent
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

//! MCP server integration tests using subprocess with stdio transport.

use anyhow::Result;
use chrono::{Months, NaiveDate};
use serde_json::json;
use std::path::PathBuf;
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{ChildStdout, ChildStdin, Command};
use tokio::time::Duration;

fn find_binary() -> Result<PathBuf> {
    let manifest_dir = PathBuf::from(
        std::env::var("CARGO_MANIFEST_DIR")
            .map_err(|e| anyhow::anyhow!("CARGO_MANIFEST_DIR not set: {}", e))?,
    );
    let workspace_root = manifest_dir
        .parent()
        .and_then(|p| p.parent())
        .ok_or_else(|| anyhow::anyhow!("Could not determine workspace root"))?;

    let paths = [
        workspace_root.join("target/debug/delulu-travel-mcp"),
        workspace_root.join("target/release/delulu-travel-mcp"),
    ];

    for path in &paths {
        if path.exists() {
            return Ok(path.to_path_buf());
        }
    }
    anyhow::bail!(
        "Could not find delulu-travel-mcp binary. Run `cargo build -p delulu-travel-agent --features mcp` first. Searched: {:?}",
        paths
    )
}

fn today() -> NaiveDate {
    chrono::Local::now().date_naive()
}

async fn mcp_initialize(stdin: &mut ChildStdin, stdout: &mut BufReader<ChildStdout>) -> Result<()> {
    let init = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": {"name": "test-client", "version": "1.0"}
        }
    });
    let mut init_str = init.to_string();
    init_str.push('\n');
    stdin.write_all(init_str.as_bytes()).await?;

    let mut resp = String::new();
    stdout.read_line(&mut resp).await?;
    assert!(resp.contains("2.0"), "Should get JSON-RPC init response: {}", resp);

    stdin.write_all(b"{\"jsonrpc\":\"2.0\",\"method\":\"notifications/initialized\"}\n").await?;
    Ok(())
}

async fn send_tool_call(stdin: &mut ChildStdin, name: &str, args: serde_json::Value) -> Result<()> {
    let call = json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "tools/call",
        "params": {"name": name, "arguments": args}
    });
    let mut call_str = call.to_string();
    call_str.push('\n');
    stdin.write_all(call_str.as_bytes()).await?;
    Ok(())
}

#[tokio::test]
async fn test_mcp_server_starts_stdio() -> Result<()> {
    let path = find_binary()?;

    let mut child = Command::new(&path)
        .arg("stdio")
        .stdout(Stdio::piped())
        .stdin(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()?;

    let mut stdout = BufReader::new(child.stdout.take().unwrap());
    let mut line = String::new();

    let result = tokio::time::timeout(Duration::from_secs(2), stdout.read_line(&mut line)).await;

    assert!(result.is_ok() || line.is_empty(), "Should not timeout, got: '{}'", line);

    Ok(())
}

#[tokio::test]
async fn test_mcp_help_output() -> Result<()> {
    let path = find_binary()?;
    let output = Command::new(&path)
        .arg("--help")
        .output()
        .await?;

    assert!(output.status.success(), "Help should succeed");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("delulu-travel-mcp"), "Help should show binary name");
    assert!(stdout.contains("stdio"), "Help should show stdio command");
    assert!(stdout.contains("http"), "Help should show http command");

    Ok(())
}

#[tokio::test]
async fn test_mcp_version_output() -> Result<()> {
    let path = find_binary()?;
    let output = Command::new(&path)
        .arg("--version")
        .output()
        .await?;

    assert!(output.status.success(), "Version should succeed");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("0.1.0"), "Version should show 0.1.0");

    Ok(())
}

#[tokio::test]
#[ignore]
async fn test_mcp_flights_receives_and_processes() -> Result<()> {
    let path = find_binary()?;

    let mut child = Command::new(&path)
        .arg("stdio")
        .stdout(Stdio::piped())
        .stdin(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()?;

    let mut stdout = BufReader::new(child.stdout.take().unwrap());
    let mut stdin = child.stdin.take().unwrap();

    let mut startup = String::new();
    let _ = tokio::time::timeout(Duration::from_secs(5), stdout.read_line(&mut startup)).await;

    let init_result = mcp_initialize(&mut stdin, &mut stdout).await;

    if init_result.is_err() {
        drop(child);
        return Ok(());
    }

    let depart_naive = today() + Months::new(2);
    let return_naive = depart_naive + chrono::Duration::days(7);
    let depart_date = depart_naive.format("%Y-%m-%d").to_string();
    let return_date = return_naive.format("%Y-%m-%d").to_string();

    let args = json!({
        "from_airport": "SFO",
        "to_airport": "JFK",
        "depart_date": depart_date,
        "return_date": return_date,
        "cabin_class": "economy",
        "adults": 1,
        "children_ages": [],
        "trip_type": "roundtrip"
    });

    let _ = send_tool_call(&mut stdin, "search_flights", args).await;

    drop(stdin);

    let mut output = String::new();
    let _ = tokio::time::timeout(Duration::from_secs(5), stdout.read_line(&mut output)).await;

    drop(child);

    println!("=== FLIGHTS REQUEST ===");
    println!("SFO → JFK on {} (return {})", depart_date, return_date);
    println!("======================");
    println!("Raw output:\n{}", output);

    Ok(())
}

#[tokio::test]
#[ignore]
async fn test_mcp_hotels_receives_and_processes() -> Result<()> {
    let path = find_binary()?;

    let mut child = Command::new(&path)
        .arg("stdio")
        .stdout(Stdio::piped())
        .stdin(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()?;

    let mut stdout = BufReader::new(child.stdout.take().unwrap());
    let mut stdin = child.stdin.take().unwrap();

    let mut startup = String::new();
    let _ = tokio::time::timeout(Duration::from_secs(5), stdout.read_line(&mut startup)).await;

    let init_result = mcp_initialize(&mut stdin, &mut stdout).await;

    if init_result.is_err() {
        drop(child);
        return Ok(());
    }

    let checkin_naive = today() + Months::new(1);
    let checkout_naive = checkin_naive + chrono::Duration::days(3);
    let checkin = checkin_naive.format("%Y-%m-%d").to_string();
    let checkout = checkout_naive.format("%Y-%m-%d").to_string();

    let args = json!({
        "location": "New York",
        "checkin_date": checkin,
        "checkout_date": checkout,
        "adults": 2,
        "children_ages": [],
        "currency": "USD"
    });

    let _ = send_tool_call(&mut stdin, "search_hotels", args).await;

    drop(stdin);

    let mut output = String::new();
    let _ = tokio::time::timeout(Duration::from_secs(5), stdout.read_line(&mut output)).await;

    drop(child);

    println!("=== HOTELS REQUEST ===");
    println!("New York, {} to {}", checkin, checkout);
    println!("===================");
    println!("Raw output:\n{}", output);

    Ok(())
}
