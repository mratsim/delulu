//! Integration tests for paper-search-pubmed using local HTTP server and fixtures.

use delulu_paper_search_pubmed::core::SearchQuery;
use paper_search_test_utils::{fixture_path, serve_fixture};
#[tokio::test]
async fn test_pubmed_search_with_fixture() {
    let path = fixture_path("paper-search-pubmed", "pubmed-search.json.zst");
    let (url, _shutdown) = serve_fixture("/entrez/eutils/esearch.fcgi", path).await;
    let server_url = format!("{}/entrez/eutils", url);

    let client = delulu_paper_search_pubmed::PubmedClient::new()
        .unwrap()
        .with_api_url(server_url);

    let query = SearchQuery {
        query: "virtual+reality".to_string(),
        max_results: Some(3),
        sort: None,
    };
    let result = client.search(&query).await.expect("search should succeed");

    assert!(!result.pmids.is_empty(), "should return at least one PMID");
}

#[tokio::test]
async fn test_pubmed_summaries_with_fixture() {
    let path = fixture_path("paper-search-pubmed", "pubmed-summary.json.zst");
    let (url, _shutdown) = serve_fixture("/entrez/eutils/esummary.fcgi", path).await;
    let server_url = format!("{}/entrez/eutils", url);

    let client = delulu_paper_search_pubmed::PubmedClient::new()
        .unwrap()
        .with_api_url(server_url);

    let papers = client
        .get_summaries("38742940")
        .await
        .expect("summaries should succeed");

    assert!(!papers.is_empty(), "should return at least one paper");
    assert!(!papers[0].title.is_empty(), "paper should have a title");
}

#[tokio::test]
async fn test_pubmed_abstracts_with_fixture() {
    let path = fixture_path("paper-search-pubmed", "pubmed-abstract.txt.zst");
    let (url, _shutdown) = serve_fixture("/entrez/eutils/efetch.fcgi", path).await;
    let server_url = format!("{}/entrez/eutils", url);

    let client = delulu_paper_search_pubmed::PubmedClient::new()
        .unwrap()
        .with_api_url(server_url);

    let abstracts = client
        .fetch_abstracts("38742940")
        .await
        .expect("abstracts should succeed");

    assert!(!abstracts.is_empty(), "should return at least one abstract");
    assert!(!abstracts[0].1.is_empty(), "abstract should have text");
}
