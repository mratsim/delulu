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

use anyhow::{Context, Error, Result};
use delulu_mcp_server_helper::clap::Parser;
use delulu_mcp_server_helper::rmcp::handler::server::tool::ToolRouter;
use delulu_mcp_server_helper::rmcp::handler::server::wrapper::Parameters;
use delulu_mcp_server_helper::rmcp::tool;
use delulu_mcp_server_helper::rmcp::tool_router;
use delulu_mcp_server_helper::{McpServerConfig, impl_server_handler, run_http, run_stdio, setup_tracing};
use delulu_paper_search_iacr::IacrClient;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Parser, Debug)]
#[command(name = "delulu-iacr-mcp")]
#[command(
    author,
    version,
    about = "MCP server for IACR ePrint paper search"
)]
struct Args {
    #[arg(long, default_value = "https://eprint.iacr.org")]
    api_base_url: String,
    #[command(subcommand)]
    command: McpServerConfig,
}

/// Input parameters for listing recent IACR papers.
#[derive(Serialize, Deserialize, JsonSchema)]
pub struct ListRecentPapersInput {}

/// Input parameters for getting paper details.
#[derive(Serialize, Deserialize, JsonSchema)]
pub struct GetPaperDetailsInput {
    /// Publication year (e.g. 2024)
    pub year: u32,
    /// Paper number within the year (e.g. 123)
    pub number: u32,
}

/// Input parameters for downloading a paper PDF.
#[derive(Serialize, Deserialize, JsonSchema)]
pub struct DownloadPaperPdfInput {
    /// Publication year (e.g. 2024)
    pub year: u32,
    /// Paper number within the year (e.g. 123)
    pub number: u32,
}

/// Input parameters for fetching a full paper as markdown.
#[derive(Serialize, Deserialize, JsonSchema)]
pub struct GetPaperInput {
    /// Publication year (e.g. 2024)
    pub year: u32,
    /// Paper number within the year (e.g. 1279)
    pub number: u32,
}

#[derive(Clone)]
pub struct IacrMcpServer {
    client: Arc<IacrClient>,
    tool_router: ToolRouter<Self>,
}

impl IacrMcpServer {
    pub fn new(client: Arc<IacrClient>) -> Self {
        Self {
            client,
            tool_router: Self::tool_router(),
        }
    }
}

#[tool_router]
impl IacrMcpServer {
    #[tool(
        name = "list_recent_papers",
        description = "List recent papers from the IACR ePrint Archive RSS feed. Returns papers with basic metadata including title, authors, and abstract."
    )]
    async fn list_recent_papers(
        &self,
        _params: Parameters<ListRecentPapersInput>,
    ) -> Result<String, String> {
        let papers = self
            .client
            .list_recent_papers()
            .await
            .map_err(|e| format!("IACR RSS fetch failed: {e}"))?;

        serde_json::to_string(&papers).map_err(|e| e.to_string())
    }

    #[tool(
        name = "get_paper_details",
        description = "Get full details for a specific IACR ePrint paper by year and number. Parameters: year (e.g. 2024), number (e.g. 123). Returns full metadata including abstract and authors."
    )]
    async fn get_paper_details(
        &self,
        params: Parameters<GetPaperDetailsInput>,
    ) -> Result<String, String> {
        let input = params.0;
        let paper = self
            .client
            .get_paper_details(input.year, input.number)
            .await
            .map_err(|e| format!("IACR paper details fetch failed: {e}"))?;

        serde_json::to_string(&paper).map_err(|e| e.to_string())
    }

    #[tool(
        name = "download_paper_pdf",
        description = "Get the PDF download URL for a specific IACR ePrint paper. Parameters: year (e.g. 2024), number (e.g. 123). Returns the PDF URL."
    )]
    async fn download_paper_pdf(
        &self,
        params: Parameters<DownloadPaperPdfInput>,
    ) -> Result<String, String> {
        let input = params.0;
        let url = self.client.download_paper_pdf(input.year, input.number);
        Ok(url)
    }

    #[tool(
        name = "get_paper",
        description = "Fetch a full paper from IACR ePrint as markdown. Downloads the PDF and converts via xberg. Parameters: year (e.g. 2024), number (e.g. 1279)."
    )]
    async fn get_paper(
        &self,
        params: Parameters<GetPaperInput>,
    ) -> Result<String, String> {
        let input = params.0;
        let md = self
            .client
            .get_paper(input.year, input.number)
            .await
            .map_err(|e| format!("IACR paper fetch failed: {e}"))?;
        Ok(md)
    }
}

impl_server_handler!(IacrMcpServer);

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
