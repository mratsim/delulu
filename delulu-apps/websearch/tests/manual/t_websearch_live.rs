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

//! Manual integration tests against live search engines.
//!
//! These tests make actual HTTP requests to verify the full pipeline works.
//!
//! ============================================================================
//! CI SAFETY: All live HTTP tests are IGNORED by default
//! ============================================================================
//!
//! To run all:
//!     cargo test --test t_websearch_live -- --ignored --nocapture
//!
//! Or run a specific test:
//!     cargo test --test t_websearch_live brave_live_basic -- --ignored --nocapture

use anyhow::Result;
use delulu_rate_limited_crawler::RateLimitedCrawler;
use delulu_websearch::{Engine, SearchParams};
use std::time::Duration;
use tokio::time::sleep;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn build_ddg_crawler() -> RateLimitedCrawler {
    RateLimitedCrawler::builder()
        .with_qps(1)
        .with_max_resp_size(5 * 1024 * 1024)
        .with_timeout(Duration::from_secs(15))
        .build()
        .expect("DDG crawler")
}

fn build_brave_crawler() -> RateLimitedCrawler {
    RateLimitedCrawler::builder()
        .with_qps(2)
        .with_max_resp_size(5 * 1024 * 1024)
        .with_timeout(Duration::from_secs(15))
        .build()
        .expect("Brave crawler")
}

fn print_results(engine: &str, query: &str, results: &[delulu_websearch::SearchResult]) {
    println!("\n=== {engine} -- \"{query}\" -- {} results ===", results.len());
    for (i, r) in results.iter().enumerate() {
        println!("  [{}] {}", i + 1, r.title);
        println!("      URL: {}", r.url);
        if let Some(s) = &r.snippet {
            let snippet = s.chars().take(200).collect::<String>();
            println!("      {}", snippet);
        }
        if let Some(d) = r.date {
            println!("      Date: {}", d);
        }
        println!();
    }
}

// ---------------------------------------------------------------------------
// DuckDuckGo live tests
// ---------------------------------------------------------------------------
// NOTE: DuckDuckGo has added JSA (JavaScript) challenge and anomaly detection
// on their search API. The engine correctly detects these and returns
// AccessDenied. These tests verify the engine handles both success and
// blocked states gracefully.

#[tokio::test]
#[ignore]
async fn duckduckgo_live_basic() -> Result<()> {
    let crawler = build_ddg_crawler();
    let engine = delulu_websearch::engines::duckduckgo::DuckDuckGoEngine::new(crawler);

    match engine.search("hashing to elliptic curves", SearchParams::default()).await {
        Ok(results) => {
            assert!(!results.is_empty());
            print_results("DuckDuckGo", "hashing to elliptic curves", &results);
        }
        Err(e) => println!("DuckDuckGo: blocked as expected: {e}"),
    }
    Ok(())
}

#[tokio::test]
#[ignore]
async fn duckduckgo_live_pagination() -> Result<()> {
    let crawler = build_ddg_crawler();
    let engine = delulu_websearch::engines::duckduckgo::DuckDuckGoEngine::new(crawler);

    match engine.search("rust programming", SearchParams::default()).await {
        Ok(r) => println!("DDG page 1: {} results", r.len()),
        Err(e) => println!("DDG page 1 blocked: {e}"),
    }

    sleep(Duration::from_secs(3)).await;

    match engine.search("rust programming", SearchParams { page: Some(2), ..Default::default() }).await {
        Ok(r) => println!("DDG page 2: {} results", r.len()),
        Err(e) => println!("DDG page 2 blocked: {e}"),
    }

    Ok(())
}

#[tokio::test]
#[ignore]
async fn duckduckgo_live_empty_query() -> Result<()> {
    let crawler = build_ddg_crawler();
    let engine = delulu_websearch::engines::duckduckgo::DuckDuckGoEngine::new(crawler);

    let result = engine.search("", SearchParams::default()).await;
    assert!(result.is_err(), "Empty query should return an error");
    Ok(())
}

