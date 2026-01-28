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

//! # Google Flights Search Client
//!
//! Effectful (time, network) operations for Google Flights search.

use crate::Trip;
use crate::consent_cookie::generate_cookie_header;
use crate::flights_query_builder::FlightSearchParams;
use crate::flights_results_parser::FlightSearchResult;
use anyhow::{Context, Result, anyhow, bail};
use delulu_query_queues::QueryQueue;
use std::sync::Arc;
use std::time::Duration;
use wreq::redirect::Policy;
use wreq_util::Emulation;

#[derive(Clone)]
pub struct GoogleFlightsClient {
    client: Arc<wreq::Client>,
    query_queue: QueryQueue,
    _language: String,
    _currency: String,
}

impl GoogleFlightsClient {
    pub fn new(
        language: String,
        currency: String,
        timeout_secs: u64,
        queries_per_second: u32,
    ) -> Result<Self> {
        let client = wreq::Client::builder()
            .emulation(Emulation::Safari18_5)
            .redirect(Policy::default())
            .timeout(Duration::from_secs(timeout_secs))
            .connect_timeout(Duration::from_secs(timeout_secs))
            .build()
            .context("Failed to build HTTP client")?;
        let query_queue = QueryQueue::with_qps_limit(queries_per_second as u64);
        Ok(Self {
            client: Arc::new(client),
            query_queue,
            _language: language,
            _currency: currency,
        })
    }
}

impl GoogleFlightsClient {
    pub async fn fetch_raw(&self, url: &str) -> Result<String> {
        let cookie_header = generate_cookie_header();
        let client_inner = Arc::clone(&self.client);

        let queue_start = std::time::Instant::now();
        let response = self
            .query_queue
            .with_retry(move || {
                let url = url.to_string();
                let cookie = cookie_header.clone();
                let http_client = client_inner.clone();
                async move {
                    let http_start = std::time::Instant::now();
                    tracing::trace!("[fetch_raw] Starting HTTP request to: {}", url);
                    let resp = http_client
                        .get(url)
                        .header("Cookie", &cookie)
                        .send()
                        .await?;
                    let http_elapsed = http_start.elapsed();
                    tracing::trace!("[fetch_raw] HTTP request completed in {:?}", http_elapsed);
                    Ok(resp)
                }
            })
            .await;
        let total_elapsed = queue_start.elapsed();
        tracing::debug!(
            "[fetch_raw] Query queue + HTTP execution time: {:?}",
            total_elapsed
        );

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
        let body_len_kb = body.len() / 1024;
        tracing::debug!(
            "[fetch_raw] Response body read in {:?}: {} KB",
            body_elapsed,
            body_len_kb
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

        Ok(body)
    }

    pub async fn search_flights(&self, params: &FlightSearchParams) -> Result<FlightSearchResult> {
        let overall_start = std::time::Instant::now();
        params.validate().context("Invalid search parameters")?;

        if params.trip_type == Trip::RoundTrip && params.return_date.is_none() {
            tracing::warn!(
                "RoundTrip selected but no return_date provided - performing one-way search"
            );
        }

        let url_build_start = std::time::Instant::now();
        let url = params.get_search_url();
        let url_build_elapsed = url_build_start.elapsed();
        tracing::info!("🔗 Search URL built in {:?}: {}", url_build_elapsed, url);

        let today = chrono::Local::now().date_naive();
        let depart_date = chrono::NaiveDate::parse_from_str(&params.depart_date, "%Y-%m-%d")
            .context("Invalid depart date")?;
        anyhow::ensure!(depart_date >= today, "Departure date cannot be in the past");

        if let Some(return_date_str) = &params.return_date {
            let return_date = chrono::NaiveDate::parse_from_str(return_date_str, "%Y-%m-%d")
                .context("Invalid return date")?;
            anyhow::ensure!(return_date >= today, "Return date cannot be in the past");
        }

        let fetch_start = std::time::Instant::now();
        tracing::info!("Starting HTTP fetch to Google Flights...");
        let html = self.fetch_raw(&url).await?;
        let fetch_elapsed = fetch_start.elapsed();
        tracing::info!(
            "HTTP fetch completed in {:?}, got {} KB",
            fetch_elapsed,
            html.len() / 1024
        );

        let parse_start = std::time::Instant::now();
        match FlightSearchResult::from_html(&html, params.clone()) {
            Ok(result) => {
                let parse_elapsed = parse_start.elapsed();
                tracing::debug!(
                    "Parsed {} itineraries in {:?}",
                    result.itineraries.len(),
                    parse_elapsed
                );
                let total_elapsed = overall_start.elapsed();
                tracing::info!("Total search_flights time: {:?}", total_elapsed);
                Ok(result)
            }
            Err(e) => {
                let parse_elapsed = parse_start.elapsed();
                let preview = html.chars().take(2000).collect::<String>();
                tracing::error!("Parse failed after {:?}: {:?}", parse_elapsed, e);

                let has_flight_cards = html.contains("pIav2d") || html.contains("JMc5Xc");
                let has_loading = html.contains("Loading results") || html.contains("jsshadow");
                let has_consent = html.contains("consent.google.com") || html.contains("ppConfig");

                if has_consent {
                    tracing::error!("Consent wall detected - cookies not accepted");
                } else if !has_flight_cards && has_loading {
                    tracing::warn!("Detected loading spinner without flight data.");
                    tracing::warn!(
                        "This often happens for sparse routes or when Google loads results via JavaScript."
                    );
                    tracing::warn!(
                        "For NRT→JFK, this may indicate Google is using dynamic JS rendering."
                    );
                    tracing::warn!(
                        "Consider using a headless browser or checking route popularity."
                    );
                } else if !has_flight_cards {
                    tracing::warn!("No flight data in response. This may indicate:");
                    tracing::warn!("  - Route returned no flights (might be sold out)");
                    tracing::warn!("  - Google using JS lazy-loading for this route");
                    tracing::warn!("  - Request caching vs fresh request behavior differs");
                } else {
                    tracing::error!(
                        "Flight HTML detected but parser failed to extract. Parser may need updating."
                    );
                }

                tracing::error!("HTML preview (first 2000 chars):\n{}", preview);
                let total_elapsed = overall_start.elapsed();
                tracing::info!("Total search_flights time (failed): {:?}", total_elapsed);
                Err(e).context("Parse failed - see HTML preview above")
            }
        }
    }
}
