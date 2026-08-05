//!  Delulu All-MCP — e2e over HTTP transport
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
//! Spawns `delulu-all-mcp http --host 127.0.0.1 --port <free>` over the
//! Streamable HTTP transport, performs `initialize` + `tools/list` at `/mcp`
//! (asserting the 21-tool union), and then runs the flag-default SSRF check:
//!
//! * **without** `--expose-local-networks`: a `webfetch` call against a local
//!   axum mock is rejected with the exact DETAILED private-IP string
//!   (proving the flag defaults to off and the webfetch tool executes through
//!   the delegator over HTTP);
//! * respawned **with** `--expose-local-networks`: the same call succeeds
//!   against the same mock.
//!
//! The HTTP requestor is same-subnet (127.0.0.1), so the DETAILED message is
//! expected. No assertion about "PeerAddr survived delegation" is made.

mod mcp_helpers;

use anyhow::{Context, Result};
use mcp_helpers::find_binary;
use serde_json::{Value, json};
use std::net::TcpListener;
use std::path::Path;
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::process::{Child, Command};

const TIMEOUT: Duration = Duration::from_secs(10);

/// The EXACT DETAILED private-IP block message (frozen contract).
const SSRF_DETAILED: &str = "URL resolves to a private IP address which is blocked by default. Use --expose-local-networks to allow fetching from local/private networks.";

/// Grab an ephemeral free port by binding `127.0.0.1:0` and dropping.
fn get_free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .expect("bind ephemeral port")
        .local_addr()
        .expect("local addr")
        .port()
}

/// Start a local axum mock serving `/page` with a minimal `text/html` body.
/// Returns the port the mock is listening on.
async fn start_html_mock() -> u16 {
    let html = Arc::new(String::from("<html><body>ok</body></html>"));
    let app = axum::Router::new().route(
        "/page",
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
        .expect("bind mock");
    let port = listener.local_addr().expect("local addr").port();
    tokio::spawn(async move {
        axum::serve(listener, app).await.expect("serve mock");
    });
    tokio::time::sleep(Duration::from_millis(100)).await;
    port
}

/// Spawn `delulu-all-mcp http --host 127.0.0.1 --port <port>` plus optional
/// leading flags, returning the child process.
async fn spawn_http_server(port: u16, extra_args: &[&str], binary: &Path) -> Child {
    let mut child = Command::new(binary)
        .args(extra_args)
        .arg("http")
        .arg("--host")
        .arg("127.0.0.1")
        .arg("--port")
        .arg(port.to_string())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .expect("spawn all-mcp http server");
    let stderr = child.stderr.take().expect("stderr");
    tokio::spawn(async move {
        let mut reader = tokio::io::BufReader::new(stderr);
        let mut line = String::new();
        loop {
            match reader.read_line(&mut line).await {
                Ok(0) => break,
                Ok(_) => {
                    eprint!("[http-stderr] {}", line);
                    line.clear();
                }
                Err(_) => break,
            }
        }
    });
    tokio::time::sleep(Duration::from_millis(300)).await; // let the server bind
    child
}

/// Send a raw HTTP/1.1 POST to `/mcp` and return `(session_id_header, raw_body)`.
///
/// The response body is the SSE stream (possibly chunked). The `mcp-session-id`
/// header, when present, is returned separately so the caller can feed it back
/// on later requests.
async fn post_mcp(port: u16, session: Option<&str>, body: &str) -> Result<(String, String)> {
    let mut stream = TcpStream::connect(format!("127.0.0.1:{port}"))
        .await
        .with_context(|| format!("connect to http server on {port}"))?;

    let mut headers = format!(
        "POST /mcp HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nContent-Type: application/json\r\nAccept: application/json, text/event-stream\r\nContent-Length: {}\r\n",
        body.len()
    );
    if let Some(sid) = session {
        headers.push_str(&format!("mcp-session-id: {sid}\r\n"));
    }
    headers.push_str("\r\n");
    headers.push_str(body);

    stream.write_all(headers.as_bytes()).await?;

    let mut raw = Vec::new();
    let mut buf = [0u8; 8192];
    loop {
        match tokio::time::timeout(TIMEOUT, stream.read(&mut buf)).await {
            Ok(Ok(0)) => break,
            Ok(Ok(n)) => {
                raw.extend_from_slice(&buf[..n]);
                let s = String::from_utf8_lossy(&raw);
                if s.contains("\r\n0\r\n\r\n") || s.contains("\n0\n\n") {
                    break;
                }
            }
            Ok(Err(_)) => break,
            Err(_) => break,
        }
    }
    let raw_str = String::from_utf8_lossy(&raw).into_owned();

    let (head, body) = match raw_str.find("\r\n\r\n") {
        Some(i) => (raw_str[..i].to_string(), raw_str[i + 4..].to_string()),
        None => (String::new(), raw_str),
    };

    let session_id = head
        .lines()
        .find(|l| l.to_ascii_lowercase().starts_with("mcp-session-id:"))
        .map(|l| {
            l.split_once(':')
                .map(|(_, v)| v.trim().to_string())
                .unwrap_or_default()
        })
        .unwrap_or_default();

    Ok((session_id, body))
}

/// Extract the JSON-RPC `data:` payload with the given `id` from a raw SSE
/// body. SSE frames are emitted as `data: {..}` lines (possibly interleaved
/// with SSE `id`/`retry` fields and HTTP chunk framing); we scan line-by-line
/// for the `data: {jsonrpc...}` frame whose `id` matches. The response JSON
/// is a single-line frame for these small tool responses.
fn extract_json_by_id(body: &str, id: u64) -> Result<Value> {
    let target = id.to_string();
    for line in body.lines() {
        let trimmed = line.trim();
        if let Some(data) = trimmed.strip_prefix("data: ")
            && let Ok(v) = serde_json::from_str::<Value>(data)
            && v.get("id").map(|i| i.to_string()) == Some(target.clone())
        {
            return Ok(v);
        }
    }
    anyhow::bail!("no JSON-RPC response with id {id} found in SSE body:\n{body}")
}

/// POST `initialize`, verify response id 1, send the `notifications/initialized`
/// notification, and return the session id to reuse.
async fn initialize_http(port: u16) -> Result<String> {
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
    let body = init.to_string();
    let (session, resp_body) = post_mcp(port, None, &body).await?;
    let _resp = extract_json_by_id(&resp_body, 1).context("initialize response missing")?;
    // Send the initialized notification (id-less) on a fresh request.
    let notif = r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#;
    let (session2, _) = post_mcp(port, Some(&session), notif).await?;
    Ok(if session2.is_empty() {
        session
    } else {
        session2
    })
}

/// Send `tools/list` and return the parsed tool entries.
async fn list_tools_http(port: u16, session: &str) -> Result<Vec<Value>> {
    let body = r#"{"jsonrpc":"2.0","id":100,"method":"tools/list","params":{}}"#;
    let (_, resp_body) = post_mcp(port, Some(session), body).await?;
    let value = extract_json_by_id(&resp_body, 100)?;
    value["result"]["tools"]
        .as_array()
        .cloned()
        .context("tools/list result.tools missing")
}

/// Send a `tools/call` and return the parsed response value.
async fn call_tool_http(port: u16, session: &str, name: &str, args: Value) -> Result<Value> {
    let call = json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "tools/call",
        "params": {"name": name, "arguments": args}
    });
    let body = call.to_string();
    let (_, resp_body) = post_mcp(port, Some(session), &body).await?;
    extract_json_by_id(&resp_body, 2)
}

