//!  Delulu IACR Paper Search — Client Unit Tests
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

//! # IACR Client Unit Tests
//!
//! Tests for `IacrClient` — builder methods, URL construction, and HTTP
//! integration with fixture data via local server.
//!
//! NOTE: This file is included via `#[path]` in `src/lib.rs`, so all paths
//! are relative to the crate root (`crate::`).

use crate::IacrClient;

// ---------------------------------------------------------------------------
// Builder / configuration tests
// ---------------------------------------------------------------------------

#[test]
fn test_new_creates_client_with_defaults() {
    let client = IacrClient::new(30).expect("new() should succeed");
    // Verify the client was constructed by testing a method
    let url = client.download_paper_pdf(2024, 123);
    assert_eq!(url, "https://eprint.iacr.org/2024/123.pdf");
}

#[test]
fn test_with_base_url_custom() {
    let client = IacrClient::with_base_url(30, "http://localhost:9999".to_string())
        .expect("with_base_url should succeed");
    let url = client.download_paper_pdf(2024, 123);
    assert_eq!(url, "http://localhost:9999/2024/123.pdf");
}

// ---------------------------------------------------------------------------
// URL construction tests
// ---------------------------------------------------------------------------

#[test]
fn test_download_paper_pdf_uses_base_url() {
    let client = IacrClient::with_base_url(30, "https://eprint.iacr.org".to_string())
        .expect("with_base_url should succeed");

    // download_paper_pdf uses a simple format! without zero-padding
    let url = client.download_paper_pdf(2024, 123);
    assert_eq!(url, "https://eprint.iacr.org/2024/123.pdf");

    // Note: download_paper_pdf does NOT zero-pad (iacr_pdf_url does)
    let url = client.download_paper_pdf(2004, 5);
    assert_eq!(url, "https://eprint.iacr.org/2004/5.pdf");
}

#[test]
fn test_download_paper_pdf_with_custom_base() {
    let client = IacrClient::with_base_url(30, "http://localhost:9999".to_string())
        .expect("with_base_url should succeed");

    let url = client.download_paper_pdf(2024, 123);
    assert_eq!(url, "http://localhost:9999/2024/123.pdf");
}

// ---------------------------------------------------------------------------
// HTTP integration tests with fixture data
// ---------------------------------------------------------------------------

/// Test that the IACR client can list recent papers using fixture data.
#[tokio::test]
async fn test_list_recent_papers_with_fixture() {
    let path = paper_search_test_utils::fixture_path(
        "paper-search-iacr",
        "iacr-rss.xml.zst",
    );
    let (url, _shutdown) = paper_search_test_utils::serve_fixture("/rss/rss.xml", path).await;
    let base_url = url.clone();

    let client = IacrClient::with_base_url(5, base_url)
        .expect("failed to create client");

    let papers = client
        .list_recent_papers()
        .await
        .expect("list_recent_papers should succeed");

    assert!(!papers.is_empty(), "should return at least one paper");
    assert!(!papers[0].id.is_empty(), "paper should have an ID");
    assert!(!papers[0].title.is_empty(), "paper should have a title");
}

/// Test that the IACR client can get paper details using fixture data.
#[tokio::test]
async fn test_get_paper_details_with_fixture() {
    let path = paper_search_test_utils::fixture_path(
        "paper-search-iacr",
        "iacr-paper.html.zst",
    );
    let (url, _shutdown) = paper_search_test_utils::serve_fixture("/2025/1", path).await;
    let base_url = url.clone();

    let client = IacrClient::with_base_url(5, base_url)
        .expect("failed to create client");

    let paper = client
        .get_paper_details(2025, 1)
        .await
        .expect("get_paper_details should succeed");

    assert_eq!(paper.id, "2025/1");
    assert_eq!(paper.year, 2025);
    assert_eq!(paper.number, 1);
    assert!(!paper.title.is_empty(), "paper should have a title");
    assert!(!paper.authors.is_empty(), "paper should have authors");
}

/// Test that a request to an unreachable server returns an error.
#[tokio::test]
async fn test_list_recent_papers_connection_refused() {
    let client = IacrClient::with_base_url(2, "http://127.0.0.1:1".to_string())
        .expect("failed to create client");

    let result = client.list_recent_papers().await;
    assert!(result.is_err(), "request to invalid endpoint should fail");
}

/// Test that get_paper_details returns an error on HTTP error (404 via fixture route mismatch).
#[tokio::test]
async fn test_get_paper_details_http_error() {
    // serve_fixture serves at a specific route; requesting a non-matching
    // path should return 404 from the axum router.
    let path = paper_search_test_utils::fixture_path(
        "paper-search-iacr",
        "iacr-paper.html.zst",
    );
    let (url, _shutdown) = paper_search_test_utils::serve_fixture("/nonexistent", path).await;
    let base_url = url.clone();

    let client = IacrClient::with_base_url(5, base_url)
        .expect("failed to create client");

    // Request a path that doesn't match the fixture route
    let result = client.get_paper_details(2025, 999).await;
    assert!(result.is_err(), "get_paper_details should fail on 404");
}

/// Test that get_paper_raw fetches raw bytes from the server.
#[tokio::test]
async fn test_get_paper_raw_with_fixture() {
    // get_paper_raw doesn't check HTTP status, so we test that it
    // successfully fetches bytes from a matching server route.
    let path = paper_search_test_utils::fixture_path(
        "paper-search-iacr",
        "iacr-paper.html.zst",
    );
    let (url, _shutdown) = paper_search_test_utils::serve_fixture("/2025/999.pdf", path).await;
    let base_url = url.clone();

    let client = IacrClient::with_base_url(5, base_url)
        .expect("failed to create client");

    let result = client.get_paper_raw(2025, 999).await;
    assert!(result.is_ok(), "get_paper_raw should succeed when server responds");
    assert!(!result.unwrap().is_empty(), "should return non-empty bytes");
}