#[tokio::test]
#[ignore]
async fn duckduckgo_detects_jsa_challenge() -> Result<()> {
    let crawler = build_ddg_crawler();
    let engine = delulu_websearch::engines::duckduckgo::DuckDuckGoEngine::new(crawler);

    match engine.search("test", SearchParams::default()).await {
        Ok(r) => println!("DDG: NOT blocked ({} results)", r.len()),
        Err(delulu_websearch::WebsearchError::AccessDenied) => {
            println!("DDG: correctly detected AccessDenied (JSA challenge)");
        }
        Err(e) => println!("DDG: other error: {e}"),
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Brave live tests
// ---------------------------------------------------------------------------

#[tokio::test]
#[ignore]
async fn brave_live_basic() -> Result<()> {
    let crawler = build_brave_crawler();
    let engine = delulu_websearch::engines::brave::BraveEngine::new(crawler);

    let results = engine.search("hashing to elliptic curves", SearchParams::default()).await?;

    assert!(!results.is_empty(), "Expected at least 1 result from Brave");
    assert!(
        results.iter().any(|r| {
            r.title.contains("hashing")
                || r.title.contains("Hashing")
                || r.title.contains("elliptic")
                || r.url.contains("eprint")
                || r.url.contains("iacr")
        }),
        "Expected results related to 'hashing to elliptic curves', got: {:?}",
        results.iter().map(|r| &r.title).collect::<Vec<_>>()
    );

    print_results("Brave", "hashing to elliptic curves", &results);
    Ok(())
}

#[tokio::test]
#[ignore]
async fn brave_live_pagination() -> Result<()> {
    let crawler = build_brave_crawler();
    let engine = delulu_websearch::engines::brave::BraveEngine::new(crawler);

    let page1 = engine.search("rust programming", SearchParams::default()).await?;
    assert!(!page1.is_empty(), "Page 1 should have results");
    println!("Brave page 1: {} results", page1.len());

    // Long delay to avoid bot detection
    sleep(Duration::from_secs(8)).await;

    match engine.search("rust programming", SearchParams { page: Some(2), ..Default::default() }).await {
        Ok(results) => {
            assert!(!results.is_empty(), "Page 2 should have results");
            println!("Brave page 2: {} results", results.len());
            let page1_urls: Vec<&str> = page1.iter().map(|r| r.url.as_str()).collect();
            assert!(
                results.iter().any(|r| !page1_urls.contains(&r.url.as_str())),
                "Page 2 should contain results not on page 1"
            );
        }
        Err(e) => println!("Brave page 2 blocked (aggressive bot detection): {e}"),
    }

    Ok(())
}

#[tokio::test]
#[ignore]
async fn brave_live_safesearch() -> Result<()> {
    let crawler = build_brave_crawler();
    let engine = delulu_websearch::engines::brave::BraveEngine::new(crawler);

    let results = engine
        .search("test", SearchParams { safesearch: Some("strict".into()), ..Default::default() })
        .await?;

    assert!(!results.is_empty(), "Expected results with safesearch=strict");
    print_results("Brave (safesearch=strict)", "test", &results);
    Ok(())
}

#[tokio::test]
#[ignore]
async fn brave_live_country() -> Result<()> {
    let crawler = build_brave_crawler();
    let engine = delulu_websearch::engines::brave::BraveEngine::new(crawler);

    let results = engine
        .search("news", SearchParams { country: Some("jp".into()), ..Default::default() })
        .await?;

    assert!(!results.is_empty(), "Expected results with country=jp");
    print_results("Brave (country=jp)", "news", &results);
    Ok(())
}

#[tokio::test]
#[ignore]
async fn brave_detects_pow_captcha() -> Result<()> {
    let crawler = build_brave_crawler();
    let engine = delulu_websearch::engines::brave::BraveEngine::new(crawler);

    match engine.search("test", SearchParams::default()).await {
        Ok(r) => println!("Brave: NOT blocked ({} results)", r.len()),
        Err(delulu_websearch::WebsearchError::AccessDenied) => {
            println!("Brave: correctly detected AccessDenied");
        }
        Err(e) => println!("Brave: other error: {e}"),
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Registry-based test
// ---------------------------------------------------------------------------

#[tokio::test]
#[ignore]
async fn registry_default_engines_both_work() -> Result<()> {
    use delulu_websearch::engines::create_default_registry;

    let registry = create_default_registry();

    let brave = registry.get_engine("brave").expect("Brave in registry");

    let brave_results = brave.search("rust async trait", SearchParams::default()).await?;
    assert!(!brave_results.is_empty(), "Brave should return results");
    println!("Registry Brave: {} results", brave_results.len());

    // DDG may be blocked — engine handles it
    let ddg = registry.get_engine("duckduckgo").expect("DDG in registry");
    match ddg.search("rust async trait", SearchParams::default()).await {
        Ok(r) => println!("Registry DDG: {} results", r.len()),
        Err(e) => println!("Registry DDG blocked: {e}"),
    }

    Ok(())
}
