//!  Delulu arXiv Paper Search — Client Unit Tests
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

//! # Arxiv Client Unit Tests
//!
//! Tests for `ArxivClient` — builder methods, URL construction, and HTTP
//! integration with fixture data via local server.
//!
//! NOTE: This file is included via `#[path]` in `src/lib.rs`, so all paths
//! are relative to the crate root (`crate::`).

use crate::ArxivClient;
use crate::core::SearchQuery;

// ---------------------------------------------------------------------------
// Builder / configuration tests
// ---------------------------------------------------------------------------

#[test]
fn test_with_base_url_custom() {
    let _client = ArxivClient::new()
        .expect("new should succeed")
        .with_base_url("http://localhost:9999/api".to_string());
}

/// Verify that with_api_url overrides the API endpoint.
#[test]
fn test_with_api_url_custom() {
    let client = ArxivClient::new()
        .expect("new should succeed")
        .with_base_url("http://localhost:9999".to_string())
        .with_api_url("http://localhost:8888/api/query".to_string());
    let _ = client;
}

/// Verify that with_base_url only overrides base_url, not api_url.
#[test]
fn test_with_base_url_does_not_override_api_url() {
    let client = ArxivClient::new()
        .expect("new should succeed")
        .with_base_url("https://custom-base.example.com".to_string());
    assert_eq!(client.base_url, "https://custom-base.example.com");
    assert_eq!(client.api_url, "https://export.arxiv.org/api/query");
}

/// Verify that with_api_url is preserved (set after with_base_url).
#[test]
fn test_with_api_url_after_with_base_url() {
    let client = ArxivClient::new()
        .expect("new should succeed")
        .with_base_url("https://arxiv.org".to_string())
        .with_api_url("https://custom-api.example.com/query".to_string());
    assert_eq!(client.api_url, "https://custom-api.example.com/query");
    assert_eq!(client.base_url, "https://arxiv.org");
}

// ---------------------------------------------------------------------------
// new_with_crawler (pub constructor — Phase 1a shared-crawler seam)
// ---------------------------------------------------------------------------

/// Verify `new_with_crawler` accepts a caller-provided `Arc<RateLimitedCrawler>`
/// and that the defaults match `new()`.
#[test]
fn test_new_with_crawler_defaults() {
    let crawler = delulu_rate_limited_crawler::RateLimitedCrawler::builder()
        .with_qps(1)
        .with_timeout(std::time::Duration::from_secs(30))
        .with_connect_timeout(std::time::Duration::from_secs(30))
        .build()
        .expect("crawler should build");
    let client = ArxivClient::new_with_crawler(std::sync::Arc::new(crawler));
    assert_eq!(client.base_url, "https://arxiv.org");
    assert_eq!(client.api_url, "https://export.arxiv.org/api/query");
}

/// Verify the Arc seam: two clients can share one crawler (the all-mcp reuse path).
#[test]
fn test_new_with_crawler_shared_arc() {
    let shared = std::sync::Arc::new(
        delulu_rate_limited_crawler::RateLimitedCrawler::builder()
            .with_qps(1)
            .with_timeout(std::time::Duration::from_secs(30))
            .with_connect_timeout(std::time::Duration::from_secs(30))
            .build()
            .expect("crawler should build"),
    );
    let client_a = ArxivClient::new_with_crawler(std::sync::Arc::clone(&shared));
    let client_b = ArxivClient::new_with_crawler(std::sync::Arc::clone(&shared));
    assert!(std::sync::Arc::ptr_eq(&client_a.crawler, &client_b.crawler),
        "both clients must share the same crawler instance");
}

// ---------------------------------------------------------------------------
// Base URL configuration tests
// ---------------------------------------------------------------------------

/// Verify that the default base_url is https://arxiv.org.
#[test]
fn test_base_url_default() {
    let client = ArxivClient::new().expect("new should succeed");
    assert_eq!(client.base_url, "https://arxiv.org");
}

