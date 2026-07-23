//!  Delulu PubMed Paper Search — Library
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

//! # PubMed Paper Search — Library
//!
//! Provides:
//! - `core` module: Paper, SearchQuery, and all response parsers
//! - `PubmedClient`: HTTP client wrapping `RateLimitedCrawler` for NCBI E-utilities

pub mod core;

use anyhow::{Context, Result};
use core::{Paper, SearchQuery, SearchResult};
use delulu_rate_limited_crawler::RateLimitedCrawler;
use urlencoding;
use std::sync::Arc;

const API_URL: &str = "https://eutils.ncbi.nlm.nih.gov/entrez/eutils";
const BASE_URL: &str = "https://www.ncbi.nlm.nih.gov/pmc";

/// HTTP client for the NCBI PubMed E-utilities API and PubMed Central PDF downloads.
#[derive(Clone)]
pub struct PubmedClient {
    crawler: Arc<RateLimitedCrawler>,
    api_url: String,      // E-utilities API endpoint (search, summaries, etc.)
    base_url: String,     // PubMed Central URL (PDF downloads)
}

impl PubmedClient {
    pub fn new(timeout_secs: u64) -> Result<Self> {
        let crawler = Self::build_crawler(timeout_secs)?;
        Ok(Self {
            crawler: Arc::new(crawler),
            api_url: API_URL.to_string(),
            base_url: BASE_URL.to_string(),
        })
    }

    /// Set a custom E-utilities API URL (for tests or proxies).
    pub fn with_api_url(mut self, api_url: String) -> Self {
        self.api_url = api_url;
        self
    }

    /// Set a custom PubMed Central base URL (for tests or proxies).
    pub fn with_base_url(mut self, base_url: String) -> Self {
        self.base_url = base_url;
        self
    }

    fn build_crawler(timeout_secs: u64) -> Result<RateLimitedCrawler> {
        RateLimitedCrawler::builder()
            .with_qps(3)
            .with_timeout(std::time::Duration::from_secs(timeout_secs))
            .with_connect_timeout(std::time::Duration::from_secs(timeout_secs))
            .build()
            .context("Failed to create rate-limited crawler")
    }

    async fn get_text(&self, url: &str) -> Result<String> {
        tracing::debug!("PubMed API request: {}", url);
        let response = self.crawler.get(url).await.map_err(|e| {
            anyhow::anyhow!("PubMed API request failed: {:?}", e)
        })?;
        let status = response.status();
        if !status.is_success() {
            let body_preview = match response.text().await {
                Ok(body) => body,
                Err(e) => format!("(failed to read error body: {e})"),
            };
            anyhow::bail!("PubMed API returned HTTP {}: {}", status, body_preview);
        }
        response.text().await.context("Failed to read response body")
    }

    pub async fn search(&self, query: &SearchQuery) -> Result<SearchResult> {
        let query_string = query.to_query_string();
        let url = format!("{}/esearch.fcgi?db=pubmed&{}&retmode=json", self.api_url, query_string);
        let body = self.get_text(&url).await?;
        core::parse_search_json(&body).map_err(|e| anyhow::anyhow!("{e}"))
    }

    pub async fn get_summaries(&self, ids: &str) -> Result<Vec<Paper>> {
        let url = format!("{}/esummary.fcgi?db=pubmed&id={}&retmode=json", self.api_url, urlencoding::encode(ids));
        let body = self.get_text(&url).await?;
        core::parse_summary_json(&body).map_err(|e| anyhow::anyhow!("{e}"))
    }

    pub async fn fetch_abstracts(&self, ids: &str) -> Result<Vec<(String, String)>> {
        let url = format!("{}/efetch.fcgi?db=pubmed&id={}&rettype=medline&retmode=text", self.api_url, urlencoding::encode(ids));
        let body = self.get_text(&url).await?;
        let abstracts = core::parse_abstract_text(&body);
        if abstracts.is_empty() && !ids.is_empty() {
            anyhow::bail!("fetch_abstracts: parsed 0 abstracts for provided PMIDs (format may have changed)");
        }
        Ok(abstracts)
    }

    pub async fn find_related(&self, ids: &str) -> Result<core::RelatedArticles> {
        let url = format!("{}/elink.fcgi?dbfrom=pubmed&db=pubmed&id={}&retmode=json", self.api_url, urlencoding::encode(ids));
        let body = self.get_text(&url).await?;
        core::parse_elink_json(&body).map_err(|e| anyhow::anyhow!("{e}"))
    }

    pub async fn get_database_info(&self) -> Result<core::DatabaseInfo> {
        let url = format!("{}/einfo.fcgi?db=pubmed&retmode=json", self.api_url);
        let body = self.get_text(&url).await?;
        core::parse_einfo_json(&body).map_err(|e| anyhow::anyhow!("{e}"))
    }

