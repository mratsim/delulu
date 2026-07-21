//!  Delulu PubMed Paper Search — MCP Server
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
//! Follows the same rmcp pattern as `delulu-travel-search` and `delulu-paper-search-arxiv`.

use anyhow::{Context, Error, Result};
use clap::{Parser, Subcommand};
use delulu_paper_search_pubmed::core::SearchQuery;
use delulu_paper_search_pubmed::PubmedClient;
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
#[command(name = "delulu-pubmed-mcp")]
#[command(
    author,
    version,
    about = "MCP server for PubMed paper search"
)]
struct Args {
    #[arg(long, default_value = "https://eutils.ncbi.nlm.nih.gov/entrez/eutils")]
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

// ---------------------------------------------------------------------------
// Tool input structs
// ---------------------------------------------------------------------------

/// Input parameters for searching PubMed.
#[derive(Serialize, Deserialize, JsonSchema)]
pub struct SearchPubmedInput {
    /// Search query using PubMed syntax (e.g. "asthma[Title] AND 2023[pdat]")
    pub query: String,
    /// Maximum number of results (default: 20)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_results: Option<u32>,
    /// Sort order: "relevance", "pub_date", "author", "journal"
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sort: Option<String>,
}

/// Input parameters for getting summaries by PMID.
#[derive(Serialize, Deserialize, JsonSchema)]
pub struct GetSummariesInput {
    /// Comma-separated list of PubMed IDs (e.g. "37994677,19393038")
    pub ids: String,
}

/// Input parameters for fetching abstracts by PMID.
#[derive(Serialize, Deserialize, JsonSchema)]
pub struct FetchAbstractsInput {
    /// Comma-separated list of PubMed IDs (e.g. "37994677,19393038")
    pub ids: String,
}

/// Input parameters for finding related articles.
#[derive(Serialize, Deserialize, JsonSchema)]
pub struct FindRelatedInput {
    /// Comma-separated list of PubMed IDs (e.g. "37994677,19393038")
    pub ids: String,
}

/// Input parameters for matching a citation.
#[derive(Serialize, Deserialize, JsonSchema)]
pub struct MatchCitationInput {
    /// Citation string in format: journal|year|volume|first_page|author|key|
    /// Example: "proc+natl+acad+sci+u+s+a|1991|88|3248|mann+bj|Art1|"
    pub bdata: String,
}

// ---------------------------------------------------------------------------
// MCP Server
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct PubmedMcpServer {
    client: Arc<PubmedClient>,
    tool_router: ToolRouter<Self>,
}

impl PubmedMcpServer {
    pub fn new(client: Arc<PubmedClient>) -> Self {
        Self {
            client,
            tool_router: Self::tool_router(),
        }
    }
}

#[tool_router]
impl PubmedMcpServer {
    #[tool(
        name = "search_pubmed",
        description = "Search for articles in PubMed by keyword, author, or date. Parameters: query (PubMed search syntax, e.g. 'asthma[Title] AND 2023[pdat]'), max_results (default 20), sort (relevance/pub_date/author/journal)."
    )]
    async fn search_pubmed(
        &self,
        params: Parameters<SearchPubmedInput>,
    ) -> Result<String, String> {
        let input = params.0;
        let query = SearchQuery {
            query: input.query,
            max_results: input.max_results,
            sort: input.sort,
        };

        let result = self
            .client
            .search(&query)
            .await
            .map_err(|e| format!("PubMed search failed: {e}"))?;

        serde_json::to_string(&result).map_err(|e| e.to_string())
    }

    #[tool(
        name = "get_summaries",
        description = "Get document summaries for a list of PubMed IDs. Returns metadata including title, authors, journal, and publication date. Parameters: ids (comma-separated PMIDs, e.g. '37994677,19393038')."
    )]
    async fn get_summaries(
        &self,
        params: Parameters<GetSummariesInput>,
    ) -> Result<String, String> {
        let input = params.0;
        let papers = self
            .client
            .get_summaries(&input.ids)
            .await
            .map_err(|e| format!("PubMed summaries failed: {e}"))?;

        serde_json::to_string(&papers).map_err(|e| e.to_string())
    }

    #[tool(
        name = "fetch_abstracts",
        description = "Fetch full abstracts for a list of PubMed IDs. Returns the full abstract text for each PMID. Parameters: ids (comma-separated PMIDs, e.g. '37994677,19393038')."
    )]
    async fn fetch_abstracts(
        &self,
        params: Parameters<FetchAbstractsInput>,
    ) -> Result<String, String> {
        let input = params.0;
        let abstracts = self
            .client
            .fetch_abstracts(&input.ids)
            .await
            .map_err(|e| format!("PubMed abstracts fetch failed: {e}"))?;

        serde_json::to_string(&abstracts).map_err(|e| e.to_string())
    }

    #[tool(
        name = "find_related",
        description = "Find articles related to a list of PubMed IDs. Returns related PMIDs for each input PMID. Parameters: ids (comma-separated PMIDs, e.g. '37994677,19393038')."
    )]
    async fn find_related(
        &self,
        params: Parameters<FindRelatedInput>,
    ) -> Result<String, String> {
        let input = params.0;
        let related = self
            .client
            .find_related(&input.ids)
            .await
            .map_err(|e| format!("PubMed related articles failed: {e}"))?;

        serde_json::to_string(&related).map_err(|e| e.to_string())
    }

    #[tool(
        name = "get_database_info",
        description = "Get information about the PubMed database, including available search fields and database statistics."
    )]
    async fn get_database_info(
        &self,
        _params: Parameters<Option<serde_json::Value>>,
    ) -> Result<String, String> {
        let info = self
            .client
            .get_database_info()
            .await
            .map_err(|e| format!("PubMed database info failed: {e}"))?;

        serde_json::to_string(&info).map_err(|e| e.to_string())
    }

    #[tool(
        name = "match_citation",
        description = "Match a citation string to a PubMed ID (PMID). Parameters: bdata (citation string in format 'journal|year|volume|first_page|author|key|', e.g. 'proc+natl+acad+sci+u+s+a|1991|88|3248|mann+bj|Art1|')."
    )]
    async fn match_citation(
        &self,
        params: Parameters<MatchCitationInput>,
    ) -> Result<String, String> {
        let input = params.0;
        let matches = self
            .client
            .match_citation(&input.bdata)
            .await
            .map_err(|e| format!("PubMed citation match failed: {e}"))?;

        serde_json::to_string(&matches).map_err(|e| e.to_string())
    }
}

impl ServerHandler for PubmedMcpServer {
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

    tracing::debug!("Creating PubMed client...");
    let client = Arc::new(
        PubmedClient::with_base_url(30, args.api_base_url.clone()).context("Failed to create PubMed client")?,
    );

    match args.command {
        Command::Stdio => {
            let server = PubmedMcpServer::new(client);
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
            let server = PubmedMcpServer::new(client);
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
