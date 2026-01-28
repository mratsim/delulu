//! Shared MCP test helpers for streaming subprocess output.

use anyhow::Result;
use std::path::PathBuf;
use tokio::io::AsyncReadExt;

pub fn find_binary() -> Result<PathBuf> {
    let manifest_dir = PathBuf::from(
        std::env::var("CARGO_MANIFEST_DIR")
            .map_err(|e| anyhow::anyhow!("CARGO_MANIFEST_DIR not set: {}", e))?,
    );
    let workspace_root = manifest_dir
        .parent()
        .and_then(|p| p.parent())
        .ok_or_else(|| anyhow::anyhow!("Could not determine workspace root"))?;

    let paths = [
        workspace_root.join("target/debug/delulu-travel-mcp"),
        workspace_root.join("target/release/delulu-travel-mcp"),
    ];

    for path in &paths {
        if path.exists() {
            return Ok(path.to_path_buf());
        }
    }
    anyhow::bail!(
        "Could not find delulu-travel-mcp binary. Run `cargo build -p delulu-travel-agent --features mcp` first. Searched: {:?}",
        paths
    )
}

pub async fn stream_stderr_to_console(mut stderr: tokio::process::ChildStderr) {
    let mut buf = [0u8; 4096];
    while let Ok(n) = stderr.read(&mut buf).await {
        if n == 0 {
            break;
        }
        let output = String::from_utf8_lossy(&buf[..n]);
        eprint!("{}", output);
        if output.contains("input stream terminated") {
            // rmcp magic string produced when stdin is dropped
            break;
        }
    }
}
