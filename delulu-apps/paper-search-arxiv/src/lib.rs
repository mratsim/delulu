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
    api_url: String,
}

impl ArxivClient {
    fn new_with_crawler(crawler: RateLimitedCrawler) -> Self {
        Self {
            crawler: Arc::new(crawler),
            base_url: "https://arxiv.org".to_string(),
            api_url: "https://export.arxiv.org/api/query".to_string(),
        }
    }

    /// Create a new arXiv client with rate limiting (1 QPS per arXiv's policy).
    pub fn new() -> Result<Self> {
        // TODO side-effect to push to main: crawler built in new() (no injection seam)
        let crawler = RateLimitedCrawler::builder()
            .with_qps(1)
            .with_timeout(std::time::Duration::from_secs(30))
            .with_connect_timeout(std::time::Duration::from_secs(30))
            .build()
            .context("Failed to create rate-limited crawler")?;
        Ok(Self::new_with_crawler(crawler))
    }

    /// Override the HTML base URL (default: https://arxiv.org).
    pub fn with_base_url(mut self, url: String) -> Self {
        self.base_url = url;
        self
    }

    /// Override the API URL (default: https://export.arxiv.org/api/query).
    pub fn with_api_url(mut self, url: String) -> Self {
        self.api_url = url;
        self
    }

    /// Fetch an arXiv API URL and parse the Atom response.
    async fn fetch_atom(&self, url: &str) -> Result<Vec<Paper>> {
        tracing::debug!("arXiv API request: {}", url);
        let response = self
            .crawler
            .get(url)
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
        let body = response
            .text()
            .await
            .context("Failed to read response body")?;
        core::parse_atom_response(&body)
            .map_err(|e| anyhow::anyhow!("Failed to parse arXiv response: {}", e))
    }

    /// Search papers on arXiv by query.
    pub async fn search_papers(&self, query: &SearchQuery) -> Result<Vec<Paper>> {
        let query_string = query.to_query_string();
        let url = format!("{}?{}", self.api_url, query_string);
        self.fetch_atom(&url).await
    }

    /// Fetch specific papers by their arXiv IDs.
    pub async fn get_papers_by_id(&self, ids: &str) -> Result<Vec<Paper>> {
        let url = format!("{}?id_list={}", self.api_url, ids);
        self.fetch_atom(&url).await
    }

    /// Fetch the full paper as markdown from arXiv HTML5.
    ///
    /// Downloads the arXiv HTML5 version of the paper and runs the
    /// `dl_arxiv` cleaning pipeline to produce clean markdown with
    /// LaTeX math preserved.
    ///
    /// # Arguments
    ///
    /// * `arxiv_id` — arXiv ID, e.g. "1706.03762" or "cond-mat/0011267"
    ///
    /// # Returns
    ///
    /// Markdown string with LaTeX math, complex tables as raw HTML.
    pub async fn get_paper(&self, arxiv_id: &str) -> Result<String> {
        let url = format!("{}/html/{}", self.base_url, arxiv_id);
        tracing::debug!("arXiv HTML5 request: {}", url);

        let response = self
            .crawler
            .get(&url)
            .await
            .context("arXiv HTML5 fetch failed")?;

        let status = response.status();
        if !status.is_success() {
            anyhow::bail!(
                "arXiv HTML5 returned HTTP {} for paper '{}'",
                status,
                arxiv_id,
            );
        }

        let body = response
            .text()
            .await
            .context("Failed to read arXiv HTML5 body")?;

        let mut dom =
            delulu_webfetch::pipelines::parse_html(&body).context("Failed to parse arXiv HTML")?;
        delulu_webfetch::pipelines::dl_arxiv::filter_arxiv(&mut dom);
        let md = delulu_webfetch::generators::gen_md::MarkdownLowerer::lower(&dom, None);

        Ok(md)
    }
}

#[cfg(test)]
#[path = "../tests/unit/arxiv_client_test.rs"]
mod client_tests;
