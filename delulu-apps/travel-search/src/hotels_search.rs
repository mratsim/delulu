//!  Delulu Travel Agent
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

//! # Google Hotels Search Client
//!
//! Effectful (time, network) operations for Google Hotels search.

use crate::consent_cookie::generate_cookie_header;
use crate::hotels_query_builder::HotelSearchParams;
use crate::hotels_results_parser::HotelSearchResult;
use anyhow::{Context, Result, anyhow, bail};
use delulu_rate_limited_crawler::RateLimitedCrawler;
use std::sync::Arc;
use std::time::Duration;
use wreq::redirect::Policy;
use wreq_util::Profile;

#[derive(Clone)]
pub struct GoogleHotelsClient {
    crawler: Arc<RateLimitedCrawler>,
}

impl GoogleHotelsClient {
    pub fn new(timeout_secs: u64, queries_per_second: u32) -> Result<Self> {
        let crawler = Arc::new(
            RateLimitedCrawler::builder()
                .with_qps(queries_per_second as u64)
                .with_emulation(Profile::Safari18_5)
                .with_redirect(Policy::default())
                .with_timeout(Duration::from_secs(timeout_secs))
                .with_connect_timeout(Duration::from_secs(timeout_secs))
                .build()
                .context("Failed to build rate-limited crawler")?,
        );
        Ok(Self { crawler })
    }
}

impl GoogleHotelsClient {
    async fn fetch_raw(&self, url: &str) -> Result<String> {
        let cookie_header = generate_cookie_header();

        let response = self
            .crawler
            .get(url)
            .merge_with_headers(vec![("Cookie".to_string(), cookie_header)])
            .with_exponential_retry(1)
            .send()
            .await;
        let response = response.map_err(|e| anyhow!("Request failed: {:?}", e))?;

        let status = response.status();
        tracing::debug!(
            "[fetch_raw] HTTP Status: {} {}",
            status.as_u16(),
            status.canonical_reason().unwrap_or("Unknown")
        );

        let body_start = std::time::Instant::now();
        let body = response.text().await.context("Read body")?;
        let body_elapsed = body_start.elapsed();
        tracing::debug!(
            "[fetch_raw] Response body read in {:?}: {} bytes",
            body_elapsed,
            body.len()
        );

        if !status.is_success() {
            let body_preview = body.chars().take(500).collect::<String>();
            bail!("HTTP error {}: {}", status, body_preview);
        }

        let is_consent_page = body.contains("consent.google.com")
            || body.contains("base href=\"https://consent.google.com\"")
            || body.contains("ppConfig");

        if is_consent_page {
            let body_preview = body.chars().take(300).collect::<String>();
            bail!(
                "Consent wall detected - cookies not accepted. \
                  Consider using a proxy or residential IP. \
                  Body preview: {}",
                body_preview
            );
        }

        let body_chars = body.chars().count();
        let has_hotel_marker = body.contains("uaTTDe")
            || body.contains("BgYkof")
            || body.contains("KFi5wf")
            || body.contains("LtjZ2d");

        tracing::debug!(
            "[fetch_raw] Response: {} chars, has_hotel_markers={}",
            body_chars,
            has_hotel_marker
        );

        if !has_hotel_marker && body_chars > 1000 {
            tracing::warn!("[fetch_raw] Page may have changed - no hotel markers found");
        }

        Ok(body)
    }

    pub async fn search_hotels(&self, params: &HotelSearchParams) -> Result<HotelSearchResult> {
        let overall_start = std::time::Instant::now();
        let today = chrono::Local::now().date_naive();
        let checkin = chrono::NaiveDate::parse_from_str(&params.checkin_date, "%Y-%m-%d")
            .context("Invalid checkin date")?;
        anyhow::ensure!(checkin >= today, "Check-in cannot be in the past");

        let url_build_start = std::time::Instant::now();
        let url = params.get_search_url();
        let url_build_elapsed = url_build_start.elapsed();
        tracing::info!("🔗 Search URL built in {:?}: {}", url_build_elapsed, url);

        let fetch_start = std::time::Instant::now();
        tracing::info!("[search_hotels] Starting HTTP fetch to Google Hotels...");
        let html = self.fetch_raw(&url).await?;
        let fetch_elapsed = fetch_start.elapsed();
        tracing::info!(
            "[search_hotels] HTTP fetch completed in {:?}, got {} KB",
            fetch_elapsed,
            html.len() / 1024
        );

        let parse_start = std::time::Instant::now();
        match HotelSearchResult::from_html(&html) {
            Ok(result) => {
                let parse_elapsed = parse_start.elapsed();
                tracing::debug!(
                    "[search_hotels] Parsed {} hotels in {:?}",
                    result.hotels.len(),
                    parse_elapsed
                );
                let total_elapsed = overall_start.elapsed();
                tracing::info!(
                    "[search_hotels] Total search_hotels time: {:?}",
                    total_elapsed
                );
                Ok(result)
            }
            Err(e) => {
                let parse_elapsed = parse_start.elapsed();
                let preview = html.chars().take(2000).collect::<String>();
                tracing::error!(
                    "[search_hotels] Parse failed after {:?}: {:?}",
                    parse_elapsed,
                    e
                );

                let has_hotel_markers =
                    html.contains("uaTTDe") || html.contains("BgYkof") || html.contains("KFi5wf");
                let has_loading = html.contains("Loading") || html.contains("jsshadow");

                if has_loading && !has_hotel_markers {
                    tracing::warn!("[search_hotels] Detected loading spinner without hotel data.");
                    tracing::warn!(
                        "[search_hotels] Google may use JS lazy-loading for this location."
                    );
                } else if !has_hotel_markers {
                    tracing::warn!("[search_hotels] No hotel markers found. This may indicate:");
                    tracing::warn!("  - Location returned no hotels");
                    tracing::warn!("  - Google using JS lazy-loading for this location");
                    tracing::warn!("  - Request caching vs fresh request behavior differs");
                }

                tracing::error!(
                    "[search_hotels] HTML preview (first 2000 chars):\n{}",
                    preview
                );
                let total_elapsed = overall_start.elapsed();
                tracing::info!(
                    "[search_hotels] Total search_hotels time (failed): {:?}",
                    total_elapsed
                );
                Err(e).context("Parse failed - see HTML preview above")
            }
        }
    }
}
