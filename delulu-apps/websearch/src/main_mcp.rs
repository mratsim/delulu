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

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Error, Result};
use delulu_mcp_server_helper::clap::Parser;
use delulu_mcp_server_helper::rmcp::handler::server::tool::ToolRouter;
use delulu_mcp_server_helper::rmcp::handler::server::wrapper::Parameters;
use delulu_mcp_server_helper::rmcp::tool;
use delulu_mcp_server_helper::rmcp::tool_router;
use delulu_mcp_server_helper::{
    McpServerConfig, impl_server_handler, run_http, run_stdio, setup_tracing,
};
use delulu_websearch::SessionCache;
use delulu_websearch::SessionKey;
use delulu_websearch::engine::{EngineId, SearchParams};
use delulu_websearch::engines::{EngineRegistry, create_default_registry};
use delulu_websearch::mcp_serialization::{
    McpNextPageResponse, McpSearchResponse, engine_name_to_id, sanitize_error_for_client,
};
use delulu_websearch::parsers::{
    parse_country, parse_max_results, parse_safesearch, validate_query,
};
use serde::{Deserialize, Serialize};

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
// Tool input
// ---------------------------------------------------------------------------

/// Input parameters for the `web_search` MCP tool.
#[derive(Serialize, Deserialize)]
#[cfg_attr(feature = "mcp", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
struct WebSearchInput {
    /// Search query (required, must be non-empty after trimming).
    pub query: String,
    /// Engine to use ("brave", "duckduckgo"). Defaults to "duckduckgo".
    #[serde(default)]
    pub engine: Option<String>,
    /// Country / region code (e.g. "us", "de", "jp"). Engine-specific.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub country: Option<String>,
    /// Safesearch level ("strict", "moderate", "off"). Engine-specific.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub safesearch: Option<String>,
    /// Time range filter (e.g. "2024-01-01to2024-12-31"). Engine-specific.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub time_range: Option<String>,
    /// Maximum number of results to return. Defaults to 20, hard limit 100.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_results: Option<u32>,
}

/// Input parameters for the `web_search_next_page` MCP tool.
#[derive(Serialize, Deserialize)]
#[cfg_attr(feature = "mcp", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
struct WebSearchNextPageInput {
    /// Session key from a previous `web_search` response.
    pub session_key: String,
}

// ---------------------------------------------------------------------------
// MCP server
// ---------------------------------------------------------------------------

/// The MCP server for web search.
///
/// Holds an `Arc<EngineRegistry>` for engine lookups and an `Arc<SessionCache>`
/// for pagination state.
#[derive(Clone)]
struct WebsearchServer {
    engine_registry: Arc<EngineRegistry>,
    session_cache: Arc<SessionCache>,
    tool_router: ToolRouter<Self>,
}

impl WebsearchServer {
    fn new(engine_registry: Arc<EngineRegistry>, session_cache: Arc<SessionCache>) -> Self {
        Self {
            engine_registry,
            session_cache,
            tool_router: Self::tool_router(),
        }
    }
}

