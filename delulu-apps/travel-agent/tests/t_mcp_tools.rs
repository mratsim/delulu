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

use anyhow::{Context, Result};
use chrono::{Months, NaiveDate};
use serde_json::json;
use serde_json::Value;
use std::path::PathBuf;
use std::process::Stdio;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::process::{ChildStdout, ChildStdin, ChildStderr, Command};
use tokio::time::Duration;

// MCP stdio never quits so seems like we need rely on timeout
// if we want to read stdout AND stderr since we can't send it a kill signal.
const TIMEOUT: Duration = Duration::from_secs(3);

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

async fn mcp_initialize(stdin: &mut ChildStdin, stdout: &mut ChildStdout) -> Result<()> {
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
    let mut buf = [0u8; 4096];
    let n = tokio::time::timeout(TIMEOUT, stdout.read(&mut buf)).await?
        .context("Failed to read init response")?;
    if n > 0 {
        resp = String::from_utf8_lossy(&buf[..n]).to_string();
    }
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

// MCP stdio never quits so seems like we need rely on timeout
// if we want to read stdout AND stderr since we can't send it a kill signal.
async fn read_json_response_with_timeout(stdout: &mut ChildStdout, dur: Duration) -> Result<Value> {
    let mut output = String::new();
    let mut buf = [0u8; 4096];

    loop {
        let read_result = tokio::time::timeout(dur, stdout.read(&mut buf)).await;

        match read_result {
            Ok(Ok(0)) => break,
            Ok(Ok(n)) => {
                output.push_str(&String::from_utf8_lossy(&buf[..n]));
            }
            Ok(Err(_)) => break,
            Err(_) => break,
        }
    }

    let response: Value = serde_json::from_str(&output)
        .context("Failed to parse JSON response")?;

    Ok(response)
}

async fn read_stderr_with_timeout(stderr: &mut ChildStderr, dur: Duration) -> Result<String> {
    let mut output = String::new();
    let mut buf = [0u8; 4096];

    loop {
        let read_result = tokio::time::timeout(dur, stderr.read(&mut buf)).await;

        match read_result {
            Ok(Ok(0)) => break,
            Ok(Ok(n)) => {
                output.push_str(&String::from_utf8_lossy(&buf[..n]));
            }
            Ok(Err(_)) => break,
            Err(_) => break,
        }
    }

    Ok(output)
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

    let mut stdout = child.stdout.take().unwrap();
    let mut stderr = child.stderr.take().unwrap();
    let mut stdin = child.stdin.take().unwrap();

    mcp_initialize(&mut stdin, &mut stdout).await.context("MCP initialize failed")?;

    drop(stdin);
    let stderr_output = read_stderr_with_timeout(&mut stderr, TIMEOUT).await?;
    if !stderr_output.is_empty() {
        println!("=== STDERR ===");
        println!("{}", stderr_output);
        println!("===========");
    }
    drop(child);

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
async fn test_mcp_flights() -> Result<()> {
    let path = find_binary()?;

    let mut child = Command::new(&path)
        .arg("stdio")
        .stdout(Stdio::piped())
        .stdin(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()?;

    let mut stdout = child.stdout.take().unwrap();
    let mut stderr = child.stderr.take().unwrap();
    let mut stdin = child.stdin.take().unwrap();

    mcp_initialize(&mut stdin, &mut stdout)
        .await
        .context("MCP initialize failed")?;

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
        "trip_type": "round_trip"
    });

    send_tool_call(&mut stdin, "search_flights", args)
        .await
        .context("Failed to send flight search tool call")?;

    let response = read_json_response_with_timeout(&mut stdout, TIMEOUT)
        .await
        .context("Failed to read flight search response")?;

    drop(stdin);
    let stderr_output = read_stderr_with_timeout(&mut stderr, TIMEOUT).await?;
    if !stderr_output.is_empty() {
        println!("=== STDERR ===");
        println!("{}", stderr_output);
        println!("===========");
    }
    drop(child);

    assert!(response.is_object(), "Response should be an object");
    let obj = response.as_object().unwrap();

    assert!(obj.contains_key("id"), "Response should have id");
    assert_eq!(obj["id"], 2, "Response id should be 2");

    if obj.contains_key("error") {
        let error = &obj["error"];
        println!("Got expected error response: {}", error);
        assert!(error.is_object(), "Error should be an object");
        assert!(error.as_object().unwrap().contains_key("code"), "Error should have code");
        assert!(error.as_object().unwrap().contains_key("message"), "Error should have message");
        return Ok(());
    }

    assert!(obj.contains_key("result"), "Response should have result");
    let result = &obj["result"];
    assert!(result.is_object(), "Result should be an object");

    let content = &result["content"];
    assert!(content.is_array(), "Content should be an array");
    let items = content.as_array().unwrap();
    assert!(!items.is_empty(), "Content should not be empty");

    let text_item = &items[0];
    assert!(text_item.is_object(), "First content item should be object");
    let text_obj = text_item.as_object().unwrap();
    assert_eq!(text_obj["type"], "text", "Content type should be text");

    let text = &text_obj["text"];
    assert!(text.is_string(), "Text should be string");

    let text_str = text.as_str().unwrap();
    let inner: Value = serde_json::from_str(text_str)
        .context("Failed to parse inner flight JSON")?;

    assert!(inner.is_object(), "Inner response should be object");
    let inner_obj = inner.as_object().unwrap();
    assert!(inner_obj.contains_key("itineraries"), "Should have itineraries key");
    assert!(inner_obj.contains_key("search_params"), "Should have search_params key");
    assert!(inner_obj["search_params"].is_object(), "search_params should be object");

    let search_params = &inner_obj["search_params"].as_object().unwrap();
    assert_eq!(search_params["from_airport"], "SFO", "From airport should be SFO");
    assert_eq!(search_params["to_airport"], "JFK", "To airport should be JFK");

    let itineraries = &inner_obj["itineraries"].as_array().unwrap();
    assert!(!itineraries.is_empty(), "Should have itineraries");

    println!("=== FLIGHTS REQUEST ===");
    println!("SFO → JFK on {} (return {})", depart_date, return_date);
    println!("======================");
    println!("Found {} itineraries", itineraries.len());

    let first_flights = &itineraries[0]["flights"].as_array().unwrap();
    println!("First itinerary: {} flights", first_flights.len());

    Ok(())
}

#[tokio::test]
#[ignore]
async fn test_mcp_hotels() -> Result<()> {
    let path = find_binary()?;

    let mut child = Command::new(&path)
        .arg("stdio")
        .stdout(Stdio::piped())
        .stdin(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()?;

    let mut stdout = child.stdout.take().unwrap();
    let mut stderr = child.stderr.take().unwrap();
    let mut stdin = child.stdin.take().unwrap();

    mcp_initialize(&mut stdin, &mut stdout)
        .await
        .context("MCP initialize failed")?;

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

    send_tool_call(&mut stdin, "search_hotels", args)
        .await
        .context("Failed to send hotel search tool call")?;

    let response = read_json_response_with_timeout(&mut stdout, TIMEOUT)
        .await
        .context("Failed to read hotel search response")?;

    drop(stdin);
    drop(child);
    let stderr_output = read_stderr_with_timeout(&mut stderr, TIMEOUT).await?;
    if !stderr_output.is_empty() {
        println!("=== STDERR ===");
        println!("{}", stderr_output);
        println!("===========");
    }

    assert!(response.is_object(), "Response should be an object");
    let obj = response.as_object().unwrap();

    assert!(obj.contains_key("id"), "Response should have id");
    assert_eq!(obj["id"], 2, "Response id should be 2");

    if obj.contains_key("error") {
        let error = &obj["error"];
        println!("Got expected error response: {}", error);
        assert!(error.is_object(), "Error should be an object");
        assert!(error.as_object().unwrap().contains_key("code"), "Error should have code");
        assert!(error.as_object().unwrap().contains_key("message"), "Error should have message");
        return Ok(());
    }

    assert!(obj.contains_key("result"), "Response should have result");
    let result = &obj["result"];
    assert!(result.is_object(), "Result should be an object");

    let content = &result["content"];
    assert!(content.is_array(), "Content should be an array");
    let items = content.as_array().unwrap();
    assert!(!items.is_empty(), "Content should not be empty");

    let text_item = &items[0];
    assert!(text_item.is_object(), "First content item should be object");
    let text_obj = text_item.as_object().unwrap();
    assert_eq!(text_obj["type"], "text", "Content type should be text");

    let text = &text_obj["text"];
    assert!(text.is_string(), "Text should be string");

    let text_str = text.as_str().unwrap();
    let inner: Value = serde_json::from_str(text_str)
        .context("Failed to parse inner hotel JSON")?;

    assert!(inner.is_object(), "Inner response should be object");
    let inner_obj = inner.as_object().unwrap();
    assert!(inner_obj.contains_key("hotels"), "Should have hotels key");
    assert!(inner_obj["hotels"].is_array(), "hotels should be array");

    let hotels = &inner_obj["hotels"].as_array().unwrap();
    assert!(!hotels.is_empty(), "Should have hotels");
    assert!(hotels[0].is_object(), "First hotel should be object");

    let first_hotel = hotels[0].as_object().unwrap();
    assert!(first_hotel.contains_key("name"), "Hotel should have name");
    let name = first_hotel["name"].as_str().unwrap();
    assert!(!name.is_empty(), "Hotel name should not be empty");

    println!("=== HOTELS REQUEST ===");
    println!("New York, {} to {}", checkin, checkout);
    println!("===================");
    println!("Found {} hotels", hotels.len());
    println!("First hotel: {}", name);

    Ok(())
}
