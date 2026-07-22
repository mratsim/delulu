//! # Unified MCP Server Entry Point
//!
//! Supports stdio and HTTP transports.

use anyhow::{Context, Error, Result};
use clap::{Parser, Subcommand};
use delulu_webfetch::{ExtractionResult, RedditComment, WebbfetchClient, fetch_and_extract};
use rmcp::handler::server::{ServerHandler, tool::ToolRouter, wrapper::Parameters};
use rmcp::service::serve_server;
use rmcp::tool;
use rmcp::tool_router;
use rmcp::transport::streamable_http_server::{
    StreamableHttpServerConfig, StreamableHttpService, session::local::LocalSessionManager,
};
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::sync::Arc;
use tracing_subscriber::{EnvFilter, layer::SubscriberExt, util::SubscriberInitExt};

#[derive(Parser, Debug)]
#[command(name = "webfetch-mcp")]
struct Args {
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

        #[arg(long, default_value = "8081")]
        port: u16,
    },
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
    client: Arc<WebbfetchClient>,
    tool_router: ToolRouter<Self>,
}

impl WebfetchServer {
    fn new(client: Arc<WebbfetchClient>) -> Self {
        Self {
            client,
            tool_router: Self::tool_router(),
        }
    }
}

// ---------------------------------------------------------------------------
// Tools
// ---------------------------------------------------------------------------

#[tool_router]
impl WebfetchServer {
    #[tool(description = "Fetch a URL and return content as Markdown with YAML frontmatter")]
    async fn webfetch(&self, params: Parameters<FetchInput>) -> Result<String, String> {
        let input = params.0;
        match fetch_and_extract(
            &input.url,
            &self.client,
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
            &self.client,
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
        match delulu_webfetch::fetch_doc(&input.url, &self.client).await {
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

impl ServerHandler for WebfetchServer {
    fn list_tools(
        &self,
        _request: Option<rmcp::model::PaginatedRequestParam>,
        _context: rmcp::service::RequestContext<rmcp::RoleServer>,
    ) -> impl Future<Output = Result<rmcp::model::ListToolsResult, rmcp::ErrorData>> + Send + '_
    {
        tracing::debug!(
            "list_tools called, tools count: {}",
            self.tool_router.list_all().len()
        );
        Box::pin(async move {
            let tools = self.tool_router.list_all();
            tracing::debug!("Returning {} tools", tools.len());
            Ok(rmcp::model::ListToolsResult::with_all_items(tools))
        })
    }

    fn call_tool(
        &self,
        request: rmcp::model::CallToolRequestParam,
        context: rmcp::service::RequestContext<rmcp::RoleServer>,
    ) -> impl Future<Output = Result<rmcp::model::CallToolResult, rmcp::ErrorData>> + Send + '_
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

    tracing::debug!("Creating webfetch client...");
    let client = Arc::new(WebbfetchClient::new(30, 2));
    tracing::debug!("Client created");

    match args.command {
        Command::Stdio => {
            let server = WebfetchServer::new(client);
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
            let server = WebfetchServer::new(client);
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
