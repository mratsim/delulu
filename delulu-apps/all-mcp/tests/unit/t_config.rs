//!  Delulu All-MCP — flag validation tests
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

//! Release-binary flag validation.
//!
//! Spawns the **release** `delulu-all-mcp` binary (which the test assumes
//! already exists — it is built by the phase's first checkpoint, not on
//! demand) and asserts that out-of-range rate/size flags and a malformed
//! base-URL flag are rejected by clap at parse time: a non-zero exit and
//! `status.code() == Some(2)` (clap usage error), with non-empty stderr.
//!
//! We deliberately do **not** use `CARGO_BIN_EXE_*`, which resolves to the
//! debug binary: the validation must run against the release build.

use std::path::Path;
use std::path::PathBuf;
use std::process::{Command, Output};

/// Locate the release `delulu-all-mcp` binary via the workspace root.
///
/// Unlike the webfetch helper (which prefers debug), this prefers release —
/// the release binary is produced by the phase's first checkpoint and is the
/// exact artifact the tests must exercise.
fn find_release_binary() -> PathBuf {
    let manifest_dir = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let workspace_root = manifest_dir
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root is two levels above the crate");
    let release = workspace_root.join("target/release/delulu-all-mcp");
    assert!(
        release.exists(),
        "release binary missing at {release:?}: run the release-build checkpoint \
         `cargo build --release -p delulu-all-mcp --features mcp` first"
    );
    release
}

/// Run the release binary with the given extra args and assert a clap usage
/// error: non-zero exit and `status.code() == Some(2)`, with usage on stderr.
///
/// `stdio` must be supplied as the subcommand so that the value_parser for
/// each flag fires (clap parses flags regardless, but this mirrors the real
/// invocation path).
fn assert_usage_error(binary: &Path, args: &[&str]) {
    let mut cmd = Command::new(binary);
    cmd.args(args).arg("stdio");

    let output: Output = cmd.output().expect("spawn release binary");
    let code = output.status.code();
    assert!(
        !output.status.success(),
        "expected non-zero exit for args {args:?}, got success"
    );
    assert_eq!(
        code,
        Some(2),
        "expected clap usage-error exit code 2 for args {args:?}, got {code:?}"
    );
    assert!(
        !output.stderr.is_empty(),
        "expected usage-error diagnostic on stderr for args {args:?}"
    );
}

/// `--qps 0` is below the allowed range (1..=10000).
#[test]
fn qps_zero_rejected() {
    let binary = find_release_binary();
    assert_usage_error(&binary, &["--qps", "0"]);
}

/// `--burst 0` is below the allowed range (1..=10000).
#[test]
fn burst_zero_rejected() {
    let binary = find_release_binary();
    assert_usage_error(&binary, &["--burst", "0"]);
}

/// `--max-resp-size-mb 0` is below the allowed range (1..=1024).
#[test]
fn max_resp_size_mb_zero_rejected() {
    let binary = find_release_binary();
    assert_usage_error(&binary, &["--max-resp-size-mb", "0"]);
}

/// `--max-resp-size-mb 1025` is above the allowed range (1..=1024).
#[test]
fn max_resp_size_mb_over_rejected() {
    let binary = find_release_binary();
    assert_usage_error(&binary, &["--max-resp-size-mb", "1025"]);
}

/// A malformed `--arxiv-api-base-url` fails the URL value_parser.
#[test]
fn arxiv_base_url_malformed_rejected() {
    let binary = find_release_binary();
    assert_usage_error(&binary, &["--arxiv-api-base-url", "not-a-url"]);
}