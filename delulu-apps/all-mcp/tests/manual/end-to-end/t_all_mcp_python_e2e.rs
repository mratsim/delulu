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
//! `delulu-all-mcp` binary. The suite covers the all-mcp-specific behavior
//! the per-package tests cannot: the 21-tool union, the prefixed
//! `*_get_paper` names, the unknown-tool error path and the bare-`get_paper`
//! did-you-mean hint, all over the official MCP SDK (stdio + HTTP).

#![cfg(test)]
#![cfg(feature = "mcp")]

use anyhow::{Context, Result};
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

/// Spawn one Python MCP SDK test script with the binary and assert it exits 0.
async fn run_python_script(script: &std::path::Path, binary: &std::path::Path) -> Result<()> {
    let manifest = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let script_str = script.to_string_lossy().to_string();
    let binary_str = binary.to_string_lossy().to_string();

    let (python, prefix_args) = find_python(&manifest);
    let py_handle = tokio::task::spawn_blocking(move || {
        let mut cmd = std::process::Command::new(python);
        cmd.args(&prefix_args)
            .arg(&script_str)
            .arg(&binary_str)
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
    let binary = find_binary()?;
    let manifest = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let script = manifest.join("tests/manual/end-to-end/test_all_mcp_stdio.py");
    run_python_script(&script, &binary).await
}

#[tokio::test(flavor = "multi_thread")]
async fn test_all_mcp_python_e2e_http() -> Result<()> {
    let binary = find_binary()?;
    let manifest = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let script = manifest.join("tests/manual/end-to-end/test_all_mcp_http.py");
    run_python_script(&script, &binary).await
}
