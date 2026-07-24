//!  Delulu MCP Server Helper
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

//! Shared MCP server infrastructure for delulu MCP servers.
//!
//! Eliminates copy-paste MCP server boilerplate across 5 crates by providing:
//!
//! - [`McpServerConfig`] — common CLI subcommand (`Stdio` / `Http { host, port }`)
//! - [`setup_tracing`] — common tracing-subscriber initialization
//! - [`run_stdio`] — runs an MCP server over stdio
//! - [`run_http`] — runs an MCP server over HTTP with graceful shutdown
//! - [`impl_server_handler`] — macro that generates the `ServerHandler` trait impl
//! - Re-exports of `rmcp`, `clap`, `axum`, `tracing_subscriber` so callers
//!   don't need to import them directly.

use axum::extract::connect_info::Connected;
use axum::serve::IncomingStream;
use std::net::SocketAddr;
use std::sync::Arc;

use anyhow::{Context, Error, Result};
use axum::extract::ConnectInfo;
use clap::Subcommand;
use rmcp::handler::server::ServerHandler;
use rmcp::handler::server::common::FromContextPart;
use rmcp::handler::server::tool::ToolCallContext;
use rmcp::service::serve_server;
use rmcp::transport::streamable_http_server::{
    StreamableHttpServerConfig, StreamableHttpService, session::local::LocalSessionManager,
};
use tracing_subscriber::{EnvFilter, layer::SubscriberExt, util::SubscriberInitExt};

// ---------------------------------------------------------------------------
// Re-exports
// ---------------------------------------------------------------------------

pub use clap;
pub use rmcp;

// ---------------------------------------------------------------------------
// Common CLI config
// ---------------------------------------------------------------------------

/// Shared MCP server subcommand configuration.
///
/// Each app embeds this via `#[command(subcommand)]` in its own `Args` struct:
///
/// ```ignore
/// #[derive(Parser)]
/// struct Args {
///     // app-specific args ...
///     #[command(subcommand)]
///     command: McpServerConfig,
/// }
/// ```
#[derive(Subcommand, Debug)]
pub enum McpServerConfig {
    /// Run MCP server over stdio (for Claude Desktop, etc.)
    Stdio,
    /// Run MCP server over HTTP
    Http {
        #[arg(long, default_value = "127.0.0.1")]
        host: String,
        /// Port to bind. Default 8080 (webfetch historically used 8081).
        #[arg(long, default_value = "8080")]
        port: u16,
    },
}

// ---------------------------------------------------------------------------
// Tracing setup
// ---------------------------------------------------------------------------

/// Initialize tracing-subscriber with env-filter, chrono timestamps, and stderr output.
///
/// This is the same initialization used by all existing delulu MCP servers.
pub fn setup_tracing() {
    tracing_subscriber::registry()
        .with(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".to_string().into()))
        .with(
            tracing_subscriber::fmt::layer()
                .with_timer(tracing_subscriber::fmt::time::ChronoUtc::rfc_3339())
                .with_writer(std::io::stderr),
        )
        .init();
}

// ---------------------------------------------------------------------------
// Server runners
// ---------------------------------------------------------------------------

/// Run an MCP server over stdio transport.
///
/// The server runs until Ctrl+C is received, then performs a graceful shutdown.
pub async fn run_stdio<T>(server: T) -> Result<(), Error>
where
    T: ServerHandler + 'static,
{
    let (stdin, stdout) = rmcp::transport::io::stdio();
    tracing::info!("Starting MCP server over stdio...");
    let _running = serve_server(Arc::new(server), (stdin, stdout))
        .await
        .map_err(|e| anyhow::anyhow!("Server error: {}", e))?;
    tracing::debug!("Server running. Press Ctrl+C to stop.");
    tokio::signal::ctrl_c().await.ok();
    tracing::info!("Shutting down...");
    Ok(())
}

/// Run an MCP server over HTTP (Streamable HTTP) transport with graceful shutdown.
///
/// The server binds to `host:port` and serves on `/mcp`.
pub async fn run_http<T>(server: T, host: String, port: u16) -> Result<(), Error>
where
    T: ServerHandler + Clone + 'static,
{
    let addr: SocketAddr = format!("{}:{}", host, port)
        .parse()
        .context("Invalid host:port")?;
    tracing::info!("Starting MCP server over HTTP on {}", addr);
    let session_manager = Arc::new(LocalSessionManager::default());
    let config = StreamableHttpServerConfig {
        stateful_mode: true,
        ..Default::default()
    };
    let service = StreamableHttpService::new(move || Ok(server.clone()), session_manager, config);
    let app = axum::Router::new().nest_service("/mcp", service);
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .context("Failed to bind to address")?;
    tracing::debug!("Listening on {}", addr);
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<PeerInfo>(),
    )
    .with_graceful_shutdown(async move {
        tokio::signal::ctrl_c().await.ok();
        tracing::info!("Shutting down HTTP server...");
    })
    .await
    .context("HTTP server error")?;
    Ok(())
}

