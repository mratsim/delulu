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
//!
//! # MCP Server
//!
//! Provides the [`PubmedMcpServer`] shared by the standalone
//! `delulu-pubmed-mcp` binary and the future `delulu-all-mcp` server.
//! Uses the shared `delulu-mcp-server-helper` for common infrastructure.

use crate::PubmedClient;
use crate::core::SearchQuery;
use delulu_mcp_server_helper::impl_server_handler;
use delulu_mcp_server_helper::rmcp::handler::server::tool::ToolRouter;
use delulu_mcp_server_helper::rmcp::handler::server::wrapper::Parameters;
use delulu_mcp_server_helper::rmcp::tool;
use delulu_mcp_server_helper::rmcp::tool_router;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

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

/// Input parameters for fetching a full paper as markdown.
#[derive(Serialize, Deserialize, JsonSchema)]
pub struct GetPaperInput {
    /// PubMed Central ID (e.g. "PMC1234567" or "1234567")
    pub pmc_id: String,
}

/// MCP server exposing PubMed tools (`search_pubmed`, `get_summaries`, `fetch_abstracts`,
/// `find_related`, `get_database_info`, `match_citation`, `get_paper`)
/// over stdio or HTTP transports.
///
/// Shared by the standalone `delulu-pubmed-mcp` binary and `delulu-all-mcp`.
///
/// Pre: constructed via [`PubmedMcpServer::new`] with an `Arc<PubmedClient>`.
/// Post: tools are registered in `tool_router` and callable through the MCP `ServerHandler` impl.
#[derive(Clone)]
pub struct PubmedMcpServer {
    client: Arc<PubmedClient>,
    tool_router: ToolRouter<Self>,
}

impl PubmedMcpServer {
    /// Create a new MCP server for the given PubMed client.
    ///
    /// Pre: `client` is an `Arc<PubmedClient>` (the server holds a shared reference).
    /// Post: returns a server with the tool router initialized; feed it to
    /// `run_stdio`/`run_http` from `delulu-mcp-server-helper`.
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
    async fn search_pubmed(&self, params: Parameters<SearchPubmedInput>) -> Result<String, String> {
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
    async fn get_summaries(&self, params: Parameters<GetSummariesInput>) -> Result<String, String> {
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
    async fn find_related(&self, params: Parameters<FindRelatedInput>) -> Result<String, String> {
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

    #[tool(
        name = "get_paper",
        description = "Fetch a full paper from PubMed Central as markdown. Downloads the PDF and converts via xberg. Parameters: pmc_id (PubMed Central ID, e.g. 'PMC1234567' or '1234567')."
    )]
    async fn get_paper(&self, params: Parameters<GetPaperInput>) -> Result<String, String> {
        let input = params.0;
        let md = self
            .client
            .get_paper(&input.pmc_id)
            .await
            .map_err(|e| format!("PubMed paper fetch failed: {e}"))?;
        Ok(md)
    }
}

impl_server_handler!(PubmedMcpServer);
