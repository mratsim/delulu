//!  Delulu Web Search — MCP Server
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

//! MCP server entry point for delulu-websearch.
//!
//! # Precondition
//! None.
//!
//! # Postcondition
//! Starts an MCP server over stdio or HTTP and serves the `web_search` tool.
//!
//! # Panic-if
//! This function MUST NOT panic. All error paths return Err.
//!
//! The server itself (tools) lives in the library (`lib_mcp` module) so
//! `delulu-all-mcp` can reuse it.

use std::sync::Arc;
use std::time::Duration;

use anyhow::{Error, Result};
use delulu_mcp_server_helper::clap::Parser;
use delulu_mcp_server_helper::{McpServerConfig, run_http, run_stdio, setup_tracing};
use delulu_websearch::SessionCache;
use delulu_websearch::WebsearchServer;
use delulu_websearch::engines::create_default_registry;

// ---------------------------------------------------------------------------
// CLI args
// ---------------------------------------------------------------------------

#[derive(Parser, Debug)]
#[command(name = "delulu-websearch-mcp")]
#[command(
    author,
    version,
    about = "MCP server for multi-engine web search (DuckDuckGo, Brave)"
)]
struct Args {
    #[command(subcommand)]
    command: McpServerConfig,
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

#[tokio::main]
async fn main() -> Result<(), Error> {
    setup_tracing();

    tracing::debug!("Parsing arguments...");
    let args = Args::parse();
    tracing::debug!("Parsed args: {:?}", args);

    tracing::debug!("Creating engine registry...");
    let engine_registry = Arc::new(create_default_registry());

    tracing::debug!("Creating session cache...");
    // Bounds for worst-case sizing:
    //   - 256 concurrent users (vLLM default)
    //   - 2 QPS per engine (rate-limited crawler)
    //   - 600s TTL
    //
    // Under sustained load the cache fills at 2 QPS. At capacity 512:
    //   time-to-fill = 512 / 2 = 256s
    // After that every store() evicts the oldest entry (~256s old).
    // So capacity, not TTL, determines entry lifetime under load.
    //
    // 256s is sufficient for a conversation turn (search + pagination).
    // If users batch >2 searches each concurrently, sessions may be evicted
    // before the full TTL — increase capacity if that becomes the pattern.
    let session_cache = Arc::new(SessionCache::new(512, Duration::from_secs(600)));

    match args.command {
        McpServerConfig::Stdio => {
            let server = WebsearchServer::new(engine_registry, session_cache);
            run_stdio(server).await?;
        }
        McpServerConfig::Http { host, port } => {
            let server = WebsearchServer::new(engine_registry, session_cache);
            run_http(server, host, port).await?;
        }
    }

    Ok(())
}
