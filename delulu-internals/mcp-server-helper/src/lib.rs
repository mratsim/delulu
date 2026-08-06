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
use rmcp::service::{QuitReason, serve_server};
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

/// Wait for a shutdown signal: SIGINT (Ctrl+C) on all platforms, plus SIGTERM
/// on unix (what Docker `docker stop` / Kubernetes send on container stop).
///
/// Returns the name of the signal that fired so callers can log it.
#[cfg(unix)]
async fn wait_for_shutdown_signal() -> &'static str {
    let mut sigterm = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        .expect("failed to install SIGTERM handler");
    tokio::select! {
        _ = tokio::signal::ctrl_c() => "SIGINT",
        _ = sigterm.recv() => "SIGTERM",
    }
}

/// Wait for a shutdown signal: SIGINT (Ctrl+C). SIGTERM handling is unix-only
/// and not available on this platform.
#[cfg(not(unix))]
async fn wait_for_shutdown_signal() -> &'static str {
    tokio::signal::ctrl_c().await.ok();
    "SIGINT"
}

/// Run an MCP server over stdio transport.
///
/// The server runs until the client closes stdin (EOF), or a shutdown signal
/// (SIGINT / Ctrl+C, or SIGTERM on unix) is received, then shuts down and
/// exits the process: code 0 on a clean shutdown (client EOF, or signal
/// shutdown), non-zero if the serve-loop task or an outbound send task
/// terminated abnormally (rmcp reports these as `QuitReason::JoinError` or
/// `Err(JoinError)` from `waiting()`). Note: panics inside individual tool
/// handlers run in detached tokio tasks and never surface here (rmcp
/// 0.13.0) — they cannot change the exit code.
///
/// This function does not return on the post-handshake paths: it terminates
/// the process explicitly to work around tokio's blocking-pool stdin read
/// hanging runtime teardown (see the comment in the function body). The
/// `Result<(), Error>` return is retained for the pre-handshake path, where
/// `serve_server` errors are still propagated to the caller.
pub async fn run_stdio<T>(server: T) -> Result<(), Error>
where
    T: ServerHandler + 'static,
{
    let (stdin, stdout) = rmcp::transport::io::stdio();
    tracing::info!("Starting MCP server over stdio...");
    let running = serve_server(Arc::new(server), (stdin, stdout))
        .await
        .map_err(|e| anyhow::anyhow!("Server error: {}", e))?;
    tracing::debug!("Server running. Press Ctrl+C to stop.");

    // Hoist the serve-task wait out of the select so the signal branch can
    // cancel the serve task explicitly and await its termination (drain).
    // `running.waiting()` consumes `running`, so grab the cancellation token
    // first.
    let cancellation_token = running.cancellation_token();
    let waiting = running.waiting();
    tokio::pin!(waiting);
    let exit_code = tokio::select! {
        quit = &mut waiting => {
            // The serve loop ended on its own (transport closure / error).
            if let Ok(QuitReason::JoinError(e)) | Err(e) = &quit {
                // A crashed serve loop or outbound send task is NOT a clean
                // disconnect: exit non-zero so supervisors (systemd
                // Restart=on-failure, container healthchecks) can react.
                // (Panics inside individual tool handlers run in detached
                // tokio tasks and never surface here — rmcp 0.13.0.)
                tracing::error!("Serve task terminated abnormally: {e}");
            } else {
                tracing::info!("Client disconnected (stdin closed). Shutting down...");
            }
            exit_code_for(&quit)
        }
        signal = wait_for_shutdown_signal() => {
            tracing::info!("Received {signal}. Shutting down...");
            // Drain: cancel the serve task and await its termination so the
            // serve loop's own cleanup (including `transport.close()`) runs
            // before the process exits. Bounded: the serve loop observes the
            // cancellation token at its next select poll and breaks
            // immediately; the stdio `transport.close()` only drops the
            // stdout writer.
            cancellation_token.cancel();
            let drain = (&mut waiting).await;
            if let Ok(QuitReason::JoinError(e)) | Err(e) = &drain {
                tracing::error!("Serve task terminated abnormally during shutdown: {e}");
            } else {
                tracing::info!("Serve task stopped. Shutting down...");
            }
            exit_code_for(&drain)
        }
    };

    // Flush stderr (tracing writes there) and exit the process deterministically.
    //
    // rmcp's stdio transport reads stdin via `tokio::io::stdin()`, which
    // performs the fd read on the runtime's BLOCKING POOL. When this function
    // returns, `#[tokio::main]` drops the runtime, which JOINS the blocking
    // pool and therefore waits forever on the blocked `read()` of the
    // still-open stdin pipe — the process hangs after a graceful shutdown.
    // This is a pre-existing tokio limitation: returning from this function
    // would hang runtime teardown, so the process must exit explicitly. (The
    // hang was latent before this change — signal paths were never exercised
    // by tests, which used kill_on_drop = SIGKILL.)
    //
    // All work this function is responsible for is done at this point — on
    // the EOF path `waiting()` awaited the serve task to termination; on the
    // signal path the serve task was cancelled explicitly and its termination
    // was awaited in the select above. The only remaining work is runtime
    // teardown, which hangs on the blocking stdin read. Bypass it with an
    // explicit process exit. `std::process::exit` does not run Rust
    // destructors, so flush stderr explicitly first.
    use std::io::Write;
    let _ = std::io::stderr().flush();
    std::process::exit(exit_code);
}

/// Run an MCP server over HTTP (Streamable HTTP) transport with graceful shutdown.
///
/// The server binds to `host:port` and serves on `/mcp`. Shuts down gracefully
/// on SIGINT (Ctrl+C) on all platforms, and on SIGTERM on unix (what Docker
/// `docker stop` / Kubernetes send on container stop).
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
        let _ = wait_for_shutdown_signal().await;
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

/// Map a serve-loop termination reason to the process exit code: clean
/// disconnects (`Closed`) and cancellations (`Cancelled`) exit 0; abnormal
/// terminations exit 1. The abnormal cases are `Err(JoinError)` (the serve
/// loop task itself panicked — surfaced by `waiting()`) and
/// `Ok(QuitReason::JoinError(_))` (an outbound transport-send task panicked
/// inside the serve loop, which rmcp reports as a quit reason).
fn exit_code_for(reason: &Result<QuitReason, tokio::task::JoinError>) -> i32 {
    match reason {
        Ok(QuitReason::Closed) | Ok(QuitReason::Cancelled) => 0,
        Ok(QuitReason::JoinError(_)) | Err(_) => 1,
    }
}

#[cfg(test)]
mod tests {
    use super::exit_code_for;
    use rmcp::service::QuitReason;

    #[test]
    fn exit_code_is_0_for_clean_disconnects() {
        assert_eq!(exit_code_for(&Ok(QuitReason::Closed)), 0);
        assert_eq!(exit_code_for(&Ok(QuitReason::Cancelled)), 0);
    }

    #[tokio::test]
    async fn exit_code_is_1_for_abnormal_terminations() {
        // A panicked spawned task yields a real JoinError (the only public
        // way to obtain one).
        let err = tokio::spawn(async { panic!("boom") }).await.unwrap_err();
        assert_eq!(exit_code_for(&Err(err)), 1);

        let err = tokio::spawn(async { panic!("boom") }).await.unwrap_err();
        assert_eq!(exit_code_for(&Ok(QuitReason::JoinError(err))), 1);
    }
}
