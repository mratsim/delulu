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
//! - `lib_mcp` module (feature `mcp`): the `IacrMcpServer` MCP server

pub mod core;

#[cfg(feature = "mcp")]
pub mod lib_mcp;

#[cfg(feature = "mcp")]
pub use lib_mcp::IacrMcpServer;

use anyhow::{Context, Result};
use core::Paper;
use delulu_rate_limited_crawler::RateLimitedCrawler;
use futures_util::StreamExt;
use std::sync::Arc;

/// Maximum document size to download (50 MiB).
const MAX_DOC_SIZE: usize = 50 * 1024 * 1024;

/// HTTP client for the IACR ePrint Archive.
#[derive(Clone)]
pub struct IacrClient {
    crawler: Arc<RateLimitedCrawler>,
    base_url: String,
}

impl IacrClient {
    /// Construct an `IacrClient` sharing a caller-provided rate-limited crawler.
    ///
    /// Pre: `crawler` is a fully configured `RateLimitedCrawler` (rates/timeouts set by the caller).
    /// Post: the returned client shares the given `crawler` and uses the default IACR base URL.
    /// Panic-if: never (infallible constructor).
    pub fn new_with_crawler(crawler: Arc<RateLimitedCrawler>) -> Self {
        Self {
            crawler,
            base_url: "https://eprint.iacr.org".to_string(),
        }
    }

    /// Create a new IACR client with rate limiting (3 QPS per IACR's policy).
    ///
    /// The crawler is built here with the default rate config; callers needing
    /// custom rates/timeouts or a shared crawler should use [`IacrClient::new_with_crawler`].
    pub fn new() -> Result<Self> {
        let crawler = RateLimitedCrawler::builder()
            .with_qps(3)
            .with_timeout(std::time::Duration::from_secs(30))
            .with_connect_timeout(std::time::Duration::from_secs(30))
            .build()
            .context("Failed to create rate-limited crawler")?;
        Ok(Self::new_with_crawler(Arc::new(crawler)))
    }

    pub fn with_base_url(mut self, url: String) -> Self {
        self.base_url = url;
        self
    }

    pub async fn list_recent_papers(&self) -> Result<Vec<Paper>> {
        let url = format!("{}/rss/rss.xml", self.base_url);
        tracing::debug!("IACR RSS request: {}", url);

        let response = self
            .crawler
            .get(&url)
            .await
            .context("IACR RSS request failed")?;

        let status = response.status();
        if !status.is_success() {
            let body_preview = match response.text().await {
                Ok(body) => body,
                Err(e) => format!("(failed to read error body: {e})"),
            };
            anyhow::bail!("IACR RSS returned HTTP {}: {}", status, body_preview);
        }

        let body = response
            .text()
            .await
            .context("Failed to read RSS response")?;
        core::parse_rss_response(&body).map_err(|e| anyhow::anyhow!("Failed to parse RSS: {}", e))
    }

    pub async fn get_paper_details(&self, year: u32, number: u32) -> Result<Paper> {
        let url = format!("{}/{}/{}", self.base_url, year, number);
        tracing::debug!("IACR paper request: {}", url);

        let response = self
            .crawler
            .get(&url)
            .await
            .context("IACR paper request failed")?;

        let status = response.status();
        if !status.is_success() {
            let body_preview = match response.text().await {
                Ok(body) => body,
                Err(e) => format!("(failed to read error body: {e})"),
            };
            anyhow::bail!("IACR paper returned HTTP {}: {}", status, body_preview);
        }

        let body = response
            .text()
            .await
            .context("Failed to read paper response")?;
        core::parse_paper_html(&body)
            .map_err(|e| anyhow::anyhow!("Failed to parse paper HTML: {}", e))
    }

    pub fn paper_pdf_url(&self, year: u32, number: u32) -> String {
        format!("{}/{}/{}.pdf", self.base_url, year, number)
    }
}

