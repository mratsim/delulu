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

//! Manual integration tests for `web_search_next_page` flow using live engines.
//!
//! These tests verify the full pagination pipeline:
//! 1. Engine search returns results + continuation
//! 2. Continuation is stored in SessionCache
//! 3. Next page is fetched using the stored continuation
//! 4. Session expiry, missing keys, and no-more-pages cases
//!
//! ============================================================================
//! CI SAFETY: All live HTTP tests are IGNORED by default
//! ============================================================================
//!
//! To run all:
//!     cargo test --test t_websearch_next_page_live -- --ignored --nocapture
//!
//! Or run a specific test:
//!     cargo test --test t_websearch_next_page_live brave_next_page -- --ignored --nocapture

use std::time::{Duration, Instant};

use delulu_rate_limited_crawler::RateLimitedCrawler;
use delulu_websearch::engine::{Continuation, Engine, EngineId, SearchParams};
use delulu_websearch::SessionCache;
use tokio::time::sleep;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn build_brave_crawler() -> RateLimitedCrawler {
    RateLimitedCrawler::builder()
        .with_qps(2)
        .with_max_resp_size(5 * 1024 * 1024)
        .with_timeout(Duration::from_secs(15))
        .build()
        .expect("Brave crawler")
}

fn build_ddg_crawler() -> RateLimitedCrawler {
    RateLimitedCrawler::builder()
        .with_qps(1)
        .with_max_resp_size(5 * 1024 * 1024)
        .with_timeout(Duration::from_secs(15))
        .build()
        .expect("DDG crawler")
}

/// Print search results to stdout for manual inspection.
fn print_results(engine: &str, label: &str, results: &[delulu_websearch::SearchResult]) {
    println!("\n=== {engine} -- {label} -- {} results ===", results.len());
    for (i, r) in results.iter().enumerate() {
        println!("  [{}] {}", i + 1, r.title);
        println!("      URL: {}", r.url);
        if let Some(s) = &r.snippet {
            let snippet = s.chars().take(200).collect::<String>();
            println!("      {}", snippet);
        }
        println!();
    }
}

// ---------------------------------------------------------------------------
// Brave next-page tests
// ---------------------------------------------------------------------------

#[tokio::test]
#[ignore]
async fn brave_next_page_basic() {
    let crawler = build_brave_crawler();
    let engine = delulu_websearch::engines::brave::BraveEngine::new(crawler);
    let cache = SessionCache::new(100, Duration::from_secs(3600));
    let now = Instant::now();

    // Page 1
    let page1 = engine
        .search("rust programming", SearchParams::default(), None)
        .await
        .expect("Brave page 1 should succeed");
    assert!(!page1.results.is_empty(), "Page 1 should have results");
    print_results("Brave", "page 1", &page1.results);
    assert!(page1.continuation.is_some(), "Brave should have continuation for page 2");

    // Store in cache
    let mut random_id = [0u8; 8];
    getrandom::getrandom(&mut random_id).expect("random id");
    let key = cache.store(
        EngineId::Brave,
        "rust programming",
        SearchParams::default(),
        page1.continuation,
        now,
        random_id,
    );

    // Long delay to avoid bot detection
    sleep(Duration::from_secs(8)).await;

    // Page 2 via cache
    let now2 = Instant::now();
    let entry = cache.get(&key, now2).expect("Session should exist");
    let cont = entry.continuation.expect("Continuation should exist");
    let page2 = engine
        .search("rust programming", SearchParams::default(), Some(&*cont))
        .await
        .expect("Brave page 2 should succeed");
    assert!(!page2.results.is_empty(), "Page 2 should have results");
    print_results("Brave", "page 2", &page2.results);

    // Verify page 2 has different results than page 1
    let page1_urls: Vec<&str> = page1.results.iter().map(|r| r.url.as_str()).collect();
    let has_new = page2.results.iter().any(|r| !page1_urls.contains(&r.url.as_str()));
    assert!(has_new, "Page 2 should contain results not on page 1");
}

#[tokio::test]
#[ignore]
async fn brave_next_page_expired_session() {
    let crawler = build_brave_crawler();
    let engine = delulu_websearch::engines::brave::BraveEngine::new(crawler);
    let cache = SessionCache::new(100, Duration::from_secs(1)); // 1 second TTL
    let now = Instant::now();

    let page1 = engine
        .search("test query", SearchParams::default(), None)
        .await
        .expect("Brave page 1 should succeed");

    let mut random_id = [0u8; 8];
    getrandom::getrandom(&mut random_id).expect("random id");
    let key = cache.store(
        EngineId::Brave,
        "test query",
        SearchParams::default(),
        page1.continuation,
        now,
        random_id,
    );

    // Wait for TTL to expire
    sleep(Duration::from_secs(2)).await;

    let later = Instant::now();
    let entry = cache.get(&key, later);
    assert!(entry.is_none(), "Session should be expired after TTL");
    println!("Brave: expired session correctly returns None");
}

