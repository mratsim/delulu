//!  Delulu IACR Paper Search — MCP Server
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
//! The `IacrMcpServer` itself lives in the library (`lib_mcp` module).

use anyhow::{Context, Error, Result};
use delulu_mcp_server_helper::clap::Parser;
use delulu_mcp_server_helper::{McpServerConfig, run_http, run_stdio, setup_tracing};
use delulu_paper_search_iacr::IacrClient;
use delulu_paper_search_iacr::IacrMcpServer;
use std::sync::Arc;

#[derive(Parser, Debug)]
#[command(name = "delulu-iacr-mcp")]
#[command(author, version, about = "MCP server for IACR ePrint paper search")]
struct Args {
    #[arg(long, default_value = "https://eprint.iacr.org")]
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

    tracing::debug!("Creating IACR client...");
    let client = Arc::new(
        IacrClient::new()
            .context("Failed to create IACR client")?
            .with_base_url(args.api_base_url.clone()),
    );

    match args.command {
        McpServerConfig::Stdio => {
            let server = IacrMcpServer::new(client);
            run_stdio(server).await?;
        }
        McpServerConfig::Http { host, port } => {
            let server = IacrMcpServer::new(client);
            run_http(server, host, port).await?;
        }
    }

    Ok(())
}
