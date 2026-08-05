//!  Delulu All-MCP — e2e CLI assertions
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

//! CLI assertions for the 7 MCP binaries:
//! - every binary's `--help` prints the command name and the `stdio`/`http`
//!   subcommands;
//! - all-mcp's `--help` shows the 7 merged flags and the 3 `--*-api-base-url`
//!   defaults match the standalone binaries' `--api-base-url` defaults;
//! - `--expose-local-networks` is default-off (the live SSRF proof is 6c; here
//!   we assert the flag exists and no default-on indication is rendered).
//!
//! Offline: invokes the installed release binaries with `--help`; each must
//! exit 0.

mod mcp_helpers;

use mcp_helpers::find_binary;
use std::process::Output;

const ALL_BINS: &[&str] = &[
    "delulu-webfetch-mcp",
    "delulu-websearch-mcp",
    "delulu-travel-mcp",
    "delulu-arxiv-mcp",
    "delulu-iacr-mcp",
    "delulu-pubmed-mcp",
    "delulu-all-mcp",
];

const MERGED_FLAGS: &[&str] = &[
    "--expose-local-networks",
    "--arxiv-api-base-url",
    "--iacr-api-base-url",
    "--pubmed-api-base-url",
    "--qps",
    "--burst",
    "--max-resp-size-mb",
];

/// Run `<bin> --help`, assert exit code 0, and return the captured output.
fn help_output(bin: &str) -> Output {
    let path = find_binary(bin).unwrap_or_else(|e| panic!("find_binary({bin}): {e}"));
    let out = std::process::Command::new(&path)
        .arg("--help")
        .output()
        .unwrap_or_else(|e| panic!("failed to run {bin} --help: {e}"));
    assert_eq!(
        out.status.code(),
        Some(0),
        "{bin} --help must exit 0; stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    out
}

fn stdout_str(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).to_string()
}

/// For all 7 binaries: `--help` exits 0, prints the command name, and lists the
/// `stdio`/`http` subcommands.
#[test]
fn help_shows_command_name_and_transport_subcommands() {
    for bin in ALL_BINS {
        let text = stdout_str(&help_output(bin));
        assert!(
            text.contains(bin),
            "{bin} --help must contain its own command name; got:\n{text}"
        );
        assert!(
            text.contains("stdio"),
            "{bin} --help must list the stdio subcommand; got:\n{text}"
        );
        assert!(
            text.contains("http"),
            "{bin} --help must list the http subcommand; got:\n{text}"
        );
    }
}

/// The all-mcp help shows all 7 merged flags.
#[test]
fn all_mcp_help_shows_merged_flags() {
    let text = stdout_str(&help_output("delulu-all-mcp"));
    for flag in MERGED_FLAGS {
        assert!(
            text.contains(flag),
            "all-mcp --help must show merged flag '{flag}'; got:\n{text}"
        );
    }
}

/// The 3 `--*-api-base-url` defaults in all-mcp equal the standalone binaries'
/// `--api-base-url` defaults.
#[test]
fn all_mcp_api_base_url_defaults_match_standalone() {
    let all = stdout_str(&help_output("delulu-all-mcp"));

    let cases = [
        (
            "delulu-arxiv-mcp",
            "--arxiv-api-base-url",
            "https://export.arxiv.org/api/query",
        ),
        (
            "delulu-iacr-mcp",
            "--iacr-api-base-url",
            "https://eprint.iacr.org",
        ),
        (
            "delulu-pubmed-mcp",
            "--pubmed-api-base-url",
            "https://eutils.ncbi.nlm.nih.gov/entrez/eutils",
        ),
    ];

    for (standalone_bin, flag, expected_default) in cases {
        let standalone = stdout_str(&help_output(standalone_bin));
        assert!(
            standalone.contains(&format!("[default: {expected_default}]")),
            "{standalone_bin} --help must render clap default '[default: {expected_default}]'; got:\n{standalone}"
        );
        assert!(
            all.contains(flag),
            "all-mcp --help must show flag '{flag}'; got:\n{all}"
        );
        assert!(
            all.contains(&format!("[default: {expected_default}]")),
            "all-mcp --help must render clap default '[default: {expected_default}]' for {flag}; got:\n{all}"
        );
    }
}

/// `--expose-local-networks` exists and is default-off (no default-on render).
#[test]
fn expose_local_networks_is_present_and_default_off() {
    let text = stdout_str(&help_output("delulu-all-mcp"));
    assert!(
        text.contains("--expose-local-networks"),
        "all-mcp --help must list --expose-local-networks; got:\n{text}"
    );
    // clap renders a `default_value_t = false` bool flag as a bare optional
    // flag. A default-on boolean would render as `[default: true]` (or with an
    // explicit `(default: true)` marker). Assert no default-on indication.
    for line in text.lines() {
        if line.contains("expose-local-networks") {
            let lower = line.to_lowercase();
            assert!(
                !lower.contains("default: true") && !lower.contains("(default: true)"),
                "all-mcp --expose-local-networks must not render a default-on indication; got:\n{line}"
            );
        }
    }
}
