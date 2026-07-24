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

use anyhow::Result;
use delulu_mcp_server_helper::clap::Parser;
use delulu_mcp_server_helper::{McpServerConfig, setup_tracing};

#[derive(Parser, Debug)]
#[command(name = "delulu-websearch-mcp")]
struct Args {
    #[command(subcommand)]
    command: McpServerConfig,
}

#[tokio::main]
async fn main() -> Result<()> {
    setup_tracing();

    let _args = Args::parse();

    // Stub: print placeholder message
    println!("MCP server not yet implemented");

    Ok(())
}
