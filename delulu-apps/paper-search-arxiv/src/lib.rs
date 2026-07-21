//!  Delulu arXiv Paper Search — Library
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

//! # arXiv Paper Search — Library
//!
//! Provides:
//! - `core` module: `Paper`, `SearchQuery`, `parse_atom_response`
//! - `ArxivClient`: HTTP client wrapping `RateLimitedCrawler` for arXiv API queries

pub mod core;

use anyhow::{Context, Result};
use core::{Paper, SearchQuery};
use delulu_rate_limited_crawler::RateLimitedCrawler;
use std::sync::Arc;

/// HTTP client for the arXiv API.
///
/// Wraps `RateLimitedCrawler` with arXiv-specific URL building and response parsing.
#[derive(Clone)]
pub struct ArxivClient {
    crawler: Arc<RateLimitedCrawler>,
    base_url: String,
}

impl ArxivClient {
    /// Create a new arXiv client with rate limiting (1 QPS per arXiv's policy).
    pub fn new(timeout_secs: u64) -> Result<Self> {
        let crawler = RateLimitedCrawler::builder()
            .with_qps(1)
            .with_timeout(std::time::Duration::from_secs(timeout_secs))
            .with_connect_timeout(std::time::Duration::from_secs(timeout_secs))
            .build()
            .context("Failed to create rate-limited crawler")?;
        Ok(Self {
            crawler: Arc::new(crawler),
            base_url: "https://export.arxiv.org/api/query".to_string(),
        })
    }

    /// Create a client with a custom base URL (for testing).
    pub fn with_base_url(timeout_secs: u64, base_url: String) -> Result<Self> {
        let crawler = RateLimitedCrawler::builder()
            .with_qps(1000) // high QPS for local test server
            .with_timeout(std::time::Duration::from_secs(timeout_secs))
            .with_connect_timeout(std::time::Duration::from_secs(timeout_secs))
            .build()
            .context("Failed to create rate-limited crawler")?;
        Ok(Self {
            crawler: Arc::new(crawler),
            base_url,
        })
    }

    /// Search papers on arXiv by query.
    pub async fn search_papers(&self, query: &SearchQuery) -> Result<Vec<Paper>> {
        let query_string = query.to_query_string();
        let url = format!("{}?{}", self.base_url, query_string);

        tracing::debug!("arXiv API request: {}", url);

        let response = self
            .crawler
            .get(&url)
            .await
            .context("arXiv API request failed")?;

        let status = response.status();
        if !status.is_success() {
            anyhow::bail!(
                "arXiv API returned HTTP {}: {}",
                status,
                response.text().await.unwrap_or_default()
            );
        }

        let body = response.text().await.context("Failed to read response body")?;
        let papers = core::parse_atom_response(&body)
            .map_err(|e| anyhow::anyhow!("Failed to parse arXiv response: {}", e))?;

        Ok(papers)
    }

    /// Fetch specific papers by their arXiv IDs.
    pub async fn get_papers_by_id(&self, ids: &str) -> Result<Vec<Paper>> {
        let url = format!("{}?id_list={}", self.base_url, ids);

        tracing::debug!("arXiv API request: {}", url);

        let response = self
            .crawler
            .get(&url)
            .await
            .context("arXiv API request failed")?;

        let status = response.status();
        if !status.is_success() {
            anyhow::bail!(
                "arXiv API returned HTTP {}: {}",
                status,
                response.text().await.unwrap_or_default()
            );
        }

        let body = response.text().await.context("Failed to read response body")?;
        let papers = core::parse_atom_response(&body)
            .map_err(|e| anyhow::anyhow!("Failed to parse arXiv response: {}", e))?;

        Ok(papers)
    }
}
