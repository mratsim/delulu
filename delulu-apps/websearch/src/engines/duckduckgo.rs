//!  Delulu Web Search
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

//! DuckDuckGo search engine backend.
//!
//! Implements a 2-step search flow:
//! 1. GET `https://duckduckgo.com/?q=...` to obtain the `deep_preload_link`
//!    (d.js URL) from the HTML response.
//! 2. GET `https://links.duckduckgo.com/d.js?...` to fetch the JSON-embedded
//!    JavaScript response and parse `DDG.pageLayout.load('d', ...)`.
//!

use async_trait::async_trait;
use delulu_rate_limited_crawler::RateLimitedCrawler;
use scraper::{Html, Selector};
use serde::{Deserialize, Serialize};
use std::any::Any;
use std::time::Instant;
use tracing::{debug, warn};

use crate::engine::{Continuation, Engine, SearchParams, SearchResponse, SearchResult};
use crate::error::WebsearchError;
use crate::sanitize_for_log;

/// DuckDuckGo search engine implementation.
///
/// # Precondition
/// - The internal `RateLimitedCrawler` must be configured with QPS 1,
///   10s timeout, and 5MB max response size.
///
/// # Postcondition
/// - Returns `Ok(SearchResponse)` with up to `max_results` results.
/// - Returns `Err(WebsearchError::AccessDenied)` if a JSA challenge or
///   captcha is detected.
/// - Returns `Err(WebsearchError::ParseFailed)` if the response cannot
///   be parsed.
///
/// # Panic-if
/// - This function MUST NOT panic. All error paths return Err.
pub struct DuckDuckGoEngine {
    crawler: RateLimitedCrawler,
}

/// DuckDuckGo User-Agent
const DDG_USER_AGENT: &str =
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:151.0) Gecko/20100101 Firefox/151.0";

/// Continuation token for DuckDuckGo pagination.
///
/// DuckDuckGo uses an opaque "n" token extracted from the d.js response
/// items for pagination. The token is used as a direct URL path to
/// `https://links.duckduckgo.com/{n_token}`.
///
/// # Security
/// The n_token is validated to contain only safe URL path characters
/// (`[a-zA-Z0-9/?=._-]`) to prevent SSRF via path traversal or
/// protocol injection.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DuckDuckGoContinuation {
    /// The next-page token extracted from the d.js response "n" field.
    /// Must contain only safe URL path characters.
    pub n_token: String,
}

impl Continuation for DuckDuckGoContinuation {
    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// Validate that n_token contains only safe URL path/query characters.
/// This prevents SSRF via path traversal (`..`) or protocol injection
/// (`https://...`). Allowed: alphanumeric, `/`, `?`, `=`, `.`, `_`, `-`, `&`.
pub(crate) fn validate_n_token(token: &str) -> bool {
    !token.is_empty()
        && token.chars().all(|c| {
            c.is_ascii_alphanumeric() || matches!(c, '/' | '?' | '=' | '.' | '_' | '-' | '&')
        })
        && !token.contains("..")
        && !token.starts_with("https://")
        && !token.starts_with("http://")
}

impl DuckDuckGoEngine {
    /// Create a new DuckDuckGo engine with the given crawler.
    pub fn new(crawler: RateLimitedCrawler) -> Self {
        Self { crawler }
    }

    /// Build the initial search URL with query parameters.
    pub fn build_search_url(query: &str, params: &SearchParams) -> String {
        let mut url = format!("https://duckduckgo.com/?q={}", urlencoding::encode(query));

        // Country parameter (kl)
        match params.country.as_deref() {
            Some("any") | None => url.push_str("&kl=wt-wt"),
            Some(country) => {
                url.push_str("&kl=");
                url.push_str(&urlencoding::encode(country));
            }
        }

        // Safesearch parameter (kp)
        if let Some(ref safesearch) = params.safesearch {
            let kp = match safesearch.as_str() {
                "strict" => "1",
                "moderate" => "-1",
                "off" => "-2",
                _ => "-1", // default moderate
            };
            url.push_str("&kp=");
            url.push_str(kp);
        }

        // Time range parameter (df)
        if let Some(ref time_range) = params.time_range {
            url.push_str("&df=");
            url.push_str(&urlencoding::encode(time_range));
        }

        url
    }

    /// Build the d.js URL with offset for pagination.
    /// Use the d.js URL from the preload link AS-IS.
    /// The preload link is a fully signed URL with a `dp` token.
    /// Modifying it (adding params, changing query) breaks the signature.
    pub fn build_djs_url(djs_path: &str) -> String {
        if djs_path.starts_with("https://") {
            djs_path.to_string()
        } else {
            format!("https://links.duckduckgo.com{}", djs_path)
        }
    }

