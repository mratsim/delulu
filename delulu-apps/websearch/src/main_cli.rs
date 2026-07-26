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
use delulu_websearch::engines::create_default_registry;
use delulu_websearch::{SearchParams, validate_query};

/// Default engine when `--engine` is not provided.
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

    // Determine engine
    let engine_name = args.engine.unwrap_or_else(|| DEFAULT_ENGINE.to_string());

    // Get registry and engine
    let registry = create_default_registry();
    let engine = registry
        .get_engine(&engine_name)
        .ok_or_else(|| anyhow::anyhow!("Engine '{}' not found", engine_name))?;

    // Execute search
    let response = engine.search(trimmed, params, None).await?;

    // Build output JSON
    let output = serde_json::json!({
        "results": response.results,
    });

    // Print JSON
    if args.json {
        println!("{}", output);
    } else {
        println!("{}", serde_json::to_string_pretty(&output)?);
    }

    Ok(())
}
