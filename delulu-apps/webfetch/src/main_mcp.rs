//!  Delulu Webfetch — MCP Server
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
//!
//! # Unified MCP Server Entry Point
//!
//! Supports stdio and HTTP transports.
//! Uses the shared `delulu-mcp-server-helper` for common infrastructure.
//! The server itself (tools + SSRF validation) lives in the library
//! (`lib_mcp` module) so `delulu-all-mcp` can reuse it.

use anyhow::{Context, Error, Result};
use delulu_mcp_server_helper::clap::Parser;
use delulu_mcp_server_helper::{McpServerConfig, run_http, run_stdio, setup_tracing};
use delulu_rate_limited_crawler::RateLimitedCrawler;
use delulu_webfetch::MAX_BODY_SIZE;
use delulu_webfetch::lib_mcp::WebfetchServer;
use std::sync::Arc;
use std::time::Duration;

#[derive(Parser, Debug)]
#[command(name = "webfetch-mcp")]
struct Args {
    /// Allow fetching URLs that resolve to private/internal IP addresses.
    /// By default, webfetch rejects requests to private IP ranges
    /// (127.0.0.0/8, 10.0.0.0/8, 172.16.0.0/12, 192.168.0.0/16,
    /// ::1, fc00::/7, and cloud metadata endpoints) to prevent SSRF.
    #[arg(long)]
    expose_local_networks: bool,

    #[command(subcommand)]
    command: McpServerConfig,
}

// ---------------------------------------------------------------------------
// main
// ---------------------------------------------------------------------------

#[tokio::main]
async fn main() -> Result<(), Error> {
    setup_tracing();

    tracing::debug!("Parsing arguments...");
    let args = Args::parse();
    tracing::debug!("Parsed args: {:?}", args);

    tracing::debug!("Creating rate-limited crawler...");
    let crawler = Arc::new(
        RateLimitedCrawler::builder()
            .with_qps(2)
            .with_max_resp_size(MAX_BODY_SIZE)
            .with_timeout(Duration::from_secs(30))
            .with_connect_timeout(Duration::from_secs(30))
            .build()
            .context("Failed to create rate-limited crawler")?,
    );
    tracing::debug!("Crawler created");

    match args.command {
        McpServerConfig::Stdio => {
            let server = WebfetchServer::new(crawler, args.expose_local_networks);
            run_stdio(server).await?;
        }
        McpServerConfig::Http { host, port } => {
            let server = WebfetchServer::new(crawler, args.expose_local_networks);
            run_http(server, host, port).await?;
        }
    }

    Ok(())
}
