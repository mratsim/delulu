//!  Delulu All-MCP — e2e stdio transport + delegation dispatch (cases a-h)
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

//! Spawns `delulu-all-mcp --expose-local-networks stdio`, runs `initialize` +
//! `tools/list` (asserting exactly 21 tools), makes one `webfetch` call against
//! a local axum mock serving `text/html`, then drives the delegation
//! acceptance cases (a)-(h) over `tools/call` and asserts the error payloads.
//!
//! All delegation cases are offline: the missing-field/not-found validation
//! errors fire before any network traffic.

mod mcp_helpers;

use mcp_helpers::{
    find_binary, list_tools, mcp_initialize, read_json_response, send_tool_call,
    spawn_stdio_server_with_args,
};
use serde_json::{Value, json};
use std::sync::Arc;
use std::time::Duration;
use tokio::process::{Child, ChildStdin, ChildStdout};

const TIMEOUT: Duration = Duration::from_secs(30);

/// Spawn all-mcp with `--expose-local-networks stdio`; initialize it.
async fn spawn_all_mcp() -> (Child, ChildStdin, ChildStdout) {
    let path = find_binary("delulu-all-mcp")
        .unwrap_or_else(|e| panic!("find_binary(delulu-all-mcp): {e}"));
    let (child, mut stdin, mut stdout) = spawn_stdio_server_with_args(&path, &["--expose-local-networks"])
        .await
        .unwrap_or_else(|e| panic!("failed to spawn delulu-all-mcp: {e}"));
    mcp_initialize(&mut stdin, &mut stdout)
        .await
        .unwrap_or_else(|e| panic!("MCP initialize failed: {e}"));
    (child, stdin, stdout)
}

/// Send a `tools/call` and return the full JSON-RPC response (result or error).
async fn call_tool(stdin: &mut ChildStdin, stdout: &mut ChildStdout, name: &str) -> Value {
    send_tool_call(stdin, name, json!({}))
        .await
        .unwrap_or_else(|e| panic!("failed to send tool call for '{name}': {e}"));
    read_json_response(stdout, TIMEOUT, Some(2))
        .await
        .unwrap_or_else(|e| panic!("failed to read tool call response for '{name}': {e}"))
}

/// Assert that a `tools/call` error response contains `needle` in its message.
fn assert_error_contains(response: &Value, tool: &str, needle: &str) {
    let message = response["error"]["message"]
        .as_str()
        .unwrap_or_else(|| panic!("tool '{tool}' should return an error object: {response}"));
    assert!(
        message.contains(needle),
        "tool '{tool}' error message should contain '{needle}', got: {message}"
    );
}

/// `list_tools` returns exactly 21 tools (the full union).
#[tokio::test(flavor = "multi_thread")]
async fn stdio_lists_21_tools() {
    let (_child, mut stdin, mut stdout) = spawn_all_mcp().await;
    let mut initialized = true; // spawn_all_mcp already initialized
    let names = list_tools(&mut stdin, &mut stdout, &mut initialized)
        .await
        .expect("tools/list must succeed");
    assert_eq!(names.len(), 21, "all-mcp must expose exactly 21 tools, got: {names:?}");
}

/// One `webfetch` call against a local text/html mock succeeds without panic.
#[tokio::test(flavor = "multi_thread")]
async fn webfetch_against_local_html_mock_succeeds() {
    // Local axum mock serving a minimal text/html document.
    let html = Arc::new(
        "<html><body><h1>delulu all-mcp e2e</h1><p>localhost fixture</p></body></html>"
            .to_string(),
    );
    let app = axum::Router::new().route(
        "/index.html",
        axum::routing::get({
            let html = Arc::clone(&html);
            move || {
                let html = Arc::clone(&html);
                async move {
                    axum::response::Response::builder()
                        .header("content-type", "text/html")
                        .body(axum::body::Body::from(html.as_str().to_string()))
                        .unwrap()
                }
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind local mock");
    let addr = listener.local_addr().expect("local addr");
    tokio::spawn(async move {
        axum::serve(listener, app).await.expect("serve mock");
    });
    // Give the mock a moment to accept.
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    let (_child, mut stdin, mut stdout) = spawn_all_mcp().await;

    let url = format!("http://{}/index.html", addr);
    send_tool_call(&mut stdin, "webfetch", json!({ "url": url }))
        .await
        .expect("failed to send webfetch call");
    let response = read_json_response(&mut stdout, TIMEOUT, Some(2))
        .await
        .expect("failed to read webfetch response");

    assert!(
        response.get("result").is_some(),
        "webfetch against local html mock should succeed, got: {response}"
    );
}

/// Delegation cases (a)-(h): offline error dispatch over `tools/call`.
#[tokio::test(flavor = "multi_thread")]
async fn delegation_cases_a_through_h() {
    let (_child, mut stdin, mut stdout) = spawn_all_mcp().await;

    // (a) arxiv_get_paper with {} -> error contains arxiv_id
    let r = call_tool(&mut stdin, &mut stdout, "arxiv_get_paper").await;
    assert_error_contains(&r, "arxiv_get_paper", "arxiv_id");

    // (b) iacr_get_paper with {} -> error contains year
    let r = call_tool(&mut stdin, &mut stdout, "iacr_get_paper").await;
    assert_error_contains(&r, "iacr_get_paper", "year");

    // (c) pubmed_get_paper with {} -> error contains pmc_id
    let r = call_tool(&mut stdin, &mut stdout, "pubmed_get_paper").await;
    assert_error_contains(&r, "pubmed_get_paper", "pmc_id");

    // (d) web_search with {} -> error contains query
    let r = call_tool(&mut stdin, &mut stdout, "web_search").await;
    assert_error_contains(&r, "web_search", "query");

    // (e) no_such_tool with {} -> code -32602 AND message contains "tool not found"
    let r = call_tool(&mut stdin, &mut stdout, "no_such_tool").await;
    assert_eq!(
        r["error"]["code"].as_i64(),
        Some(-32602),
        "no_such_tool must return code -32602, got: {r}"
    );
    let msg = r["error"]["message"]
        .as_str()
        .unwrap_or_else(|| panic!("no_such_tool must have an error message, got: {r}"));
    assert!(
        msg.contains("tool not found"),
        "no_such_tool message must contain 'tool not found', got: {msg}"
    );

    // (f) get_paper with {} -> "did you mean" hint (case-insensitive: the
    // implementation capitalizes "Did you mean", the spec asserts "did you mean")
    let r = call_tool(&mut stdin, &mut stdout, "get_paper").await;
    let msg = r["error"]["message"]
        .as_str()
        .unwrap_or_else(|| panic!("get_paper must have an error message, got: {r}"));
    assert!(
        msg.to_lowercase().contains("did you mean"),
        "get_paper message must contain 'did you mean' (case-insensitive), got: {msg}"
    );

    // (g) search_flights with {} -> error contains from
    let r = call_tool(&mut stdin, &mut stdout, "search_flights").await;
    assert_error_contains(&r, "search_flights", "from");

    // (h) search_hotels with {} -> error contains location
    let r = call_tool(&mut stdin, &mut stdout, "search_hotels").await;
    assert_error_contains(&r, "search_hotels", "location");
}