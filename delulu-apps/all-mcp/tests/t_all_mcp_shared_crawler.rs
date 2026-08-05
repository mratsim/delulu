//!  Delulu All-MCP — shared-crawler loopback smoke
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
//! Loopback smoke test for the shared rate-limited crawler:
//!
//! A single axum mock on `127.0.0.1:PORT` returns `200` with a minimal
//! `<html><body>ok</body></html>` body for **every** route and records the
//! `Instant` of each arrival per path. The all-mcp binary is spawned as
//!
//! ```text
//! delulu-all-mcp --qps 2 --burst 1 --expose-local-networks \
//!     --arxiv-api-base-url http://127.0.0.1:{PORT}/query \
//!     --iacr-api-base-url http://127.0.0.1:{PORT} \
//!     --pubmed-api-base-url http://127.0.0.1:{PORT} stdio
//! ```
//!
//! and, in order, these four tools fire network requests whose **path** the
//! mock records:
//!
//! * `webfetch`           → `/page`
//! * `search_papers`      → `/query`
//! * `list_recent_papers` → `/rss/rss.xml`
//! * `search_pubmed`      → `/esearch.fcgi`
//!
//! Tool responses may be parse failures (the mock returns naive HTML) — we
//! only need the requests to arrive. Assertions: ≥1 arrival on each of the 4
//! paths, and every consecutive arrival gap is ≥ 250 ms (500 ms nominal at
//! qps 2). This ≥250 ms check is a smoke signal, NOT a discriminator of
//! shared-vs-separate crawlers.

mod mcp_helpers;

use anyhow::Result;
use mcp_helpers::{
    find_binary, mcp_initialize, read_json_response, send_tool_call, spawn_stdio_server_with_args,
};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::process::{Child, ChildStdin, ChildStdout};

const TIMEOUT: Duration = Duration::from_secs(30);
const MIN_GAP: Duration = Duration::from_millis(250);

type Arrivals = Arc<Mutex<HashMap<String, Vec<Instant>>>>;

/// Record an arrival instant for a given request path.
fn record_arrival(arrivals: &Arrivals, path: &str) {
    arrivals
        .lock()
        .expect("mock arrivals lock")
        .entry(path.to_string())
        .or_default()
        .push(Instant::now());
}

/// Start an axum mock on `127.0.0.1:0` that returns a minimal HTML body for
/// every route and records the arrival instant per path. Returns the port and
/// the shared arrival map.
async fn start_recording_mock() -> (u16, Arrivals) {
    let arrivals: Arrivals = Arc::new(Mutex::new(HashMap::new()));

    async fn handler(
        axum::extract::State(state): axum::extract::State<Arrivals>,
        uri: axum::http::Uri,
    ) -> axum::response::Response {
        record_arrival(&state, uri.path());
        axum::response::Response::builder()
            .header("content-type", "text/html")
            .body(axum::body::Body::from("<html><body>ok</body></html>"))
            .unwrap()
    }

    let app = axum::Router::new()
        .fallback(axum::routing::get(handler).with_state(Arc::clone(&arrivals)));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind mock");
    let port = listener.local_addr().expect("local addr").port();
    tokio::spawn(async move {
        axum::serve(listener, app).await.expect("serve mock");
    });
    tokio::time::sleep(Duration::from_millis(100)).await;
    (port, arrivals)
}

/// Spawn all-mcp with the shared-crawler flags and initialize it.
async fn spawn_all_with_base_urls(port: u16) -> (Child, ChildStdin, ChildStdout) {
    let binary = find_binary("delulu-all-mcp")
        .unwrap_or_else(|e| panic!("find_binary(delulu-all-mcp): {e}"));
    let args = [
        "--qps",
        "2",
        "--burst",
        "1",
        "--expose-local-networks",
        &format!("--arxiv-api-base-url=http://127.0.0.1:{port}/query"),
        &format!("--iacr-api-base-url=http://127.0.0.1:{port}"),
        &format!("--pubmed-api-base-url=http://127.0.0.1:{port}"),
    ];
    let (child, mut stdin, mut stdout) = spawn_stdio_server_with_args(&binary, &args)
        .await
        .unwrap_or_else(|e| panic!("failed to spawn delulu-all-mcp: {e}"));
    mcp_initialize(&mut stdin, &mut stdout)
        .await
        .unwrap_or_else(|e| panic!("MCP initialize failed: {e}"));
    (child, stdin, stdout)
}

/// Send a tool call and read its response (result or error — both acceptable).
async fn call_any(stdin: &mut ChildStdin, stdout: &mut ChildStdout, name: &str, args: Value) -> Value {
    send_tool_call(stdin, name, args)
        .await
        .unwrap_or_else(|e| panic!("failed to send {name}: {e}"));
    read_json_response(stdout, TIMEOUT, Some(2))
        .await
        .unwrap_or_else(|e| panic!("failed to read {name} response: {e}"))
}

#[tokio::test(flavor = "multi_thread")]
async fn shared_crawler_loopback_smoke() -> Result<()> {
    let (port, arrivals) = start_recording_mock().await;
    let (mut child, mut stdin, mut stdout) = spawn_all_with_base_urls(port).await;

    // Fire the four tools in order. Only the request arrival matters; the
    // naive HTML responses may fail to parse and that is acceptable.
    call_any(
        &mut stdin,
        &mut stdout,
        "webfetch",
        json!({ "url": format!("http://127.0.0.1:{port}/page") }),
    )
    .await;
    call_any(&mut stdin, &mut stdout, "search_papers", json!({ "query": "test" })).await;
    call_any(&mut stdin, &mut stdout, "list_recent_papers", json!({})).await;
    call_any(&mut stdin, &mut stdout, "search_pubmed", json!({ "query": "test" })).await;

    // Wait briefly so any in-flight trailing request lands on the mock.
    tokio::time::sleep(Duration::from_millis(1500)).await;

    let snapshot: HashMap<String, Vec<Instant>> = arrivals
        .lock()
        .expect("mock arrivals lock")
        .clone();

    // (1) every expected path received at least one request. The IACR and
    // PubMed clients build `{base}/path` from a base URL that `parse_url`
    // normalizes with a trailing slash, so the mock may record those arrivals
    // with an extra leading slash (`//rss/rss.xml`). Compare on the
    // slash-trimmed form so the 4 injection sites are matched idempotently.
    let expected = ["/page", "/query", "/rss/rss.xml", "/esearch.fcgi"];
    for path in &expected {
        let trimmed = path.trim_start_matches('/');
        let n = snapshot
            .iter()
            .filter(|(k, v)| k.trim_start_matches('/') == trimmed && !v.is_empty())
            .count();
        assert!(
            n >= 1,
            "mock must receive at least one request on '{path}', got {n} (all: {snapshot:?})"
        );
    }

    // (2) every consecutive distinct arrival gap must be >= MIN_GAP.
    let mut timeline: Vec<(Instant, String)> = Vec::new();
    for (path, instants) in snapshot.iter() {
        for t in instants {
            timeline.push((*t, path.clone()));
        }
    }
    timeline.sort_by_key(|(t, _)| *t);

    for pair in timeline.windows(2) {
        let (t1, p1) = &pair[0];
        let (t2, p2) = &pair[1];
        let gap = *t2 - *t1;
        assert!(
            gap >= MIN_GAP,
            "consecutive arrival gap on '{p2}' after '{p1}' was {gap:?} (<250ms); timeline={timeline:?}"
        );
    }

    drop(stdin);
    child.kill().await?;
    Ok(())
}