/// Concatenate the `text` fields of a `tools/call` result's content array.
/// Falls back to the serialized value when there is no text content.
fn tool_text(resp: &Value) -> String {
    let Some(content) = resp.get("result").and_then(|r| r.get("content")) else {
        return resp.to_string();
    };
    let texts: Vec<String> = content
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|c| c.get("text").and_then(|t| t.as_str()).map(String::from))
                .collect()
        })
        .unwrap_or_default();
    if texts.is_empty() {
        resp.to_string()
    } else {
        texts.join("\n")
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn http_initialize_list_tools_and_ssrf_flag_default() -> Result<()> {
    let binary = find_binary("delulu-all-mcp")?;
    let mock_port = start_html_mock().await;
    let mock_url = format!("http://127.0.0.1:{mock_port}/page");

    // ---- Server A: WITHOUT --expose-local-networks (flag defaults off) ----
    let port_a = get_free_port();
    let mut child_a = spawn_http_server(port_a, &[], &binary).await;
    let sid_a = initialize_http(port_a).await?;

    let tools = list_tools_http(port_a, &sid_a).await?;
    assert_eq!(
        tools.len(),
        21,
        "all-mcp must expose exactly 21 tools over HTTP, got {}",
        tools.len()
    );

    // webfetch WITHOUT the flag -> the tool is rejected and surfaces the exact
    // DETAILED private-IP string in its result content (with isError: true).
    let fetch_a = call_tool_http(port_a, &sid_a, "webfetch", json!({ "url": mock_url })).await?;
    let text = tool_text(&fetch_a);
    assert!(
        text.contains(SSRF_DETAILED),
        "webfetch over HTTP without --expose-local-networks must return the exact DETAILED string; got: {fetch_a}"
    );
    child_a.kill().await?;

    // ---- Server B: WITH --expose-local-networks (flag on) -> success ----
    let port_b = get_free_port();
    let mut child_b = spawn_http_server(port_b, &["--expose-local-networks"], &binary).await;
    let sid_b = initialize_http(port_b).await?;
    let fetch_b = call_tool_http(port_b, &sid_b, "webfetch", json!({ "url": mock_url })).await?;
    let text_b = tool_text(&fetch_b);
    assert!(
        !text_b.contains(SSRF_DETAILED),
        "webfetch over HTTP WITH --expose-local-networks must not be SSRF-blocked, got: {fetch_b}"
    );
    assert!(
        !text_b.is_empty(),
        "webfetch with the flag must return the mock body content, got: {fetch_b}"
    );
    child_b.kill().await?;
    Ok(())
}