// ---------------------------------------------------------------------------
// get_paper / get_paper_raw
// ---------------------------------------------------------------------------
impl IacrClient {
    /// Download an IACR ePrint paper by year and number and convert to markdown.
    ///
    /// Downloads the PDF from IACR ePrint Archive via the rate-limited crawler
    /// and converts to Markdown via xberg + webfetch. For pre-2005 papers, the
    /// number is zero-padded to 3 digits.
    pub async fn get_paper(&self, year: u32, number: u32) -> Result<String> {
        let url = iacr_pdf_url(&self.base_url, year, number);
        let response = self
            .crawler
            .get(&url)
            .send()
            .await
            .context("Failed to fetch IACR paper PDF")?;

        let status = response.status();
        if !status.is_success() {
            let body_preview = match response.text().await {
                Ok(body) => body,
                Err(e) => format!("(failed to read error body: {e})"),
            };
            anyhow::bail!("IACR paper PDF returned HTTP {}: {}", status, body_preview);
        }

        let bytes = response
            .bytes()
            .await
            .map_err(|e| anyhow::anyhow!("Failed to read PDF bytes: {}", e))?;

        let html = delulu_webfetch::doc_to_html(bytes.to_vec(), &url)
            .await
            .context("Failed to process IACR paper PDF")?;
        let md = delulu_webfetch::doc_html_to_markdown(&html, None)
            .context("Failed to convert HTML to markdown")?;
        Ok(md)
    }

    /// Download an IACR ePrint paper by year and number and return raw PDF bytes.
    ///
    /// For pre-2005 papers, the number is zero-padded to 3 digits.
    pub async fn get_paper_raw(&self, year: u32, number: u32) -> Result<Vec<u8>> {
        let url = iacr_pdf_url(&self.base_url, year, number);
        let response = self
            .crawler
            .get(&url)
            .send()
            .await
            .context("Failed to fetch IACR paper PDF")?;

        let status = response.status();
        if !status.is_success() {
            let body_preview = match response.text().await {
                Ok(body) => body,
                Err(e) => format!("(failed to read error body: {e})"),
            };
            anyhow::bail!("IACR paper PDF returned HTTP {}: {}", status, body_preview);
        }
        let mut stream = response.bytes_stream();
        let mut bytes = Vec::new();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.context("Failed to read PDF chunk")?;
            bytes.extend_from_slice(&chunk);
            if bytes.len() > MAX_DOC_SIZE {
                anyhow::bail!(
                    "IACR paper PDF exceeds maximum size of {} bytes",
                    MAX_DOC_SIZE
                );
            }
        }
        Ok(bytes)
    }
}
// Return the IACR ePrint PDF URL for a given year and number.
// Zero-pads the number to 3 digits for pre-2005 papers.
fn iacr_pdf_url(base_url: &str, year: u32, number: u32) -> String {
    if year < 2005 {
        format!("{base_url}/{year}/{number:03}.pdf")
    } else {
        format!("{base_url}/{year}/{number}.pdf")
    }
}
// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_iacr_pdf_url_pre_2005_zero_padding() {
        let url = iacr_pdf_url("https://eprint.iacr.org", 2004, 5);
        assert_eq!(url, "https://eprint.iacr.org/2004/005.pdf");
    }

    #[test]
    fn test_iacr_pdf_url_pre_2005_three_digit() {
        let url = iacr_pdf_url("https://eprint.iacr.org", 2004, 123);
        assert_eq!(url, "https://eprint.iacr.org/2004/123.pdf");
    }

    #[test]
    fn test_iacr_pdf_url_year_2005_no_padding() {
        let url = iacr_pdf_url("https://eprint.iacr.org", 2005, 1);
        assert_eq!(url, "https://eprint.iacr.org/2005/1.pdf");
    }

    #[test]
    fn test_iacr_pdf_url_post_2005_no_padding() {
        let url = iacr_pdf_url("https://eprint.iacr.org", 2024, 123);
        assert_eq!(url, "https://eprint.iacr.org/2024/123.pdf");
    }

    #[test]
    fn test_iacr_pdf_url_single_digit_pre_2005() {
        let url = iacr_pdf_url("https://eprint.iacr.org", 1999, 7);
        assert_eq!(url, "https://eprint.iacr.org/1999/007.pdf");
    }

    #[test]
    fn test_iacr_pdf_url_boundary_2004() {
        let url = iacr_pdf_url("https://eprint.iacr.org", 2004, 999);
        assert_eq!(url, "https://eprint.iacr.org/2004/999.pdf");
    }

    #[test]
    fn test_get_paper_url_construction() {
        let year = 2024;
        let number = 123;
        let url = iacr_pdf_url("https://eprint.iacr.org", year, number);
        assert_eq!(url, "https://eprint.iacr.org/2024/123.pdf");
    }

    #[test]
    fn test_get_paper_raw_url_construction() {
        let year = 2003;
        let number = 42;
        let url = iacr_pdf_url("https://eprint.iacr.org", year, number);
        assert_eq!(url, "https://eprint.iacr.org/2003/042.pdf");
    }
}

#[cfg(test)]
#[path = "../tests/unit/iacr_client_test.rs"]
mod client_tests;
