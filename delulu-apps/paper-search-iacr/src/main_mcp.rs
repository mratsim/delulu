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
//! Follows the same rmcp pattern as `delulu-travel-search`.

use anyhow::{Context, Error, Result};
use clap::{Parser, Subcommand};
use delulu_paper_search_iacr::IacrClient;
use rmcp::handler::server::{ServerHandler, tool::ToolRouter, wrapper::Parameters};
use rmcp::service::serve_server;
use rmcp::tool;
use rmcp::tool_router;
use rmcp::transport::streamable_http_server::{
    StreamableHttpServerConfig, StreamableHttpService, session::local::LocalSessionManager,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::sync::Arc;
use tracing_subscriber::{EnvFilter, layer::SubscriberExt, util::SubscriberInitExt};

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
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Run MCP server over stdio (for Claude Desktop, etc.)
    Stdio,

    /// Run MCP server over HTTP
    Http {
        #[arg(long, default_value = "0.0.0.0")]
        host: String,

        #[arg(long, default_value = "8080")]
        port: u16,
    },
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

impl ServerHandler for IacrMcpServer {
    fn list_tools(
        &self,
        _request: Option<rmcp::model::PaginatedRequestParam>,
        _context: rmcp::service::RequestContext<rmcp::RoleServer>,
    ) -> impl std::future::Future<Output = Result<rmcp::model::ListToolsResult, rmcp::ErrorData>> + Send + '_
    {
        tracing::debug!(
            "list_tools called, tools count: {}",
            self.tool_router.list_all().len()
        );
        let tools = self.tool_router.list_all();
        Box::pin(async move {
            tracing::debug!("Returning {} tools", tools.len());
            Ok(rmcp::model::ListToolsResult::with_all_items(tools))
        })
    }

    fn call_tool(
        &self,
        request: rmcp::model::CallToolRequestParam,
        context: rmcp::service::RequestContext<rmcp::RoleServer>,
    ) -> impl std::future::Future<Output = Result<rmcp::model::CallToolResult, rmcp::ErrorData>> + Send + '_
    {
        let router = self.tool_router.clone();
        let self_clone = self.clone();
        Box::pin(async move {
            let context =
                rmcp::handler::server::tool::ToolCallContext::new(&self_clone, request, context);
            router.call(context).await
        })
    }

    fn get_info(&self) -> rmcp::model::ServerInfo {
        rmcp::model::ServerInfo {
            protocol_version: rmcp::model::ProtocolVersion::V_2025_03_26,
            capabilities: rmcp::model::ServerCapabilities {
                tools: Some(rmcp::model::ToolsCapability::default()),
                ..Default::default()
            },
            server_info: rmcp::model::Implementation::from_build_env(),
            instructions: None,
        }
    }
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

    tracing::debug!("Parsing arguments...");
    let args = Args::parse();
    tracing::debug!("Parsed args: {:?}", args);

    tracing::debug!("Creating IACR client...");
    let client = Arc::new(
        IacrClient::with_base_url(30, args.api_base_url.clone())
            .context("Failed to create IACR client")?
    );

    match args.command {
        Command::Stdio => {
            let server = IacrMcpServer::new(client);
            let (stdin, stdout) = rmcp::transport::io::stdio();
            tracing::info!("Starting MCP server over stdio...");
            let _running = serve_server(Arc::new(server), (stdin, stdout))
                .await
                .map_err(|e| anyhow::anyhow!("Server error: {}", e))?;
            tracing::debug!("Server running. Press Ctrl+C to stop.");
            tokio::signal::ctrl_c().await.ok();
            tracing::info!("Shutting down...");
        }
        Command::Http { host, port } => {
            let addr: SocketAddr = format!("{}:{}", host, port)
                .parse()
                .context("Invalid host:port")?;
            tracing::info!("Starting MCP server over HTTP on {}", addr);
            let server = IacrMcpServer::new(client);
            let session_manager = Arc::new(LocalSessionManager::default());
            let config = StreamableHttpServerConfig {
                stateful_mode: true,
                ..Default::default()
            };
            let service =
                StreamableHttpService::new(move || Ok(server.clone()), session_manager, config);
            let app = axum::Router::new().nest_service("/mcp", service);
            let listener = tokio::net::TcpListener::bind(addr)
                .await
                .context("Failed to bind to address")?;
            tracing::debug!("Listening on {}", addr);
            axum::serve(listener, app)
                .with_graceful_shutdown(async move {
                    tokio::signal::ctrl_c().await.ok();
                    tracing::info!("Shutting down HTTP server...");
                })
                .await
                .context("HTTP server error")?;
        }
    }

    Ok(())
}
