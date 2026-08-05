//!  Delulu All-MCP — Python MCP SDK e2e tests (stdio + HTTP)
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
//! Orchestrates the official-MCP-SDK Python test scripts
//! (`test_all_mcp_stdio.py`, `test_all_mcp_http.py`) against the
//! `delulu-all-mcp` binary.
//!
//! A small local fixture server serves the three paper fixtures (zstd
//! decompressed on the fly) so the paper tools run fully offline:
//!
//! * `search_papers`      → `/query`        (arxiv-search-response.xml.zst)
//! * `list_recent_papers` → `/rss/rss.xml`  (iacr-rss.xml.zst)
//! * `search_pubmed`      → `/esearch.fcgi` (pubmed-search.json.zst)
//!
//! The fixture base-url flags are forwarded to the binary so the three paper
//! tools hit the fixture server instead of the live APIs:
//!
//! ```text
//! delulu-all-mcp --arxiv-api-base-url http://127.0.0.1:{PORT}/query \
//!     --iacr-api-base-url http://127.0.0.1:{PORT} \
//!     --pubmed-api-base-url http://127.0.0.1:{PORT} stdio
//! ```
//!
//! The HTTP python script spawns its own `delulu-all-mcp http --port <free>`
//! instance with the same fixture flags.

#![cfg(test)]
#![cfg(feature = "mcp")]

use anyhow::{Context, Result};
use std::sync::Arc;
use std::time::Duration;

/// Python test scripts may need to create the uv `.venv` on first run
/// (pyproject.toml declares `mcp>=1.0`), so allow a generous timeout.
const PY_TIMEOUT: Duration = Duration::from_secs(180);

/// Find a Python interpreter that can run the MCP test scripts.
///
/// Prefers `uv` when available (handles pyproject.toml in tests/), then
/// `python3`, then `python`. The uv directory is this crate's
/// `tests/manual/end-to-end` — its pyproject.toml declares `mcp>=1.0` and
/// `uv` creates the `.venv` on first run.
/// Returns the command name and any prefix args needed before the script path.
fn find_python(manifest: &std::path::Path) -> (String, Vec<String>) {
    if std::process::Command::new("uv")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
    {
        let dir = manifest.join("tests/manual/end-to-end");
        let dir_str = dir.to_string_lossy().to_string();
        (
            "uv".to_string(),
            vec![
                "run".to_string(),
                "--directory".to_string(),
                dir_str,
                "python3".to_string(),
            ],
        )
    } else if std::process::Command::new("python3")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
    {
        ("python3".to_string(), vec![])
    } else {
        ("python".to_string(), vec![])
    }
}

/// Locate the `delulu-all-mcp` binary: release first, then debug.
fn find_binary() -> Result<std::path::PathBuf> {
    let manifest = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let workspace = manifest
        .parent()
        .and_then(|p| p.parent())
        .context("could not determine workspace root")?;
    for dir in ["release", "debug"] {
        let candidate = workspace.join("target").join(dir).join("delulu-all-mcp");
        if candidate.exists() {
            return Ok(candidate);
        }
    }
    anyhow::bail!(
        "delulu-all-mcp binary not found — run `cargo build --release -p delulu-all-mcp --features mcp`"
    )
}