#[tool_router]
impl WebsearchServer {
    /// Search the web using one or more search engines.
    ///
    /// Parameters:
    /// - `query` (required): The search query.
    /// - `engine` (optional, default "all"): Engine to use ("brave", "duckduckgo", "all").
    /// - `page` (optional, default 1): Page number (1-indexed).
    /// - `country` (optional): Country / region code.
    /// - `safesearch` (optional): Safesearch level ("strict", "moderate", "off").
    /// - `time_range` (optional): Time range filter.
    /// - `max_results` (optional): Maximum results (default 20, max 100).
    #[tool(
        name = "web_search",
        description = "Search the web using DuckDuckGo. Parameters: query (required), engine (optional, default 'duckduckgo': 'brave', 'duckduckgo'), country (optional, ISO alpha-2), safesearch (optional, 'strict'/'moderate'/'off'), time_range (optional), max_results (optional, default 20, max 100). Returns JSON with session_key, results, has_next_page, and continuation_engine."
    )]
    async fn web_search(&self, params: Parameters<WebSearchInput>) -> Result<String, String> {
        let input = params.0;

        // Validate query
        let query =
            validate_query(input.query.trim()).map_err(|e| format!("Invalid query: {e}"))?;

        // Determine engine to search
        let engine_names: Vec<&str> = match input.engine.as_deref() {
            None | Some("duckduckgo") => vec!["duckduckgo"],
            Some(name) => vec![name],
        };

        if engine_names.is_empty() {
            return Err("No search engines available".to_string());
        }

        // Validate and build common search params
        let max_results = parse_max_results(input.max_results)
            .map_err(|e| format!("Invalid max_results: {e}"))?;
        let safesearch = parse_safesearch(input.safesearch.as_deref())
            .map_err(|e| format!("Invalid safesearch: {e}"))?;
        let country =
            parse_country(input.country.as_deref()).map_err(|e| format!("Invalid country: {e}"))?;
        let search_params = SearchParams {
            page: None,
            country: Some(country.to_string()),
            safesearch: Some(safesearch.to_string()),
            time_range: input.time_range,
            max_results: Some(max_results as u32),
        };

        // Search each engine and collect results
        let mut all_results: HashMap<String, Vec<delulu_websearch::SearchResult>> = HashMap::new();
        let mut engine_errors: Vec<String> = Vec::new();
        let mut stored_continuation_engine: Option<String> = None;
        let mut continuation_box: Option<Box<dyn delulu_websearch::Continuation>> = None;

        for engine_name in &engine_names {
            let engine = match self.engine_registry.get_engine(engine_name) {
                Some(e) => e,
                None => continue,
            };

            let response = match engine.search(query, search_params.clone(), None).await {
                Ok(resp) => resp,
                Err(e) => {
                    let err_msg = format!("{engine_name}: {e}");
                    tracing::warn!("Engine '{engine_name}' search failed: {e}");
                    engine_errors.push(err_msg);
                    continue;
                }
            };

            // Store results
            all_results.insert(engine_name.to_string(), response.results);

            // Track continuation from the first engine that has one
            if response.continuation.is_some() && stored_continuation_engine.is_none() {
                stored_continuation_engine = Some(engine_name.to_string());
                continuation_box = response.continuation;
            }
        }

        if all_results.is_empty() {
            let detail = if engine_errors.is_empty() {
                "No search engines available".to_string()
            } else {
                format!("All search engines failed: {}", engine_errors.join("; "))
            };
            return Err(detail);
        }

        // Generate session key and store continuation in cache
        let mut random_id = [0u8; 8];
        getrandom::getrandom(&mut random_id)
            .map_err(|e| format!("Failed to generate random session ID: {e}"))?;

        let now = std::time::Instant::now();
        let has_next_page = continuation_box.is_some();

        // Determine the engine ID for the session key (use first successful engine)
        let session_engine = stored_continuation_engine
            .as_deref()
            .or_else(|| engine_names.first().copied())
            .unwrap_or("all");

        let session_engine_id = engine_name_to_id(session_engine).unwrap_or(EngineId::Brave);

        let session_key = self.session_cache.store(
            session_engine_id,
            query,
            search_params,
            continuation_box,
            now,
            random_id,
        );

        let mcp_response = McpSearchResponse {
            session_key: session_key.to_string(),
            results: all_results,
            has_next_page,
            continuation_engine: stored_continuation_engine,
            engine_errors: if engine_errors.is_empty() {
                None
            } else {
                Some(engine_errors)
            },
        };

        serde_json::to_string(&mcp_response).map_err(|e| format!("Serialization failed: {e}"))
    }

    /// Fetch the next page of results for an existing search session.
    ///
    /// Parameters:
    /// - `session_key` (required): The session key from a previous `web_search` response.
    #[tool(
        name = "web_search_next_page",
        description = "Fetch the next page of results for a search session. Parameters: session_key (required, from web_search response). Returns JSON with results and has_next_page."
    )]
    async fn web_search_next_page(
        &self,
        params: Parameters<WebSearchNextPageInput>,
    ) -> Result<String, String> {
        let input = params.0;
        let now = std::time::Instant::now();

        // Parse session key
        let key: SessionKey =
            match serde_json::from_value(serde_json::Value::String(input.session_key.clone())) {
                Ok(k) => k,
                Err(_) => {
                    tracing::warn!("Invalid session key format");
                    return Err("Session not found or expired".to_string());
                }
            };

        // Look up session in cache
        let entry = match self.session_cache.get(&key, now) {
            Some(e) => e,
            None => {
                tracing::warn!("Session not found or expired");
                return Err("Session not found or expired".to_string());
            }
        };

        // Check if continuation exists
        let continuation = match entry.continuation {
            Some(c) => c,
            None => {
                tracing::info!("No more pages available");
                return Err("No more pages available".to_string());
            }
        };

        // Check max pages guard
        // Look up engine in registry
        let engine_name = entry.engine.to_string();
        let engine = match self.engine_registry.get_engine(&engine_name) {
            Some(e) => e,
            None => {
                tracing::warn!("Session engine not available: {}", engine_name);
                return Err(format!("Session engine not available: {engine_name}"));
            }
        };

        // Call engine.search with continuation
        let response = match engine
            .search(&entry.query, entry.params, Some(&*continuation))
            .await
        {
            Ok(resp) => resp,
            Err(e) => {
                tracing::error!("Next page search failed for session: {e:?}");
                return Err(sanitize_error_for_client(&e));
            }
        };

        // Store new continuation
        let has_next_page = response.continuation.is_some();
        if let Err(e) = self
            .session_cache
            .update_continuation(&key, response.continuation, now)
        {
            tracing::error!("Failed to update continuation: {e:?}");
        }

        let mcp_response = McpNextPageResponse {
            results: response.results,
            has_next_page,
        };

        serde_json::to_string(&mcp_response).map_err(|e| format!("Serialization failed: {e}"))
    }
}

impl_server_handler!(WebsearchServer);

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
    let session_cache = Arc::new(SessionCache::new(1024, Duration::from_secs(3600)));

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
