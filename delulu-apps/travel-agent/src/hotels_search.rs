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
use anyhow::{anyhow, bail, Context, Result};
use delulu_query_queues::QueryQueue;
use std::sync::Arc;
use wreq::redirect::Policy;
use wreq_util::Emulation;

fn build_search_url(location: &str, ts_param: &str) -> String {
    let encoded_location = location.replace(' ', "+");
    format!(
        "https://www.google.com/travel/search?q={}&ts={}",
        encoded_location, ts_param
    )
}

#[derive(Clone)]
pub struct GoogleHotelsClient {
    client: Arc<wreq::Client>,
    query_queue: QueryQueue,
}

impl GoogleHotelsClient {
    pub fn new(max_concurrent: u64) -> Result<Self> {
        let client = wreq::Client::builder()
            .emulation(Emulation::Safari18_5)
            .redirect(Policy::default())
            .build()
            .context("Failed to build HTTP client")?;
        let query_queue = QueryQueue::with_max_concurrent(max_concurrent);
        Ok(Self {
            client: Arc::new(client),
            query_queue,
        })
    }
}

impl GoogleHotelsClient {
    async fn fetch_raw(&self, request: &HotelSearchParams) -> Result<String> {
        let ts_param = request.generate_ts().context("TS encode failed")?;
        let location = request.location();
        let cookie_header = generate_cookie_header();
        let client_inner = Arc::clone(&self.client);

        let response = self
            .query_queue
            .with_retry(move || {
                let url = build_search_url(&location, &ts_param);
                let cookie = cookie_header.clone();
                let http_client = client_inner.clone();
                async move {
                    tracing::info!("Fetching Google Hotels URL: {}", url);
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
        let body = response.text().await.context("Read body")?;

        let is_consent_page = body.contains("consent.google.com")
            || body.contains("base href=\"https://consent.google.com\"")
            || body.contains("ppConfig");

        if is_consent_page {
            bail!(
                "Consent wall detected - cookies not accepted. \
                 Consider using a proxy or residential IP. \
                 Body preview: {}",
                &body[..body.len().min(300)]
            );
        }

        let body_chars = body.chars().count();
        let has_hotel_marker = body.contains("uaTTDe")
            || body.contains("BgYkof")
            || body.contains("KFi5wf")
            || body.contains("LtjZ2d");

        tracing::debug!(
            "Response: {} chars, has_hotel_markers={}, status={}",
            body_chars,
            has_hotel_marker,
            status
        );

        if !has_hotel_marker && body_chars > 1000 {
            tracing::warn!("Page may have changed - no hotel markers found");
        }

        Ok(body)
    }

    pub async fn search_hotels(&self, params: &HotelSearchParams) -> Result<HotelSearchResult> {
        params.validate()?;

        let today = chrono::Local::now().date_naive();
        let checkin = chrono::NaiveDate::parse_from_str(&params.checkin_date, "%Y-%m-%d")
            .context("Invalid checkin date")?;
        if checkin < today {
            bail!("Check-in cannot be in the past");
        }

        let html = self.fetch_raw(params).await?;
        let result = HotelSearchResult::from_html(&html)?;
        Ok(result)
    }
}