    /// Extract the deep_preload_link (d.js URL) from the initial HTML page.
    pub fn extract_djs_url(html: &str) -> Result<String, WebsearchError> {
        let document = Html::parse_document(html);

        // Check for captcha first
        let challenge_selector = Selector::parse("form#challenge-form").expect("valid selector");
        if document.select(&challenge_selector).next().is_some() {
            return Err(WebsearchError::AccessDenied);
        }

        // Extract deep_preload_link href
        let link_selector = Selector::parse("link#deep_preload_link").expect("valid selector");
        let link =
            document
                .select(&link_selector)
                .next()
                .ok_or_else(|| WebsearchError::ParseFailed {
                    parser: "duckduckgo_deep_preload",
                    source: "no <link id='deep_preload_link'> found".into(),
                })?;

        let href = link
            .value()
            .attr("href")
            .ok_or_else(|| WebsearchError::MissingField {
                field: "href",
                engine: "duckduckgo",
            })?;

        Ok(href.to_string())
    }

    /// Fetch and parse a d.js response, returning results and an optional n_token.
    async fn fetch_and_parse_djs(
        &self,
        url: &str,
        max_results: usize,
    ) -> Result<(Vec<SearchResult>, Option<String>), WebsearchError> {
        let djs_response = self
            .crawler
            .get(url)
            .with_headers(vec![
                ("User-Agent".into(), DDG_USER_AGENT.into()),
                ("Accept".into(), "*/*".into()),
                ("Accept-Encoding".into(), "gzip, deflate, br, zstd".into()),
                ("Accept-Language".into(), "en-US,en;q=0.9".into()),
                ("Referer".into(), "https://duckduckgo.com/".into()),
                ("DNT".into(), "1".into()),
                ("Sec-GPC".into(), "1".into()),
                ("Connection".into(), "keep-alive".into()),
                ("Sec-Fetch-Dest".into(), "script".into()),
                ("Sec-Fetch-Mode".into(), "no-cors".into()),
                ("Sec-Fetch-Site".into(), "same-site".into()),
                ("Priority".into(), "u=1".into()),
            ])
            .with_exponential_retry(1)
            .send()
            .await?;

        let djs_status = djs_response.status().as_u16();

        if !(200..300).contains(&djs_status) {
            if djs_status == 429 {
                warn!(
                    "DuckDuckGo: rate limited on d.js (429) - retry after {:?}",
                    djs_response.headers().get("retry-after")
                );
                return Err(WebsearchError::HttpStatus {
                    code: djs_status,
                    engine: "duckduckgo",
                });
            }
            return Err(WebsearchError::HttpStatus {
                code: djs_status,
                engine: "duckduckgo",
            });
        }

        let djs_body = djs_response
            .text()
            .await
            .map_err(|e| WebsearchError::ParseFailed {
                parser: "duckduckgo_djs_body",
                source: Box::new(e),
            })?;

        // Parse the d.js response
        match Self::parse_djs_response(&djs_body, max_results) {
            Ok((results, n_token)) => Ok((results, n_token)),
            Err(e) => {
                // Log first 2KB on parse failure
                let end = djs_body.floor_char_boundary(djs_body.len().min(2048));
                debug!(
                    "DuckDuckGo: d.js parse failure, first 2KB of response: {}",
                    sanitize_for_log(&djs_body[..end])
                );
                Err(e)
            }
        }
    }

