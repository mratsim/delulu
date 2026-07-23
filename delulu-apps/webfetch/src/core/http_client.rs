//! HTTP client trait for the delulu-webfetch agent.
//!
//! NOTE: The `WebfetchClient` struct has been removed. All callers should use
//! `RateLimitedCrawler` from `delulu-rate-limited-crawler` for rate-limited HTTP
//! fetching. The `HttpClient` trait remains for test mocking.

use async_trait::async_trait;

use super::types::*;

/// A minimal HTTP client trait, abstracted for test mocking.
#[async_trait]
pub trait HttpClient: Send + Sync {
    async fn get(&self, url: &str) -> Result<Response, WebfetchError>;
    async fn get_bytes(&self, url: &str) -> Result<Vec<u8>, WebfetchError>;
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------
#[cfg(test)]
#[path = "http_client_test.rs"]
mod tests;
