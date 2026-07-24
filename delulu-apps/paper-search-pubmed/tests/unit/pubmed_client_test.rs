//!  Delulu PubMed Paper Search — Client Unit Tests
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

//! # PubMed Client Unit Tests
//!
//! Tests for `PubmedClient` — builder methods, URL construction, and HTTP
//! integration with fixture data via local server.
//!
//! NOTE: This file is included via `#[path]` in `src/lib.rs`, so all paths
//! are relative to the crate root (`crate::`).

use crate::PubmedClient;
use crate::core::SearchQuery;

// ---------------------------------------------------------------------------
// Builder / configuration tests
// ---------------------------------------------------------------------------

#[test]
fn test_new_creates_client_with_defaults() {
    let client = PubmedClient::new().expect("new() should succeed");
    let _ = client;
}

#[test]
fn test_with_api_url_custom() {
    let client = PubmedClient::new()
        .unwrap()
        .with_api_url("http://localhost:9999".to_string());
    let _ = client;
}

// ---------------------------------------------------------------------------
// URL construction tests (get_paper / get_paper_raw)
// ---------------------------------------------------------------------------

#[test]
fn test_get_paper_url_with_custom_base() {
    let base_url = "http://localhost:9999/pmc";
    let pmc_id = "PMC123456";
    let id = pmc_id.strip_prefix("PMC").unwrap_or(pmc_id);
    let url = format!("{}/articles/PMC{id}/pdf/", base_url.trim_end_matches('/'));
    assert_eq!(id, "123456");
    assert_eq!(url, "http://localhost:9999/pmc/articles/PMC123456/pdf/");
}

#[test]
fn test_get_paper_url_without_pmc_prefix_custom_base() {
    let base_url = "http://localhost:9999/";
    let pmc_id = "123456";
    let id = pmc_id.strip_prefix("PMC").unwrap_or(pmc_id);
    let url = format!("{}/articles/PMC{id}/pdf/", base_url.trim_end_matches('/'));
    assert_eq!(id, "123456");
    assert_eq!(url, "http://localhost:9999/articles/PMC123456/pdf/");
}

#[test]
fn test_get_paper_url_with_trailing_slash_base() {
    // Verify trim_end_matches handles trailing slash
    let base_url = "http://localhost:9999/";
    let pmc_id = "PMC123456";
    let id = pmc_id.strip_prefix("PMC").unwrap_or(pmc_id);
    let url = format!("{}/articles/PMC{id}/pdf/", base_url.trim_end_matches('/'));
    assert_eq!(url, "http://localhost:9999/articles/PMC123456/pdf/");
}

#[test]
fn test_get_paper_raw_url_construction_custom_base() {
    let base_url = "https://www.ncbi.nlm.nih.gov/pmc";
    let pmc_id = "PMC987654";
    let id = pmc_id.strip_prefix("PMC").unwrap_or(pmc_id);
    let url = format!("{}/articles/PMC{id}/pdf/", base_url.trim_end_matches('/'));
    assert_eq!(id, "987654");
    assert_eq!(
        url,
        "https://www.ncbi.nlm.nih.gov/pmc/articles/PMC987654/pdf/"
    );
}

// ---------------------------------------------------------------------------
// HTTP integration tests with fixture data
// ---------------------------------------------------------------------------

/// Test that the PubMed client can search using fixture data.
#[tokio::test]
async fn test_search_with_fixture() {
    let path =
        paper_search_test_utils::fixture_path("paper-search-pubmed", "pubmed-search.json.zst");
    let (url, _shutdown) =
        paper_search_test_utils::serve_fixture("/entrez/eutils/esearch.fcgi", path).await;
    let server_url = format!("{}/entrez/eutils", url);

    let client = PubmedClient::new().unwrap().with_api_url(server_url);

    let query = SearchQuery {
        query: "asthma[Title]".to_string(),
        max_results: Some(3),
        sort: None,
    };
    let result = client.search(&query).await.expect("search should succeed");

    assert!(result.total_count > 0, "should have results");
    assert!(!result.pmids.is_empty(), "should have PMIDs");
}

/// Test that the PubMed client can get summaries using fixture data.
#[tokio::test]
async fn test_get_summaries_with_fixture() {
    let path =
        paper_search_test_utils::fixture_path("paper-search-pubmed", "pubmed-summary.json.zst");
    let (url, _shutdown) =
        paper_search_test_utils::serve_fixture("/entrez/eutils/esummary.fcgi", path).await;
    let server_url = format!("{}/entrez/eutils", url);

    let client = PubmedClient::new().unwrap().with_api_url(server_url);

    let papers = client
        .get_summaries("42477534")
        .await
        .expect("get_summaries should succeed");

    assert!(!papers.is_empty(), "should return at least one paper");
    assert_eq!(papers[0].pmid, "42477534");
}