    /// Parse the d.js response to extract search results and next page token.
    ///
    /// Returns a tuple of (results, optional n_token).
    /// The n_token is extracted from the "n" field of items in the response array.
    /// If no "n" field is found, returns None (last page).
    pub fn parse_djs_response(
        body: &str,
        max_results: usize,
    ) -> Result<(Vec<SearchResult>, Option<String>), WebsearchError> {
        // Check for JSA challenge (javascript challenge)
        if body.contains("DDG.deep.initialize(") {
            return Err(WebsearchError::AccessDenied);
        }

        // Check for anomaly detection block
        if body.contains("DDG.deep.anomalyDetectionBlock({") {
            return Err(WebsearchError::AccessDenied);
        }

        // Split on DDG.pageLayout.load('d',
        let split_marker = "DDG.pageLayout.load('d',";
        let parts: Vec<&str> = body.splitn(2, split_marker).collect();
        if parts.len() <= 1 {
            return Err(WebsearchError::ParseFailed {
                parser: "duckduckgo_djs",
                source: "no DDG.pageLayout.load('d', marker found".into(),
            });
        }

        // Extract JSON: it's everything after the marker until the closing paren + semicolon
        let json_str = extract_json_from_js(parts[1])?;

        // Parse as JSON array
        let items: Vec<serde_json::Value> =
            serde_json::from_str(&json_str).map_err(|e| WebsearchError::ParseFailed {
                parser: "duckduckgo_djs",
                source: Box::new(e),
            })?;

        let mut results = Vec::new();
        let mut n_token: Option<String> = None;

        for item in &items {
            if results.len() >= max_results {
                break;
            }

            // Track the "n" (next page token) field from items.
            // Validate to prevent SSRF: reject path traversal or protocol injection.
            if let Some(n_val) = item
                .get("n")
                .and_then(|v| v.as_str())
                .filter(|v| validate_n_token(v))
            {
                n_token = Some(n_val.to_string());
            }

            // Skip items without URL field
            let url_val = match item.get("c") {
                Some(v) if v.is_string() => v.as_str().unwrap(),
                _ => continue,
            };

            // Extract title — skip items without a valid (non-empty) title
            let title_val = match item.get("t").and_then(|v| v.as_str()) {
                Some(t) if !t.is_empty() => t,
                _ => continue,
            };

            // Skip if title is "EOF" and URL contains "google"
            if title_val == "EOF" && url_val.contains("google") {
                continue;
            }

            // Skip items that are no-result indicators
            if item.get("s").is_none()
                && (title_val == "DEEP_ERROR_NO_RESULTS" || title_val == "DEEP_SIMPLE_NO_RESULTS")
            {
                continue;
            }

            // Validate URL
            let url_str = url_val.to_string();
            if url::Url::parse(&url_str).is_err() {
                debug!("DuckDuckGo: filtering out malformed URL: {}", url_str);
                continue;
            }

            // Extract snippet
            let snippet = item
                .get("a")
                .and_then(|v| v.as_str())
                .map(html_entity_decode);

            // Extract date
            let date = item
                .get("e")
                .and_then(|v| v.as_str())
                .and_then(parse_ddg_date);

            results.push(SearchResult {
                title: html_entity_decode(title_val),
                url: url_str,
                snippet,
                date,
            });
        }

        Ok((results, n_token))
    }
}

#[async_trait]
impl Engine for DuckDuckGoEngine {
    async fn search(
        &self,
        query: &str,
        params: SearchParams,
        continuation: Option<&dyn Continuation>,
    ) -> Result<SearchResponse, WebsearchError> {
        crate::parsers::validate_query(query)?;
        let start = Instant::now();
        let max_results = crate::parsers::parse_max_results(params.max_results)?;

        // Determine the d.js URL based on continuation
        let djs_url: String;
        let djs_start: Instant;

        match continuation {
            None => {
                // Step 1: Fetch the initial HTML page to get the d.js URL
                let search_url = Self::build_search_url(query, &params);
                debug!("DuckDuckGo: fetching initial page (query hidden, status=?, duration=?)");
                let response = self.crawler.get(&search_url)
                    .with_headers(vec![
                        ("User-Agent".into(), DDG_USER_AGENT.into()),
                        ("Accept".into(), "text/html,application/xhtml+xml,application/xml;q=0.9,image/avif,image/webp,*/*;q=0.8".into()),
                        ("Accept-Encoding".into(), "gzip".into()),
                        ("Accept-Language".into(), "en-US,en;q=0.5".into()),
                        ("DNT".into(), "1".into()),
                        ("Sec-GPC".into(), "1".into()),
                        ("Connection".into(), "keep-alive".into()),
                        ("Upgrade-Insecure-Requests".into(), "1".into()),
                        ("Sec-Fetch-Dest".into(), "document".into()),
                        ("Sec-Fetch-Mode".into(), "navigate".into()),
                        ("Sec-Fetch-Site".into(), "same-origin".into()),
                        ("Sec-Fetch-User".into(), "?1".into()),
                        ("Priority".into(), "u=0, i".into()),
                        ("TE".into(), "trailers".into()),
                    ])
                    .with_exponential_retry(1)
                    .send()
                    .await?;

                let status = response.status().as_u16();
                let duration = start.elapsed();
                debug!(
                    "DuckDuckGo: initial page status={}, duration={:?}",
                    status, duration
                );

                if !(200..300).contains(&status) {
                    if status == 429 {
                        warn!(
                            "DuckDuckGo: rate limited (429) - retry after {:?}",
                            response.headers().get("retry-after")
                        );
                        return Err(WebsearchError::HttpStatus {
                            code: status,
                            engine: "duckduckgo",
                        });
                    }
                    return Err(WebsearchError::HttpStatus {
                        code: status,
                        engine: "duckduckgo",
                    });
                }

                let body = response
                    .text()
                    .await
                    .map_err(|e| WebsearchError::ParseFailed {
                        parser: "duckduckgo_response_body",
                        source: Box::new(e),
                    })?;

                let djs_path = match Self::extract_djs_url(&body) {
                    Ok(path) => path,
                    Err(e) => {
                        // Log first 2KB on parse failure
                        let end = body.floor_char_boundary(body.len().min(2048));
                        debug!(
                            "DuckDuckGo: parse failure, first 2KB of response: {}",
                            sanitize_for_log(&body[..end])
                        );
                        return Err(e);
                    }
                };

                // Step 2: Fetch the d.js URL (use the signed URL as-is, don't modify)
                djs_url = Self::build_djs_url(&djs_path);
                djs_start = Instant::now();

                debug!("DuckDuckGo: fetching d.js (query hidden, status=?, duration=?)");
            }
            Some(c) => {
                let ddg_cont = c
                    .as_any()
                    .downcast_ref::<DuckDuckGoContinuation>()
                    .ok_or_else(|| WebsearchError::ContinuationTypeMismatch {
                        expected: "DuckDuckGoContinuation",
                        received: std::any::type_name_of_val(c),
                    })?;
                // Validate n_token to prevent SSRF via continuation injection.
                if !validate_n_token(&ddg_cont.n_token) {
                    return Err(WebsearchError::ContinuationInvalidValue {
                        reason: "n_token contains unsafe characters",
                    });
                }
                // Use the n_token as a direct URL path
                djs_url = Self::build_djs_url(&ddg_cont.n_token);
                djs_start = Instant::now();

                debug!(
                    "DuckDuckGo: fetching continuation d.js (query hidden, status=?, duration=?)"
                );
            }
        }

        let (results, n_token) = self.fetch_and_parse_djs(&djs_url, max_results).await?;

        let djs_duration = djs_start.elapsed();
        debug!("DuckDuckGo: d.js status=200, duration={:?}", djs_duration);

        // Build continuation from n_token
        let continuation: Option<Box<dyn Continuation>> = n_token.map(|token| {
            Box::new(DuckDuckGoContinuation { n_token: token }) as Box<dyn Continuation>
        });

        let total_duration = start.elapsed();
        debug!(
            "DuckDuckGo: search complete, {} results, duration={:?}",
            results.len(),
            total_duration
        );

        Ok(SearchResponse {
            results,
            continuation,
        })
    }
}

/// Extract JSON from a JavaScript string starting after `DDG.pageLayout.load('d',`.
///
/// The format is: `DDG.pageLayout.load('d', [...]);` — we need to extract
/// the JSON array `[...]` by finding matching brackets.
pub fn extract_json_from_js(s: &str) -> Result<String, WebsearchError> {
    let s = s.trim();

    // Find the first opening bracket
    let start = s.find('[').ok_or_else(|| WebsearchError::ParseFailed {
        parser: "duckduckgo_djs_json_extract",
        source: "no opening bracket found after load marker".into(),
    })?;

    // Find the matching closing bracket (respecting string boundaries)
    let mut depth = 0u32;
    let mut in_string = false;
    let mut escaped = false;
    let mut end = start;

    for (i, ch) in s[start..].char_indices() {
        if escaped {
            escaped = false;
            continue;
        }

        match ch {
            '\\' if in_string => escaped = true,
            '"' => in_string = !in_string,
            _ if in_string => {}
            '[' => depth += 1,
            ']' => {
                // Defense-in-depth: depth cannot be 0 here with the current
                // code structure because s.find('[') (line 512) ensures the
                // loop starts at the first '[', setting depth = 1. However,
                // this guard prevents a u32 underflow panic if a future
                // refactor changes the loop entry point.
                if depth == 0 {
                    return Err(WebsearchError::ParseFailed {
                        parser: "duckduckgo_djs_json_extract",
                        source: "unmatched closing bracket".into(),
                    });
                }
                depth -= 1;
                if depth == 0 {
                    end = start + i + 1;
                    break;
                }
            }
            _ => {}
        }
    }

    if depth != 0 {
        return Err(WebsearchError::ParseFailed {
            parser: "duckduckgo_djs_json_extract",
            source: "unmatched brackets in d.js response".into(),
        });
    }

    Ok(s[start..end].to_string())
}

/// Decode HTML entities in a string.
///
/// Uses a single-pass approach to prevent double-decoding (e.g., `&amp;lt;`
/// should decode to `&lt;`, not `<`).
pub fn html_entity_decode(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '&' {
            let entity: String = chars.by_ref().take_while(|&c| c != ';').collect();
            match entity.as_str() {
                "amp" => result.push('&'),
                "lt" => result.push('<'),
                "gt" => result.push('>'),
                "quot" => result.push('"'),
                "#39" | "#x27" => result.push('\''),
                _ => {
                    result.push('&');
                    result.push_str(&entity);
                    result.push(';');
                }
            }
        } else {
            result.push(c);
        }
    }
    result
}

