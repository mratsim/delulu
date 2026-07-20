mod openapi;
mod proxy;
mod server;

use openapi::OpenApiSpec;
use server::McpifyServer;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use rmcp::service::serve_server;
use rmcp::transport::streamable_http_server::{
    StreamableHttpServerConfig, StreamableHttpService, session::local::LocalSessionManager,
};
use std::net::SocketAddr;
use std::sync::Arc;
use tracing_subscriber::{EnvFilter, layer::SubscriberExt, util::SubscriberInitExt};

#[derive(Parser, Debug)]
#[command(name = "mcpify")]
#[command(
    author,
    version,
    about = "Turn OpenAPI spec into MCP server"
)]
struct Args {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Run MCP server over stdio (for Claude Desktop, etc.)
    Stdio {
        path: String,
    },

    /// Run MCP server over HTTP
    Http {
        path: String,

        #[arg(long, default_value = "0.0.0.0")]
        host: String,

        #[arg(long, default_value = "8080")]
        port: u16,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::registry()
        .with(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".to_string().into()))
        .with(
            tracing_subscriber::fmt::layer()
                .with_timer(tracing_subscriber::fmt::time::ChronoUtc::rfc_3339())
                .with_writer(std::io::stderr),
        )
        .init();

    let args = Args::parse();

    let spec = match &args.command {
        Command::Stdio { path } | Command::Http { path, .. } => {
            OpenApiSpec::from_file(path).context("Failed to load OpenAPI spec")?
        }
    };

    tracing::info!("Loaded OpenAPI spec: {}", spec.info.title);

    let server = McpifyServer::from_openapi(&spec).context("Failed to build server")?;

    match args.command {
        Command::Stdio { .. } => {
            tracing::info!("Starting MCP server over stdio...");
            let (stdin, stdout) = rmcp::transport::io::stdio();
            let _running = serve_server(Arc::new(server), (stdin, stdout))
                .await
                .map_err(|e| anyhow::anyhow!("Server error: {}", e))?;
            tokio::signal::ctrl_c().await.ok();
            tracing::info!("Shutting down...");
        }
        Command::Http { host, port, .. } => {
            let addr: SocketAddr = format!("{}:{}", host, port)
                .parse()
                .context("Invalid host:port")?;
            tracing::info!("Starting MCP server over HTTP on {}", addr);
            let session_manager = Arc::new(LocalSessionManager::default());
            let config = StreamableHttpServerConfig {
                stateful_mode: false,
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
