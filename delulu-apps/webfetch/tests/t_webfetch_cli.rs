//! CLI integration tests for the `delulu-fetch` binary.
//!
//! These tests run the compiled binary as a subprocess and verify its output.
//! The binary must be built with `--features cli` before running these tests:
//!
//! ```bash
//! cargo build --release -p delulu-webfetch-agent --features cli
//! ```

use std::path::PathBuf;
use std::process::Command;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Path to the compiled `delulu-fetch` binary.
fn binary_path() -> PathBuf {
    let base: PathBuf = [
        env!("CARGO_MANIFEST_DIR"),
        "..",
        "..",
        "target",
        "release",
        "delulu-fetch",
    ]
    .iter()
    .collect();
    base
}

fn fixture_path(name: &str) -> PathBuf {
    let base: PathBuf = [env!("CARGO_MANIFEST_DIR"), "tests", "fixtures-webfetch"]
        .iter()
        .collect();
    base.join(name)
}

/// Build a `Command` pointing at `delulu-fetch`.
fn fetch_command() -> Command {
    Command::new(binary_path())
}

// ---------------------------------------------------------------------------
// Help
// ---------------------------------------------------------------------------

#[test]
fn test_cli_help_exits_successfully() {
    let output = fetch_command()
        .arg("--help")
        .output()
        .expect("failed to run delulu-fetch --help");

    assert!(
        output.status.success(),
        "delulu-fetch --help should exit 0\nstderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Usage") || stdout.contains("delulu-fetch"),
        "stdout should contain usage info, got: {stdout}"
    );
}

// ---------------------------------------------------------------------------
// Error handling
// ---------------------------------------------------------------------------

#[test]
fn test_cli_invalid_url_scheme_exits_with_error() {
    // An unsupported scheme (ftp) fails URL validation before any HTTP call.
    let output = fetch_command()
        .arg("ftp://example.com/file")
        .output()
        .expect("failed to run delulu-fetch");

    assert!(
        !output.status.success(),
        "delulu-fetch with invalid URL scheme should exit non-zero"
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Error") || stderr.contains("error") || stderr.contains("Invalid"),
        "stderr should contain error info, got: {stderr}"
    );
}

#[test]
fn test_cli_invalid_url_no_host_exits_with_error() {
    let output = fetch_command()
        .arg("https:///path")
        .output()
        .expect("failed to run delulu-fetch");

    assert!(
        !output.status.success(),
        "delulu-fetch with malformed URL should exit non-zero"
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!stderr.is_empty(), "stderr should contain error message");
}

#[test]
fn test_cli_missing_url_exits_with_error() {
    // No URL argument provided.
    let output = fetch_command()
        .output()
        .expect("failed to run delulu-fetch without args");

    assert!(
        !output.status.success(),
        "delulu-fetch without a URL should exit non-zero"
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    // Clap outputs usage info on stderr when required argument is missing.
    assert!(
        stderr.contains("error") || stderr.contains("required") || stderr.contains("Usage"),
        "stderr should indicate a missing argument, got: {stderr}"
    );
}

// ---------------------------------------------------------------------------
// Test-mode: fixture-based extraction (no live HTTP)
// ---------------------------------------------------------------------------

#[test]
fn test_cli_test_mode_missing_fixture_exits_with_error() {
    let fixtures_dir = fixture_path(".");
    let fixtures_dir_str = fixtures_dir.to_string_lossy().to_string();

    // Use a URL that base64-encodes to something that won't exist in fixtures.
    let output = fetch_command()
        .arg("--test-mode")
        .arg(&fixtures_dir_str)
        .arg("https://nonexistent.example.com/page")
        .output()
        .expect("failed to run delulu-fetch");

    assert!(
        !output.status.success(),
        "delulu-fetch with missing fixture should exit non-zero"
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Error") || stderr.contains("error"),
        "stderr should contain error, got: {stderr}"
    );
}

// ---------------------------------------------------------------------------
// Doc subcommand tests
// ---------------------------------------------------------------------------

#[test]
fn test_cli_doc_subcommand_is_registered() {
    // Verify that 'doc' is handled as a subcommand (not an unknown command)
    // The doc subcommand is implemented via manual argv parsing, not clap subcommands.
    // Running 'doc' without a URL should fail with a URL-related error, not an
    // "unrecognized argument" error.
    let output = fetch_command()
        .arg("doc")
        .output()
        .expect("failed to run delulu-fetch doc");

    // Should exit non-zero (missing URL)
    assert!(
        !output.status.success(),
        "delulu-fetch doc without URL should exit non-zero"
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("required") || stderr.contains("error") || stderr.contains("url")
        || stderr.contains("URL"),
        "stderr should indicate missing URL argument, got: {stderr}"
    );
}

#[test]
fn test_cli_doc_subcommand_help_shows_url_arg() {
    // Verify that 'doc --help' shows the URL argument
    let output = fetch_command()
        .arg("doc")
        .arg("--help")
        .output()
        .expect("failed to run delulu-fetch doc --help");

    assert!(
        output.status.success(),
        "delulu-fetch doc --help should exit 0"
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("URL") || stdout.contains("url"),
        "doc --help should mention URL argument, got: {stdout}"
    );
}

