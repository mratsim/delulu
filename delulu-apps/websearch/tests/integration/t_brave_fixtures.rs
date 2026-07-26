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

//! Fixture-based integration tests for Brave search result parsing.
//!
//! These tests load real captured Brave HTML responses (zstd-compressed)
//! and verify that `parse_search_results` correctly extracts titles, URLs,
//! snippets, and dates.
//!
//! ============================================================================
//! These tests do NOT make HTTP requests — they use on-disk fixtures.
//! ============================================================================

use std::io::Read;
use std::path::PathBuf;

use delulu_websearch::engines::brave::parse_search_results;

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
// Brave fixture tests
// ---------------------------------------------------------------------------

#[test]
fn brave_fixture_parse_search_results() {
    let html = load_fixture("brave/20260724-hashing-to-elliptic-curves/response.html.zst");

    let results = parse_search_results(&html, 20)
        .expect("parse_search_results should succeed on real Brave HTML");

    // There are 19 snippet divs in the fixture; some may be filtered (empty
    // title, search.brave.com internal links). Expect a reasonable number.
    assert!(
        results.len() >= 3,
        "Expected at least 3 results from Brave fixture, got {}",
        results.len()
    );

    // Verify each result has required fields
    for (i, result) in results.iter().enumerate() {
        assert!(!result.title.is_empty(), "Result {} has empty title", i);
        assert!(!result.url.is_empty(), "Result {} has empty URL", i);
        // URL must be valid
        assert!(
            url::Url::parse(&result.url).is_ok(),
            "Result {} has malformed URL: {}",
            i,
            result.url
        );
    }

    // Verify first result has expected structure
    let first = &results[0];
    assert!(
        first.url.starts_with("https://"),
        "First result URL should start with https://, got: {}",
        first.url
    );
}

#[test]
fn brave_fixture_parse_with_max_results() {
    let html = load_fixture("brave/20260724-hashing-to-elliptic-curves/response.html.zst");

    // Parse with max_results = 3
    let results = parse_search_results(&html, 3)
        .expect("parse_search_results should succeed with max_results=3");

    assert!(
        results.len() <= 3,
        "Expected at most 3 results with max_results=3, got {}",
        results.len()
    );
}

#[test]
fn brave_fixture_results_have_dates() {
    let html = load_fixture("brave/20260724-hashing-to-elliptic-curves/response.html.zst");

    let results = parse_search_results(&html, 20).expect("parse_search_results should succeed");

    // At least some results should have dates (the fixture has t-secondary spans)
    let results_with_dates: Vec<_> = results.iter().filter(|r| r.date.is_some()).collect();
    assert!(
        !results_with_dates.is_empty(),
        "Expected at least one result with a date from Brave fixture"
    );

    // Verify dates are reasonable Unix timestamps (2020-2030 range)
    for result in &results_with_dates {
        let date = result.date.unwrap();
        assert!(
            date >= 1577836800, // 2020-01-01
            "Date {} is before 2020 for result: {}",
            date,
            result.title
        );
        assert!(
            date <= 1893456000, // 2030-01-01
            "Date {} is after 2030 for result: {}",
            date,
            result.title
        );
    }
}

#[test]
fn brave_fixture_results_have_snippets() {
    let html = load_fixture("brave/20260724-hashing-to-elliptic-curves/response.html.zst");

    let results = parse_search_results(&html, 20).expect("parse_search_results should succeed");

    // Most results should have snippets
    let results_with_snippets: Vec<_> = results.iter().filter(|r| r.snippet.is_some()).collect();
    assert!(
        !results_with_snippets.is_empty(),
        "Expected at least one result with a snippet from Brave fixture"
    );
}

#[test]
fn brave_fixture_no_pow_captcha() {
    let html = load_fixture("brave/20260724-hashing-to-elliptic-curves/response.html.zst");

    // The fixture is a real search results page, not a captcha page
    let result = parse_search_results(&html, 20);
    assert!(
        result.is_ok(),
        "Brave fixture should not trigger captcha detection"
    );
}