/// Parse a DuckDuckGo date string (ISO format) into a Unix timestamp.
pub fn parse_ddg_date(s: &str) -> Option<i64> {
    let s = s.trim();

    // Try ISO 8601 with timezone: "2024-01-15T12:00:00+00:00" or "2024-01-15T12:00:00Z"
    if let Some(ts) = parse_iso_with_tz(s) {
        return Some(ts);
    }

    // Try naive ISO: "2024-01-15T12:00:00"
    if let Some(ts) = parse_naive_iso(s) {
        return Some(ts);
    }

    None
}

/// Parse ISO 8601 date-time with timezone.
pub fn parse_iso_with_tz(s: &str) -> Option<i64> {
    if s.len() < 20 {
        return None;
    }

    let year: i64 = s[0..4].parse().ok()?;
    let month: u32 = s[5..7].parse().ok()?;
    let day: u32 = s[8..10].parse().ok()?;
    let hour: u32 = s[11..13].parse().ok()?;
    let min: u32 = s[14..16].parse().ok()?;
    let sec: u32 = s[17..19].parse().ok()?;

    // Parse timezone offset using safe byte slicing (get() returns None on invalid boundaries)
    let offset_secs: i64 = if s.len() > 19 {
        let tz = &s[19..];
        if tz == "Z" || tz == "+00:00" || tz == "-00:00" {
            0
        } else if tz.len() >= 6 && (tz.starts_with('+') || tz.starts_with('-')) {
            let sign: i64 = if tz.starts_with('-') { -1 } else { 1 };
            let tz_hour: i64 = tz.get(1..3)?.parse().ok()?;
            let tz_min: i64 = tz.get(4..6)?.parse().ok()?;
            sign * (tz_hour * 3600 + tz_min * 60)
        } else {
            0
        }
    } else {
        0
    };

    unix_timestamp(year, month, day, hour, min, sec, offset_secs)
}

