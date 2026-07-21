//! Integration tests for paper-search-arxiv using local HTTP server and fixtures.

use delulu_paper_search_arxiv::core::SearchQuery;
use paper_search_test_utils::{fixture_path, serve_fixture};

/// Test that the arXiv client can search for papers using fixture data.
#[tokio::test]
async fn test_arxiv_search_with_fixture() {
    let path = fixture_path("paper-search-arxiv", "arxiv-search-response.xml.zst");
    let (url, _shutdown) = serve_fixture("/api/query", path).await;
    let server_url = format!("{}/api/query", url);

    let client = delulu_paper_search_arxiv::ArxivClient::with_base_url(5, server_url)
        .expect("failed to create client");

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
    assert!(!papers[0].authors.is_empty(), "paper should have authors");
    assert!(!papers[0].primary_category.is_empty(), "paper should have a primary category");
    assert!(papers[0].abs_url.contains("arxiv.org"), "paper should have an HTML URL");
    assert!(papers[0].pdf_url.contains("arxiv.org"), "paper should have a PDF URL");
}

/// Test that the arXiv client can fetch papers by ID using fixture data.
#[tokio::test]
async fn test_arxiv_get_by_id_with_fixture() {
    let path = fixture_path("paper-search-arxiv", "arxiv-search-response.xml.zst");
    let (url, _shutdown) = serve_fixture("/api/query", path).await;
    let server_url = format!("{}/api/query", url);

    let client = delulu_paper_search_arxiv::ArxivClient::with_base_url(5, server_url)
        .expect("failed to create client");

    let papers = client
        .get_papers_by_id("cond-mat/0011267")
        .await
        .expect("get_by_id should succeed");

    assert!(!papers.is_empty(), "should return at least one paper");
}
