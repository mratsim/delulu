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

// Request schemas defined for documentation and potential future validation
#[allow(dead_code)]
const FLIGHTS_REQUEST_SCHEMA: &str = r#"
{
    "$schema": "http://json-schema.org/draft-07/schema#",
    "type": "object",
    "required": ["from", "to", "date", "adults"],
    "properties": {
        "from": {"type": "string", "description": "Origin airport code (IATA)"},
        "to": {"type": "string", "description": "Destination airport code (IATA)"},
        "date": {"type": "string", "description": "Departure date (YYYY-MM-DD)"},
        "return_date": {"type": "string"},
        "seat": {"type": "string", "enum": ["Economy", "PremiumEconomy", "Business", "First"]},
        "adults": {"type": "integer", "minimum": 1},
        "children_ages": {"type": "array", "items": {"type": "integer", "minimum": 0}},
        "trip_type": {"type": "string", "enum": ["round-trip", "one-way"]},
        "max_stops": {"type": "integer", "minimum": 0}
    }
}
"#;

#[allow(dead_code)]
const HOTELS_REQUEST_SCHEMA: &str = r#"
{
    "$schema": "http://json-schema.org/draft-07/schema#",
    "type": "object",
    "required": ["location", "checkin_date", "checkout_date", "adults"],
    "properties": {
        "location": {"type": "string"},
        "checkin_date": {"type": "string"},
        "checkout_date": {"type": "string"},
        "adults": {"type": "integer", "minimum": 1},
        "children_ages": {"type": "array", "items": {"type": "integer"}},
        "min_guest_rating": {"type": "number", "minimum": 0, "maximum": 5},
        "stars": {"type": "array", "items": {"type": "integer", "minimum": 1, "maximum": 5}},
        "amenities": {"type": "array", "items": {"type": "string"}},
        "min_price": {"type": "integer"},
        "max_price": {"type": "integer"}
    }
}
"#;

const FLIGHTS_RESPONSE_SCHEMA: &str = r#"
{
    "$schema": "http://json-schema.org/draft-07/schema#",
    "type": "object",
    "required": ["search_flights"],
    "properties": {
        "search_flights": {
            "type": "object",
            "required": ["total", "query", "results"],
            "properties": {
                "total": {"type": "integer", "minimum": 0},
                "query": {
                    "type": "object",
                    "required": ["from", "to", "date", "curr", "seat"],
                    "properties": {
                        "from": {"type": "string"},
                        "to": {"type": "string"},
                        "date": {"type": "string"},
                        "curr": {"type": "string"},
                        "seat": {"type": "string"}
                    }
                },
                "results": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "required": ["price", "airlines", "dur_min"],
                        "properties": {
                            "price": {"type": "integer", "minimum": 0},
                            "airlines": {"type": "array", "items": {"type": "string"}},
                            "dur_min": {"type": "integer", "minimum": 0},
                            "layover": {
                                "type": "array",
                                "items": {
                                    "type": "object",
                                    "required": ["city", "dur_min"],
                                    "properties": {
                                        "city": {"type": "string"},
                                        "dur_min": {"type": "integer", "minimum": 0}
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}
"#;

const HOTELS_RESPONSE_SCHEMA: &str = r#"
{
    "$schema": "http://json-schema.org/draft-07/schema#",
    "type": "object",
    "required": ["search_hotels"],
    "properties": {
        "search_hotels": {
            "type": "object",
            "required": ["total", "query", "results"],
            "properties": {
                "total": {"type": "integer", "minimum": 0},
                "query": {
                    "type": "object",
                    "required": ["loc", "in", "out", "curr"],
                    "properties": {
                        "loc": {"type": "string"},
                        "in": {"type": "string"},
                        "out": {"type": "string"},
                        "curr": {"type": "string"}
                    }
                },
                "results": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "required": ["name", "price", "rating", "amenities"],
                        "properties": {
                            "name": {"type": "string"},
                            "price": {"type": "integer", "minimum": 0},
                            "rating": {"type": "number"},
                            "stars": {"type": "integer"},
                            "amenities": {"type": "array", "items": {"type": "string"}}
                        }
                    }
                }
            }
        }
    }
}
"#;

fn validate_json_schema(instance: &Value, schema_str: &str, schema_name: &str) -> Result<()> {
    let schema: Value = serde_json::from_str(schema_str)
        .context(format!("Failed to parse {} schema", schema_name))?;

    let validator = jsonschema::Validator::new(&schema)
        .context(format!("Failed to create validator for {}", schema_name))?;

    let errors: Vec<String> = validator.iter_errors(instance).map(|e| format!("{}: {}", schema_name, e)).collect();

    if errors.is_empty() {
        Ok(())
    } else {
        anyhow::bail!("Schema validation failed for {}:\n{}", schema_name, errors.join("\n"))
    }
}

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
        let error_obj = error.as_object().unwrap();
        let code = error_obj["code"].as_i64().unwrap_or(-1);
        let message = error_obj["message"].as_str().unwrap_or("unknown");
        anyhow::bail!("API error: code={}, message={}", code, message);
    }

    let text_str = &obj["result"]["content"][0]["text"];
    println!("=== RAW RESPONSE ===");
    println!("{}", text_str);
    println!("====================");
    let inner: Value = serde_json::from_str(text_str.as_str().unwrap())
        .context("Failed to parse inner flight JSON")?;

    validate_json_schema(&inner, FLIGHTS_RESPONSE_SCHEMA, "flights_response")?;

    let inner_obj = inner.as_object().unwrap();
    let sf_obj = inner_obj["search_flights"].as_object().unwrap();
    let results = sf_obj["results"].as_array().unwrap();
    let total = sf_obj["total"].as_u64().unwrap();

    assert!(!results.is_empty(), "Results should not be empty");
    assert_eq!(results.len() as u64, total, "Result count should match total");

    println!("=== FLIGHTS REQUEST ===");
    println!("SFO → JFK on {} (return {})", depart_date, return_date);
    println!("======================");
    println!("✓ Response validated against FLIGHTS_RESPONSE_SCHEMA");
    println!("✓ Found {} results (total: {})", results.len(), total);

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

    send_tool_call(&mut stdin, "search_hotels", args)
        .await
        .context("Failed to send hotel search tool call")?;

    let response = read_json_response_with_timeout(&mut stdout, TIMEOUT)
        .await
        .context("Failed to read hotel search response")?;

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
        let error_obj = error.as_object().unwrap();
        let code = error_obj["code"].as_i64().unwrap_or(-1);
        let message = error_obj["message"].as_str().unwrap_or("unknown");
        anyhow::bail!("API error: code={}, message={}", code, message);
    }

    let text_str = &obj["result"]["content"][0]["text"];
    let inner: Value = serde_json::from_str(text_str.as_str().unwrap())
        .context("Failed to parse inner hotel JSON")?;

    validate_json_schema(&inner, HOTELS_RESPONSE_SCHEMA, "hotels_response")?;

    let inner_obj = inner.as_object().unwrap();
    let sh_obj = inner_obj["search_hotels"].as_object().unwrap();
    let results = sh_obj["results"].as_array().unwrap();
    let total = sh_obj["total"].as_u64().unwrap();

    assert!(!results.is_empty(), "Results should not be empty");
    assert_eq!(results.len() as u64, total, "Result count should match total");

    println!("=== HOTELS REQUEST ===");
    println!("Paris, {} to {}", checkin, checkout);
    println!("===================");
    println!("✓ Response validated against HOTELS_RESPONSE_SCHEMA");
    println!("✓ Found {} results (total: {})", results.len(), total);

    Ok(())
}
