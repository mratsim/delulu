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
use delulu_webfetch::WebbfetchClient;
use std::sync::Arc;

const IACR_BASE_URL: &str = "https://eprint.iacr.org";

/// HTTP client for the IACR ePrint Archive.
#[derive(Clone)]
pub struct IacrClient {
    crawler: Arc<RateLimitedCrawler>,
    webfetch_client: Arc<WebbfetchClient>,
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
        let webfetch_client = WebbfetchClient::new(120, 3);
        Ok(Self {
            crawler: Arc::new(crawler),
            webfetch_client: Arc::new(webfetch_client),
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

// ---------------------------------------------------------------------------
// get_paper / get_paper_raw
// ---------------------------------------------------------------------------
impl IacrClient {
    /// Download an IACR ePrint paper by year and number and convert to markdown.
    ///
    /// Downloads the PDF from IACR ePrint Archive and converts to Markdown
    /// via xberg + webfetch. For pre-2005 papers, the number is zero-padded
    /// to 3 digits.
    pub async fn get_paper(&self, year: u32, number: u32) -> Result<String> {
        let url = iacr_pdf_url(year, number);
        let result = delulu_webfetch::fetch_doc(&url, &self.webfetch_client)
            .await
            .context("Failed to fetch IACR paper")?;
        match result {
            delulu_webfetch::ExtractionResult::GenericHtml { content_md } => {
                Ok(content_md.body)
            }
            _ => anyhow::bail!("Unexpected result type from fetch_doc"),
        }
    }

    /// Download an IACR ePrint paper by year and number and return raw PDF bytes.
    ///
    /// For pre-2005 papers, the number is zero-padded to 3 digits.
    pub async fn get_paper_raw(&self, year: u32, number: u32) -> Result<Vec<u8>> {
        let url = iacr_pdf_url(year, number);
        let response = self.crawler.get(&url).send().await
            .context("Failed to fetch IACR paper PDF")?;
        let bytes = response.bytes().await
            .map_err(|e| anyhow::anyhow!("Failed to read PDF bytes: {}", e))?;
        Ok(bytes.to_vec())
    }
}

// Return the IACR ePrint PDF URL for a given year and number.
// Zero-pads the number to 3 digits for pre-2005 papers.
fn iacr_pdf_url(year: u32, number: u32) -> String {
    if year < 2005 {
        format!("{IACR_BASE_URL}/{year}/{number:03}.pdf")
    } else {
        format!("{IACR_BASE_URL}/{year}/{number}.pdf")
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
        // Pre-2005 papers should have 3-digit zero-padding
        let url = iacr_pdf_url(2004, 5);
        assert_eq!(url, "https://eprint.iacr.org/2004/005.pdf");
    }

    #[test]
    fn test_iacr_pdf_url_pre_2005_three_digit() {
        // Pre-2005 with 3-digit number already
        let url = iacr_pdf_url(2004, 123);
        assert_eq!(url, "https://eprint.iacr.org/2004/123.pdf");
    }

    #[test]
    fn test_iacr_pdf_url_year_2005_no_padding() {
        // Year 2005 exactly — no zero-padding
        let url = iacr_pdf_url(2005, 1);
        assert_eq!(url, "https://eprint.iacr.org/2005/1.pdf");
    }

    #[test]
    fn test_iacr_pdf_url_post_2005_no_padding() {
        // Post-2005 — no zero-padding
        let url = iacr_pdf_url(2024, 123);
        assert_eq!(url, "https://eprint.iacr.org/2024/123.pdf");
    }

    #[test]
    fn test_iacr_pdf_url_single_digit_pre_2005() {
        // Single digit with zero-padding
        let url = iacr_pdf_url(1999, 7);
        assert_eq!(url, "https://eprint.iacr.org/1999/007.pdf");
    }

    #[test]
    fn test_iacr_pdf_url_boundary_2004() {
        // Boundary: 2004 (last year with padding)
        let url = iacr_pdf_url(2004, 999);
        assert_eq!(url, "https://eprint.iacr.org/2004/999.pdf");
    }

    #[test]
    fn test_get_paper_url_construction() {
        // Test URL construction logic used in get_paper / get_paper_raw
        let year = 2024;
        let number = 123;
        let url = iacr_pdf_url(year, number);
        assert_eq!(url, "https://eprint.iacr.org/2024/123.pdf");
    }

    #[test]
    fn test_get_paper_raw_url_construction() {
        // Test URL construction logic used in get_paper_raw
        let year = 2003;
        let number = 42;
        let url = iacr_pdf_url(year, number);
        assert_eq!(url, "https://eprint.iacr.org/2003/042.pdf");
    }
}
