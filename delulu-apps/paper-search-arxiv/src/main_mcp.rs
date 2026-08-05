//!  Delulu arXiv Paper Search — MCP Server
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

//! # MCP Server Entry Point
//!
//! Supports stdio and HTTP transports.
//! Uses the shared `delulu-mcp-server-helper` for common infrastructure.
//! The `ArxivMcpServer` itself lives in the library (`lib_mcp` module).

use anyhow::{Context, Error, Result};
use delulu_mcp_server_helper::clap::Parser;
use delulu_mcp_server_helper::{McpServerConfig, run_http, run_stdio, setup_tracing};
use delulu_paper_search_arxiv::ArxivClient;
use delulu_paper_search_arxiv::ArxivMcpServer;
use std::sync::Arc;

#[derive(Parser, Debug)]
#[command(name = "delulu-arxiv-mcp")]
#[command(author, version, about = "MCP server for arXiv paper search")]
struct Args {
    /// Base URL for the arXiv API (default: https://export.arxiv.org/api/query)
    #[arg(long, default_value = "https://export.arxiv.org/api/query")]
    api_base_url: String,

    #[command(subcommand)]
    command: McpServerConfig,
}

#[tokio::main]
async fn main() -> Result<(), Error> {
    setup_tracing();

    tracing::debug!("Parsing arguments...");
    let args = Args::parse();
    tracing::debug!("Parsed args: {:?}", args);

    tracing::debug!("Creating arXiv client...");
    let client = Arc::new(
        ArxivClient::new()
            .context("Failed to create arXiv client")?
            .with_api_url(args.api_base_url.clone()),
    );
    match args.command {
        McpServerConfig::Stdio => {
            let server = ArxivMcpServer::new(client);
            run_stdio(server).await?;
        }
        McpServerConfig::Http { host, port } => {
            let server = ArxivMcpServer::new(client);
            run_http(server, host, port).await?;
        }
    }

    Ok(())
}
