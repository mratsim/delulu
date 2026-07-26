//!  Delulu Web Search
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

//! Fixture-based integration tests for DuckDuckGo search result parsing.
//!
//! These tests load real captured DuckDuckGo responses (zstd-compressed)
//! and verify that `parse_djs_response` correctly handles them.
//!
//! ============================================================================
//! These tests do NOT make HTTP requests — they use on-disk fixtures.
//! ============================================================================

use std::io::Read;
use std::path::PathBuf;

use delulu_websearch::WebsearchError;
use delulu_websearch::engines::duckduckgo::DuckDuckGoEngine;

/// Resolve the path to the websearch test fixtures directory.
fn fixture_dir() -> PathBuf {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest.join("tests/fixtures")
}

/// Load a zstd-compressed fixture and return the decompressed content.
fn load_fixture(relative_path: &str) -> String {
    let path = fixture_dir().join(relative_path);
    let compressed = std::fs::read(&path)
        .unwrap_or_else(|e| panic!("Failed to read fixture '{}': {e}", path.display()));

    let mut decoder = zstd::Decoder::new(compressed.as_slice()).unwrap_or_else(|e| {
        panic!(
            "Failed to create zstd decoder for '{}': {e}",
            path.display()
        )
    });

    let mut content = String::new();
    decoder
        .read_to_string(&mut content)
        .unwrap_or_else(|e| panic!("Failed to decompress fixture '{}': {e}", path.display()));
    content
}

// ---------------------------------------------------------------------------
// DuckDuckGo fixture tests
// ---------------------------------------------------------------------------

#[test]
fn duckduckgo_fixture_detects_block() {
    // The captured fixture is an HTML challenge/block page from DuckDuckGo's
    // Lite interface. It contains an anomaly detection form (challenge-form).
    // Since this is NOT a d.js response, `parse_djs_response` will fail to
    // find the `DDG.pageLayout.load('d',` marker and return ParseFailed.
    //
    // Note: The fixture does NOT contain the `DDG.deep.initialize(` or
    // `DDG.deep.anomalyDetectionBlock({` JavaScript markers that would
    // trigger AccessDenied — those only appear in d.js responses, not in
    // the initial HTML challenge page.
    let body = load_fixture("duckduckgo/20260724-hashing-to-elliptic-curves/response.html.zst");

    let result = DuckDuckGoEngine::parse_djs_response(&body, 20);

    // The response is an HTML challenge page, not a d.js response,
    // so parsing should fail.
    assert!(
        result.is_err(),
        "Expected parse_djs_response to fail on HTML challenge page"
    );

    match result {
        Err(WebsearchError::ParseFailed { parser, .. }) => {
            assert_eq!(
                parser, "duckduckgo_djs",
                "Expected duckduckgo_djs parser to fail"
            );
        }
        Err(WebsearchError::AccessDenied) => {
            // This would also be acceptable — the fixture is a block page.
        }
        Err(other) => {
            panic!(
                "Unexpected error type: {}. Expected ParseFailed or AccessDenied",
                other
            );
        }
        Ok(_) => {
            panic!("Expected error but got Ok");
        }
    }
}

#[test]
fn duckduckgo_fixture_not_valid_djs() {
    // Verify the fixture does NOT contain d.js response markers
    let body = load_fixture("duckduckgo/20260724-hashing-to-elliptic-curves/response.html.zst");

    // The fixture is HTML, not a d.js JavaScript response
    assert!(
        body.contains("<!DOCTYPE html>") || body.starts_with("<!doctype html>"),
        "Fixture should be an HTML page"
    );
    assert!(
        !body.contains("DDG.pageLayout.load('d',"),
        "Fixture should not contain d.js marker"
    );
}

#[test]
fn duckduckgo_fixture_contains_challenge_form() {
    // The fixture contains DuckDuckGo's anomaly detection challenge form
    let body = load_fixture("duckduckgo/20260724-hashing-to-elliptic-curves/response.html.zst");

    assert!(
        body.contains("challenge-form"),
        "Fixture should contain a challenge form"
    );
    assert!(
        body.contains("anomaly"),
        "Fixture should contain anomaly detection content"
    );
}

#[test]
fn duckduckgo_fixture_parse_with_max_results() {
    let body = load_fixture("duckduckgo/20260724-hashing-to-elliptic-curves/response.html.zst");

    // Verify max_results parameter doesn't affect the error result
    let result_small = DuckDuckGoEngine::parse_djs_response(&body, 1);
    let result_large = DuckDuckGoEngine::parse_djs_response(&body, 100);

    assert_eq!(
        result_small.is_err(),
        result_large.is_err(),
        "max_results should not affect error detection"
    );
}
