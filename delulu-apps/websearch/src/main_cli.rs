//!  Delulu Web Search — CLI
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

/// CLI arguments for web search.
#[derive(Parser, Debug)]
#[command(name = "delulu-websearch")]
#[command(about = "Multi-engine web search CLI", long_about = None)]
struct Cli {
    /// Search query (required, must be non-empty after trimming).
    #[arg(short = 'q', long)]
    query: Option<String>,

    /// Search engine to use (default: "duckduckgo").
    #[arg(short = 'e', long, default_value = "duckduckgo")]
    engine: String,

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
    if query.trim().is_empty() {
        anyhow::bail!("'query' must be a non-empty string");
    }

    // Stub: print placeholder message
    println!("CLI not yet implemented");
    Ok(())
}
