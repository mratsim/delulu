//! Integration tests for paper-search-iacr using local HTTP server and fixtures.

use paper_search_test_utils::{fixture_path, serve_fixture};
#[tokio::test]
async fn test_iacr_list_recent_with_fixture() {
    let path = fixture_path("paper-search-iacr", "iacr-rss.xml.zst");
    let (url, _shutdown) = serve_fixture("/rss/rss.xml", path).await;

    let client = delulu_paper_search_iacr::IacrClient::with_base_url(5, url)
        .expect("failed to create client");

    let papers = client
        .list_recent_papers()
        .await
        .expect("list_recent should succeed");

    assert!(!papers.is_empty(), "should return at least one paper");
    assert!(!papers[0].title.is_empty(), "paper should have a title");
}

#[tokio::test]
async fn test_iacr_get_details_with_fixture() {
    let path = fixture_path("paper-search-iacr", "iacr-paper.html.zst");
    let (url, _shutdown) = serve_fixture("/2025/1", path).await;

    let client = delulu_paper_search_iacr::IacrClient::with_base_url(5, url)
        .expect("failed to create client");

    let paper = client
        .get_paper_details(2025, 1)
        .await
        .expect("get_details should succeed");

    assert!(!paper.title.is_empty(), "paper should have a title");
    assert!(!paper.authors.is_empty(), "paper should have authors");
}