/// Parse naive ISO date-time (no timezone).
pub fn parse_naive_iso(s: &str) -> Option<i64> {
    if s.len() < 19 {
        return None;
    }
    let year: i64 = s[0..4].parse().ok()?;
    let month: u32 = s[5..7].parse().ok()?;
    let day: u32 = s[8..10].parse().ok()?;
    let hour: u32 = s[11..13].parse().ok()?;
    let min: u32 = s[14..16].parse().ok()?;
    let sec: u32 = s[17..19].parse().ok()?;

    unix_timestamp(year, month, day, hour, min, sec, 0)
}

/// Compute Unix timestamp from date components.
pub fn unix_timestamp(
    year: i64,
    month: u32,
    day: u32,
    hour: u32,
    min: u32,
    sec: u32,
    offset_secs: i64,
) -> Option<i64> {
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) || hour > 23 || min > 59 || sec > 59 {
        return None;
    }

    let days = days_since_epoch(year, month, day)?;
    let total =
        days as i64 * 86400 + hour as i64 * 3600 + min as i64 * 60 + sec as i64 - offset_secs;
    Some(total)
}

/// Calculate days since 1970-01-01.
fn days_since_epoch(year: i64, month: u32, day: u32) -> Option<u64> {
    let mut total_days = 0i64;

    // Add days for whole years
    for y in 1970..year {
        total_days += if is_leap_year(y) { 366 } else { 365 };
    }

    // Add days for months in the current year
    let month_days = [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    for (m, &days_in_month) in month_days
        .iter()
        .enumerate()
        .take((month as usize).saturating_sub(1))
    {
        total_days += days_in_month;
        if m == 1 && is_leap_year(year) {
            total_days += 1; // February in leap year
        }
    }

    // Add days in the current month
    total_days += day as i64 - 1;

    if total_days >= 0 {
        Some(total_days as u64)
    } else {
        None
    }
}

/// Check if a year is a leap year.
pub fn is_leap_year(year: i64) -> bool {
    (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0)
}
#[cfg(test)]
#[path = "../../tests/unit/engines/duckduckgo_test.rs"]
mod duckduckgo_test;
