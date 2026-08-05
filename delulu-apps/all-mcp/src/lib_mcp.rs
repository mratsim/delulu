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
//! `ServerId`, and the single source of truth `TOOL_ROUTES` table that
//! maps all-mcp tool names to their owning server and inner tool name.

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
/// Single source of truth for both `list_tools` and `call_tool`: the three
/// colliding `get_paper` tools are renamed to `arxiv_get_paper`,
/// `iacr_get_paper`, and `pubmed_get_paper`; the remaining 18 names are
/// already unique and kept identical. Inner name is what is forwarded to the
/// owning server's macro-generated tool handler.
pub static TOOL_ROUTES: &[(&str, ServerId, &str)] = &[
    // webfetch (3)
    ("webfetch", ServerId::Webfetch, "webfetch"),
    ("webfetch_raw", ServerId::Webfetch, "webfetch_raw"),
    ("fetch_doc", ServerId::Webfetch, "fetch_doc"),
    // websearch (2)
    ("web_search", ServerId::Websearch, "web_search"),
    ("web_search_next_page", ServerId::Websearch, "web_search_next_page"),
    // travel (2)
    ("search_flights", ServerId::Travel, "search_flights"),
    ("search_hotels", ServerId::Travel, "search_hotels"),
    // arxiv (3)
    ("search_papers", ServerId::Arxiv, "search_papers"),
    ("get_papers_by_id", ServerId::Arxiv, "get_papers_by_id"),
    ("arxiv_get_paper", ServerId::Arxiv, "get_paper"),
    // iacr (4)
    ("list_recent_papers", ServerId::Iacr, "list_recent_papers"),
    ("get_paper_details", ServerId::Iacr, "get_paper_details"),
    ("paper_pdf_url", ServerId::Iacr, "paper_pdf_url"),
    ("iacr_get_paper", ServerId::Iacr, "get_paper"),
    // pubmed (7)
    ("search_pubmed", ServerId::Pubmed, "search_pubmed"),
    ("get_summaries", ServerId::Pubmed, "get_summaries"),
    ("fetch_abstracts", ServerId::Pubmed, "fetch_abstracts"),
    ("find_related", ServerId::Pubmed, "find_related"),
    ("get_database_info", ServerId::Pubmed, "get_database_info"),
    ("match_citation", ServerId::Pubmed, "match_citation"),
    ("pubmed_get_paper", ServerId::Pubmed, "get_paper"),
];

/// Merged CLI flags for the all-mcp server.
///
/// Pre: all default values are valid (rates within 1..=10000, max size within
/// 1..=1024, URLs parseable).
/// Post: the `--expose-local-networks`, `--qps`, `--burst`, and
/// `--max-resp-size-mb` flags are consumed by the server's crawler factory;
/// the `--*-api-base-url` flags carry the upstream API base URLs that the
/// paper servers (re)target. Rate and size values are validated: an
/// out-of-range value is rejected at parse time.
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

/// Parse and validate a URL flag value, returning a normalized string.
///
/// Pre: `s` is a non-empty string.
/// Post: returns the parsed URL if it is an absolute URL with an http/https
/// scheme; otherwise returns a human-readable clap error.
/// Panic-if: never.
fn parse_url(s: &str) -> Result<String, String> {
    let parsed = url::Url::parse(s).map_err(|e| format!("invalid URL '{s}': {e}"))?;
    match parsed.scheme() {
        "http" | "https" => Ok(parsed.to_string()),
        other => Err(format!(
            "invalid URL '{s}': scheme '{other}' is not allowed (expected http or https)"
        )),
    }
}