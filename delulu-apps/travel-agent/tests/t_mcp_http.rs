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

//! MCP server integration tests using HTTP transport.

#![cfg(test)]

use anyhow::{Context, Result};
use chrono::{Months, NaiveDate};
use serde_json::json;
use serde_json::Value;
use std::path::PathBuf;
use std::process::Stdio;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::process::{ChildStderr, Command};
use tokio::time::Duration;
use tracing::{debug, instrument};
use tracing_subscriber;
use tracing_subscriber::EnvFilter;

// MCP http never quits so seems like we need rely on timeout
// if we want to read stdout AND stderr since we can't send it a kill signal.
const TIMEOUT: Duration = Duration::from_secs(3);

fn init_tracing() {
    use std::sync::Once;
    static INIT: Once = Once::new();
    INIT.call_once(|| {
        tracing_subscriber::fmt()
            .with_thread_ids(true)
            .with_timer(tracing_subscriber::fmt::time::ChronoUtc::rfc_3339())
            .with_writer(std::io::stderr)
            .with_env_filter(EnvFilter::new("debug"))
            .init();
    });
}

fn get_free_port() -> u16 {
    use std::net::TcpListener;
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.local_addr().unwrap().port()
}

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

#[instrument(skip(stream))]
async fn mcp_http_initialize(stream: &mut TcpStream, port: u16) -> Result<String> {
    let init_request = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"test-client","version":"1.0"}}}"#;

    debug!("Sending initialize request...");
    let headers = format!(
        "POST /mcp HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nContent-Type: application/json\r\nAccept: application/json, text/event-stream\r\nContent-Length: {}\r\n\r\n{}",
        port,
        init_request.len(),
        init_request
    );

    stream.write_all(headers.as_bytes()).await?;
    debug!("Request sent, waiting for response...");

    let mut response = Vec::new();
    let mut buf = [0u8; 8192];
    let mut iterations = 0;

    loop {
        iterations += 1;
        match tokio::time::timeout(TIMEOUT, stream.read(&mut buf)).await {
            Ok(Ok(0)) => {
                debug!("Read EOF after {} iterations", iterations);
                break;
            }
            Ok(Ok(n)) => {
                debug!("Read {} bytes after {} iterations", n, iterations);
                response.extend_from_slice(&buf[..n]);
                let response_str = String::from_utf8_lossy(&response);
                if response_str.contains("\r\n0\r\n") || response_str.contains("\n0\n") {
                    debug!("Response complete (chunked end marker found)");
                    break;
                }
                if iterations > 10 {
                    debug!("Response complete (max iterations)");
                    break;
                }
            }
            Ok(Err(e)) => {
                debug!("Read error after {} iterations: {:?}", iterations, e);
                break;
            }
            Err(_) => {
                debug!("Timeout after {} iterations", iterations);
                break;
            }
        }
    }

    let response_str = String::from_utf8_lossy(&response);
    debug!("Response received ({} bytes): {:?}", response_str.len(), &response_str[..200.min(response_str.len())]);

    if response_str.is_empty() {
        debug!("Response is empty!");
    }

    let session_id = response_str
        .lines()
        .find(|l| l.starts_with("mcp-session-id:"))
        .map(|l| l.trim_start_matches("mcp-session-id: ").trim().to_string());

    match &session_id {
        Some(id) => debug!("Session ID found: {}", id),
        None => debug!("No session ID found in response"),
    }

    session_id.context("No session ID")
}

async fn mcp_http_send(stream: &mut TcpStream, session_id: &str, request: &str, _wait_for_id: Option<i32>) -> Result<String> {
    let headers = format!(
        "POST /mcp HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/json\r\nAccept: application/json, text/event-stream\r\nmcp-session-id: {}\r\nContent-Length: {}\r\n\r\n{}",
        session_id,
        request.len(),
        request
    );

    stream.write_all(headers.as_bytes()).await?;

    let mut response = Vec::new();
    let mut buf = [0u8; 8192];

    loop {
        match tokio::time::timeout(TIMEOUT, stream.read(&mut buf)).await {
            Ok(Ok(0)) => break,
            Ok(Ok(n)) => {
                response.extend_from_slice(&buf[..n]);
                let response_str = String::from_utf8_lossy(&response);
                if response_str.contains("\r\n0\r\n") || response_str.contains("\n0\n") {
                    break;
                }
            }
            _ => break,
        }
    }

    Ok(String::from_utf8_lossy(&response).into_owned())
}

async fn mcp_http_send_notification(stream: &mut TcpStream, session_id: &str, request: &str) -> Result<()> {
    let headers = format!(
        "POST /mcp HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/json\r\nAccept: application/json, text/event-stream\r\nmcp-session-id: {}\r\nContent-Length: {}\r\n\r\n{}",
        session_id,
        request.len(),
        request
    );

    stream.write_all(headers.as_bytes()).await?;

    stream.shutdown().await?;

    Ok(())
}

