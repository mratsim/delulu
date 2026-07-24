//! End-to-end HTTP transport test for the arXiv MCP server.
//!
//! Follows the mcpify pattern:
//! 1. Start fixture server
//! 2. Spawn MCP server in HTTP mode
//! 3. Run Python script that connects via streamable_http_client

use paper_search_test_utils::{fixture_path, serve_fixture};
use std::process::{Child, Command, Stdio};
use std::time::Duration;
const PYTHON_SCRIPT: &str = "tests/integration/test_arxiv_mcp_e2e_http.py";
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

fn get_free_port() -> u16 {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.local_addr().unwrap().port()
}

struct ChildGuard(Child);

impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[tokio::test]
async fn test_arxiv_mcp_e2e_http() {
    let path = fixture_path("paper-search-arxiv", "arxiv-search-response.xml.zst");
    let (fixture_url, _shutdown) = serve_fixture("/api/query", path).await;
    let fixture_url = format!("{}/api/query", fixture_url);

    let config = serde_json::json!({ "fixture_url": fixture_url });
    let config_path =
        std::env::temp_dir().join(format!("arxiv_e2e_http_{}.json", std::process::id()));
    std::fs::write(&config_path, config.to_string()).unwrap();

    let binary = find_binary();
    let mcp_port = get_free_port();

    // Spawn MCP server in HTTP mode
    let mut mcp_child = Command::new(&binary)
        .args([
            "--api-base-url",
            &fixture_url,
            "http",
            "--host",
            "127.0.0.1",
            "--port",
            &mcp_port.to_string(),
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .expect("Failed to spawn MCP server");

    let _stderr_handle = std::thread::spawn({
        let stderr = mcp_child.stderr.take().unwrap();
        move || {
            use std::io::Read;
            let mut buf = String::new();
            std::io::BufReader::new(stderr)
                .read_to_string(&mut buf)
                .ok();
            if !buf.is_empty() {
                eprint!("{}", buf);
            }
        }
    });

    let _mcp_child = ChildGuard(mcp_child);

    // Wait for MCP server to be ready
    let start = std::time::Instant::now();
    let timeout = Duration::from_secs(10);
    loop {
        if std::net::TcpStream::connect(("127.0.0.1", mcp_port)).is_ok() {
            break;
        }
        if start.elapsed() > timeout {
            panic!("MCP server not ready on port {mcp_port} after {timeout:?}");
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    // Run Python test
    let script = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(PYTHON_SCRIPT);
    let config_path_arg = config_path.clone();

    let output = tokio::time::timeout(
        Duration::from_secs(15),
        tokio::task::spawn_blocking(move || {
            Command::new("python3")
                .arg(&script)
                .arg(mcp_port.to_string())
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

    // Cleanup (ChildGuard handles process kill via Drop)
    let _ = std::fs::remove_file(&config_path);

    assert!(
        output.status.success(),
        "Python HTTP e2e tests failed (exit: {:?})",
        output.status.code()
    );
}
