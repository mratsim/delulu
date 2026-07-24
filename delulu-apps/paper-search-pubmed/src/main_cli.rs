//!  Delulu PubMed Paper Search — CLI
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

//! # PubMed Paper Search — CLI Entry Point
//!
//! Provides 6 subcommands matching the 6 NCBI E-utilities endpoints.

use anyhow::{Context, Error, Result};
use clap::{Parser, Subcommand};
use delulu_paper_search_pubmed::PubmedClient;
use delulu_paper_search_pubmed::core::SearchQuery;
use std::sync::Arc;
use tracing_subscriber::{EnvFilter, layer::SubscriberExt, util::SubscriberInitExt};

#[derive(Parser, Debug)]
#[command(name = "delulu-pubmed")]
#[command(author, version, about = "PubMed paper search CLI")]
struct Args {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Search PubMed for articles by keyword
    Search {
        /// Search query (e.g. "asthma[Title] AND 2023[pdat]")
        query: String,
        /// Maximum number of results (default: 20)
        #[arg(long, default_value = "20")]
        max_results: u32,
        /// Sort order: relevance, pub_date, author, journal
        #[arg(long)]
        sort: Option<String>,
    },
    /// Get summaries for a list of PMIDs
    Summaries {
        /// Comma-separated list of PMIDs
        ids: String,
    },
    /// Fetch abstracts for a list of PMIDs
    Abstracts {
        /// Comma-separated list of PMIDs
        ids: String,
    },
    /// Find related articles for a list of PMIDs
    Related {
        /// Comma-separated list of PMIDs
        ids: String,
    },
    /// Get database information for PubMed
    Info,
    /// Match a citation string to a PMID
    MatchCitation {
        /// Citation string in format: journal|year|volume|first_page|author|key|
        bdata: String,
    },
}

#[tokio::main]
async fn main() -> Result<(), Error> {
    tracing_subscriber::registry()
        .with(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".to_string().into()))
        .with(
            tracing_subscriber::fmt::layer()
                .with_timer(tracing_subscriber::fmt::time::ChronoUtc::rfc_3339())
                .with_writer(std::io::stderr),
        )
        .init();

    let args = Args::parse();

    let client = Arc::new(PubmedClient::new().context("Failed to create PubMed client")?);

    match args.command {
        Command::Search {
            query,
            max_results,
            sort,
        } => {
            let query_obj = SearchQuery {
                query,
                max_results: Some(max_results),
                sort,
            };
            let result = client.search(&query_obj).await?;
            println!("{}", serde_json::to_string_pretty(&result)?);
        }
        Command::Summaries { ids } => {
            let papers = client.get_summaries(&ids).await?;
            println!("{}", serde_json::to_string_pretty(&papers)?);
        }
        Command::Abstracts { ids } => {
            let abstracts = client.fetch_abstracts(&ids).await?;
            println!("{}", serde_json::to_string_pretty(&abstracts)?);
        }
        Command::Related { ids } => {
            let related = client.find_related(&ids).await?;
            println!("{}", serde_json::to_string_pretty(&related)?);
        }
        Command::Info => {
            let info = client.get_database_info().await?;
            println!("{}", serde_json::to_string_pretty(&info)?);
        }
        Command::MatchCitation { bdata } => {
            let matches = client.match_citation(&bdata).await?;
            println!("{}", serde_json::to_string_pretty(&matches)?);
        }
    }

    Ok(())
}
