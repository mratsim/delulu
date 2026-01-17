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
    pub fn new(language: String, currency: String) -> Result<Self> {
        let client = wreq::Client::builder()
            .emulation(Emulation::Safari18_5)
            .redirect(Policy::default())
            .timeout(Duration::from_secs(5))
            .connect_timeout(Duration::from_secs(5))
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
    pub async fn fetch_raw(&self, request: &FlightSearchParams) -> Result<String> {
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

        match FlightSearchResult::from_html(&html, params.clone()) {
            Ok(result) => {
                tracing::debug!("Parsed {} itineraries", result.itineraries.len());
                Ok(result)
            }
            Err(e) => {
                let preview = html.chars().take(2000).collect::<String>();
                tracing::error!("Parse failed: {:?}", e);

                let has_flight_cards = html.contains("pIav2d") || html.contains("JMc5Xc");
                let has_loading = html.contains("Loading results") || html.contains("jsshadow");
                let has_consent = html.contains("consent.google.com") || html.contains("ppConfig");

                if has_consent {
                    tracing::error!("Consent wall detected - cookies not accepted");
                } else if !has_flight_cards && has_loading {
                    tracing::error!("Detected loading spinner without flight data.");
                    tracing::error!("This often happens for sparse routes (small airports) or when Google loads results via JavaScript.");
                    tracing::error!(
                        "For YYD (Smithers), CDG (Paris), etc., Google may require JS rendering."
                    );
                } else if !has_flight_cards {
                    tracing::error!("No flight data in response. This may indicate:");
                    tracing::error!("  - SOCS cookie expired or invalid");
                    tracing::error!("  - Bot detection triggered");
                    tracing::error!("  - Rate limiting applied");
                    tracing::error!("  - Route has no available flights");
                } else {
                    tracing::error!("Flight HTML detected but parser failed to extract. Parser may need updating.");
                }

                tracing::error!("HTML preview (first 2000 chars):\n{}", preview);
                Err(e).context("Parse failed - see HTML preview above")
            }
        }
    }
}

impl Default for GoogleFlightsClient {
    fn default() -> Self {
        Self::new("en".into(), "USD".into()).expect(
            "GoogleFlightsClient::default() requires wreq client to initialize successfully",
        )
    }
}
