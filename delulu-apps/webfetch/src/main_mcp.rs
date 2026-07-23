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
use delulu_mcp_server_helper::{McpServerConfig, impl_server_handler, run_http, run_stdio, setup_tracing};
use delulu_rate_limited_crawler::RateLimitedCrawler;
use delulu_webfetch::{ExtractionResult, RedditComment, MAX_BODY_SIZE, fetch_and_extract};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Duration;

#[derive(Parser, Debug)]
#[command(name = "webfetch-mcp")]
struct Args {
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
    tool_router: ToolRouter<Self>,
}

impl WebfetchServer {
    fn new(crawler: Arc<RateLimitedCrawler>) -> Self {
        Self {
            crawler,
            tool_router: Self::tool_router(),
        }
    }
}

// ---------------------------------------------------------------------------
// Tools
//
// ⚠️ KNOWN ISSUE: SSRF via arbitrary URL fetch
// The webfetch and fetch_doc tools accept arbitrary URLs with no domain
// allowlist, no IP-range validation, and no authentication. An attacker who
// reaches the port can probe internal services, cloud metadata endpoints,
// and localhost resources.
//
// This is intentional: we assume the MCP server is only accessed by
// trusted clients (e.g., bound to localhost, behind a reverse proxy with
// auth, or over stdio). Adding URL validation would prevent fetching
// internal network pages (intranet docs, private paper repositories).
//
// If deploying on a network with untrusted access, either:
//   - Bind to 127.0.0.1 instead of 0.0.0.0
//   - Use stdio transport instead of HTTP
// ---------------------------------------------------------------------------

#[tool_router]
impl WebfetchServer {
    #[tool(description = "Fetch a URL and return content as Markdown with YAML frontmatter")]
    async fn webfetch(&self, params: Parameters<FetchInput>) -> Result<String, String> {
        let input = params.0;
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
    async fn webfetch_raw(&self, params: Parameters<FetchInput>) -> Result<String, String> {
        let input = params.0;
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
    async fn fetch_doc(&self, params: Parameters<FetchDocInput>) -> Result<String, String> {
        let input = params.0;
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
            let server = WebfetchServer::new(crawler);
            run_stdio(server).await?;
        }
        McpServerConfig::Http { host, port } => {
            let server = WebfetchServer::new(crawler);
            run_http(server, host, port).await?;
        }
    }

    Ok(())
}