    pub async fn match_citation(&self, bdata: &str) -> Result<Vec<core::CitationMatch>> {
        let url = format!("{}/ecitmatch.cgi?db=pubmed&bdata={}", self.api_url, urlencoding::encode(bdata));
        let body = self.get_text(&url).await?;
        Ok(core::parse_ecitmatch_text(&body))
    }
}

// ---------------------------------------------------------------------------
// get_paper / get_paper_raw
// ---------------------------------------------------------------------------
impl PubmedClient {
    /// Download a PubMed Central paper by PMC ID and convert to markdown.
    ///
    /// Strips the leading "PMC" prefix if present, downloads the PDF from
    /// PubMed Central via the rate-limited crawler, and converts to Markdown
    /// via xberg + webfetch.
    pub async fn get_paper(&self, pmc_id: &str) -> Result<String> {
        let id = pmc_id.strip_prefix("PMC").unwrap_or(pmc_id);
        let url = format!("{}/articles/PMC{id}/pdf/", self.base_url.trim_end_matches('/'));
        let response = self.crawler.get(&url).send().await
            .context("Failed to fetch PubMed paper PDF")?;

        let status = response.status();
        if !status.is_success() {
            let body_preview = match response.text().await {
                Ok(body) => body,
                Err(e) => format!("(failed to read error body: {e})"),
            };
            anyhow::bail!("PubMed paper PDF returned HTTP {}: {}", status, body_preview);
        }

        let bytes = response.bytes().await
            .map_err(|e| anyhow::anyhow!("Failed to read PDF bytes: {}", e))?;

        let result = delulu_webfetch::process_doc_bytes(bytes.to_vec(), &url)
            .await
            .context("Failed to process PubMed paper PDF")?;

        match result {
            delulu_webfetch::ExtractionResult::GenericHtml { content_md } => {
                Ok(content_md.body)
            }
            _ => anyhow::bail!("Unexpected result type from fetch_doc"),
        }
    }

    /// Download a PubMed Central paper by PMC ID and return raw PDF bytes.
    ///
    /// Strips the leading "PMC" prefix if present and downloads the raw PDF.
    pub async fn get_paper_raw(&self, pmc_id: &str) -> Result<Vec<u8>> {
        let id = pmc_id.strip_prefix("PMC").unwrap_or(pmc_id);
        let url = format!("{}/articles/PMC{id}/pdf/", self.base_url.trim_end_matches('/'));
        let response = self.crawler.get(&url).send().await
            .context("Failed to fetch PubMed paper PDF")?;
        let bytes = response.bytes().await
            .map_err(|e| anyhow::anyhow!("Failed to read PDF bytes: {}", e))?;
        Ok(bytes.to_vec())
    }
}
// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_paper_url_with_pmc_prefix() {
        let base_url = "https://www.ncbi.nlm.nih.gov/pmc";
        let pmc_id = "PMC123456";
        let id = pmc_id.strip_prefix("PMC").unwrap_or(pmc_id);
        let url = format!("{}/articles/PMC{id}/pdf/", base_url.trim_end_matches('/'));
        assert_eq!(id, "123456");
        assert_eq!(url, "https://www.ncbi.nlm.nih.gov/pmc/articles/PMC123456/pdf/");
    }

    #[test]
    fn test_get_paper_url_without_pmc_prefix() {
        let base_url = "https://www.ncbi.nlm.nih.gov/pmc";
        let pmc_id = "123456";
        let id = pmc_id.strip_prefix("PMC").unwrap_or(pmc_id);
        let url = format!("{}/articles/PMC{id}/pdf/", base_url.trim_end_matches('/'));
        assert_eq!(id, "123456");
        assert_eq!(url, "https://www.ncbi.nlm.nih.gov/pmc/articles/PMC123456/pdf/");
    }

    #[test]
    fn test_get_paper_url_with_pmc_lowercase() {
        let base_url = "https://www.ncbi.nlm.nih.gov/pmc";
        let pmc_id = "pmc123456";
        let id = pmc_id.strip_prefix("PMC").unwrap_or(pmc_id);
        let url = format!("{}/articles/PMC{id}/pdf/", base_url.trim_end_matches('/'));
        assert_eq!(id, "pmc123456");
        assert_eq!(url, "https://www.ncbi.nlm.nih.gov/pmc/articles/PMCpmc123456/pdf/");
    }

    #[test]
    fn test_get_paper_raw_url_construction() {
        let base_url = "https://www.ncbi.nlm.nih.gov/pmc";
        let pmc_id = "PMC987654";
        let id = pmc_id.strip_prefix("PMC").unwrap_or(pmc_id);
        let url = format!("{}/articles/PMC{id}/pdf/", base_url.trim_end_matches('/'));
        assert_eq!(id, "987654");
        assert_eq!(url, "https://www.ncbi.nlm.nih.gov/pmc/articles/PMC987654/pdf/");
    }
}

#[cfg(test)]
#[path = "../tests/unit/pubmed_client_test.rs"]
mod client_tests;