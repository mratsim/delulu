//!  Delulu Webfetch — MCP Server
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
//! # MCP Server (library)
//!
//! Provides the [`WebfetchServer`] shared by the standalone
//! `delulu-webfetch-mcp` binary and `delulu-all-mcp`.
//!
//! SSRF protection strings and logic are byte-identical to the standalone
//! binary's previous implementation (INV-007).

use delulu_mcp_server_helper::rmcp::handler::server::tool::ToolRouter;
use delulu_mcp_server_helper::rmcp::handler::server::wrapper::Parameters;
use delulu_mcp_server_helper::rmcp::tool;
use delulu_mcp_server_helper::rmcp::tool_router;
use delulu_mcp_server_helper::impl_server_handler;
use delulu_mcp_server_helper::PeerAddr;
use delulu_rate_limited_crawler::RateLimitedCrawler;
use crate::webfetch_raw_response;
use crate::{fetch_and_extract, fetch_and_extract_with_status};
// Shared Markdown output formatting (one definition, included here via #[path]).
use crate::core::markdown::md_doc_to_string;
use crate::{is_private_ip, same_subnet_16};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::net::IpAddr;
use std::net::SocketAddr;
use std::sync::Arc;
use url::Host;

#[derive(Serialize, Deserialize)]
#[cfg_attr(feature = "mcp", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
struct FetchInput {
    pub url: String,
}

#[derive(Serialize, Deserialize)]
#[cfg_attr(feature = "mcp", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
struct FetchDocInput {
    pub url: String,
    /// Timeout in seconds (reserved for future use; currently uses server default of 30s)
    pub timeout: Option<u64>,
}
/// MCP server exposing webfetch tools (`webfetch`, `webfetch_raw`, `fetch_doc`)
/// over stdio or HTTP transports.
///
/// Shared by the standalone `delulu-webfetch-mcp` binary and `delulu-all-mcp`.
///
/// Pre: constructed via [`WebfetchServer::new`] with an `Arc<RateLimitedCrawler>`
/// and the `expose_local_networks` flag.
/// Post: tools are registered in `tool_router` and callable through the MCP
/// `ServerHandler` impl.
#[derive(Clone)]
pub struct WebfetchServer {
    crawler: Arc<RateLimitedCrawler>,
    expose_local_networks: bool,
    tool_router: ToolRouter<Self>,
}

impl WebfetchServer {
    /// Create a new webfetch MCP server.
    ///
    /// Pre: `crawler` is a fully configured `RateLimitedCrawler` shared with the
    /// caller (single shared instance across all tools); `expose_local_networks`
    /// toggles SSRF protection.
    /// Post: the server's tool router is initialized; feed it to
    /// `run_stdio`/`run_http` from `delulu-mcp-server-helper`.
    pub fn new(crawler: Arc<RateLimitedCrawler>, expose_local_networks: bool) -> Self {
        Self {
            crawler,
            expose_local_networks,
            tool_router: Self::tool_router(),
        }
    }
}

// ---------------------------------------------------------------------------
// Tools
//
// SSRF protection: by default, URLs that resolve to private/internal IP
// ranges are rejected. Use --expose-local-networks to allow fetching from
// local/private networks (intranet docs, private paper repositories).
//
// External requestors get a generic "DNS resolution failed" error
// regardless of whether the URL is invalid, the domain doesn't exist, or
// the IP is private. This prevents the MCP server from being used as an
// oracle to probe the internal LAN topology.
// ---------------------------------------------------------------------------

#[tool_router]
impl WebfetchServer {
    #[tool(description = "Fetch a URL and return content as Markdown with YAML frontmatter")]
    async fn webfetch(
        &self,
        params: Parameters<FetchInput>,
        peer: PeerAddr,
    ) -> Result<String, String> {
        let input = params.0;
        let (remote_addr, local_addr) = match peer.0 {
            Some(info) => (Some(info.remote_addr), Some(info.local_addr)),
            None => (None, None),
        };
        validate_url(
            &input.url,
            self.expose_local_networks,
            remote_addr,
            local_addr,
        )
        .await?;
        match fetch_and_extract(
            &input.url,
            &self.crawler,
            &[crate::pipelines::mozilla_readability::filter_mozilla_readability],
        )
        .await
        {
            Ok(result) => Ok(md_doc_to_string(result)),
            Err(e) => Ok(format!(
                "---\nerror: true\nerror_type: {:?}\n---\n\nFetch failed",
                e
            )),
        }
    }

