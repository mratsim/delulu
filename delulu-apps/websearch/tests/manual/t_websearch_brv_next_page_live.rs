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

//! Manual integration tests for Brave next-page pagination (live HTTP).
//!
//! Tests the full pipeline: search → store continuation → fetch next page.
//!
//! ============================================================================
//! CI SAFETY: All live HTTP tests are IGNORED by default
//! ============================================================================
//!
//! To run:
//!     cargo test --test t_websearch_brv_next_page_live -- --ignored --nocapture

use std::time::{Duration, Instant};

use delulu_websearch::engine::{EngineId, SearchParams};
use delulu_websearch::engines::create_default_registry;
use delulu_websearch::SessionCache;
use tokio::time::sleep;

fn print_results(label: &str, results: &[delulu_websearch::SearchResult]) {
    println!("\n=== Brave -- {label} -- {} results ===", results.len());
    for (i, r) in results.iter().enumerate() {
        println!("  [{}] {}", i + 1, r.title);
        println!("      URL: {}", r.url);
        if let Some(s) = &r.snippet {
            println!("      {}", s.chars().take(200).collect::<String>());
        }
        println!();
    }
}

#[tokio::test]
#[ignore]
async fn brave_next_page_basic() {
    let engine = create_default_registry()
        .get_engine("brave")
        .expect("Brave in registry");
    let cache = SessionCache::new(100, Duration::from_secs(3600));
    let now = Instant::now();

    let page1 = engine
        .search(
            "Flash Attention with tensor cores",
            SearchParams::default(),
            None,
        )
        .await
        .expect("Brave page 1 should succeed");
    assert!(!page1.results.is_empty(), "Page 1 should have results");
    print_results("page 1 — Flash Attention with tensor cores", &page1.results);
    assert!(
        page1.continuation.is_some(),
        "Brave should have continuation for page 2"
    );

    let mut random_id = [0u8; 8];
    getrandom::getrandom(&mut random_id).expect("random id");
    let key = cache.store(
        EngineId::Brave,
        "Flash Attention with tensor cores",
        SearchParams::default(),
        page1.continuation,
        now,
        random_id,
    );

    let now2 = Instant::now();
    let entry = cache.get(&key, now2).expect("Session should exist");
    let cont = entry.continuation.expect("Continuation should exist");
    let page2 = engine
        .search(
            "Flash Attention with tensor cores",
            SearchParams::default(),
            Some(&*cont),
        )
        .await
        .expect("Brave page 2 should succeed");
    assert!(!page2.results.is_empty(), "Page 2 should have results");
    print_results("page 2", &page2.results);

    let page1_urls: Vec<&str> = page1.results.iter().map(|r| r.url.as_str()).collect();
    let has_new = page2
        .results
        .iter()
        .any(|r| !page1_urls.contains(&r.url.as_str()));
    assert!(has_new, "Page 2 should contain results not on page 1");
}

#[tokio::test]
#[ignore]
async fn brave_next_page_expired_session() {
    let engine = create_default_registry()
        .get_engine("brave")
        .expect("Brave in registry");
    let cache = SessionCache::new(100, Duration::from_secs(1));
    let now = Instant::now();

    let page1 = engine
        .search(
            "Flash Attention with tensor cores",
            SearchParams::default(),
            None,
        )
        .await
        .expect("Brave page 1 should succeed");

    let mut random_id = [0u8; 8];
    getrandom::getrandom(&mut random_id).expect("random id");
    let key = cache.store(
        EngineId::Brave,
        "Flash Attention with tensor cores",
        SearchParams::default(),
        page1.continuation,
        now,
        random_id,
    );

    sleep(Duration::from_secs(2)).await;

    let later = Instant::now();
    let entry = cache.get(&key, later);
    assert!(entry.is_none(), "Session should be expired after TTL");
    println!("Brave: expired session correctly returns None");
}
