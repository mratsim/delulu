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
    let client = IacrClient::new().expect("new() should succeed");
    // Verify the client was constructed by testing a method
    let url = client.paper_pdf_url(2024, 123);
    assert_eq!(url, "https://eprint.iacr.org/2024/123.pdf");
}

#[test]
fn test_with_base_url_custom() {
    let client = IacrClient::new()
        .unwrap()
        .with_base_url("http://localhost:9999".to_string());
    let url = client.paper_pdf_url(2024, 123);
    assert_eq!(url, "http://localhost:9999/2024/123.pdf");
}

// ---------------------------------------------------------------------------
// new_with_crawler (pub constructor — shared-crawler seam)
// ---------------------------------------------------------------------------
/// Verify `new_with_crawler` accepts a caller-provided `Arc<RateLimitedCrawler>`
/// and uses the same default base URL as `new()`.
#[test]
fn test_new_with_crawler_defaults() {
    let crawler = delulu_rate_limited_crawler::RateLimitedCrawler::builder()
        .with_qps(3)
        .with_timeout(std::time::Duration::from_secs(30))
        .with_connect_timeout(std::time::Duration::from_secs(30))
        .build()
        .expect("crawler should build");
    let client = IacrClient::new_with_crawler(std::sync::Arc::new(crawler));
    let default_client = IacrClient::new().expect("new should succeed");
    assert_eq!(client.base_url, default_client.base_url);
    assert_eq!(client.base_url, "https://eprint.iacr.org");
}

/// Verify the Arc seam: two clients can share one crawler (the all-mcp reuse path).
#[test]
fn test_new_with_crawler_shared_arc() {
    let shared = std::sync::Arc::new(
        delulu_rate_limited_crawler::RateLimitedCrawler::builder()
            .with_qps(3)
            .with_timeout(std::time::Duration::from_secs(30))
            .with_connect_timeout(std::time::Duration::from_secs(30))
            .build()
            .expect("crawler should build"),
    );
    let client_a = IacrClient::new_with_crawler(std::sync::Arc::clone(&shared));
    let client_b = IacrClient::new_with_crawler(std::sync::Arc::clone(&shared));
    assert!(
        std::sync::Arc::ptr_eq(&client_a.crawler, &client_b.crawler),
        "both clients must share the same crawler instance"
    );
}

// ---------------------------------------------------------------------------
// URL construction tests
// ---------------------------------------------------------------------------

#[test]
fn test_paper_pdf_url_uses_base_url() {
    let client = IacrClient::new()
        .unwrap()
        .with_base_url("https://eprint.iacr.org".to_string());

    // paper_pdf_url uses a simple format! without zero-padding
    let url = client.paper_pdf_url(2024, 123);
    assert_eq!(url, "https://eprint.iacr.org/2024/123.pdf");

    // Note: paper_pdf_url does NOT zero-pad (iacr_pdf_url does)
    let url = client.paper_pdf_url(2004, 5);
    assert_eq!(url, "https://eprint.iacr.org/2004/5.pdf");
}

#[test]
fn test_paper_pdf_url_with_custom_base() {
    let client = IacrClient::new()
        .unwrap()
        .with_base_url("http://localhost:9999".to_string());

    let url = client.paper_pdf_url(2024, 123);
    assert_eq!(url, "http://localhost:9999/2024/123.pdf");
}

// ---------------------------------------------------------------------------
// HTTP integration tests with fixture data
// ---------------------------------------------------------------------------

/// Test that the IACR client can list recent papers using fixture data.
#[tokio::test]
async fn test_list_recent_papers_with_fixture() {
    let path = paper_search_test_utils::fixture_path("paper-search-iacr", "iacr-rss.xml.zst");
    let (url, _shutdown) = paper_search_test_utils::serve_fixture("/rss/rss.xml", path).await;
    let base_url = url.clone();

    let client = IacrClient::new().unwrap().with_base_url(base_url);

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
    let path = paper_search_test_utils::fixture_path("paper-search-iacr", "iacr-paper.html.zst");
    let (url, _shutdown) = paper_search_test_utils::serve_fixture("/2025/1", path).await;
    let base_url = url.clone();

    let client = IacrClient::new().unwrap().with_base_url(base_url);

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
    let client = IacrClient::new()
        .unwrap()
        .with_base_url("http://127.0.0.1:1".to_string());

    let result = client.list_recent_papers().await;
    assert!(result.is_err(), "request to invalid endpoint should fail");
}

/// Test that get_paper_details returns an error on HTTP error (404 via fixture route mismatch).
#[tokio::test]
async fn test_get_paper_details_http_error() {
    // serve_fixture serves at a specific route; requesting a non-matching
    // path should return 404 from the axum router.
    let path = paper_search_test_utils::fixture_path("paper-search-iacr", "iacr-paper.html.zst");
    let (url, _shutdown) = paper_search_test_utils::serve_fixture("/nonexistent", path).await;
    let base_url = url.clone();

    let client = IacrClient::new().unwrap().with_base_url(base_url);

    // Request a path that doesn't match the fixture route
    let result = client.get_paper_details(2025, 999).await;
    assert!(result.is_err(), "get_paper_details should fail on 404");
}

/// Test that get_paper_raw fetches raw bytes from the server.
#[tokio::test]
async fn test_get_paper_raw_with_fixture() {
    // get_paper_raw doesn't check HTTP status, so we test that it
    // successfully fetches bytes from a matching server route.
    let path = paper_search_test_utils::fixture_path("paper-search-iacr", "iacr-paper.html.zst");
    let (url, _shutdown) = paper_search_test_utils::serve_fixture("/2025/999.pdf", path).await;
    let base_url = url.clone();

    let client = IacrClient::new().unwrap().with_base_url(base_url);

    let result = client.get_paper_raw(2025, 999).await;
    assert!(
        result.is_ok(),
        "get_paper_raw should succeed when server responds"
    );
    assert!(!result.unwrap().is_empty(), "should return non-empty bytes");
}