    #[tool(description = "Fetch a URL and return raw structured data as JSON")]
    async fn webfetch_raw(
        &self,
        params: Parameters<FetchInput>,
        peer: PeerAddr,
    ) -> Result<String, String> {
        let input = params.0;
        let (remote_addr, local_addr) = match peer.0 {
            Some(info) => (Some(info.remote_addr), Some(info.local_addr)),
            None => (None, None),
        };
        validate_url(
            &input.url,
            self.expose_local_networks,
            remote_addr,
            local_addr,
        )
        .await?;
        match fetch_and_extract_with_status(
            &input.url,
            &self.crawler,
            &[crate::pipelines::mozilla_readability::filter_mozilla_readability],
        )
        .await
        {
            // Serialize the ExtractionResult at top level plus a sibling
            // `page_status` key. Never nested under a
            // `result` wrapper.
            Ok((result, status)) => Ok(webfetch_raw_response(&result, &status)),
            Err(e) => Ok(json!({
                "error": true,
                "error_type": e.to_string(),
            })
            .to_string()),
        }
    }

    #[tool(description = "Fetch a document (PDF, DOCX, etc.) and convert to markdown")]
    async fn fetch_doc(
        &self,
        params: Parameters<FetchDocInput>,
        peer: PeerAddr,
    ) -> Result<String, String> {
        let input = params.0;
        let (remote_addr, local_addr) = match peer.0 {
            Some(info) => (Some(info.remote_addr), Some(info.local_addr)),
            None => (None, None),
        };
        validate_url(
            &input.url,
            self.expose_local_networks,
            remote_addr,
            local_addr,
        )
        .await?;
        match crate::fetch_doc(&input.url, &self.crawler).await {
            Ok(result) => Ok(md_doc_to_string(result)),
            Err(e) => Ok(format!(
                "---\nerror: true\nerror_type: {:?}\n---\n\nFetch failed",
                e
            )),
        }
    }
}
// ---------------------------------------------------------------------------
// ServerHandler impl
// ---------------------------------------------------------------------------

impl_server_handler!(WebfetchServer);

// ---------------------------------------------------------------------------
// URL validation (SSRF protection)
// ---------------------------------------------------------------------------

/// Validate that a URL does not target a private/internal IP address.
///
/// Returns `Ok(())` if the URL is safe to fetch, or an error message
/// describing why it was rejected.
///
/// Skips validation entirely when `expose_local_networks` is true.
///
/// `local_addr` is the server's actual local address from the TCP connection
/// (None for stdio). Used for same-subnet detection.
///
/// Error messages are tailored based on whether the requestor appears to be
/// on the same subnet as the server (same /16 for IPv4, same /64 for IPv6).
/// External requestors get a generic "DNS resolution failed" to prevent
/// the MCP server from being used as an oracle to probe the internal LAN
/// topology (distinguishing "domain exists but resolves to 10.x.x.x" from
/// "domain doesn't exist").
///
/// Pre: `url_str` is a URL string; `peer_addr`/`local_addr` are the requestor
/// and server socket addresses (`None` for stdio).
/// Post: returns `Ok(())` when `expose_local_networks` is true or the URL
/// resolves outside private ranges; returns `Err` with the detailed private-IP
/// message for stdio/same-subnet requestors, or the generic "DNS resolution
/// failed" message for external requestors. Unparseable URLs, missing hosts,
/// and DNS failures return the generic message for all requestors.
pub async fn validate_url(
    url_str: &str,
    expose_local_networks: bool,
    peer_addr: Option<SocketAddr>,
    local_addr: Option<SocketAddr>,
) -> Result<(), String> {
    if expose_local_networks {
        return Ok(());
    }

    let parsed = url::Url::parse(url_str).map_err(|_| "DNS resolution failed".to_string())?;
    let host = parsed
        .host()
        .ok_or_else(|| "DNS resolution failed".to_string())?;

    // Determine if the requestor is on the same subnet as the server.
    // stdio (None) is always local — no network attacker can reach it.
    // HTTP sharing the same /16 (/64 for IPv6) → likely same subnet, detailed error is safe.
    // HTTP from a different subnet → could be external, use generic error.
    let requestor_same_subnet = peer_addr
        .zip(local_addr)
        .is_some_and(|(peer, server)| same_subnet_16(peer, server));
    let requestor_is_stdio = peer_addr.is_none();

    let blocked_msg = if requestor_is_stdio || requestor_same_subnet {
        "URL resolves to a private IP address which is blocked by default. ".to_string()
            + "Use --expose-local-networks to allow fetching from local/private networks."
    } else {
        "DNS resolution failed".to_string()
    };

    match host {
        Host::Domain(domain) => {
            let addrs = tokio::net::lookup_host((domain, 0))
                .await
                .map_err(|_| "DNS resolution failed".to_string())?;

            for addr in addrs {
                if is_private_ip(&addr.ip()) {
                    return Err(blocked_msg);
                }
            }
            Ok(())
        }
        Host::Ipv4(ip) => {
            if is_private_ip(&IpAddr::V4(ip)) {
                Err(blocked_msg)
            } else {
                Ok(())
            }
        }
        Host::Ipv6(ip) => {
            if is_private_ip(&IpAddr::V6(ip)) {
                Err(blocked_msg)
            } else {
                Ok(())
            }
        }
    }
}
