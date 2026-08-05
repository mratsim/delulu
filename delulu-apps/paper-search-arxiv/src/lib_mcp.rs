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
//!
//! # MCP Server
//!
//! Provides the [`ArxivMcpServer`] shared by the standalone
//! `delulu-arxiv-mcp` binary and the future `delulu-all-mcp` server.
//! Uses the shared `delulu-mcp-server-helper` for common infrastructure.

use crate::core::SearchQuery;
use crate::ArxivClient;
use delulu_mcp_server_helper::impl_server_handler;
use delulu_mcp_server_helper::rmcp::handler::server::tool::ToolRouter;
use delulu_mcp_server_helper::rmcp::handler::server::wrapper::Parameters;
use delulu_mcp_server_helper::rmcp::tool;
use delulu_mcp_server_helper::rmcp::tool_router;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

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

/// Input parameters for fetching a full paper as markdown.
#[derive(Serialize, Deserialize, JsonSchema)]
pub struct GetPaperInput {
    /// arXiv ID (e.g. "1706.03762" or "cond-mat/0011267")
    pub arxiv_id: String,
}
/// MCP server exposing arXiv tools (`search_papers`, `get_papers_by_id`, `get_paper`)
/// over stdio or HTTP transports.
///
/// Shared by the standalone `delulu-arxiv-mcp` binary and `delulu-all-mcp`.
///
/// Pre: constructed via [`ArxivMcpServer::new`] with an `Arc<ArxivClient>`.
/// Post: tools are registered in `tool_router` and callable through the MCP `ServerHandler` impl.
#[derive(Clone)]
pub struct ArxivMcpServer {
    client: Arc<ArxivClient>,
    tool_router: ToolRouter<Self>,
}

impl ArxivMcpServer {
    /// Create a new MCP server for the given arXiv client.
    ///
    /// Pre: `client` is an `Arc<ArxivClient>` (the server holds a shared reference).
    /// Post: returns a server with the tool router initialized; feed it to
    /// `run_stdio`/`run_http` from `delulu-mcp-server-helper`.
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
    async fn search_papers(&self, params: Parameters<SearchPapersInput>) -> Result<String, String> {
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

    #[tool(
        name = "get_paper",
        description = "Fetch a full paper from arXiv as markdown. Downloads the arXiv HTML5 version, strips navigation chrome, and converts to markdown with LaTeX math preserved. Parameters: arxiv_id (arXiv ID, e.g. '1706.03762')."
    )]
    async fn get_paper(&self, params: Parameters<GetPaperInput>) -> Result<String, String> {
        let input = params.0;
        let md = self
            .client
            .get_paper(&input.arxiv_id)
            .await
            .map_err(|e| format!("arXiv paper fetch failed: {e}"))?;
        Ok(md)
    }
}

impl_server_handler!(ArxivMcpServer);