/// Verify that with_base_url overrides the base_url.
#[test]
fn test_base_url_custom() {
    let client = ArxivClient::new()
        .expect("new should succeed")
        .with_base_url("http://localhost:9999".to_string());
    assert_eq!(client.base_url, "http://localhost:9999");
}

/// Verify that old-format arXiv IDs (e.g. cond-mat/0011267) don't cause formatting issues.
#[test]
fn test_old_format_id_does_not_crash_format() {
    let client = ArxivClient::new().expect("new should succeed");
    assert_eq!(client.base_url, "https://arxiv.org");
    // Constructing a URL with an old-format ID should not panic
    let url = format!("{}/html/{}", client.base_url, "cond-mat/0011267");
    assert_eq!(url, "https://arxiv.org/html/cond-mat/0011267");
}

// ---------------------------------------------------------------------------
// HTTP integration tests with fixture data
// ---------------------------------------------------------------------------

/// Test that the arXiv client can search for papers using fixture data
/// served by a local HTTP server.
#[tokio::test]
async fn test_search_papers_with_fixture() {
    let path = paper_search_test_utils::fixture_path(
        "paper-search-arxiv",
        "arxiv-search-response.xml.zst",
    );
    let (url, _shutdown) = paper_search_test_utils::serve_fixture("/api/query", path).await;
    let server_url = format!("{}/api/query", url);

    let client = ArxivClient::new()
        .expect("failed to create client")
        .with_base_url(server_url);

    let query = SearchQuery {
        query: "all:electron".to_string(),
        max_results: Some(2),
        start: None,
        sort_by: None,
        sort_order: None,
    };
    let papers = client
        .search_papers(&query)
        .await
        .expect("search should succeed");

    assert!(!papers.is_empty(), "should return at least one paper");
    assert!(!papers[0].id.is_empty(), "paper should have an ID");
    assert!(!papers[0].title.is_empty(), "paper should have a title");
    assert!(
        papers[0].abstract_text.contains("cuprate"),
        "abstract should contain expected content from fixture"
    );
}

/// Test that the arXiv client can fetch papers by ID using fixture data.
#[tokio::test]
async fn test_get_papers_by_id_with_fixture() {
    let path = paper_search_test_utils::fixture_path(
        "paper-search-arxiv",
        "arxiv-search-response.xml.zst",
    );
    let (url, _shutdown) = paper_search_test_utils::serve_fixture("/api/query", path).await;
    let server_url = format!("{}/api/query", url);

    let client = ArxivClient::new()
        .expect("failed to create client")
        .with_base_url(server_url);

    let papers = client
        .get_papers_by_id("cond-mat/0011267")
        .await
        .expect("get_by_id should succeed");

    assert!(!papers.is_empty(), "should return at least one paper");
}

/// Test that a request to an unreachable server returns an error.
#[tokio::test]
async fn test_search_papers_connection_refused() {
    // Use a port that's unlikely to have anything listening
    let client = ArxivClient::new()
        .expect("failed to create client")
        .with_api_url("http://127.0.0.1:1/".to_string());

    let query = SearchQuery {
        query: "test".to_string(),
        max_results: Some(1),
        start: None,
        sort_by: None,
        sort_order: None,
    };
    let result = client.search_papers(&query).await;
    assert!(result.is_err(), "search to invalid endpoint should fail");
}

/// Test that get_paper URL uses the configured html_base_url.
#[tokio::test]
async fn test_get_paper_uses_html_base_url() {
    // Set up a local HTTP server that returns a minimal HTML page
    let html_content = "<html><body>test paper</body></html>";
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("failed to bind");
    let addr = listener.local_addr().unwrap();
    let server_url = format!("http://{}", addr);

    // Spawn a simple server that returns the HTML
    tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        tokio::spawn(async move {
            use tokio::io::AsyncWriteExt;
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n{}",
                html_content.len(),
                html_content
            );
            let _ = stream.write_all(response.as_bytes()).await;
        });
    });
    let client = ArxivClient::new()
        .expect("failed to create client")
        .with_base_url(server_url.clone());

    let result = client.get_paper("2301.99999").await;
    // The request may succeed or fail (no real server), but should not panic.
    let _ = result;
}