async fn read_stderr_to_string(stderr: &mut ChildStderr) -> String {
    let mut output = String::new();
    let mut buf = [0u8; 4096];
    while let Ok(n) = stderr.read(&mut buf).await {
        if n == 0 { break; }
        output.push_str(&String::from_utf8_lossy(&buf[..n]));
    }
    output
}

fn parse_sse_events(body: &str) -> Vec<String> {
    let mut events = Vec::new();
    let mut current_event = String::new();

    for line in body.lines() {
        if line.is_empty() {
            if !current_event.is_empty() {
                events.push(current_event.clone());
                current_event.clear();
            }
        } else {
            if !current_event.is_empty() {
                current_event.push_str("\n");
            }
            current_event.push_str(line);
        }
    }

    if !current_event.is_empty() {
        events.push(current_event);
    }

    events
}

fn extract_json_from_sse(event: &str) -> Option<String> {
    let data_prefix = "data: ";
    for line in event.lines() {
        if line.starts_with(data_prefix) {
            let data = line.trim_start_matches(data_prefix);
            return Some(data.to_string());
        }
    }
    None
}

#[tokio::test]
async fn test_mcp_help_output() -> Result<()> {
    init_tracing();
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
    init_tracing();
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
async fn test_mcp_http_server_starts() -> Result<()> {
    init_tracing();
    let path = find_binary()?;
    let port = get_free_port();
    debug!("Starting server on port {}", port);

    let mut child = Command::new(&path)
        .arg("http")
        .arg("--port")
        .arg(port.to_string())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()?;
    debug!("Server process spawned");

    debug!("Waiting 1 second for server to start...");
    tokio::time::sleep(Duration::from_secs(1)).await;
    debug!("Sleep complete, connecting to TCP...");

    let mut stream = TcpStream::connect(format!("127.0.0.1:{}", port))
        .await
        .context("Failed to connect")?;
    debug!("TCP connected, initializing...");

    let session_id = mcp_http_initialize(&mut stream, port).await.context("Initialize failed")?;
    debug!("Initialize complete, session_id={}", session_id);
    assert!(!session_id.is_empty(), "Should have session ID");

    debug!("Dropping stream...");
    drop(stream);
    debug!("Stream dropped");
    debug!("Killing child process (HTTP server won't exit on disconnect)...");
    let _ = child.kill().await;
    debug!("Kill sent");
    debug!("Waiting for child process to exit...");
    let result = child.wait().await;
    debug!("Child process exited: {:?}", result);

    Ok(())
}

#[tokio::test]
#[ignore]
async fn test_mcp_flights_http() -> Result<()> {
    let path = find_binary()?;
    let port = get_free_port();

    let mut child = Command::new(&path)
        .arg("http")
        .arg("--port")
        .arg(port.to_string())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()?;

    let mut stderr = child.stderr.take().unwrap();

    tokio::time::sleep(Duration::from_secs(1)).await;

    let mut stream = TcpStream::connect(format!("127.0.0.1:{}", port))
        .await
        .context("Failed to connect")?;

    let session_id = mcp_http_initialize(&mut stream, port).await.context("Initialize failed")?;
    assert!(!session_id.is_empty(), "Should have session ID");

    let initialized_notification = r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#;
    mcp_http_send_notification(&mut stream, &session_id, initialized_notification)
        .await
        .context("Failed to send initialized")?;

    let depart_naive = today() + Months::new(2);
    let return_naive = depart_naive + chrono::Duration::days(7);
    let depart_date = depart_naive.format("%Y-%m-%d").to_string();
    let return_date = return_naive.format("%Y-%m-%d").to_string();

    let args = json!({
        "from": "NRT",
        "to": "JFK",
        "date": depart_date,
        "return_date": return_date,
        "seat": "economy",
        "adults": 2,
        "children_ages": [5, 8],
        "trip_type": "round_trip",
        "max_stops": 2
    });

    let call_request = json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "tools/call",
        "params": {"name": "search_flights", "arguments": args}
    }).to_string();

    let response_body = mcp_http_send(&mut stream, &session_id, &call_request, Some(2))
        .await
        .context("Failed to send tool call")?;

    let body = response_body.split("\r\n\r\n").nth(1).unwrap_or("");
    let events = parse_sse_events(body);
    let call_event = events.iter()
        .find(|e| extract_json_from_sse(e).map_or(false, |j| j.contains("\"id\":2")))
        .context("Tool call response not found")?;
    let json_str = extract_json_from_sse(call_event)
        .context("Failed to extract JSON from SSE event")?;

    let response: Value = serde_json::from_str(&json_str)
        .context("Failed to parse JSON response")?;

    drop(stream);
    debug!("Killing child process (HTTP server won't exit on disconnect)...");
    let _ = child.kill().await;
    debug!("Kill sent");
    debug!("Waiting for child process to exit...");
    let result = child.wait().await;
    debug!("Child process exited: {:?}", result);

    let stderr_output = read_stderr_to_string(&mut stderr).await;
    if !stderr_output.is_empty() {
        debug!("STDERR: {}", stderr_output);
    }

    assert!(response.is_object(), "Response should be an object");
    let obj = response.as_object().unwrap();

    assert!(obj.contains_key("id"), "Response should have id");
    assert_eq!(obj["id"], 2, "Response id should be 2");

    if obj.contains_key("error") {
        let error = &obj["error"];
        let error_obj = error.as_object().unwrap();
        let code = error_obj["code"].as_i64().unwrap_or(-1);
        let message = error_obj["message"].as_str().unwrap_or("unknown");
        anyhow::bail!("API error: code={}, message={}", code, message);
    }

    let text_str = &obj["result"]["content"][0]["text"];
    debug!("=== RAW RESPONSE ===");
    debug!("{}", text_str);
    debug!("====================");

    let inner: Value = serde_json::from_str(text_str.as_str().unwrap())
        .context("Failed to parse inner flight JSON")?;

    let inner_obj = inner.as_object().unwrap();
    let sf_obj = inner_obj["search_flights"].as_object().unwrap();
    let results = sf_obj["results"].as_array().unwrap();
    let total = sf_obj["total"].as_u64().unwrap();

    assert!(!results.is_empty(), "Results should not be empty");
    assert_eq!(results.len() as u64, total, "Result count should match total");

    println!("=== FLIGHTS REQUEST ===");
    println!("NRT → JFK on {} (return {})", depart_date, return_date);
    println!("======================");
    println!("✓ HTTP transport flight search successful");
    println!("✓ Found {} results (total: {})", results.len(), total);

    Ok(())
}

