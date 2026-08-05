//!  Delulu All-MCP — Binary
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

//! Entry point for the unified `delulu-all-mcp` binary.
//!
//! Parses the merged 7-flag `AllMcpConfig` plus the `McpServerConfig`
//! subcommand, builds **one** shared `RateLimitedCrawler` from the rate
//! fields, constructs a single `AllServer`, and serves it over stdio or HTTP.
//!
//! # Precondition
//! None.
//!
//! # Postcondition
//! An MCP server for the 21-tool union is started over the chosen transport.
//!
//! # Panic-if
//! None — all error paths return `Err`.

use std::sync::Arc;

use delulu_all_mcp::lib_mcp::{AllMcpConfig, AllServer};
use delulu_mcp_server_helper::clap::Parser;
use delulu_mcp_server_helper::{McpServerConfig, run_http, run_stdio, setup_tracing};
use delulu_rate_limited_crawler::RateLimitedCrawler;

// ---------------------------------------------------------------------------
// CLI args
// ---------------------------------------------------------------------------

#[derive(Parser, Debug)]
#[command(name = "delulu-all-mcp")]
#[command(
    author,
    version,
    about = "Unified MCP server exposing the 21-tool union across webfetch, websearch, travel, arxiv, iacr, and pubmed"
)]
struct Args {
    /// Merged per-domain flags (expose-local-networks, base URLs, rates).
    #[command(flatten)]
    config: AllMcpConfig,

    /// Transport subcommand.
    #[command(subcommand)]
    command: McpServerConfig,
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    setup_tracing();

    let args = Args::parse();
    tracing::debug!(?args, "parsed arguments");

    // One shared crawler built from the rate fields. Safari18_5 emulation and
    // the 30s timeouts are builder defaults; http2 is enabled explicitly.
    let crawler = Arc::new(
        RateLimitedCrawler::builder()
            .with_qps(args.config.qps as u64)
            .with_burst(args.config.burst as u64)
            .with_max_resp_size(args.config.max_resp_size_mb as usize * 1024 * 1024)
            .with_http2()
            .build()?,
    );
    tracing::debug!("shared rate-limited crawler built");

    let server = AllServer::new(args.config, crawler);

    match args.command {
        McpServerConfig::Stdio => {
            run_stdio(server).await?;
        }
        McpServerConfig::Http { host, port } => {
            run_http(server, host, port).await?;
        }
    }

    Ok(())
}