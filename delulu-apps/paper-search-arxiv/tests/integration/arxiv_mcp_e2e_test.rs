//! End-to-end integration test for the arXiv MCP server.
//!
//! Follows the mcpify pattern:
//! 1. Start a local HTTP server serving fixture data
//! 2. Write a config file with the fixture URL
//! 3. Spawn Python to run MCP stdio tests against the fixture server

use std::process::{Command, Stdio};
use std::time::Duration;

use paper_search_test_utils::{fixture_path, serve_fixture};
const PYTHON_SCRIPT: &str = "tests/integration/test_arxiv_mcp_e2e_stdio.py";
const BINARY_NAME: &str = "delulu-arxiv-mcp";

fn find_binary() -> std::path::PathBuf {
    let workspace = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent().unwrap().parent().unwrap()
        .to_path_buf();
    for candidate in [
        workspace.join("target").join("debug").join(BINARY_NAME),
        workspace.join("target").join("release").join(BINARY_NAME),
    ] {
        if candidate.exists() {
            return candidate;
        }
    }
    panic!("Could not find {BINARY_NAME} binary");
}

#[tokio::test]
async fn test_arxiv_mcp_e2e_stdio() {
    let path = fixture_path("paper-search-arxiv", "arxiv-search-response.xml.zst");
    let (fixture_url, _shutdown) = serve_fixture("/api/query", path).await;
    let fixture_url = format!("{}/api/query", fixture_url);

    let config = serde_json::json!({ "fixture_url": fixture_url });
    let config_path = std::env::temp_dir().join(format!("arxiv_e2e_{}.json", std::process::id()));
    std::fs::write(&config_path, config.to_string()).unwrap();

    let binary = find_binary();
    let script = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(PYTHON_SCRIPT);
    let config_path_arg = config_path.clone();

    let output = tokio::time::timeout(
        Duration::from_secs(15),
        tokio::task::spawn_blocking(move || {
            Command::new("python3")
                .arg(&script)
                .arg(&binary)
                .arg(&config_path_arg)
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .output()
                .expect("Failed to run Python test script")
        }),
    )
    .await
    .expect("Python test timed out")
    .expect("Python task panicked");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    print!("{}", stdout);
    if !stderr.is_empty() {
        eprint!("{}", stderr);
    }

    let _ = std::fs::remove_file(&config_path);

    assert!(
        output.status.success(),
        "Python MCP e2e tests failed (exit: {:?})",
        output.status.code()
    );
}
