//!  Delulu All-MCP — MCP types
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

//! MCP types for the unified all-mcp server (feature `mcp`).
//!
//! Holds the merged `AllMcpConfig` CLI flags, the owning-server enum
//! `ServerId`, the single source of truth `TOOL_ROUTES` table that
//! maps all-mcp tool names to their owning server and inner tool name, and
//! the hand-written `AllServer` delegator that serves the 21-tool union.

/// Identifier for the inner server that owns a tool.
///
/// Pre: none.
/// Post: each variant corresponds to exactly one of the six consumed app
/// crates' MCP servers.
/// Panic-if: never (plain data enum).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServerId {
    /// webfetch server: `webfetch`, `webfetch_raw`, `fetch_doc`.
    Webfetch,
    /// websearch server: `web_search`, `web_search_next_page`.
    Websearch,
    /// travel server: `search_flights`, `search_hotels`.
    Travel,
    /// arxiv paper-search server: `search_papers`, `get_papers_by_id`, `get_paper`.
    Arxiv,
    /// iacr paper-search server: `list_recent_papers`, `get_paper_details`,
    /// `paper_pdf_url`, `get_paper`.
    Iacr,
    /// pubmed paper-search server: `search_pubmed`, `get_summaries`,
    /// `fetch_abstracts`, `find_related`, `get_database_info`, `match_citation`,
    /// `get_paper`.
    Pubmed,
}

/// The 21-tool union, as (all-mcp name, owning server, inner tool name).
///
/// Single source of truth for both `list_tools` and `call_tool`. The three
/// paper servers are separate repositories with overlapping vocabulary
/// (papers, ids, details, abstracts, citations), so every paper tool is
/// namespaced by repository prefix (`arxiv_*`, `iacr_*`, `pubmed_*`) — the
/// name alone must tell the caller which repository it talks to. The
/// webfetch/websearch/travel tools already self-identify and keep their
/// names. Inner name is what is forwarded to the owning server's
/// macro-generated tool handler.
pub static TOOL_ROUTES: &[(&str, ServerId, &str)] = &[
    // webfetch (3)
    ("webfetch", ServerId::Webfetch, "webfetch"),
    ("webfetch_raw", ServerId::Webfetch, "webfetch_raw"),
    ("fetch_doc", ServerId::Webfetch, "fetch_doc"),
    // websearch (2)
    ("web_search", ServerId::Websearch, "web_search"),
    (
        "web_search_next_page",
        ServerId::Websearch,
        "web_search_next_page",
    ),
    // travel (2)
    ("search_flights", ServerId::Travel, "search_flights"),
    ("search_hotels", ServerId::Travel, "search_hotels"),
    // arxiv (3)
    ("arxiv_search_papers", ServerId::Arxiv, "search_papers"),
    (
        "arxiv_get_papers_by_id",
        ServerId::Arxiv,
        "get_papers_by_id",
    ),
    ("arxiv_get_paper", ServerId::Arxiv, "get_paper"),
    // iacr (4)
    (
        "iacr_list_recent_papers",
        ServerId::Iacr,
        "list_recent_papers",
    ),
    (
        "iacr_get_paper_details",
        ServerId::Iacr,
        "get_paper_details",
    ),
    ("iacr_paper_pdf_url", ServerId::Iacr, "paper_pdf_url"),
    ("iacr_get_paper", ServerId::Iacr, "get_paper"),
    // pubmed (7)
    ("pubmed_search", ServerId::Pubmed, "search_pubmed"),
    ("pubmed_get_summaries", ServerId::Pubmed, "get_summaries"),
    (
        "pubmed_fetch_abstracts",
        ServerId::Pubmed,
        "fetch_abstracts",
    ),
    ("pubmed_find_related", ServerId::Pubmed, "find_related"),
    (
        "pubmed_get_database_info",
        ServerId::Pubmed,
        "get_database_info",
    ),
    ("pubmed_match_citation", ServerId::Pubmed, "match_citation"),
    ("pubmed_get_paper", ServerId::Pubmed, "get_paper"),
];

/// Merged CLI flags for the all-mcp server.
///
/// Pre: all default values are valid (rates within 1..=10000, max size within
/// 1..=1024, URLs parseable).
/// Post: the `--qps`, `--burst`, and `--max-resp-size-mb` flags drive the
/// shared crawler built by `main`; `--expose-local-networks` and the
/// `--*-api-base-url` flags are read by `AllServer::new` (webfetch SSRF
/// policy and the paper servers' API base URLs respectively). Rate and size
/// values are validated: an out-of-range value is rejected at parse time.
/// Panic-if: `clap` rejects a value outside the declared ranges before this
/// struct is constructed.
#[derive(Debug, Clone, clap::Args)]
pub struct AllMcpConfig {
    /// Allow the webfetch tools to reach local/private networks (default: false).
    #[arg(long, default_value_t = false)]
    pub expose_local_networks: bool,

