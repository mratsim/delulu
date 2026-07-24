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
use std::time::Instant;
use tracing::{debug, warn};

use crate::engine::{Engine, SearchParams, SearchResult};
use crate::error::WebsearchError;
use crate::sanitize_for_log;

/// DuckDuckGo search engine implementation.
///
/// # Precondition
/// - The internal `RateLimitedCrawler` must be configured with QPS 1,
///   10s timeout, and 5MB max response size.
///
/// # Postcondition
/// - Returns `Ok(Vec<SearchResult>)` with up to `max_results` results.
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
const DDG_USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:151.0) Gecko/20100101 Firefox/151.0";

impl DuckDuckGoEngine {
    /// Create a new DuckDuckGo engine with the given crawler.
    pub fn new(crawler: RateLimitedCrawler) -> Self {
        Self { crawler }
    }

    /// Build the initial search URL with query parameters.
    fn build_search_url(query: &str, params: &SearchParams) -> String {
        let mut url = format!(
            "https://duckduckgo.com/?q={}",
            urlencoding::encode(query)
        );

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
    fn build_djs_url(djs_path: &str) -> String {
        if djs_path.starts_with("https://") {
            djs_path.to_string()
        } else {
            format!("https://links.duckduckgo.com{}", djs_path)
        }
    }

    /// Extract the deep_preload_link (d.js URL) from the initial HTML page.
    fn extract_djs_url(html: &str) -> Result<String, WebsearchError> {
        let document = Html::parse_document(html);

        // Check for captcha first
        let challenge_selector =
            Selector::parse("form#challenge-form").expect("valid selector");
        if document.select(&challenge_selector).next().is_some() {
            return Err(WebsearchError::AccessDenied);
        }

        // Extract deep_preload_link href
        let link_selector =
            Selector::parse("link#deep_preload_link").expect("valid selector");
        let link = document
            .select(&link_selector)
            .next()
            .ok_or_else(|| {
                WebsearchError::ParseFailed {
                    parser: "duckduckgo_deep_preload",
                    source: "no <link id='deep_preload_link'> found".into(),
                }
            })?;

        let href = link
            .value()
            .attr("href")
            .ok_or_else(|| {
                WebsearchError::MissingField {
                    field: "href",
                    engine: "duckduckgo",
                }
            })?;

        Ok(href.to_string())
    }

    /// Parse the d.js response to extract search results and next page token.
    fn parse_djs_response(
        body: &str,
        max_results: usize,
    ) -> Result<Vec<SearchResult>, WebsearchError> {
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
        let items: Vec<serde_json::Value> = serde_json::from_str(&json_str).map_err(|e| {
            WebsearchError::ParseFailed {
                parser: "duckduckgo_djs",
                source: Box::new(e),
            }
        })?;

        let mut results = Vec::new();

        for item in &items {
            if results.len() >= max_results {
                break;
            }

            // Skip items without URL field
            let url_val = match item.get("c") {
                Some(v) if v.is_string() => v.as_str().unwrap(),
                _ => continue,
            };

            // Skip if title is "EOF" and URL contains "google"
            let title_val = item.get("t").and_then(|v| v.as_str()).unwrap_or("");
            if title_val == "EOF" && url_val.contains("google") {
                continue;
            }

            // Skip items that are no-result indicators
            if item.get("s").is_none()
                && (title_val == "DEEP_ERROR_NO_RESULTS" || title_val == "DEEP_SIMPLE_NO_RESULTS")
            {
                continue;
            }

            // Extract title
            if title_val.is_empty() {
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

        Ok(results)
    }
}

#[async_trait]
impl Engine for DuckDuckGoEngine {
    async fn search(
        &self,
        query: &str,
        params: SearchParams,
    ) -> Result<Vec<SearchResult>, WebsearchError> {
        let start = Instant::now();
        let max_results = params
            .max_results
            .map(|m| m.min(100) as usize)
            .unwrap_or(20);

        // Step 1: Fetch the initial HTML page to get the d.js URL
        let search_url = Self::build_search_url(query, &params);
        debug!(
            "DuckDuckGo: fetching initial page (query hidden, status=?, duration=?)"
        );
        let headers = vec![
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
        ];
        let response = self.crawler.get(&search_url)
            .with_headers(headers)
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
                warn!("DuckDuckGo: rate limited (429) - retry after {:?}", response.headers().get("retry-after"));
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

        let body = response.text().await.map_err(|e| {
            WebsearchError::ParseFailed {
                parser: "duckduckgo_response_body",
                source: Box::new(e),
            }
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
        let djs_url = Self::build_djs_url(&djs_path);
        let djs_start = Instant::now();

        debug!(
            "DuckDuckGo: fetching d.js (query hidden, status=?, duration=?)"
        );
        let djs_headers = vec![
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
        ];
        let djs_response = self.crawler.get(&djs_url)
            .with_headers(djs_headers)
            .send()
            .await?;

        let djs_status = djs_response.status().as_u16();
        let djs_duration = djs_start.elapsed();
        debug!(
            "DuckDuckGo: d.js status={}, duration={:?}",
            djs_status, djs_duration
        );

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

        let djs_body = djs_response.text().await.map_err(|e| {
            WebsearchError::ParseFailed {
                parser: "duckduckgo_djs_body",
                source: Box::new(e),
            }
        })?;

        // Parse the d.js response
        match Self::parse_djs_response(&djs_body, max_results) {
            Ok(results) => {
                let total_duration = start.elapsed();
                debug!(
                    "DuckDuckGo: search complete, {} results, duration={:?}",
                    results.len(),
                    total_duration
                );
                Ok(results)
            }
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
}

/// Extract JSON from a JavaScript string starting after `DDG.pageLayout.load('d',`.
///
/// The format is: `DDG.pageLayout.load('d', [...]);` — we need to extract
/// the JSON array `[...]` by finding matching brackets.
fn extract_json_from_js(s: &str) -> Result<String, WebsearchError> {
    let s = s.trim();

    // Find the first opening bracket
    let start = s.find('[').ok_or_else(|| {
        WebsearchError::ParseFailed {
            parser: "duckduckgo_djs_json_extract",
            source: "no opening bracket found after load marker".into(),
        }
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
fn html_entity_decode(s: &str) -> String {
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
fn parse_ddg_date(s: &str) -> Option<i64> {
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
fn parse_iso_with_tz(s: &str) -> Option<i64> {
    if s.len() < 20 {
        return None;
    }

    let year: i64 = s[0..4].parse().ok()?;
    let month: u32 = s[5..7].parse().ok()?;
    let day: u32 = s[8..10].parse().ok()?;
    let hour: u32 = s[11..13].parse().ok()?;
    let min: u32 = s[14..16].parse().ok()?;
    let sec: u32 = s[17..19].parse().ok()?;

    // Parse timezone offset
    let offset_secs: i64 = if s.len() > 19 {
        let tz = &s[19..];
        if tz == "Z" || tz == "+00:00" || tz == "-00:00" {
            0
        } else if tz.len() >= 6 && (tz.starts_with('+') || tz.starts_with('-')) {
            let sign: i64 = if tz.starts_with('-') { -1 } else { 1 };
            let tz_hour: i64 = tz[1..3].parse().ok()?;
            let tz_min: i64 = tz[4..6].parse().ok()?;
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
fn parse_naive_iso(s: &str) -> Option<i64> {
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
fn unix_timestamp(
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
    for (m, &days_in_month) in month_days.iter().enumerate().take((month as usize).saturating_sub(1)) {
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
fn is_leap_year(year: i64) -> bool {
    (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn engine_is_send_sync() {
        fn assert_send<T: Send>() {}
        fn assert_sync<T: Sync>() {}
        assert_send::<DuckDuckGoEngine>();
        assert_sync::<DuckDuckGoEngine>();
    }

    #[test]
    fn extract_json_from_js_works() {
        let after_marker = " [{\"c\":\"https://example.com\",\"t\":\"Test\"}]);";
        let json = extract_json_from_js(after_marker).unwrap();
        assert_eq!(json, "[{\"c\":\"https://example.com\",\"t\":\"Test\"}]");
    }

    #[test]
    fn extract_json_from_js_nested() {
        let after_marker = " [[{\"a\":[1,2,3]}]]);";
        let json = extract_json_from_js(after_marker).unwrap();
        assert_eq!(json, "[[{\"a\":[1,2,3]}]]");
    }

    #[test]
    fn extract_json_from_js_no_bracket() {
        let result = extract_json_from_js("no brackets here");
        assert!(result.is_err());
    }

    #[test]
    fn extract_json_from_js_unmatched() {
        let result = extract_json_from_js(" [[");
        assert!(result.is_err());
    }

    #[test]
    fn build_search_url_basic() {
        let params = SearchParams::default();
        let url = DuckDuckGoEngine::build_search_url("rust", &params);
        assert!(url.contains("q=rust"));
        assert!(url.contains("kl=wt-wt"));
    }

    #[test]
    fn build_search_url_with_safesearch() {
        let params = SearchParams {
            safesearch: Some("strict".into()),
            ..Default::default()
        };
        let url = DuckDuckGoEngine::build_search_url("rust", &params);
        assert!(url.contains("kp=1"));
    }

    #[test]
    fn build_search_url_with_safesearch_off() {
        let params = SearchParams {
            safesearch: Some("off".into()),
            ..Default::default()
        };
        let url = DuckDuckGoEngine::build_search_url("rust", &params);
        assert!(url.contains("kp=-2"));
    }

    #[test]
    fn build_search_url_with_country() {
        let params = SearchParams {
            country: Some("de-de".into()),
            ..Default::default()
        };
        let url = DuckDuckGoEngine::build_search_url("rust", &params);
        assert!(url.contains("kl=de-de"));
    }

    #[test]
    fn build_search_url_with_time_range() {
        let params = SearchParams {
            time_range: Some("2024-01-01..2024-12-31".into()),
            ..Default::default()
        };
        let url = DuckDuckGoEngine::build_search_url("rust", &params);
        assert!(url.contains("df=2024-01-01..2024-12-31"));
    }

    #[test]
    fn parse_djs_response_basic() {
        let body = r#"DDG.pageLayout.load('d', [{"c":"https://example.com","t":"Example","a":"An example site"},{"c":"https://test.org","t":"Test","a":"A test site","e":"2024-01-15T12:00:00"}]);"#;
        let results = DuckDuckGoEngine::parse_djs_response(body, 10).unwrap();
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].title, "Example");
        assert_eq!(results[0].url, "https://example.com");
        assert_eq!(results[0].snippet.as_deref(), Some("An example site"));
        assert_eq!(results[1].title, "Test");
        assert_eq!(results[1].url, "https://test.org");
        assert!(results[1].date.is_some());
    }

    #[test]
    fn parse_djs_response_jsa_challenge() {
        let body = "DDG.deep.initialize('some_token' + jsa";
        let result = DuckDuckGoEngine::parse_djs_response(body, 10);
        assert!(matches!(result, Err(WebsearchError::AccessDenied)));
    }

    #[test]
    fn parse_djs_response_anomaly() {
        let body = "DDG.deep.anomalyDetectionBlock({";
        let result = DuckDuckGoEngine::parse_djs_response(body, 10);
        assert!(matches!(result, Err(WebsearchError::AccessDenied)));
    }

    #[test]
    fn parse_djs_response_no_results() {
        let body = r#"DDG.pageLayout.load('d', [{"c":"","t":"DEEP_ERROR_NO_RESULTS"}]);"#;
        let results = DuckDuckGoEngine::parse_djs_response(body, 10).unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn parse_djs_response_filters_malformed_urls() {
        let body = r#"DDG.pageLayout.load('d', [{"c":"not-a-valid-url","t":"Bad URL","a":"test"}]);"#;
        let results = DuckDuckGoEngine::parse_djs_response(body, 10).unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn html_entity_decode_works() {
        assert_eq!(html_entity_decode("Tom &amp; Jerry"), "Tom & Jerry");
        assert_eq!(html_entity_decode("&lt;script&gt;"), "<script>");
    }

    #[test]
    fn html_entity_decode_no_double_decode() {
        // &amp;lt; should decode to &lt;, not <
        assert_eq!(html_entity_decode("&amp;lt;"), "&lt;");
    }

    #[test]
    fn parse_ddg_date_rfc3339() {
        let ts = parse_ddg_date("2024-01-15T12:00:00+00:00");
        assert!(ts.is_some(), "RFC 3339 should parse");
        assert_eq!(ts.unwrap(), 1705320000);
    }

    #[test]
    fn parse_ddg_date_utc_z() {
        let ts = parse_ddg_date("2024-01-15T12:00:00Z");
        assert!(ts.is_some(), "UTC Z should parse");
        assert_eq!(ts.unwrap(), 1705320000);
    }

    #[test]
    fn parse_ddg_date_naive() {
        let ts = parse_ddg_date("2024-01-15T12:00:00");
        assert!(ts.is_some(), "naive should parse");
        assert_eq!(ts.unwrap(), 1705320000);
    }

    #[test]
    fn parse_ddg_date_invalid() {
        let ts = parse_ddg_date("not a date");
        assert!(ts.is_none());
    }

    #[test]
    fn is_leap_year_divisible_by_4() {
        assert!(is_leap_year(2024));
    }

    #[test]
    fn is_leap_year_century() {
        assert!(!is_leap_year(1900));
    }

    #[test]
    fn is_leap_year_400() {
        assert!(is_leap_year(2000));
    }

    // ---- build_djs_url tests ----

    #[test]
    fn build_djs_url_path_only() {
        let url = DuckDuckGoEngine::build_djs_url("/d.js?q=test&vqd=abc");
        assert_eq!(url, "https://links.duckduckgo.com/d.js?q=test&vqd=abc");
    }

    #[test]
    fn build_djs_url_full_url() {
        let url = DuckDuckGoEngine::build_djs_url("https://links.duckduckgo.com/d.js?q=test&vqd=abc");
        assert_eq!(url, "https://links.duckduckgo.com/d.js?q=test&vqd=abc");
    }

    // ---- NEW TESTS for extract_djs_url ----

    #[test]
    fn extract_djs_url_success() {
        let html = r#"<html><head><link id="deep_preload_link" href="/d.js?t=test"/></head></html>"#;
        let result = DuckDuckGoEngine::extract_djs_url(html).unwrap();
        assert_eq!(result, "/d.js?t=test");
    }

    #[test]
    fn extract_djs_url_captcha_detected() {
        let html = r#"<html><body><form id="challenge-form"><input type="text"/></form></body></html>"#;
        let result = DuckDuckGoEngine::extract_djs_url(html);
        assert!(matches!(result, Err(WebsearchError::AccessDenied)));
    }

    #[test]
    fn extract_djs_url_missing_link() {
        let html = r#"<html><head></head><body>no link here</body></html>"#;
        let result = DuckDuckGoEngine::extract_djs_url(html);
        assert!(matches!(result, Err(WebsearchError::ParseFailed { .. })));
    }

    #[test]
    fn extract_djs_url_missing_href() {
        let html = r#"<html><head><link id="deep_preload_link" rel="preload"/></head></html>"#;
        let result = DuckDuckGoEngine::extract_djs_url(html);
        assert!(matches!(result, Err(WebsearchError::MissingField { .. })));
    }
}