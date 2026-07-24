//! End-to-end stdio + HTTP tests for the IACR MCP server.

use std::process::{Child, Command, Stdio};
use std::time::Duration;

use paper_search_test_utils::{fixture_path, serve_fixture};
const BINARY_NAME: &str = "delulu-iacr-mcp";

fn find_binary() -> std::path::PathBuf {
    let ws = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf();
    for c in [
        ws.join("target").join("debug").join(BINARY_NAME),
        ws.join("target").join("release").join(BINARY_NAME),
    ] {
        if c.exists() {
            return c;
        }
    }
    panic!("Could not find {BINARY_NAME}");
}

fn get_free_port() -> u16 {
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    l.local_addr().unwrap().port()
}

async fn run_python(script: &str, args: &[String]) -> std::process::Output {
    let manifest = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let script_path = manifest.join("tests/integration").join(script);
    let args = args.to_vec();
    tokio::task::spawn_blocking(move || {
        Command::new("python3")
            .arg(&script_path)
            .args(&args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .expect("Failed to run Python test")
    })
    .await
    .expect("spawn_blocking panicked")
}

fn check_output(output: std::process::Output) {
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    print!("{}", stdout);
    if !stderr.is_empty() {
        eprint!("{}", stderr);
    }
    assert!(
        output.status.success(),
        "Python tests failed (exit: {:?})",
        output.status.code()
    );
}

#[tokio::test]
async fn test_iacr_mcp_e2e_stdio() {
    let path = fixture_path("paper-search-iacr", "iacr-rss.xml.zst");
    let (url, _s) = serve_fixture("/rss/rss.xml", path).await;
    let cfg = serde_json::json!({ "fixture_url": url });
    let cfg_path = std::env::temp_dir().join(format!("iacr_e2e_{}.json", std::process::id()));
    std::fs::write(&cfg_path, cfg.to_string()).unwrap();
    let output = run_python(
        "test_iacr_mcp_e2e_stdio.py",
        &[
            find_binary().to_string_lossy().to_string(),
            cfg_path.to_string_lossy().to_string(),
        ],
    )
    .await;
    let _ = std::fs::remove_file(&cfg_path);
    check_output(output);
}
struct ChildGuard(Child);

impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[tokio::test]
async fn test_iacr_mcp_e2e_http() {
    let path = fixture_path("paper-search-iacr", "iacr-rss.xml.zst");
    let (url, _s) = serve_fixture("/rss/rss.xml", path).await;
    let cfg = serde_json::json!({ "fixture_url": url });
    let cfg_path = std::env::temp_dir().join(format!("iacr_e2e_http_{}.json", std::process::id()));
    std::fs::write(&cfg_path, cfg.to_string()).unwrap();

    let binary = find_binary();
    let port = get_free_port();
    let child = Command::new(&binary)
        .args([
            "--api-base-url",
            &url,
            "http",
            "--host",
            "127.0.0.1",
            "--port",
            &port.to_string(),
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn MCP");

    let start = std::time::Instant::now();
    loop {
        if std::net::TcpStream::connect(("127.0.0.1", port)).is_ok() {
            break;
        }
        assert!(
            start.elapsed() < Duration::from_secs(10),
            "MCP server not ready"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    let _child = ChildGuard(child);

    let output = run_python(
        "test_iacr_mcp_e2e_http.py",
        &[port.to_string(), cfg_path.to_string_lossy().to_string()],
    )
    .await;
    let _ = std::fs::remove_file(&cfg_path);
    check_output(output);
}