    /// Base URL for the arXiv API (default: https://export.arxiv.org/api/query).
    #[arg(long, default_value = "https://export.arxiv.org/api/query", value_parser = parse_url)]
    pub arxiv_api_base_url: String,

    /// Base URL for the IACR eprint API (default: https://eprint.iacr.org).
    #[arg(long, default_value = "https://eprint.iacr.org", value_parser = parse_url)]
    pub iacr_api_base_url: String,

    /// Base URL for the PubMed E-utilities API (default: https://eutils.ncbi.nlm.nih.gov/entrez/eutils).
    #[arg(long, default_value = "https://eutils.ncbi.nlm.nih.gov/entrez/eutils", value_parser = parse_url)]
    pub pubmed_api_base_url: String,

    /// Queries per second for rate-limited upstream calls (default: 1).
    #[arg(long, default_value_t = 1, value_parser = clap::value_parser!(u32).range(1..=10000))]
    pub qps: u32,

    /// Burst size for rate-limited upstream calls (default: 1).
    #[arg(long, default_value_t = 1, value_parser = clap::value_parser!(u32).range(1..=10000))]
    pub burst: u32,

    /// Maximum response size in MB for downloads (default: 50).
    #[arg(long, default_value_t = 50, value_parser = clap::value_parser!(u32).range(1..=1024))]
    pub max_resp_size_mb: u32,
}

/// Parse and validate a URL flag value, returning the ORIGINAL input string.
///
/// Pre: `s` is a non-empty string.
/// Post: returns `s` unchanged if it is an absolute URL with an http/https
/// scheme (validation only, no normalization — `Url::to_string()` would
/// append a trailing slash to bare origins like `https://eprint.iacr.org`,
/// which downstream clients then double with their own `/`); otherwise
/// returns a human-readable clap error.
/// Panic-if: never.
fn parse_url(s: &str) -> Result<String, String> {
    let parsed = url::Url::parse(s).map_err(|e| format!("invalid URL '{s}': {e}"))?;
    match parsed.scheme() {
        "http" | "https" => Ok(s.to_string()),
        other => Err(format!(
            "invalid URL '{s}': scheme '{other}' is not allowed (expected http or https)"
        )),
    }
}

// ---------------------------------------------------------------------------
// AllServer — hand-written delegator
// ---------------------------------------------------------------------------

use delulu_mcp_server_helper::rmcp::RoleServer;
use delulu_mcp_server_helper::rmcp::handler::server::ServerHandler;
use delulu_mcp_server_helper::rmcp::model::{
    CallToolRequestParam, CallToolResult, ErrorData, Implementation, ListToolsResult,
    PaginatedRequestParam, ProtocolVersion, ServerCapabilities, ServerInfo, Tool, ToolsCapability,
};
use delulu_mcp_server_helper::rmcp::service::RequestContext;
use delulu_paper_search_arxiv::{ArxivClient, ArxivMcpServer};
use delulu_paper_search_iacr::{IacrClient, IacrMcpServer};
use delulu_paper_search_pubmed::{PubmedClient, PubmedMcpServer};
use delulu_rate_limited_crawler::RateLimitedCrawler;
use delulu_travel_search::{GoogleFlightsClient, GoogleHotelsClient, TravelAgentServer};
use delulu_webfetch::WebfetchServer;
use delulu_websearch::engines::create_registry_with_crawler;
use delulu_websearch::{SessionCache, WebsearchServer};
use std::borrow::Cow;
use std::future::Future;
use std::sync::Arc;
use std::time::Duration;

/// The unified all-mcp server exposing the 21-tool union across webfetch,
/// websearch, travel, and the three paper-search crates.
///
/// Constructed once with a single shared [`RateLimitedCrawler`]; each inner
/// server delegates to its own macro-generated `ServerHandler`.
///
/// Pre: `config` is a validated [`AllMcpConfig`]; `crawler` is a fully
/// configured `Arc<RateLimitedCrawler>` shared by all six inner servers.
/// Post: all six inner servers are constructed with `crawler` cloned by
/// reference; the crate is ready to serve the full 21-tool union.
/// Panic-if: never (infallible constructor).
#[derive(Clone)]
pub struct AllServer {
    webfetch: WebfetchServer,
    websearch: WebsearchServer,
    travel: TravelAgentServer,
    arxiv: ArxivMcpServer,
    iacr: IacrMcpServer,
    pubmed: PubmedMcpServer,
}

