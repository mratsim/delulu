use anyhow::{Context, Result};
use std::path::PathBuf;
use tokio::io::AsyncReadExt;
use tokio::sync::oneshot;
use tokio::task::JoinHandle;
use tokio::time::{Duration, timeout};

/// Timeout for individual MCP RPC operations (initialize, list_tools, call_tool).
pub const READ_TIMEOUT: Duration = Duration::from_secs(3);
/// Timeout for the full MCP initialization handshake (includes server startup).
pub const INIT_TIMEOUT: Duration = Duration::from_secs(10);

/// Locate the compiled `mcpify` binary in `target/debug/` or `target/release/`.
/// Requires `cargo build -p delulu-mcpify --features mcp` to have been run first.
pub fn find_binary() -> Result<PathBuf> {
    let root = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").map_err(|e| anyhow::anyhow!("CARGO_MANIFEST_DIR: {e}"))?)
        .parent().and_then(|p| p.parent()).ok_or_else(|| anyhow::anyhow!("no workspace root"))?.to_path_buf();
    for p in [root.join("target/debug/delulu-mcpify"), root.join("target/release/delulu-mcpify")] {
        if p.exists() { return Ok(p); }
    }
    anyhow::bail!("delulu-mcpify binary not found; run `cargo build -p delulu-mcpify --features mcp`")
}

/// Bind to `127.0.0.1:0` and return the OS-assigned free port.
/// Infallible in practice — panics only if no port is available.
pub fn get_free_port() -> u16 {
    let l = std::net::TcpListener::bind("127.0.0.1:0").expect("bind 127.0.0.1:0");
    l.local_addr().expect("local_addr").port()
}

/// Spawn a blocking task that copies a subprocess's stderr to `eprint!`.
/// Errors in the streaming task are intentionally swallowed (best-effort diagnostics).
pub fn stream_stderr_to_console(stderr: std::process::ChildStderr) -> JoinHandle<()> {
    tokio::task::spawn_blocking(move || {
        use std::io::Read;
        let mut r = stderr;
        let mut buf = [0u8; 4096];
        while let Ok(n) = r.read(&mut buf) { if n == 0 { break; } eprint!("{}", String::from_utf8_lossy(&buf[..n])); }
    })
}

/// Write an OpenAPI spec JSON string to a temp file, replacing `{PORT}` with the given port.
/// Returns the file path. Errors if file I/O fails.
pub fn write_spec(port: u16, spec_json: &str) -> Result<String> {
    let s = spec_json.replace("{PORT}", &port.to_string());
    assert!(!s.contains("{PORT}"), "{{PORT}} not replaced");
    let p = std::env::temp_dir().join(format!("spec_{}_{}.json", port, std::process::id()));
    std::fs::write(&p, &s).with_context(|| format!("write {:?}", p))?;
    Ok(p.to_string_lossy().to_string())
}

/// Poll a port's HTTP `/health` endpoint until it returns 200, or until timeout.
/// Returns `true` if the endpoint responded successfully.
pub async fn health_check(port: u16) -> bool {
    timeout(Duration::from_millis(500), async {
        if let Ok(mut s) = tokio::net::TcpStream::connect(("127.0.0.1", port)).await {
            let req = format!("GET /health HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n");
            let _ = tokio::io::AsyncWriteExt::write_all(&mut s, req.as_bytes()).await;
            let mut buf = [0u8; 128];
            if let Ok(n) = s.read(&mut buf).await {
                let resp = String::from_utf8_lossy(&buf[..n]);
                return resp.starts_with("HTTP/1.1 200");
            }
        }
        false
    }).await.unwrap_or(false)
}

/// Initialise tracing once per process. Safe to call multiple times.
pub fn init_tracing() {
    let _ = tracing_subscriber::fmt().with_env_filter(tracing_subscriber::EnvFilter::from_default_env()).try_init();
}

/// Drop guard that ensures E2E test services are cleaned up on early return.
/// Sends shutdown signals to backend services and kills child processes.
pub struct E2eGuard {
    /// Backend service shutdown senders
    pub shutdown_senders: Vec<oneshot::Sender<()>>,
    /// Backend service JoinHandles (not awaited in Drop, but kept alive)
    pub _server_tasks: Vec<JoinHandle<()>>,
    /// mcpify child processes
    pub children: Vec<Option<std::process::Child>>,
    /// stderr streaming tasks (not awaited in Drop)
    pub _stderr_tasks: Vec<Option<JoinHandle<()>>>,
}

impl Drop for E2eGuard {
    fn drop(&mut self) {
        // Kill child processes
        for c in &mut self.children {
            if let Some(c) = c {
                let _ = c.kill();
                let _ = c.wait();
            }
        }
        // Send shutdown signals to backend services
        for s in self.shutdown_senders.drain(..) {
            let _ = s.send(());
        }
        // Note: server_tasks and stderr_tasks are NOT awaited here because
        // Drop cannot be async. The runtime will clean them up.
    }
}