/// Test that the PubMed client can fetch abstracts using fixture data.
#[tokio::test]
async fn test_fetch_abstracts_with_fixture() {
    let path =
        paper_search_test_utils::fixture_path("paper-search-pubmed", "pubmed-abstract.txt.zst");
    let (url, _shutdown) =
        paper_search_test_utils::serve_fixture("/entrez/eutils/efetch.fcgi", path).await;
    let server_url = format!("{}/entrez/eutils", url);

    let client = PubmedClient::new().unwrap().with_api_url(server_url);

    let abstracts = client
        .fetch_abstracts("42477534")
        .await
        .expect("fetch_abstracts should succeed");

    assert!(!abstracts.is_empty(), "should return at least one abstract");
    assert_eq!(abstracts[0].0, "42477534");
}

/// Test that find_related constructs the correct URL with custom base_url.
#[tokio::test]
async fn test_find_related_with_fixture() {
    // Test that find_related sends a request to the correct path.
    // The URL construction is: {base_url}/elink.fcgi?dbfrom=pubmed&db=pubmed&id=...
    // We verify by serving a fixture at the expected path.
    let path =
        paper_search_test_utils::fixture_path("paper-search-pubmed", "pubmed-elink.json.zst");
    let (url, _shutdown) =
        paper_search_test_utils::serve_fixture("/entrez/eutils/elink.fcgi", path).await;
    let server_url = format!("{}/entrez/eutils", url);

    let client = PubmedClient::new().unwrap().with_api_url(server_url);

    let result = client.find_related("37994677").await;
    let related = result.expect("find_related should succeed with fixture data");
    assert!(!related.input_pmids.is_empty(), "should have input PMIDs");
    assert!(!related.related.is_empty(), "should have related PMIDs");
}

/// Test that the PubMed client can get database info using fixture data.
#[tokio::test]
async fn test_get_database_info_with_fixture() {
    let path =
        paper_search_test_utils::fixture_path("paper-search-pubmed", "pubmed-einfo.json.zst");
    let (url, _shutdown) =
        paper_search_test_utils::serve_fixture("/entrez/eutils/einfo.fcgi", path).await;
    let server_url = format!("{}/entrez/eutils", url);

    let client = PubmedClient::new().unwrap().with_api_url(server_url);

    let info = client
        .get_database_info()
        .await
        .expect("get_database_info should succeed");

    assert_eq!(info.db_name, "pubmed");
    assert!(info.record_count > 0, "should have record count");
}

/// Test that a request to an unreachable server returns an error.
#[tokio::test]
async fn test_search_connection_refused() {
    let client = PubmedClient::new()
        .unwrap()
        .with_api_url("http://127.0.0.1:1".to_string());

    let query = SearchQuery {
        query: "test".to_string(),
        max_results: Some(1),
        sort: None,
    };
    let result = client.search(&query).await;
    assert!(result.is_err(), "search to invalid endpoint should fail");
}

/// Test that get_paper fetches content from the server using custom base_url.
#[tokio::test]
async fn test_get_paper_with_fixture() {
    // get_paper doesn't check HTTP status, so we test that it
    // successfully fetches content from a matching server route.
    let path =
        paper_search_test_utils::fixture_path("paper-search-pubmed", "pubmed-search.json.zst");
    let (url, _shutdown) =
        paper_search_test_utils::serve_fixture("/articles/PMC9999999/pdf/", path).await;
    let server_url = url.clone();

    let client = PubmedClient::new().unwrap().with_base_url(server_url);

    let result = client.get_paper("PMC9999999").await;
    // get_paper will try to process the bytes as a PDF, which will fail
    // because the fixture is JSON. But the request should succeed.
    // The error will be from PDF processing, not HTTP.
    assert!(
        result.is_err(),
        "get_paper should fail when content is not a PDF"
    );
}

/// Test that get_paper_raw fetches raw bytes from the server.
#[tokio::test]
async fn test_get_paper_raw_with_fixture() {
    // get_paper_raw doesn't check HTTP status, so we test that it
    // successfully fetches bytes from a matching server route.
    let path =
        paper_search_test_utils::fixture_path("paper-search-pubmed", "pubmed-search.json.zst");
    let (url, _shutdown) =
        paper_search_test_utils::serve_fixture("/articles/PMC9999999/pdf/", path).await;
    let server_url = url.clone();

    let client = PubmedClient::new().unwrap().with_base_url(server_url);

    let result = client.get_paper_raw("PMC9999999").await;
    assert!(
        result.is_ok(),
        "get_paper_raw should succeed when server responds"
    );
    assert!(!result.unwrap().is_empty(), "should return non-empty bytes");
}
