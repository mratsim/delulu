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
        .parent()
        .unwrap()
        .parent()
        .unwrap()
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

/// Find a Python interpreter that can run the MCP test scripts.
///
/// Prefers `uv` when available (handles pyproject.toml in tests/), then
/// `python3`, then `python`. The uv directory is the websearch crate's
/// `tests/manual/end-to-end` (its .venv has the official `mcp` SDK installed).
/// Returns the command name and any prefix args needed before the script path.
fn find_python(websearch_manifest: &std::path::Path) -> (String, Vec<String>) {
    if std::process::Command::new("uv")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
    {
        let dir = websearch_manifest.join("tests/manual/end-to-end");
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

    let manifest = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let (python, prefix_args) = find_python(&manifest.parent().unwrap().join("websearch"));

    let output = tokio::time::timeout(
        Duration::from_secs(15),
        tokio::task::spawn_blocking(move || {
            let mut cmd = Command::new(python);
            cmd.args(&prefix_args)
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