/// Start a small fixture server on `127.0.0.1:0` serving the three paper
/// fixtures (zstd-decompressed at startup). Returns the bound port.
///
/// Routes (matched after trimming leading slashes, because the IACR/PubMed
/// clients build `{base}/path` from a base URL that `parse_url` normalizes
/// with a trailing slash, so the server may see `//rss/rss.xml`):
/// `/query`, `/rss/rss.xml`, `/esearch.fcgi`. Any other path → 404.
async fn start_fixture_server() -> Result<u16> {
    let manifest = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let workspace = manifest
        .parent()
        .and_then(|p| p.parent())
        .context("could not determine workspace root")?;

    let fixtures = [
        (
            "/query",
            "application/atom+xml",
            workspace.join(
                "delulu-apps/paper-search-arxiv/tests/fixtures/arxiv-search-response.xml.zst",
            ),
        ),
        (
            "/rss/rss.xml",
            "application/rss+xml",
            workspace.join("delulu-apps/paper-search-iacr/tests/fixtures/iacr-rss.xml.zst"),
        ),
        (
            "/esearch.fcgi",
            "application/json",
            workspace.join("delulu-apps/paper-search-pubmed/tests/fixtures/pubmed-search.json.zst"),
        ),
    ];

    // Pre-decompress the fixtures once; serve the plain bodies on request.
    let mut bodies: Vec<(String, String, String)> = Vec::with_capacity(fixtures.len());
    for (route, content_type, path) in fixtures {
        let compressed = std::fs::read(&path)
            .with_context(|| format!("failed to read fixture {}", path.display()))?;
        let decoded = zstd::decode_all(compressed.as_slice())
            .with_context(|| format!("failed to zstd-decode fixture {}", path.display()))?;
        let body = String::from_utf8(decoded)
            .with_context(|| format!("fixture {} is not valid UTF-8", path.display()))?;
        bodies.push((route.to_string(), content_type.to_string(), body));
    }
    let bodies = Arc::new(bodies);

    let app = axum::Router::new().fallback(move |uri: axum::http::Uri| {
        let bodies = Arc::clone(&bodies);
        async move {
            let path = uri.path().trim_start_matches('/');
            for (route, content_type, body) in bodies.iter() {
                if route.trim_start_matches('/') == path {
                    return axum::response::Response::builder()
                        .header("content-type", content_type.as_str())
                        .body(axum::body::Body::from(body.clone()))
                        .expect("fixture response body");
                }
            }
            axum::response::Response::builder()
                .status(axum::http::StatusCode::NOT_FOUND)
                .body(axum::body::Body::from("fixture not found"))
                .expect("404 response body")
        }
    });

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let port = listener.local_addr()?.port();
    tokio::spawn(async move {
        axum::serve(listener, app)
            .await
            .expect("fixture server failed");
    });
    tokio::time::sleep(Duration::from_millis(100)).await;

    Ok(port)
}

/// Spawn one Python MCP SDK test script with the binary and the fixture
/// base-url flags, and assert it exits 0.
async fn run_python_script(
    script: &std::path::Path,
    binary: &std::path::Path,
    port: u16,
) -> Result<()> {
    let manifest = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let script_str = script.to_string_lossy().to_string();
    let binary_str = binary.to_string_lossy().to_string();
    let arxiv_url = format!("http://127.0.0.1:{port}/query");
    let iacr_url = format!("http://127.0.0.1:{port}");
    let pubmed_url = format!("http://127.0.0.1:{port}");

    let (python, prefix_args) = find_python(&manifest);
    let py_handle = tokio::task::spawn_blocking(move || {
        let mut cmd = std::process::Command::new(python);
        cmd.args(&prefix_args)
            .arg(&script_str)
            .arg(&binary_str)
            .arg("--arxiv-api-base-url")
            .arg(&arxiv_url)
            .arg("--iacr-api-base-url")
            .arg(&iacr_url)
            .arg("--pubmed-api-base-url")
            .arg(&pubmed_url)
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

    let py_result = tokio::time::timeout(PY_TIMEOUT, py_handle)
        .await
        .context("Python test timed out")?
        .context("Python task panicked")?;
    let py_output = py_result.context("failed to run Python test")?;

    let stdout = String::from_utf8_lossy(&py_output.stdout);
    let stderr = String::from_utf8_lossy(&py_output.stderr);
    print!("{}", stdout);
    if !stderr.is_empty() {
        eprint!("{}", stderr);
    }

    anyhow::ensure!(
        py_output.status.success(),
        "Python MCP tests failed (exit: {:?})",
        py_output.status.code()
    );

    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn test_all_mcp_python_e2e_stdio() -> Result<()> {
    let port = start_fixture_server().await?;
    let binary = find_binary()?;
    let manifest = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let script = manifest.join("tests/manual/end-to-end/test_all_mcp_stdio.py");
    run_python_script(&script, &binary, port).await
}

#[tokio::test(flavor = "multi_thread")]
async fn test_all_mcp_python_e2e_http() -> Result<()> {
    let port = start_fixture_server().await?;
    let binary = find_binary()?;
    let manifest = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let script = manifest.join("tests/manual/end-to-end/test_all_mcp_http.py");
    run_python_script(&script, &binary, port).await
}
