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

//! Manual integration tests for DuckDuckGo single-page search (live HTTP).
//!
//! ============================================================================
//! CI SAFETY: All live HTTP tests are IGNORED by default
//! ============================================================================
//!
//! To run:
//!     cargo test --test t_websearch_ddg_live -- --ignored --nocapture

use anyhow::Result;
use delulu_websearch::engines::create_default_registry;
use delulu_websearch::{Engine, EngineRef, SearchParams};

fn print_results(label: &str, results: &[delulu_websearch::SearchResult]) {
    println!(
        "\n=== DuckDuckGo -- {label} -- {} results ===",
        results.len()
    );
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
async fn duckduckgo_live_basic() -> Result<()> {
    let engine = create_default_registry()
        .get_engine("duckduckgo")
        .expect("DDG in registry");

    let response = engine
        .search(
            "CuTe Layout Algebra tutorial",
            SearchParams::default(),
            None,
        )
        .await?;
    assert!(
        !response.results.is_empty(),
        "Expected results from DuckDuckGo"
    );
    print_results("CuTe Layout Algebra tutorial", &response.results);
    Ok(())
}

#[tokio::test]
#[ignore]
async fn duckduckgo_live_empty_query() -> Result<()> {
    let engine = create_default_registry()
        .get_engine("duckduckgo")
        .expect("DDG in registry");
    let result = engine.search("", SearchParams::default(), None).await;
    assert!(result.is_err(), "Empty query should return an error");
    Ok(())
}
