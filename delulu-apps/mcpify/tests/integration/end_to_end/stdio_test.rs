#![cfg(feature = "mcp")]

use anyhow::{Context, Result};
use std::process::{Command, Stdio};
use tokio::time::{Duration, timeout};

mod helpers;
mod service_a;
mod service_b;
use helpers::*;
use service_a::start_service_a;
use service_b::start_service_b;

const HEALTH_POLL_TIMEOUT: Duration = Duration::from_secs(10);
const HEALTH_POLL_INTERVAL: Duration = Duration::from_millis(100);

#[tokio::test]
async fn test_e2e_stdio_transport() -> Result<()> {
    init_tracing();

    // Start backend services
    let (sa, sda, pa) = start_service_a().await;
    let (sb, sdb, pb) = start_service_b().await;

    // Create drop guard for backend services
    let _backend_guard = E2eGuard {
        shutdown_senders: vec![sda, sdb],
        _server_tasks: vec![sa, sb],
        children: vec![],
        _stderr_tasks: vec![],
    };

    // Wait for services to be healthy
    for (p, label) in [(pa, "A"), (pb, "B")] {
        let start = std::time::Instant::now();
        let mut ready = false;
        while start.elapsed() < HEALTH_POLL_TIMEOUT {
            if health_check(p).await {
                ready = true;
                break;
            }
            tokio::time::sleep(HEALTH_POLL_INTERVAL).await;
        }
        anyhow::ensure!(
            ready,
            "service {label} not healthy after {HEALTH_POLL_TIMEOUT:?}"
        );
    }

    // Write OpenAPI spec files with the assigned ports
    let spec_a = write_spec(pa, include_str!("spec_a.json"))?;
    let spec_b = write_spec(pb, include_str!("spec_b.json"))?;
    // Run the Python stdio test suite
    let script = manifest_dir().join("tests/integration/end_to_end/test_mcp_stdio.py");
    let script_str = script.to_string_lossy().to_string();
    let binary = find_binary()?;
    let binary_str = binary.to_string_lossy().to_string();
    let manifest = manifest_dir();
    let (python, prefix_args) = find_python(&manifest.parent().unwrap().join("websearch"));
    let py_handle = tokio::task::spawn_blocking(move || {
        let mut cmd = Command::new(python);
        cmd.args(&prefix_args)
            .arg(&script_str)
            .arg(&binary_str)
            .arg(&spec_a)
            .arg(&spec_b)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
    });
    let py_output = timeout(Duration::from_secs(10), py_handle)
        .await
        .context("python3 timed out")?
        .context("python3 task panicked")?
        .context("python3 failed")?;

    let stdout = String::from_utf8_lossy(&py_output.stdout);
    let stderr = String::from_utf8_lossy(&py_output.stderr);

    // Print Python output for debugging
    if !stdout.is_empty() {
        print!("{}", stdout);
    }
    if !stderr.is_empty() {
        eprint!("{}", stderr);
    }

    anyhow::ensure!(
        py_output.status.success(),
        "Python tests failed (exit: {:?})",
        py_output.status.code()
    );
    Ok(())
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

fn manifest_dir() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}
