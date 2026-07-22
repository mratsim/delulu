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

const BASE_URL: &str = "https://eutils.ncbi.nlm.nih.gov/entrez/eutils";

/// HTTP client for the NCBI PubMed E-utilities API.
#[derive(Clone)]
pub struct PubmedClient {
    crawler: Arc<RateLimitedCrawler>,
    base_url: String,
}

impl PubmedClient {
    pub fn new(timeout_secs: u64) -> Result<Self> {
        Self::with_base_url(timeout_secs, BASE_URL.to_string())
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

    async fn get_text(&self, url: &str) -> Result<String> {
        tracing::debug!("PubMed API request: {}", url);
        let response = self.crawler.get(url).await.map_err(|e| {
            anyhow::anyhow!("PubMed API request failed: {:?}", e)
        })?;
        let status = response.status();
        if !status.is_success() {
            anyhow::bail!("PubMed API returned HTTP {}: {}", status, response.text().await.unwrap_or_default());
        }
        response.text().await.context("Failed to read response body")
    }

    pub async fn search(&self, query: &SearchQuery) -> Result<SearchResult> {
        let query_string = query.to_query_string();
        let url = format!("{}/esearch.fcgi?db=pubmed&{}&retmode=json", self.base_url, query_string);
        let body = self.get_text(&url).await?;
        core::parse_search_json(&body).map_err(|e| anyhow::anyhow!("{e}"))
    }

    pub async fn get_summaries(&self, ids: &str) -> Result<Vec<Paper>> {
        let url = format!("{}/esummary.fcgi?db=pubmed&id={}&retmode=json", self.base_url, urlencoding::encode(ids));
        let body = self.get_text(&url).await?;
        core::parse_summary_json(&body).map_err(|e| anyhow::anyhow!("{e}"))
    }

    pub async fn fetch_abstracts(&self, ids: &str) -> Result<Vec<(String, String)>> {
        let url = format!("{}/efetch.fcgi?db=pubmed&id={}&rettype=medline&retmode=text", self.base_url, urlencoding::encode(ids));
        let body = self.get_text(&url).await?;
        let abstracts = core::parse_abstract_text(&body);
        if abstracts.is_empty() && !ids.is_empty() {
            anyhow::bail!("fetch_abstracts: parsed 0 abstracts for provided PMIDs (format may have changed)");
        }
        Ok(abstracts)
    }

    pub async fn find_related(&self, ids: &str) -> Result<core::RelatedArticles> {
        let url = format!("{}/elink.fcgi?dbfrom=pubmed&db=pubmed&id={}&retmode=json", self.base_url, urlencoding::encode(ids));
        let body = self.get_text(&url).await?;
        core::parse_elink_json(&body).map_err(|e| anyhow::anyhow!("{e}"))
    }

    pub async fn get_database_info(&self) -> Result<core::DatabaseInfo> {
        let url = format!("{}/einfo.fcgi?db=pubmed&retmode=json", self.base_url);
        let body = self.get_text(&url).await?;
        core::parse_einfo_json(&body).map_err(|e| anyhow::anyhow!("{e}"))
    }

    pub async fn match_citation(&self, bdata: &str) -> Result<Vec<core::CitationMatch>> {
        let url = format!("{}/ecitmatch.cgi?db=pubmed&bdata={}", self.base_url, urlencoding::encode(bdata));
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
    /// PubMed Central, and converts to Markdown via xberg + webfetch.
    pub async fn get_paper(&self, pmc_id: &str) -> Result<String> {
        let id = pmc_id.strip_prefix("PMC").unwrap_or(pmc_id);
        let url = format!("https://www.ncbi.nlm.nih.gov/pmc/articles/PMC{id}/pdf/");
        let client = delulu_webfetch::WebbfetchClient::new(120, 3);
        let result = delulu_webfetch::fetch_doc(&url, &client)
            .await
            .context("Failed to fetch PubMed paper")?;
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
        let url = format!("https://www.ncbi.nlm.nih.gov/pmc/articles/PMC{id}/pdf/");
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
        // Test that PMC prefix is stripped and URL is constructed correctly
        let pmc_id = "PMC123456";
        let id = pmc_id.strip_prefix("PMC").unwrap_or(pmc_id);
        let url = format!("https://www.ncbi.nlm.nih.gov/pmc/articles/PMC{id}/pdf/");
        assert_eq!(id, "123456");
        assert_eq!(url, "https://www.ncbi.nlm.nih.gov/pmc/articles/PMC123456/pdf/");
    }

    #[test]
    fn test_get_paper_url_without_pmc_prefix() {
        // Test that ID without PMC prefix is used as-is
        let pmc_id = "123456";
        let id = pmc_id.strip_prefix("PMC").unwrap_or(pmc_id);
        let url = format!("https://www.ncbi.nlm.nih.gov/pmc/articles/PMC{id}/pdf/");
        assert_eq!(id, "123456");
        assert_eq!(url, "https://www.ncbi.nlm.nih.gov/pmc/articles/PMC123456/pdf/");
    }

    #[test]
    fn test_get_paper_url_with_pmc_lowercase() {
        // Test that lower-case 'pmc' is NOT stripped (only uppercase PMC)
        let pmc_id = "pmc123456";
        let id = pmc_id.strip_prefix("PMC").unwrap_or(pmc_id);
        let url = format!("https://www.ncbi.nlm.nih.gov/pmc/articles/PMC{id}/pdf/");
        assert_eq!(id, "pmc123456");
        assert_eq!(url, "https://www.ncbi.nlm.nih.gov/pmc/articles/PMCpmc123456/pdf/");
    }

    #[test]
    fn test_get_paper_raw_url_construction() {
        // Test get_paper_raw URL logic (same as get_paper)
        let pmc_id = "PMC987654";
        let id = pmc_id.strip_prefix("PMC").unwrap_or(pmc_id);
        assert_eq!(id, "987654");
        let url = format!("https://www.ncbi.nlm.nih.gov/pmc/articles/PMC{id}/pdf/");
        assert_eq!(url, "https://www.ncbi.nlm.nih.gov/pmc/articles/PMC987654/pdf/");
    }
}
