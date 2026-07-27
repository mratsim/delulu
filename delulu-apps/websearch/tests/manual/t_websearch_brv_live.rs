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

//! Manual integration tests for Brave single-page search (live HTTP).
//!
//! ============================================================================
//! CI SAFETY: All live HTTP tests are IGNORED by default
//! ============================================================================
//!
//! To run:
//!     cargo test --test t_websearch_brv_live -- --ignored --nocapture

use anyhow::Result;
use delulu_websearch::engines::create_default_registry;
use delulu_websearch::{Engine, EngineRef, SearchParams};

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
async fn brave_live_basic() -> Result<()> {
    let engine = create_default_registry()
        .get_engine("brave")
        .expect("Brave in registry");
    let response = engine
        .search(
            "CuTe Layout Algebra tutorial",
            SearchParams::default(),
            None,
        )
        .await?;

    assert!(
        !response.results.is_empty(),
        "Expected at least 1 result from Brave"
    );
    assert!(
        response.results.iter().any(|r| {
            r.title.contains("CuTe")
                || r.title.contains("cuTe")
                || r.title.contains("CUTE")
                || r.url.contains("nvidia")
                || r.url.contains("github")
        }),
        "Expected results related to 'CuTe Layout Algebra', got: {:?}",
        response
            .results
            .iter()
            .map(|r| &r.title)
            .collect::<Vec<_>>()
    );

    print_results("CuTe Layout Algebra tutorial", &response.results);
    Ok(())
}

#[tokio::test]
#[ignore]
async fn brave_live_safesearch() -> Result<()> {
    let engine = create_default_registry()
        .get_engine("brave")
        .expect("Brave in registry");
    let response = engine
        .search(
            "CuTe Layout Algebra tutorial",
            SearchParams {
                safesearch: Some("strict".into()),
                ..Default::default()
            },
            None,
        )
        .await?;

    assert!(
        !response.results.is_empty(),
        "Expected results with safesearch=strict"
    );
    print_results(
        "safesearch=strict — CuTe Layout Algebra tutorial",
        &response.results,
    );
    Ok(())
}

#[tokio::test]
#[ignore]
async fn brave_live_country() -> Result<()> {
    let engine = create_default_registry()
        .get_engine("brave")
        .expect("Brave in registry");
    let response = engine
        .search(
            "CuTe Layout Algebra tutorial",
            SearchParams {
                country: Some("jp".into()),
                ..Default::default()
            },
            None,
        )
        .await?;

    assert!(
        !response.results.is_empty(),
        "Expected results with country=jp"
    );
    print_results(
        "country=jp — CuTe Layout Algebra tutorial",
        &response.results,
    );
    Ok(())
}
