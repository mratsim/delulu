mod openapi;
mod proxy;
#[cfg(feature = "mcp")]
mod server;

/// Re-exported for integration tests. Parses an OpenAPI 3.x spec from JSON.
pub use openapi::OpenApiSpec;

/// Re-exported for integration tests. Builds an MCP server from an OpenAPI spec.
#[cfg(feature = "mcp")]
pub use server::McpifyServer;
