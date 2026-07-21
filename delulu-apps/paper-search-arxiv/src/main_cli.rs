//!  Delulu arXiv Paper Search — CLI
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

//! # CLI Entry Point
//!
//! Subcommands matching the MCP tools:
//! - `search`: search papers by query
//! - `get-by-id`: fetch papers by arXiv ID

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use delulu_paper_search_arxiv::{core::SearchQuery, ArxivClient};
use tracing_subscriber::{EnvFilter, layer::SubscriberExt, util::SubscriberInitExt};

#[derive(Parser, Debug)]
#[command(name = "delulu-arxiv")]
#[command(
    author,
    version,
    about = "Search and fetch papers from arXiv"
)]
struct Args {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Search papers on arXiv by query
    Search {
        /// Search query using arXiv syntax
        /// (e.g. "ti:transformer AND abs:attention")
        query: String,

        /// Maximum number of results (default: 10, max: 2000)
        #[arg(long, default_value = "10")]
        max_results: u32,

        /// Start index for pagination (0-based)
        #[arg(long)]
        start: Option<u32>,

        /// Sort field: "relevance", "lastUpdatedDate", "submittedDate"
        #[arg(long)]
        sort_by: Option<String>,

        /// Sort order: "ascending" or "descending"
        #[arg(long)]
        sort_order: Option<String>,
    },

    /// Fetch specific papers by arXiv ID
    GetById {
        /// Comma-separated list of arXiv IDs (e.g. "2301.12345,2302.67890")
        ids: String,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::registry()
        .with(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".to_string().into()))
        .with(
            tracing_subscriber::fmt::layer()
                .with_timer(tracing_subscriber::fmt::time::ChronoUtc::rfc_3339())
                .with_writer(std::io::stderr),
        )
        .init();

    let args = Args::parse();

    let client = ArxivClient::new(30).context("Failed to create arXiv client")?;

    match args.command {
        Command::Search {
            query,
            max_results,
            start,
            sort_by,
            sort_order,
        } => {
            let search_query = SearchQuery {
                query,
                max_results: Some(max_results),
                start,
                sort_by,
                sort_order,
            };

            tracing::info!("Searching arXiv...");
            let papers = client
                .search_papers(&search_query)
                .await
                .context("arXiv search failed")?;

            let output = serde_json::to_string_pretty(&papers)
                .context("Failed to serialize results")?;
            println!("{}", output);
        }
        Command::GetById { ids } => {
            tracing::info!("Fetching papers by ID: {}", ids);
            let papers = client
                .get_papers_by_id(&ids)
                .await
                .context("arXiv fetch failed")?;

            let output = serde_json::to_string_pretty(&papers)
                .context("Failed to serialize results")?;
            println!("{}", output);
        }
    }

    Ok(())
}
