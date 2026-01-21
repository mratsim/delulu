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

#![cfg(test)]

use anyhow::{Context, Result};
use chrono::{Months, NaiveDate};
use delulu_travel_agent::{FlightSearchParams, HotelSearchParams};
use serde_json::Value;
use serde_json::json;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Once;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::process::{ChildStdout, Command};
use tokio::time::Duration;
use tracing;
use tracing_subscriber;
use tracing_subscriber::EnvFilter;

mod mcp_helpers;
use mcp_helpers::{find_binary, stream_stderr_to_console};

const TIMEOUT: Duration = Duration::from_secs(3);

fn init_tracing() {
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

fn load_schema_from_file(name: &str) -> Result<Value> {
    let manifest_dir = PathBuf::from(
        std::env::var("CARGO_MANIFEST_DIR")
            .map_err(|e| anyhow::anyhow!("CARGO_MANIFEST_DIR not set: {}", e))?,
    );
    let schema_path = manifest_dir.join("src").join("schemas").join(name);

    let content = std::fs::read_to_string(&schema_path)
        .context(format!("Failed to read schema file: {:?}", schema_path))?;

    serde_json::from_str(&content)
        .context(format!("Failed to parse schema file: {:?}", schema_path))
}

fn get_flights_response_schema() -> Result<Value> {
    load_schema_from_file("flights-response.json")
}

fn get_hotels_response_schema() -> Result<Value> {
    load_schema_from_file("hotels-response.json")
}

fn validate_json_schema(instance: &Value, schema: &Value, schema_name: &str) -> Result<()> {
    let validator = jsonschema::Validator::new(schema)
        .context(format!("Failed to create validator for {}", schema_name))?;

    let errors: Vec<String> = validator
        .iter_errors(instance)
        .map(|e| format!("{}: {}", schema_name, e))
        .collect();

    if errors.is_empty() {
        Ok(())
    } else {
        anyhow::bail!(
            "Schema validation failed for {}:\n{}",
            schema_name,
            errors.join("\n")
        )
    }
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
    tracing::debug!("Sending init request...");
    stdin.write_all(init_str.as_bytes()).await?;
    tracing::debug!("Init request sent");

    let mut resp = String::new();
    let mut buf = [0u8; 4096];
    tracing::debug!("Waiting for init response...");
    let n = tokio::time::timeout(TIMEOUT, stdout.read(&mut buf))
        .await?
        .context("Failed to read init response")?;
    if n > 0 {
        resp = String::from_utf8_lossy(&buf[..n]).to_string();
        tracing::debug!(
            "Init response received ({} bytes): {:?}",
            resp.len(),
            &resp[..200.min(resp.len())]
        );
    } else {
        tracing::debug!("No init response received (n={})", n);
    }
    assert!(
        resp.contains("2.0"),
        "Should get JSON-RPC init response: {}",
        resp
    );

    tracing::debug!("Sending initialized notification...");
    stdin
        .write_all(b"{\"jsonrpc\":\"2.0\",\"method\":\"notifications/initialized\"}\n")
        .await?;
    tracing::debug!("Initialized notification sent");
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
    tracing::debug!("Sending tool call: {}", name);
    stdin.write_all(call_str.as_bytes()).await?;
    tracing::debug!("Tool call sent");
    Ok(())
}

async fn read_json_response_with_timeout(stdout: &mut ChildStdout, dur: Duration) -> Result<Value> {
    let mut output = String::new();
    let mut buf = [0u8; 4096];
    let mut iterations = 0;
    let total_start = std::time::Instant::now();

    loop {
        iterations += 1;
        let elapsed = total_start.elapsed();

        let read_result = tokio::time::timeout(dur, stdout.read(&mut buf)).await;

        match read_result {
            Ok(Ok(0)) => {
                tracing::debug!("Iteration {}: EOF received after {:?}", iterations, elapsed);
                break;
            }
            Ok(Ok(n)) => {
                let chunk = String::from_utf8_lossy(&buf[..n]);
                output.push_str(&chunk);
                tracing::debug!(
                    "Iteration {}: read {} bytes, total {} bytes",
                    iterations,
                    n,
                    output.len()
                );

                if let Ok(response) = serde_json::from_str::<Value>(&output) {
                    if response.is_object() {
                        let obj = response.as_object().unwrap();
                        if obj.contains_key("id") && obj.contains_key("result") {
                            tracing::debug!(
                                "Iteration {}: complete JSON-RPC response received",
                                iterations
                            );
                            return Ok(response);
                        }
                    }
                }
            }
            Ok(Err(e)) => {
                tracing::debug!("Iteration {}: error: {:?}", iterations, e);
                break;
            }
            Err(_) => {
                tracing::debug!("Iteration {}: timeout after {:?}", iterations, elapsed);
                break;
            }
        }
    }

    tracing::debug!(
        "Read loop complete after {:?} and {} iterations, total bytes: {}",
        total_start.elapsed(),
        iterations,
        output.len()
    );

    if output.is_empty() {
        anyhow::bail!("Stdout output is empty - server produced no response");
    }

    let response: Value = serde_json::from_str(&output).context(format!(
        "Failed to parse JSON response ({} bytes): {}",
        output.len(),
        &output[..output.len().min(500)]
    ))?;

    Ok(response)
}

#[tokio::test]
async fn test_mcp_server_starts_stdio() -> Result<()> {
    init_tracing();
    let path = find_binary()?;

    let mut child = Command::new(&path)
        .arg("stdio")
        .stdout(Stdio::piped())
        .stdin(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()?;

    let stderr = child.stderr.take().unwrap();
    let mut stdout = child.stdout.take().unwrap();
    let mut stdin = child.stdin.take().unwrap();

    let _stderr_task = tokio::spawn(stream_stderr_to_console(stderr));

    mcp_initialize(&mut stdin, &mut stdout)
        .await
        .context("MCP initialize failed")?;

    drop(stdin);
    drop(child);

    Ok(())
}

#[tokio::test]
async fn test_mcp_help_output() -> Result<()> {
    init_tracing();
    let path = find_binary()?;
    let output = Command::new(&path).arg("--help").output().await?;

    assert!(output.status.success(), "Help should succeed");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("delulu-travel-mcp"),
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
    let output = Command::new(&path).arg("--version").output().await?;

    assert!(output.status.success(), "Version should succeed");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("0.1.0"), "Version should show 0.1.0");

    Ok(())
}

#[tokio::test]
#[ignore]
async fn test_mcp_flights_stdio() -> Result<()> {
    init_tracing();
    let path = find_binary()?;

    let mut child = Command::new(&path)
        .arg("stdio")
        .stdout(Stdio::piped())
        .stdin(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()?;

    let mut stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();
    let mut stdin = child.stdin.take().unwrap();

    mcp_initialize(&mut stdin, &mut stdout)
        .await
        .context("MCP initialize failed")?;

    let depart_date = today() + Months::new(2);
    let depart_date_str = depart_date.format("%Y-%m-%d").to_string();

    let args = json!({
        "from": "LHR",
        "to": "IST",
        "date": depart_date_str,
        "seat": "economy",
        "adults": 2,
        "children_ages": [5, 8],
        "trip_type": "round_trip",
        "max_stops": 2
    });
    let input_from = args["from"].as_str().unwrap().to_string();
    let input_to = args["to"].as_str().unwrap().to_string();
    let input_date = args["date"].as_str().unwrap().to_string();
    let input_seat = args["seat"].as_str().unwrap().to_string();
    let input_adults = args["adults"].as_u64().unwrap() as u32;
    let input_children_ages: Vec<i32> = args["children_ages"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_i64().unwrap() as i32)
        .collect();
    let input_trip = args["trip_type"].as_str().unwrap().to_string();
    let input_max_stops = args["max_stops"].as_i64().unwrap() as i32;

    send_tool_call(&mut stdin, "search_flights", args)
        .await
        .context("Failed to send flight search tool call")?;

    let response = read_json_response_with_timeout(&mut stdout, TIMEOUT)
        .await
        .context("Failed to read flight search response")?;

    drop(stdin);
    let stderr_task = tokio::spawn(stream_stderr_to_console(stderr));
    let _ = stderr_task.await;
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

    let flights_schema = get_flights_response_schema()?;
    validate_json_schema(&inner, &flights_schema, "flights_response")?;

    let inner_obj = inner.as_object().unwrap();
    let sf_obj = inner_obj["search_flights"].as_object().unwrap();
    let results = sf_obj["results"].as_array().unwrap();
    let total = sf_obj["total"].as_u64().unwrap();

    assert!(!results.is_empty(), "Results should not be empty");
    assert_eq!(
        results.len() as u64,
        total,
        "Result count should match total"
    );

    let query = sf_obj["query"].as_object().unwrap();
    let search_url = query["search_url"].as_str().unwrap();
    assert!(
        search_url.starts_with("https://www.google.com/travel/flights"),
        "search_url should be a valid Google Flights URL, got: {}",
        search_url
    );
    assert!(
        search_url.contains("tfs="),
        "search_url should contain tfs parameter"
    );
    let tfs_value = search_url
        .split("tfs=")
        .nth(1)
        .and_then(|s| s.split('&').next())
        .unwrap_or("");
    assert!(
        !tfs_value.is_empty(),
        "search_url should have non-empty tfs value"
    );
    base64::Engine::decode(&base64::engine::general_purpose::STANDARD, tfs_value)
        .context("tfs parameter should be valid base64")?;
    println!("✓ search_url present and valid: {} chars", search_url.len());

    let decoded_params = FlightSearchParams::from_tfs(tfs_value)
        .context("Failed to decode TFS parameter")?;
    println!("✓ TFS decoded successfully");

    assert_eq!(
        decoded_params.from_airport, input_from,
        "from airport should match"
    );
    assert_eq!(
        decoded_params.to_airport, input_to,
        "to airport should match"
    );
    assert_eq!(
        decoded_params.depart_date, input_date,
        "depart date should match"
    );
    assert_eq!(
        decoded_params.cabin_class.as_str_name().to_lowercase(),
        input_seat,
        "seat should match"
    );

    let decoded_adults: u32 = decoded_params
        .passengers
        .iter()
        .filter(|(t, _)| *t == delulu_travel_agent::Passenger::Adult)
        .map(|(_, c)| *c)
        .sum();
    assert_eq!(decoded_adults, input_adults, "adults should match");

    let decoded_children: u32 = decoded_params
        .passengers
        .iter()
        .filter(|(t, _)| *t == delulu_travel_agent::Passenger::Child)
        .map(|(_, c)| *c)
        .sum();
    assert_eq!(
        decoded_children,
        input_children_ages.len() as u32,
        "children count should match"
    );

    assert_eq!(
        decoded_params.trip_type.as_str_name().to_lowercase(),
        input_trip,
        "trip type should match"
    );
    assert_eq!(
        decoded_params.max_stops,
        Some(input_max_stops),
        "max_stops should match"
    );

    println!("✓ TFS roundtrip validated - all parameters match input");

    println!("=== FLIGHTS REQUEST ===");
    println!("{} → {} on {}", decoded_params.from_airport, decoded_params.to_airport, decoded_params.depart_date);
    println!("======================");
    println!("✓ Response validated against FLIGHTS_RESPONSE_SCHEMA");
    println!("✓ Found {} results (total: {})", results.len(), total);

    println!("=== FIRST RESULT ===");
    println!("{}", serde_json::to_string_pretty(&results[0]).unwrap());
    println!("======================");

    Ok(())
}

#[tokio::test]
#[ignore]
async fn test_mcp_hotels_stdio() -> Result<()> {
    init_tracing();
    let path = find_binary()?;

    let mut child = Command::new(&path)
        .arg("stdio")
        .stdout(Stdio::piped())
        .stdin(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()?;

    let mut stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();
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
    let _input_location = args["location"].as_str().unwrap().to_string();
    let input_checkin = args["checkin_date"].as_str().unwrap().to_string();
    let input_checkout = args["checkout_date"].as_str().unwrap().to_string();
    let input_adults = args["adults"].as_u64().unwrap() as u32;
    let input_children: Vec<i32> = args["children_ages"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_i64().unwrap() as i32)
        .collect();
    let input_min_rating = args["min_guest_rating"].as_f64().unwrap();
    let input_stars: Vec<i32> = args["stars"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_i64().unwrap() as i32)
        .collect();
    let input_min_price = args["min_price"].as_i64().unwrap() as i32;
    let input_max_price = args["max_price"].as_i64().unwrap() as i32;

    send_tool_call(&mut stdin, "search_hotels", args)
        .await
        .context("Failed to send hotel search tool call")?;

    let response = read_json_response_with_timeout(&mut stdout, TIMEOUT)
        .await
        .context("Failed to read hotel search response")?;

    drop(stdin);
    let stderr_task = tokio::spawn(stream_stderr_to_console(stderr));
    let _ = stderr_task.await;
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

    let hotels_schema = get_hotels_response_schema()?;
    validate_json_schema(&inner, &hotels_schema, "hotels_response")?;

    let inner_obj = inner.as_object().unwrap();
    let sh_obj = inner_obj["search_hotels"].as_object().unwrap();
    let results = sh_obj["results"].as_array().unwrap();
    let total = sh_obj["total"].as_u64().unwrap();

    assert!(!results.is_empty(), "Results should not be empty");
    assert_eq!(
        results.len() as u64,
        total,
        "Result count should match total"
    );

    let query = sh_obj["query"].as_object().unwrap();
    let search_url = query["search_url"].as_str().unwrap();
    assert!(
        search_url.starts_with("https://www.google.com/travel/search"),
        "search_url should be a valid Google Hotels URL, got: {}",
        search_url
    );
    assert!(
        search_url.contains("q=Paris"),
        "search_url should contain q=Paris, got: {}",
        search_url
    );
    assert!(
        search_url.contains("q="),
        "search_url should contain q parameter"
    );
    assert!(
        search_url.contains("ts="),
        "search_url should contain ts parameter"
    );
    let ts_value = search_url
        .split("ts=")
        .nth(1)
        .and_then(|s| s.split('&').next())
        .unwrap_or("");
    assert!(
        !ts_value.is_empty(),
        "search_url should have non-empty ts value"
    );
    println!("✓ search_url present and valid: {} chars", search_url.len());

    let decoded_params = HotelSearchParams::from_ts(ts_value)
        .context("Failed to decode ts parameter")?;
    println!("✓ ts decoded successfully");

    assert_eq!(
        decoded_params.checkin_date, input_checkin,
        "checkin date should match"
    );
    assert_eq!(
        decoded_params.checkout_date, input_checkout,
        "checkout date should match"
    );
    assert_eq!(decoded_params.adults, input_adults, "adults should match");
    assert_eq!(
        decoded_params.children_ages.len(),
        input_children.len(),
        "children count should match"
    );
    for (decoded_age, expected_age) in decoded_params
        .children_ages
        .iter()
        .zip(input_children.iter())
    {
        assert_eq!(*decoded_age, *expected_age, "children ages should match");
    }
    assert_eq!(
        decoded_params.min_guest_rating,
        Some(input_min_rating),
        "min_guest_rating should match"
    );
    assert_eq!(
        decoded_params.hotel_stars, input_stars,
        "hotel_stars should match"
    );
    assert_eq!(
        decoded_params.min_price,
        Some(input_min_price),
        "min_price should match"
    );
    assert_eq!(
        decoded_params.max_price,
        Some(input_max_price),
        "max_price should match"
    );

    println!("✓ ts roundtrip validated - all parameters match input");

    println!("=== HOTELS REQUEST ===");
    println!("{}, {} to {}", decoded_params.loc_q_search, decoded_params.checkin_date, decoded_params.checkout_date);
    println!("===================");
    println!("✓ Response validated against HOTELS_RESPONSE_SCHEMA");
    println!("✓ Found {} results (total: {})", results.len(), total);

    println!("=== FIRST RESULT ===");
    println!("{}", serde_json::to_string_pretty(&results[0]).unwrap());
    println!("======================");

    Ok(())
}

#[tokio::test]
#[ignore]
async fn test_mcp_hotels_with_unknown_amenity_warning() -> Result<()> {
    init_tracing();
    let path = find_binary()?;

    let mut child = Command::new(&path)
        .arg("stdio")
        .stdout(Stdio::piped())
        .stdin(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()?;

    let mut stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();
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
        "children_ages": [],
        "min_guest_rating": 4.0,
        "stars": [4, 5],
        "amenities": ["pool", "swiming_pool", "spa"],
        "min_price": 50,
        "max_price": 500
    });

    send_tool_call(&mut stdin, "search_hotels", args)
        .await
        .context("Failed to send hotel search tool call")?;

    let response = read_json_response_with_timeout(&mut stdout, TIMEOUT)
        .await
        .context("Failed to read hotel search response")?;

    drop(stdin);
    let stderr_task = tokio::spawn(stream_stderr_to_console(stderr));
    let _ = stderr_task.await;
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

    let inner_obj = inner.as_object().unwrap();
    let sh_obj = inner_obj["search_hotels"].as_object().unwrap();
    let warnings = sh_obj["warnings"].as_array().unwrap();

    assert!(!warnings.is_empty(), "Should have warnings for misspelled amenity");
    let warning_text = warnings[0].as_str().unwrap();
    assert!(
        warning_text.contains("swiming_pool"),
        "Warning should mention misspelled amenity: {}",
        warning_text
    );
    assert!(
        warning_text.contains("Unknown amenity"),
        "Warning should mention unknown amenity: {}",
        warning_text
    );

    println!("✓ Got expected warning for misspelled amenity: {}", warning_text);

    Ok(())
}
