//!  Delulu Travel Agent
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

//! # Unified MCP Server Entry Point
//!
//! Supports stdio transport via subcommand.
//! Uses the shared `delulu-mcp-server-helper` for common infrastructure.
//! The `TravelAgentServer` itself lives in the library (`lib_mcp` module).

use anyhow::{Context, Error, Result};
use delulu_mcp_server_helper::clap::Parser;
use delulu_mcp_server_helper::{McpServerConfig, run_http, run_stdio, setup_tracing};
use delulu_travel_search::TravelAgentServer;
use delulu_travel_search::{GoogleFlightsClient, GoogleHotelsClient};
use std::sync::Arc;

#[derive(Parser, Debug)]
#[command(name = "travel-mcp")]
#[command(
    author,
    version,
    about = "MCP server for travel search (flights & hotels)"
)]
struct Args {
    #[command(subcommand)]
    command: McpServerConfig,
}

#[tokio::main]
async fn main() -> Result<(), Error> {
    setup_tracing();

    tracing::debug!("Parsing arguments...");
    let args = Args::parse();
    tracing::debug!("Parsed args: {:?}", args);

    tracing::debug!("Creating flights client...");
    let flights_client = Arc::new(
        GoogleFlightsClient::new(
            "en".into(),
            "USD".into(),
            5, // timeout_secs
            2, // queries_per_second
        )
        .context("Failed to create flights client")?,
    );
    tracing::debug!("Creating hotels client...");
    let hotels_client = Arc::new(
        GoogleHotelsClient::new(
            5, // timeout_secs
            2, // queries_per_second
        )
        .context("Failed to create hotels client")?,
    );
    tracing::debug!("Clients created");

    match args.command {
        McpServerConfig::Stdio => {
            let server = TravelAgentServer::new(flights_client, hotels_client);
            run_stdio(server).await?;
        }
        McpServerConfig::Http { host, port } => {
            let server = TravelAgentServer::new(flights_client, hotels_client);
            run_http(server, host, port).await?;
        }
    }

    Ok(())
}
