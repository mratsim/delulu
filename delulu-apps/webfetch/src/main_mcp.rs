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

//! # Unified MCP Server Entry Point
//!
//! Supports stdio and HTTP transports.
//! Uses the shared `delulu-mcp-server-helper` for common infrastructure.

use anyhow::{Context, Error, Result};
use delulu_mcp_server_helper::clap::Parser;
use delulu_mcp_server_helper::rmcp::handler::server::tool::ToolRouter;
use delulu_mcp_server_helper::rmcp::handler::server::wrapper::Parameters;
use delulu_mcp_server_helper::rmcp::tool;
use delulu_mcp_server_helper::rmcp::tool_router;
use delulu_mcp_server_helper::{
    McpServerConfig, PeerAddr, impl_server_handler, run_http, run_stdio, setup_tracing,
};
use delulu_rate_limited_crawler::RateLimitedCrawler;
use delulu_webfetch::{ExtractionResult, MAX_BODY_SIZE, RedditComment, fetch_and_extract};
use serde::{Deserialize, Serialize};
use std::net::IpAddr;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use url::Host;

#[derive(Parser, Debug)]
#[command(name = "webfetch-mcp")]
struct Args {
    /// Allow fetching URLs that resolve to private/internal IP addresses.
    /// By default, webfetch rejects requests to private IP ranges
    /// (127.0.0.0/8, 10.0.0.0/8, 172.16.0.0/12, 192.168.0.0/16,
    /// ::1, fc00::/7, and cloud metadata endpoints) to prevent SSRF.
    #[arg(long)]
    expose_local_networks: bool,

    #[command(subcommand)]
    command: McpServerConfig,
}

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
#[derive(Clone)]
struct WebfetchServer {
    crawler: Arc<RateLimitedCrawler>,
    expose_local_networks: bool,
    tool_router: ToolRouter<Self>,
}

impl WebfetchServer {
    fn new(crawler: Arc<RateLimitedCrawler>, expose_local_networks: bool) -> Self {
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
            &[delulu_webfetch::pipelines::mozilla_readability::filter_mozilla_readability],
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
        match fetch_and_extract(
            &input.url,
            &self.crawler,
            &[delulu_webfetch::pipelines::mozilla_readability::filter_mozilla_readability],
        )
        .await
        {
            Ok(result) => Ok(serde_json::to_string(&result).unwrap_or_default()),
            Err(e) => Ok(format!("{{\"error\": true, \"error_type\": \"{:?}\"}}", e)),
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
        match delulu_webfetch::fetch_doc(&input.url, &self.crawler).await {
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
async fn validate_url(
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

/// Check if an IP address is in a private/internal range.
fn is_private_ip(ip: &IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            v4.is_loopback()         // 127.0.0.0/8
                || v4.is_private()    // 10.0.0.0/8, 172.16.0.0/12, 192.168.0.0/16
                || v4.is_link_local() // 169.254.0.0/16 (includes cloud metadata 169.254.169.254)
        }
        IpAddr::V6(v6) => {
            v6.is_loopback() // ::1
                || is_ula(v6) // fc00::/7
        }
    }
}

/// Check if an IPv6 address is a Unique Local Address (fc00::/7).
fn is_ula(v6: &std::net::Ipv6Addr) -> bool {
    v6.octets()[0] & 0xfe == 0xfc
}

/// Check if two socket addresses share the same subnet.
/// Uses /16 for IPv4, /64 for IPv6.
fn same_subnet_16(a: SocketAddr, b: SocketAddr) -> bool {
    match (a.ip(), b.ip()) {
        (IpAddr::V4(a), IpAddr::V4(b)) => (u32::from(a) >> 16) == (u32::from(b) >> 16),
        (IpAddr::V6(a), IpAddr::V6(b)) => (u128::from(a) >> 64) == (u128::from(b) >> 64),
        _ => false,
    }
}
// ---------------------------------------------------------------------------
// md_doc_to_string: Convert ExtractionResult to a Markdown string
// ---------------------------------------------------------------------------

/// Convert an `ExtractionResult` into a Markdown string with YAML frontmatter.
fn md_doc_to_string(result: ExtractionResult) -> String {
    match result {
        ExtractionResult::GenericHtml { content_md } => {
            let mut out = String::new();
            if !content_md.frontmatter.is_empty() {
                out.push_str("---\n");
                out.push_str(&content_md.frontmatter);
                if !content_md.frontmatter.ends_with('\n') {
                    out.push('\n');
                }
                out.push_str("---\n\n");
            }
            out.push_str(&content_md.body);
            out
        }
        ExtractionResult::Reddit {
            title,
            selftext,
            author,
            score,
            permalink,
            comments,
        } => {
            let frontmatter = format!(
                "title: {}\nauthor: {}\nscore: {}\nsource_type: reddit\npermalink: {}",
                title, author, score, permalink
            );
            let mut out = format!("---\n{frontmatter}\n---\n\n");
            out.push_str(&selftext);
            out.push('\n');
            for comment in &comments {
                format_reddit_comment(&mut out, comment, 0);
            }
            out
        }
        ExtractionResult::Discourse {
            title,
            topic_id,
            posts,
        } => {
            let frontmatter = format!(
                "title: {}\ntopic_id: {}\nsource_type: discourse",
                title, topic_id
            );
            let mut out = format!("---\n{frontmatter}\n---\n\n");
            for post in &posts {
                out.push_str(&format!(
                    "**{}** (post #{}):\n\n{}\n\n",
                    post.username, post.post_number, post.raw
                ));
            }
            out
        }
    }
}

/// Format a reddit comment recursively into markdown.
fn format_reddit_comment(out: &mut String, comment: &RedditComment, depth: u32) {
    // TODO: fuzz/hardening — unbounded recursion on attacker-controlled Reddit
    // comment trees. Add MAX_DEPTH guard (e.g. 50) to prevent stack exhaustion.
    // See https://github.com/mratsim/delulu/pull/7
    let prefix = "> ".repeat(depth as usize);
    out.push_str(&format!(
        "{}**{}** (score: {}): {}\n\n",
        prefix, comment.author, comment.score, comment.body
    ));
    for reply in &comment.replies {
        format_reddit_comment(out, reply, depth + 1);
    }
}

// ---------------------------------------------------------------------------
// main
// ---------------------------------------------------------------------------

#[tokio::main]
async fn main() -> Result<(), Error> {
    setup_tracing();

    tracing::debug!("Parsing arguments...");
    let args = Args::parse();
    tracing::debug!("Parsed args: {:?}", args);

    tracing::debug!("Creating rate-limited crawler...");
    let crawler = Arc::new(
        RateLimitedCrawler::builder()
            .with_qps(2)
            .with_max_resp_size(MAX_BODY_SIZE)
            .with_timeout(Duration::from_secs(30))
            .with_connect_timeout(Duration::from_secs(30))
            .build()
            .context("Failed to create rate-limited crawler")?,
    );
    tracing::debug!("Crawler created");

    match args.command {
        McpServerConfig::Stdio => {
            let server = WebfetchServer::new(crawler, args.expose_local_networks);
            run_stdio(server).await?;
        }
        McpServerConfig::Http { host, port } => {
            let server = WebfetchServer::new(crawler, args.expose_local_networks);
            run_http(server, host, port).await?;
        }
    }

    Ok(())
}