impl AllServer {
    /// Create a new unified all-mcp server.
    ///
    /// Pre: `config` carries the merged CLI flags; `crawler` is the one shared
    /// crawler built by `main()` from the rate fields.
    /// Post: returns an `AllServer` holding all six inner MCP servers, each
    ///     sharing the same `crawler` via `Arc::clone`. The three rate fields
    ///     (`qps`, `burst`, `max_resp_size_mb`) are **not** read here — they
    ///     are consumed upstream by `main()` when building the crawler.
    /// Panic-if: never (infallible constructor).
    pub fn new(config: AllMcpConfig, crawler: Arc<RateLimitedCrawler>) -> Self {
        let webfetch = WebfetchServer::new(Arc::clone(&crawler), config.expose_local_networks);

        let websearch = {
            let registry = Arc::new(create_registry_with_crawler(Arc::clone(&crawler)));
            let session_cache = Arc::new(SessionCache::new(512, Duration::from_secs(600)));
            WebsearchServer::new(registry, session_cache)
        };

        let travel = {
            let flights = GoogleFlightsClient::new_with_crawler(Arc::clone(&crawler), "en", "USD");
            let hotels = GoogleHotelsClient::new_with_crawler(Arc::clone(&crawler));
            TravelAgentServer::new(Arc::new(flights), Arc::new(hotels))
        };

        let arxiv = {
            let client = ArxivClient::new_with_crawler(Arc::clone(&crawler))
                .with_api_url(config.arxiv_api_base_url);
            ArxivMcpServer::new(Arc::new(client))
        };

        let iacr = {
            let client = IacrClient::new_with_crawler(Arc::clone(&crawler))
                .with_base_url(config.iacr_api_base_url);
            IacrMcpServer::new(Arc::new(client))
        };

        let pubmed = {
            let client = PubmedClient::new_with_crawler(Arc::clone(&crawler))
                .with_api_url(config.pubmed_api_base_url);
            PubmedMcpServer::new(Arc::new(client))
        };

        Self {
            webfetch,
            websearch,
            travel,
            arxiv,
            iacr,
            pubmed,
        }
    }

    /// Union of the six inner servers' tool lists, renamed per `TOOL_ROUTES`.
    ///
    /// All paper tools are namespaced by repository prefix; their
    /// descriptions and input schemas are kept byte-identical (no description
    /// suffix).
    ///
    /// Pre: `ctx` is the current request context for a `list_tools` call.
    /// Post: returns the 21-tool union in `TOOL_ROUTES` order, each tool
    ///     carrying its owning inner server's description and schema.
    /// Panic-if: never (errors are returned).
    async fn collect_tools(&self, ctx: RequestContext<RoleServer>) -> Result<Vec<Tool>, ErrorData> {
        let webfetch = self.webfetch.list_tools(None, ctx.clone()).await?;
        let websearch = self.websearch.list_tools(None, ctx.clone()).await?;
        let travel = self.travel.list_tools(None, ctx.clone()).await?;
        let arxiv = self.arxiv.list_tools(None, ctx.clone()).await?;
        let iacr = self.iacr.list_tools(None, ctx.clone()).await?;
        let pubmed = self.pubmed.list_tools(None, ctx.clone()).await?;

        let mut emitted = Vec::with_capacity(TOOL_ROUTES.len());

        for (route_name, server_id, inner_name) in TOOL_ROUTES {
            let inner_tools = match server_id {
                ServerId::Webfetch => &webfetch.tools,
                ServerId::Websearch => &websearch.tools,
                ServerId::Travel => &travel.tools,
                ServerId::Arxiv => &arxiv.tools,
                ServerId::Iacr => &iacr.tools,
                ServerId::Pubmed => &pubmed.tools,
            };

            let orig = inner_tools.iter().find(|t| t.name.as_ref() == *inner_name);
            match orig {
                Some(tool) => {
                    let out = if *route_name == *inner_name {
                        tool.clone()
                    } else {
                        // The 3 colliding `get_paper` tools are renamed; the
                        // description and input schema are kept byte-identical.
                        Tool {
                            name: Cow::Owned((*route_name).to_string()),
                            ..tool.clone()
                        }
                    };
                    emitted.push(out);
                }
                None => {
                    tracing::error!(
                        "all-mcp route '{route_name}' -> inner '{inner_name}' ({server_id:?}) has no matching live tool"
                    );
                    debug_assert!(
                        false,
                        "route '{route_name}' -> '{inner_name}' not in {server_id:?} live tools"
                    );
                }
            }
        }

        Ok(emitted)
    }
}

