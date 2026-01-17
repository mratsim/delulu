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

use crate::consent_cookie::generate_cookie_header;
use crate::flights_query_builder::FlightSearchParams;
use crate::flights_results_parser::FlightSearchResult;
use anyhow::{anyhow, bail, Context, Result};
use delulu_query_queues::QueryQueue;
use std::sync::Arc;
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
    pub fn new(language: String, currency: String) -> Result<Self> {
        let client = wreq::Client::builder()
            .emulation(Emulation::Safari18_5)
            .redirect(Policy::default())
            .build()
            .context("Failed to build HTTP client")?;
        let query_queue = QueryQueue::with_max_concurrent(1);
        Ok(Self {
            client: Arc::new(client),
            query_queue,
            _language: language,
            _currency: currency,
        })
    }
}

impl GoogleFlightsClient {
    async fn fetch_raw(&self, request: &FlightSearchParams) -> Result<String> {
        let cookie_header = generate_cookie_header();
        let client_inner = Arc::clone(&self.client);
        let url = request.get_search_url();

        let response = self
            .query_queue
            .with_retry(move || {
                let url = url.clone();
                let cookie = cookie_header.clone();
                let http_client = client_inner.clone();
                async move {
                    tracing::trace!("Fetching Google Flights URL: {}", url);
                    let resp = http_client
                        .get(&url)
                        .header("Cookie", &cookie)
                        .send()
                        .await?;
                    Ok(resp)
                }
            })
            .await
            .map_err(|e| anyhow!("Request failed: {:?}", e))?;

        let status = response.status();
        tracing::debug!(
            "HTTP Status: {} {}",
            status.as_u16(),
            status.canonical_reason().unwrap_or("Unknown")
        );

        let body = response.text().await.context("Read body")?;
        let body_len_kb = body.len() / 1024;
        tracing::debug!("Response body: {} KB", body_len_kb);

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
        let url = params.get_search_url();
        tracing::info!("🔗 Search URL:\n{}", url);

        let today = chrono::Local::now().date_naive();
        let depart_date = chrono::NaiveDate::parse_from_str(&params.depart_date, "%Y-%m-%d")
            .context("Invalid depart date")?;
        anyhow::ensure!(depart_date >= today, "Departure date cannot be in the past");

        let html = self.fetch_raw(params).await?;
        let result = FlightSearchResult::from_html(&html, params.clone())?;
        tracing::debug!("Parsed {} itineraries", result.itineraries.len());
        Ok(result)
    }

    pub async fn search_flights_url(&self, url: &str) -> Result<FlightSearchResult> {
        let cookie_header = generate_cookie_header();
        let client_inner = Arc::clone(&self.client);
        let url = url.to_string();
        let cookie = cookie_header.clone();

        let response = self
            .query_queue
            .with_retry(move || {
                let http_client = client_inner.clone();
                let url = url.clone();
                let cookie = cookie.clone();
                async move {
                    tracing::trace!("Fetching Google Flights URL: {}", url);
                    let resp = http_client
                        .get(&url)
                        .header("Cookie", &cookie)
                        .send()
                        .await?;
                    Ok(resp)
                }
            })
            .await
            .map_err(|e| anyhow!("Request failed: {:?}", e))?;

        let status = response.status();
        tracing::debug!(
            "HTTP Status: {} {}",
            status.as_u16(),
            status.canonical_reason().unwrap_or("Unknown")
        );

        let body = response.text().await.context("Read body")?;
        let body_len_kb = body.len() / 1024;
        tracing::debug!("Response body: {} KB", body_len_kb);

        if !status.is_success() {
            let body_preview = body.chars().take(500).collect::<String>();
            bail!("HTTP error {}: {}", status, body_preview);
        }

        let is_consent_page = body.contains("consent.google.com")
            || body.contains("base href=\"https://consent.google.com\"")
            || body.contains("ppConfig");

        if is_consent_page {
            bail!("Consent wall detected - cookies not accepted");
        }

        let params = FlightSearchParams::builder(
            "FROM".into(),
            "TO".into(),
            chrono::Local::now().date_naive(),
        )
        .build()
        .unwrap();
        let result = FlightSearchResult::from_html(&body, params)?;
        Ok(result)
    }
}

impl Default for GoogleFlightsClient {
    fn default() -> Self {
        Self::new("en".into(), "USD".into()).expect(
            "GoogleFlightsClient::default() requires wreq client to initialize successfully",
        )
    }
}
