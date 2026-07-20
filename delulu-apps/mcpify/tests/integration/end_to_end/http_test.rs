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
async fn test_e2e_http_transport() -> Result<()> {
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
            if health_check(p).await { ready = true; break; }
            tokio::time::sleep(HEALTH_POLL_INTERVAL).await;
        }
        anyhow::ensure!(ready, "service {label} not healthy after {HEALTH_POLL_TIMEOUT:?}");
    }

    // Write OpenAPI spec files with the assigned ports
    let spec_a = write_spec(pa, include_str!("spec_a.json"))?;
    let spec_b = write_spec(pb, include_str!("spec_b.json"))?;

    // Allocate ports for mcpify instances
    let pa2 = get_free_port();
    let pb2 = get_free_port();

    // Start mcpify instances
    let binary = find_binary()?;
    let mut ca: Option<std::process::Child> = None;
    let mut cb: Option<std::process::Child> = None;
    let mut sea: Option<tokio::task::JoinHandle<()>> = None;
    let mut seb: Option<tokio::task::JoinHandle<()>> = None;

    let mut c = Command::new(&binary)
        .args(["http", "--host", "127.0.0.1", "--port", &pa2.to_string(), &spec_a])
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .context("spawn mcpify A")?;
    sea = Some(stream_stderr_to_console(c.stderr.take().expect("stderr")));
    ca = Some(c);

    let mut c = Command::new(&binary)
        .args(["http", "--host", "127.0.0.1", "--port", &pb2.to_string(), &spec_b])
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .context("spawn mcpify B")?;
    seb = Some(stream_stderr_to_console(c.stderr.take().expect("stderr")));
    cb = Some(c);

    // Create drop guard for mcpify children
    let _mcpify_guard = E2eGuard {
        shutdown_senders: vec![],
        _server_tasks: vec![],
        children: vec![ca, cb],
        _stderr_tasks: vec![sea, seb],
    };

    // Wait for mcpify instances to be ready (TCP connect, no /health endpoint)
    for (p, label) in [(pa2, "mcpify A"), (pb2, "mcpify B")] {
        let start = std::time::Instant::now();
        let mut ready = false;
        while start.elapsed() < HEALTH_POLL_TIMEOUT {
            if tokio::net::TcpStream::connect(("127.0.0.1", p)).await.is_ok() { ready = true; break; }
            tokio::time::sleep(HEALTH_POLL_INTERVAL).await;
        }
        anyhow::ensure!(ready, "{label} not ready after {HEALTH_POLL_TIMEOUT:?}");
    }

    // Run the Python MCP test suite
    let script = manifest_dir().join("tests/integration/end_to_end/test_mcp_http.py");
    let script_str = script.to_string_lossy().to_string();
    let binary = find_binary()?;
    let binary_str = binary.to_string_lossy().to_string();
    let py_handle = tokio::task::spawn_blocking(move || {
        Command::new("python3")
            .arg(&script_str)
            .arg(&binary_str)
            .arg(pa2.to_string())
            .arg(pb2.to_string())
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
    if !stdout.is_empty() { print!("{}", stdout); }
    if !stderr.is_empty() { eprint!("{}", stderr); }

    anyhow::ensure!(py_output.status.success(), "Python tests failed (exit: {:?})", py_output.status.code());
    Ok(())
}

fn manifest_dir() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}