impl ServerHandler for AllServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            protocol_version: ProtocolVersion::V_2025_03_26,
            capabilities: ServerCapabilities {
                tools: Some(ToolsCapability::default()),
                ..Default::default()
            },
            server_info: Implementation::from_build_env(),
            instructions: None,
        }
    }

    fn list_tools(
        &self,
        _request: Option<PaginatedRequestParam>,
        context: RequestContext<RoleServer>,
    ) -> impl Future<Output = Result<ListToolsResult, ErrorData>> + Send + '_ {
        Box::pin(async move {
            let tools = self.collect_tools(context).await?;
            Ok(ListToolsResult::with_all_items(tools))
        })
    }

    fn call_tool(
        &self,
        request: CallToolRequestParam,
        context: RequestContext<RoleServer>,
    ) -> impl Future<Output = Result<CallToolResult, ErrorData>> + Send + '_ {
        // Read the all-mcp tool name off the request (NOT request.params).
        let all_name = request.name.as_ref().to_string();
        tracing::debug!(tool = %all_name, "call_tool dispatch");

        // Clone/copy the static route out before matching on `request`.
        let route = TOOL_ROUTES
            .iter()
            .find(|&&(n, _, _)| n == all_name)
            .copied();

        Box::pin(async move {
            let (server_id, inner_name, needs_rename) = match route {
                Some((rn, sid, inner)) => (sid, inner, inner != rn),
                None => {
                    // Unknown tool. The available tools are already listed to
                    // the client via tools/list (injected into the LLM's
                    // context), so a did-you-mean hint would be noise — the
                    // caller can retry with a name it already has.
                    tracing::warn!(tool = %all_name, "tool not found");
                    return Err(ErrorData::invalid_params("tool not found", None));
                }
            };

            // For the renamed tools (all paper tools), rewrite the name to the
            // inner tool name; for the self-named tools forward the original
            // request untouched.
            let forward = if needs_rename {
                CallToolRequestParam {
                    name: Cow::Owned(inner_name.to_string()),
                    arguments: request.arguments.clone(),
                    task: request.task.clone(),
                }
            } else {
                request
            };

            match server_id {
                ServerId::Webfetch => self.webfetch.call_tool(forward, context).await,
                ServerId::Websearch => self.websearch.call_tool(forward, context).await,
                ServerId::Travel => self.travel.call_tool(forward, context).await,
                ServerId::Arxiv => self.arxiv.call_tool(forward, context).await,
                ServerId::Iacr => self.iacr.call_tool(forward, context).await,
                ServerId::Pubmed => self.pubmed.call_tool(forward, context).await,
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::parse_url;

    #[test]
    fn parse_url_returns_original_input_without_trailing_slash() {
        // BUG-B-001 regression: the parser must VALIDATE without normalizing.
        // `Url::to_string()` appends a trailing slash to bare origins, which
        // downstream clients (e.g. IacrClient) would then double.
        assert_eq!(
            parse_url("https://eprint.iacr.org").unwrap(),
            "https://eprint.iacr.org"
        );
        assert_eq!(
            parse_url("https://export.arxiv.org/api/query").unwrap(),
            "https://export.arxiv.org/api/query"
        );
        assert_eq!(
            parse_url("http://localhost:8080").unwrap(),
            "http://localhost:8080"
        );
    }

    #[test]
    fn parse_url_keeps_explicit_trailing_slash() {
        // User-provided trailing slash is preserved verbatim.
        assert_eq!(
            parse_url("https://eprint.iacr.org/").unwrap(),
            "https://eprint.iacr.org/"
        );
    }

    #[test]
    fn parse_url_rejects_non_http_schemes() {
        assert!(parse_url("ftp://example.com").is_err());
        assert!(parse_url("file:///etc/passwd").is_err());
        assert!(parse_url("javascript:alert(1)").is_err());
    }

    #[test]
    fn parse_url_rejects_relative_urls() {
        assert!(parse_url("eprint.iacr.org").is_err());
        assert!(parse_url("/api/query").is_err());
        assert!(parse_url("").is_err());
    }
}
