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

//! CLI entry point for delulu-websearch.
//!
//! # Precondition
//! None.
//!
//! # Postcondition
//! Prints search results as JSON to stdout and exits with code 0 on success,
//! or prints an error to stderr and exits with code 1 on failure.
//!
//! # Panic-if
//! This function MUST NOT panic. All error paths return Err.

use anyhow::Result;
use clap::Parser;
use delulu_websearch::engines::brave::BraveContinuation;
use delulu_websearch::engines::duckduckgo::DuckDuckGoContinuation;
use delulu_websearch::engines::create_default_registry;
use delulu_websearch::{validate_query, SearchParams};

/// Default engine when neither `--engine` nor `--continuation` is provided.
const DEFAULT_ENGINE: &str = "duckduckgo";

/// CLI arguments for web search.
#[derive(Parser, Debug)]
#[command(name = "delulu-websearch")]
#[command(about = "Multi-engine web search CLI", long_about = None)]
struct Cli {
    /// Search query (required, must be non-empty after trimming).
    #[arg(short = 'q', long)]
    query: Option<String>,

    /// Search engine to use (default: "duckduckgo").
    #[arg(short = 'e', long)]
    engine: Option<String>,

    /// Page number (1-indexed).
    #[arg(short = 'p', long)]
    page: Option<u32>,

    /// Country / region code.
    #[arg(long)]
    country: Option<String>,

    /// Safesearch level (strict, moderate, off).
    #[arg(long)]
    safesearch: Option<String>,

    /// Time range filter.
    #[arg(long)]
    time_range: Option<String>,

    /// Maximum number of results.
    #[arg(long)]
    max_results: Option<u32>,

    /// Output compact JSON (no pretty-print).
    #[arg(long)]
    json: bool,

    /// Continuation JSON string for pagination.
    #[arg(short = 'c', long)]
    continuation: Option<String>,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Cli::parse();

    let query = args.query.as_deref().unwrap_or("");
    let trimmed = validate_query(query)?;

    // Parse search parameters
    let params = SearchParams {
        page: args.page,
        country: args.country,
        safesearch: args.safesearch,
        time_range: args.time_range,
        max_results: args.max_results,
    };

    // Parse continuation if provided
    let (continuation, inferred_engine) =
        if let Some(ref cont_json) = args.continuation {
            // Try Brave first, then DuckDuckGo
            if let Ok(brave_cont) = serde_json::from_str::<BraveContinuation>(cont_json) {
                let brave: Box<dyn delulu_websearch::Continuation> = Box::new(brave_cont);
                (Some(brave), Some("brave"))
            } else if let Ok(ddg_cont) =
                serde_json::from_str::<DuckDuckGoContinuation>(cont_json)
            {
                let ddg: Box<dyn delulu_websearch::Continuation> = Box::new(ddg_cont);
                (Some(ddg), Some("duckduckgo"))
            } else {
                anyhow::bail!("Invalid --continuation JSON: failed to parse as BraveContinuation or DuckDuckGoContinuation");
            }
        } else {
            (None, None)
        };

    // Determine engine name with validation against continuation type
    let engine_name = match (args.engine.as_deref(), inferred_engine) {
        (Some(explicit), Some(inferred)) => {
            if explicit != inferred {
                anyhow::bail!(
                    "Engine/continuation mismatch: continuation type {} does not match engine {}",
                    inferred,
                    explicit,
                );
            }
            explicit.to_string()
        }
        (Some(explicit), None) => explicit.to_string(),
        (None, Some(inferred)) => inferred.to_string(),
        (None, None) => DEFAULT_ENGINE.to_string(),
    };

    // Get registry and engine
    let registry = create_default_registry();
    let engine = registry
        .get_engine(&engine_name)
        .ok_or_else(|| anyhow::anyhow!("Engine '{}' not found", engine_name))?;

    // Execute search
    let response = engine
        .search(trimmed, params, continuation.as_deref())
        .await?;

    // Build output JSON
    let has_next_page = response.continuation.is_some();
    let output = serde_json::json!({
        "results": response.results,
        "session_key": null,
        "has_next_page": has_next_page,
    });

    // Print JSON
    if args.json {
        println!("{}", output);
    } else {
        println!("{}", serde_json::to_string_pretty(&output)?);
    }

    Ok(())
}