// ---------------------------------------------------------------------------
// ServerHandler impl macro
// ---------------------------------------------------------------------------

/// Generate a `ServerHandler` implementation for a type that has a `tool_router: ToolRouter<Self>` field.
///
/// The type must also implement `Clone`.
///
/// # Example
///
/// ```ignore
/// use delulu_mcp_server_helper::impl_server_handler;
///
/// #[derive(Clone)]
/// struct MyServer {
///     tool_router: ToolRouter<Self>,
/// }
///
/// impl_server_handler!(MyServer);
/// ```
///
/// This generates the `list_tools`, `call_tool`, and `get_info` methods
/// identical to what every delulu MCP server was hand-writing.
#[macro_export]
macro_rules! impl_server_handler {
    ($ty:ty) => {
        impl $crate::rmcp::handler::server::ServerHandler for $ty {
            fn list_tools(
                &self,
                _request: Option<$crate::rmcp::model::PaginatedRequestParam>,
                _context: $crate::rmcp::service::RequestContext<$crate::rmcp::RoleServer>,
            ) -> impl std::future::Future<
                Output = std::result::Result<
                    $crate::rmcp::model::ListToolsResult,
                    $crate::rmcp::ErrorData,
                >,
            > + Send
            + '_ {
                tracing::debug!(
                    "list_tools called, tools count: {}",
                    self.tool_router.list_all().len()
                );
                let tools = self.tool_router.list_all();
                Box::pin(async move {
                    tracing::debug!("Returning {} tools", tools.len());
                    Ok($crate::rmcp::model::ListToolsResult::with_all_items(tools))
                })
            }

            fn call_tool(
                &self,
                request: $crate::rmcp::model::CallToolRequestParam,
                context: $crate::rmcp::service::RequestContext<$crate::rmcp::RoleServer>,
            ) -> impl std::future::Future<
                Output = std::result::Result<
                    $crate::rmcp::model::CallToolResult,
                    $crate::rmcp::ErrorData,
                >,
            > + Send
            + '_ {
                let router = self.tool_router.clone();
                let self_clone = self.clone();
                Box::pin(async move {
                    let context = $crate::rmcp::handler::server::tool::ToolCallContext::new(
                        &self_clone,
                        request,
                        context,
                    );
                    router.call(context).await
                })
            }

            fn get_info(&self) -> $crate::rmcp::model::ServerInfo {
                $crate::rmcp::model::ServerInfo {
                    protocol_version: $crate::rmcp::model::ProtocolVersion::V_2025_03_26,
                    capabilities: $crate::rmcp::model::ServerCapabilities {
                        tools: Some($crate::rmcp::model::ToolsCapability::default()),
                        ..Default::default()
                    },
                    server_info: $crate::rmcp::model::Implementation::from_build_env(),
                    instructions: None,
                }
            }
        }
    };
}

// ---------------------------------------------------------------------------
// PeerAddr extractor for tool handlers
// ---------------------------------------------------------------------------

/// Connection info for an MCP peer: both the client's address and the
/// server's actual local address (the IP the client connected to).
///
/// For stdio transport, PeerAddr is None — no network addresses available.
#[derive(Debug, Clone, Copy)]
pub struct PeerInfo {
    pub remote_addr: SocketAddr,
    pub local_addr: SocketAddr,
}

impl Connected<IncomingStream<'_>> for PeerInfo {
    fn connect_info(target: IncomingStream<'_>) -> Self {
        Self {
            remote_addr: target.remote_addr(),
            local_addr: target
                .local_addr()
                .unwrap_or_else(|_| SocketAddr::from(([0, 0, 0, 0], 0))),
        }
    }
}

/// Extracts the remote peer's connection info from the request context.
///
/// Returns `None` for stdio transport (no network addresses available).
/// Returns `Some(PeerInfo)` for HTTP transport.
#[derive(Debug, Clone, Copy)]
pub struct PeerAddr(pub Option<PeerInfo>);

impl<S> FromContextPart<ToolCallContext<'_, S>> for PeerAddr {
    fn from_context_part(context: &mut ToolCallContext<S>) -> Result<Self, rmcp::ErrorData> {
        let info = context
            .request_context
            .extensions
            .get::<axum::http::request::Parts>()
            .and_then(|parts| {
                parts
                    .extensions
                    .get::<ConnectInfo<PeerInfo>>()
                    .map(|ci| ci.0)
            });
        Ok(PeerAddr(info))
    }
}
