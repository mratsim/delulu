//!  Delulu IACR Paper Search — Library
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

//! # IACR Paper Search — Library
//!
//! Provides:
//! - `core` module: `Paper`, `parse_rss_response`, `parse_paper_html`
//! - `IacrClient`: HTTP client wrapping `RateLimitedCrawler` for IACR ePrint Archive

pub mod core;

use anyhow::{Context, Result};
use core::Paper;
use delulu_rate_limited_crawler::RateLimitedCrawler;
use std::sync::Arc;

const IACR_BASE_URL: &str = "https://eprint.iacr.org";

/// HTTP client for the IACR ePrint Archive.
#[derive(Clone)]
pub struct IacrClient {
    crawler: Arc<RateLimitedCrawler>,
    base_url: String,
}

impl IacrClient {
    pub fn new(timeout_secs: u64) -> Result<Self> {
        Self::with_base_url(timeout_secs, IACR_BASE_URL.to_string())
    }

    pub fn with_base_url(timeout_secs: u64, base_url: String) -> Result<Self> {
        let crawler = RateLimitedCrawler::builder()
            .with_qps(3)
            .with_timeout(std::time::Duration::from_secs(timeout_secs))
            .with_connect_timeout(std::time::Duration::from_secs(timeout_secs))
            .build()
            .context("Failed to create rate-limited crawler")?;
        Ok(Self {
            crawler: Arc::new(crawler),
            base_url,
        })
    }

    pub async fn list_recent_papers(&self) -> Result<Vec<Paper>> {
        let url = format!("{}/rss/rss.xml", self.base_url);
        tracing::debug!("IACR RSS request: {}", url);

        let response = self.crawler.get(&url).await.context("IACR RSS request failed")?;

        let status = response.status();
        if !status.is_success() {
            anyhow::bail!("IACR RSS returned HTTP {}: {}", status, response.text().await.unwrap_or_default());
        }

        let body = response.text().await.context("Failed to read RSS response")?;
        core::parse_rss_response(&body).map_err(|e| anyhow::anyhow!("Failed to parse RSS: {}", e))
    }

    pub async fn get_paper_details(&self, year: u32, number: u32) -> Result<Paper> {
        let url = format!("{}/{}/{}", self.base_url, year, number);
        tracing::debug!("IACR paper request: {}", url);

        let response = self.crawler.get(&url).await.context("IACR paper request failed")?;

        let status = response.status();
        if !status.is_success() {
            anyhow::bail!("IACR paper returned HTTP {}: {}", status, response.text().await.unwrap_or_default());
        }

        let body = response.text().await.context("Failed to read paper response")?;
        core::parse_paper_html(&body)
            .map_err(|e| anyhow::anyhow!("Failed to parse paper HTML: {}", e))
    }

    pub fn download_paper_pdf(&self, year: u32, number: u32) -> String {
        format!("{}/{}/{}.pdf", self.base_url, year, number)
    }
}