#[tokio::test]
#[ignore]
async fn test_mcp_hotels_http() -> Result<()> {
    let path = find_binary()?;
    let port = get_free_port();

    let mut child = Command::new(&path)
        .arg("http")
        .arg("--port")
        .arg(port.to_string())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()?;

    let mut stderr = child.stderr.take().unwrap();

    tokio::time::sleep(Duration::from_secs(1)).await;

    let mut stream = TcpStream::connect(format!("127.0.0.1:{}", port))
        .await
        .context("Failed to connect")?;

    let session_id = mcp_http_initialize(&mut stream, port).await.context("Initialize failed")?;
    assert!(!session_id.is_empty(), "Should have session ID");

    let initialized_notification = r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#;
    mcp_http_send_notification(&mut stream, &session_id, initialized_notification)
        .await
        .context("Failed to send initialized")?;

    let checkin_naive = today() + Months::new(1);
    let checkout_naive = checkin_naive + chrono::Duration::days(3);
    let checkin = checkin_naive.format("%Y-%m-%d").to_string();
    let checkout = checkout_naive.format("%Y-%m-%d").to_string();

    let args = json!({
        "location": "Paris",
        "checkin_date": checkin,
        "checkout_date": checkout,
        "adults": 2,
        "children_ages": [10],
        "min_guest_rating": 4.5,
        "stars": [4, 5],
        "amenities": ["pool", "spa"],
        "min_price": 100,
        "max_price": 500
    });

    let call_request = json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "tools/call",
        "params": {"name": "search_hotels", "arguments": args}
    }).to_string();

    let response_body = mcp_http_send(&mut stream, &session_id, &call_request, Some(2))
        .await
        .context("Failed to send tool call")?;

    let body = response_body.split("\r\n\r\n").nth(1).unwrap_or("");
    let events = parse_sse_events(body);
    let call_event = events.iter()
        .find(|e| extract_json_from_sse(e).map_or(false, |j| j.contains("\"id\":2")))
        .context("Tool call response not found")?;
    let json_str = extract_json_from_sse(call_event)
        .context("Failed to extract JSON from SSE event")?;

    let response: Value = serde_json::from_str(&json_str)
        .context("Failed to parse JSON response")?;

    drop(stream);
    let _ = child.wait().await;

    let stderr_output = read_stderr_to_string(&mut stderr).await;
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
        let error_obj = error.as_object().unwrap();
        let code = error_obj["code"].as_i64().unwrap_or(-1);
        let message = error_obj["message"].as_str().unwrap_or("unknown");
        anyhow::bail!("API error: code={}, message={}", code, message);
    }

    let text_str = &obj["result"]["content"][0]["text"];
    debug!("=== RAW RESPONSE ===");
    debug!("{}", text_str);
    debug!("====================");

    let inner: Value = serde_json::from_str(text_str.as_str().unwrap())
        .context("Failed to parse inner hotel JSON")?;

    let inner_obj = inner.as_object().unwrap();
    let sh_obj = inner_obj["search_hotels"].as_object().unwrap();
    let results = sh_obj["results"].as_array().unwrap();
    let total = sh_obj["total"].as_u64().unwrap();

    assert!(!results.is_empty(), "Results should not be empty");
    assert_eq!(results.len() as u64, total, "Result count should match total");

    println!("=== HOTELS REQUEST ===");
    println!("Paris, {} to {}", checkin, checkout);
    println!("===================");
    println!("✓ HTTP transport hotel search successful");
    println!("✓ Found {} results (total: {})", results.len(), total);

    Ok(())
}