// ---------------------------------------------------------------------------
// DuckDuckGo next-page tests
// ---------------------------------------------------------------------------

#[tokio::test]
#[ignore]
async fn duckduckgo_next_page_basic() {
    let crawler = build_ddg_crawler();
    let engine = delulu_websearch::engines::duckduckgo::DuckDuckGoEngine::new(crawler);
    let cache = SessionCache::new(100, Duration::from_secs(3600));
    let now = Instant::now();

    // Page 1
    let page1 = match engine.search("rust programming", SearchParams::default(), None).await {
        Ok(r) => r,
        Err(e) => {
            println!("DuckDuckGo page 1 blocked: {e}");
            return; // DDG often blocked, skip gracefully
        }
    };

    assert!(!page1.results.is_empty(), "Page 1 should have results");
    print_results("DuckDuckGo", "page 1", &page1.results);

    if page1.continuation.is_none() {
        println!("DuckDuckGo: no continuation available (no more pages or blocked)");
        return;
    }

    // Store in cache
    let mut random_id = [0u8; 8];
    getrandom::getrandom(&mut random_id).expect("random id");
    let key = cache.store(
        EngineId::DuckDuckGo,
        "rust programming",
        SearchParams::default(),
        page1.continuation,
        now,
        random_id,
    );

    // Delay
    sleep(Duration::from_secs(3)).await;

    // Page 2 via cache
    let now2 = Instant::now();
    let entry = cache.get(&key, now2).expect("Session should exist");
    let cont = entry.continuation.expect("Continuation should exist");
    match engine.search("rust programming", SearchParams::default(), Some(&*cont)).await {
        Ok(page2) => {
            print_results("DuckDuckGo", "page 2", &page2.results);
            if !page2.results.is_empty() {
                let page1_urls: Vec<&str> = page1.results.iter().map(|r| r.url.as_str()).collect();
                let has_new = page2.results.iter().any(|r| !page1_urls.contains(&r.url.as_str()));
                assert!(has_new, "Page 2 should contain results not on page 1");
            }
        }
        Err(e) => println!("DuckDuckGo page 2 failed: {e}"),
    }
}

#[tokio::test]
#[ignore]
async fn duckduckgo_next_page_expired_session() {
    let crawler = build_ddg_crawler();
    let engine = delulu_websearch::engines::duckduckgo::DuckDuckGoEngine::new(crawler);
    let cache = SessionCache::new(100, Duration::from_secs(1)); // 1 second TTL
    let now = Instant::now();

    let page1 = match engine.search("test query", SearchParams::default(), None).await {
        Ok(r) => r,
        Err(e) => {
            println!("DuckDuckGo page 1 blocked: {e}");
            return;
        }
    };

    let mut random_id = [0u8; 8];
    getrandom::getrandom(&mut random_id).expect("random id");
    let key = cache.store(
        EngineId::DuckDuckGo,
        "test query",
        SearchParams::default(),
        page1.continuation,
        now,
        random_id,
    );

    // Wait for TTL to expire
    sleep(Duration::from_secs(2)).await;

    let later = Instant::now();
    let entry = cache.get(&key, later);
    assert!(entry.is_none(), "Session should be expired after TTL");
    println!("DuckDuckGo: expired session correctly returns None");
}

// ---------------------------------------------------------------------------
// Cache-level tests (no HTTP)
// ---------------------------------------------------------------------------

#[tokio::test]
#[ignore]
async fn session_cache_missing_key() {
    let cache = SessionCache::new(100, Duration::from_secs(3600));
    let now = Instant::now();
    let key = delulu_websearch::SessionKey::new(EngineId::Brave, [0x00; 8]);

    let entry = cache.get(&key, now);
    assert!(entry.is_none(), "Missing key should return None");
    println!("Cache: missing key correctly returns None");
}

#[tokio::test]
#[ignore]
async fn session_cache_update_then_get() {
    let cache = SessionCache::new(100, Duration::from_secs(3600));
    let now = Instant::now();
    let mut random_id = [0u8; 8];
    getrandom::getrandom(&mut random_id).expect("random id");

    let key = cache.store(
        EngineId::Brave,
        "update test",
        SearchParams::default(),
        None,
        now,
        random_id,
    );

    // Update continuation
    cache
        .update_continuation(&key, None, now)
        .expect("update should succeed");

    // Retrieve
    let entry = cache.get(&key, now);
    assert!(entry.is_some(), "Entry should exist after update");
    println!("Cache: update then get works correctly");
}
