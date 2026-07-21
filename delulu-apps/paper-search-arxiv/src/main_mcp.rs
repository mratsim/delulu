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
//! Follows the same rmcp pattern as `delulu-travel-search`.

use anyhow::{Context, Error, Result};
use clap::{Parser, Subcommand};
use delulu_paper_search_arxiv::{core::SearchQuery, ArxivClient};
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
#[command(name = "delulu-arxiv-mcp")]
#[command(
    author,
    version,
    about = "MCP server for arXiv paper search"
)]
struct Args {
    /// Base URL for the arXiv API (default: https://export.arxiv.org/api/query)
    #[arg(long, default_value = "https://export.arxiv.org/api/query")]
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

/// Input parameters for searching arXiv papers.
#[derive(Serialize, Deserialize, JsonSchema)]
pub struct SearchPapersInput {
    /// Search query using arXiv syntax (e.g. "ti:transformer AND abs:attention")
    pub query: String,
    /// Maximum number of results (default: 10, max: 2000)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_results: Option<u32>,
    /// Start index for pagination (0-based)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start: Option<u32>,
    /// Sort field: "relevance", "lastUpdatedDate", "submittedDate"
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sort_by: Option<String>,
    /// Sort order: "ascending" or "descending"
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sort_order: Option<String>,
}

/// Input parameters for fetching papers by arXiv ID.
#[derive(Serialize, Deserialize, JsonSchema)]
pub struct GetPapersByIdInput {
    /// Comma-separated list of arXiv IDs (e.g. "2301.12345,2302.67890")
    pub ids: String,
}

#[derive(Clone)]
pub struct ArxivMcpServer {
    client: Arc<ArxivClient>,
    tool_router: ToolRouter<Self>,
}

impl ArxivMcpServer {
    pub fn new(client: Arc<ArxivClient>) -> Self {
        Self {
            client,
            tool_router: Self::tool_router(),
        }
    }
}

#[tool_router]
impl ArxivMcpServer {
    #[tool(
        name = "search_papers",
        description = "Search for papers on arXiv by keyword, title, author, or abstract. Parameters: query (arXiv search syntax), max_results, start, sort_by (relevance/lastUpdatedDate/submittedDate), sort_order (ascending/descending)."
    )]
    async fn search_papers(
        &self,
        params: Parameters<SearchPapersInput>,
    ) -> Result<String, String> {
        let input = params.0;
        let query = SearchQuery {
            query: input.query,
            max_results: input.max_results,
            start: input.start,
            sort_by: input.sort_by,
            sort_order: input.sort_order,
        };

        let papers = self
            .client
            .search_papers(&query)
            .await
            .map_err(|e| format!("arXiv search failed: {e}"))?;

        serde_json::to_string(&papers).map_err(|e| e.to_string())
    }

    #[tool(
        name = "get_papers_by_id",
        description = "Fetch specific papers from arXiv by their IDs. Parameters: ids (comma-separated arXiv IDs, e.g. '2301.12345,2302.67890')."
    )]
    async fn get_papers_by_id(
        &self,
        params: Parameters<GetPapersByIdInput>,
    ) -> Result<String, String> {
        let input = params.0;
        let papers = self
            .client
            .get_papers_by_id(&input.ids)
            .await
            .map_err(|e| format!("arXiv fetch failed: {e}"))?;

        serde_json::to_string(&papers).map_err(|e| e.to_string())
    }
}

impl ServerHandler for ArxivMcpServer {
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

    tracing::debug!("Creating arXiv client...");
    let client = Arc::new(
        ArxivClient::with_base_url(30, args.api_base_url.clone())
        .context("Failed to create arXiv client")?
    );

    match args.command {
        Command::Stdio => {
            let server = ArxivMcpServer::new(client);
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
            let server = ArxivMcpServer::new(client);
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
