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

//! Brave search engine backend.
//!
//! Implements HTML-based search against `https://search.brave.com/search`.
//! Parses the embedded JavaScript data object (`kit.start(...)`) from
//! `<script>` tags to extract search results.
//!

use async_trait::async_trait;
use chrono::NaiveDate;
use delulu_rate_limited_crawler::RateLimitedCrawler;
use std::time::Instant;
use tracing::{debug, warn};

use crate::engine::{DEFAULT_USER_AGENT, Engine, SearchParams, SearchResult};
use crate::error::WebsearchError;

/// Brave search engine implementation.
///
/// # Precondition
/// - The internal `RateLimitedCrawler` must be configured with QPS 2,
///   10s timeout, and 5MB max response size.
///
/// # Postcondition
/// - Returns `Ok(Vec<SearchResult>)` with up to `max_results` results.
/// - Returns `Err(WebsearchError::AccessDenied)` if a PoW captcha is detected.
/// - Returns `Err(WebsearchError::ParseFailed)` if the response cannot
///   be parsed.
///
/// # Panic-if
/// - This function MUST NOT panic. All error paths return Err.
pub struct BraveEngine {
    crawler: RateLimitedCrawler,
}

/// Brave User-Agent constant.
const BRAVE_USER_AGENT: &str = DEFAULT_USER_AGENT;

impl BraveEngine {
    /// Create a new Brave engine with the given crawler.
    pub fn new(crawler: RateLimitedCrawler) -> Self {
        Self { crawler }
    }

    /// Build the search URL with query parameters.
    fn build_search_url(query: &str, params: &SearchParams, page: u32) -> String {
        let offset = page.saturating_sub(1);
        let mut url = format!(
            "https://search.brave.com/search?q={}&source=web",
            urlencoding::encode(query)
        );

        if offset > 0 {
            url.push_str(&format!("&offset={}", offset));
        }

        // Time range parameter (tf)
        if let Some(ref time_range) = params.time_range {
            url.push_str("&tf=");
            url.push_str(&urlencoding::encode(time_range));
        }

        // Spellcheck (disabled by default)
        url.push_str("&spellcheck=0");

        url
    }

    /// Build the cookie header for Brave requests.
    /// Validates safesearch and country via parsers to prevent cookie injection.
    fn build_cookie_header(params: &SearchParams) -> Result<String, WebsearchError> {
        let safesearch = crate::parsers::parse_safesearch(params.safesearch.as_deref())?;
        let country = crate::parsers::parse_country(params.country.as_deref())?;
        Ok(format!(
            "safesearch={}; country={}; useLocation=0; summarizer=0",
            safesearch, country,
        ))
    }
}


#[async_trait]
impl Engine for BraveEngine {
    async fn search(
        &self,
        query: &str,
        params: SearchParams,
    ) -> Result<Vec<SearchResult>, WebsearchError> {
        crate::parsers::validate_query(query)?;
        let start = Instant::now();
        let max_results = crate::parsers::parse_max_results(params.max_results)?;
        let page = crate::parsers::parse_page(params.page)?;
        // Build URL and cookie header
        let search_url = Self::build_search_url(query, &params, page);
        let cookie = Self::build_cookie_header(&params)?;

        debug!("Brave: fetching search page (query hidden, status=?, duration=?)");

        let response = self
            .crawler
            .get(&search_url)
            .with_headers(vec![
                ("User-Agent".into(), BRAVE_USER_AGENT.into()),
                ("Cookie".into(), cookie),
            ])
            .with_exponential_retry(1)
            .send()
            .await?;

        let status = response.status().as_u16();
        let duration = start.elapsed();
        debug!(
            "Brave: search page status={}, duration={:?}",
            status, duration
        );

        if !(200..300).contains(&status) {
            if status == 429 {
                warn!(
                    "Brave: rate limited (429) - retry after {:?}",
                    response.headers().get("retry-after")
                );
                return Err(WebsearchError::HttpStatus {
                    code: status,
                    engine: "brave",
                });
            }
            return Err(WebsearchError::HttpStatus {
                code: status,
                engine: "brave",
            });
        }

        let body = response.text().await.map_err(|e| {
            WebsearchError::ParseFailed {
                parser: "brave_response_body",
                source: Box::new(e),
            }
        })?;
        // Parse HTML for search results
        let results = parse_search_results(&body, max_results)?;

        Ok(results)
    }
}


/// Parse Brave search results HTML into structured results.
///
/// Extracts URL, title, snippet, and date from Brave's `<div class="snippet">` elements.
/// Returns `AccessDenied` if the page contains a PoW captcha challenge.
pub fn parse_search_results(
    body: &str,
    max_results: usize,
) -> Result<Vec<SearchResult>, WebsearchError> {
    let document = scraper::Html::parse_document(body);
    let snippet_selector = scraper::Selector::parse("div.snippet").expect("valid selector");
    let url_selector = scraper::Selector::parse("a[href]").expect("valid selector");
    let title_selector = scraper::Selector::parse("div.title").expect("valid selector");
    let content_selector = scraper::Selector::parse("div.content").expect("valid selector");
    let date_selector = scraper::Selector::parse("span.t-secondary").expect("valid selector");

    let mut results = Vec::new();
    for snippet in document.select(&snippet_selector) {
        // Extract URL from the first <a> href
        let url = snippet.select(&url_selector).next()
            .and_then(|a| a.value().attr("href").map(|s| s.to_string()))
            .unwrap_or_default();
        if url.is_empty() {
            continue;
        }
        // Skip Brave-internal navigation links (search.brave.com),
        // but NOT legitimate external results (brave.com, brave.blog, etc.)
        if url::Url::parse(&url).map(|u| u.host_str() == Some("search.brave.com")).unwrap_or(false) {
            continue;
        }

        // Extract title from <div class="title">
        let title = snippet.select(&title_selector).next()
            .map(|t| t.text().collect::<Vec<_>>().join(" ").trim().to_string())
            .unwrap_or_default();
        if title.is_empty() {
            continue;
        }

        // Extract snippet from <div class="content">
        let content_elem = snippet.select(&content_selector).next();

        // Extract date from <span class="t-secondary"> inside content
        let (date, snippet_text) = if let Some(content) = &content_elem {
            let date_str = content.select(&date_selector).next()
                .map(|d| d.text().collect::<String>().trim().to_string());
            let raw_text = content.text().collect::<Vec<_>>().join(" ").trim().to_string();
            let (cleaned, parsed_date) = strip_date_prefix(raw_text, date_str);
            (parsed_date, Some(cleaned))
        } else {
            (None, None)
        };

        if results.len() >= max_results {
            break;
        }

        results.push(SearchResult {
            title,
            url,
            snippet: snippet_text,
            date,
        });
    }

    // If HTML parsing found nothing, check for PoW captcha
    if results.is_empty() {
        if body.to_lowercase().contains("pow captcha") || body.to_lowercase().contains("captcha") {
            return Err(WebsearchError::AccessDenied);
        }
    }

    Ok(results)
}

/// Extract a date string from Brave's HTML and clean it from the snippet text.
///
/// Brave renders dates inside `<span class="t-secondary">` inside the content div.
/// The span text includes a trailing " -" separator (e.g. "January 7, 2025 -").
fn strip_date_prefix(raw_text: String, date_str: Option<String>) -> (String, Option<i64>) {
    let raw = match date_str {
        Some(ref d) if !d.is_empty() => d.trim().to_string(),
        _ => return (raw_text, None),
    };

    // Strip trailing " -" separator from the date span text
    let clean_date = raw
        .trim_end_matches(|c: char| c == '-' || c == ' ')
        .trim()
        .to_string();

    // Try parsing "Month Day, Year" format (e.g. "January 7, 2025")
    let parsed = NaiveDate::parse_from_str(&clean_date, "%B %d, %Y")
        .or_else(|_| NaiveDate::parse_from_str(&clean_date, "%B %e, %Y"))
        .ok()
        .and_then(|d| d.and_hms_opt(0, 0, 0))
        .map(|dt| dt.and_utc().timestamp());

    // Strip the date prefix from the snippet text
    // The raw text is "January 7, 2025 - actual content..."
    let cleaned = match raw_text.strip_prefix(&raw) {
        Some(rest) => rest.trim().trim_start_matches('-').trim().to_string(),
        None => raw_text,
    };

    (cleaned, parsed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn engine_is_send_sync() {
        fn assert_send<T: Send>() {}
        fn assert_sync<T: Sync>() {}
        assert_send::<BraveEngine>();
        assert_sync::<BraveEngine>();
    }

    #[test]
    fn build_search_url_basic() {
        let params = SearchParams::default();
        let url = BraveEngine::build_search_url("rust", &params, 1);
        assert!(url.contains("q=rust"));
        assert!(url.contains("source=web"));
        assert!(!url.contains("offset"));
    }

    #[test]
    fn build_search_url_page_2() {
        let params = SearchParams::default();
        let url = BraveEngine::build_search_url("rust", &params, 2);
        assert!(url.contains("offset=1"));
    }

    #[test]
    fn build_search_url_page_3() {
        let params = SearchParams::default();
        let url = BraveEngine::build_search_url("rust", &params, 3);
        assert!(url.contains("offset=2"));
    }

    #[test]
    fn build_cookie_header_default() {
        let params = SearchParams::default();
        let cookie = BraveEngine::build_cookie_header(&params).unwrap();
        assert!(cookie.contains("safesearch=moderate"));
        assert!(cookie.contains("country=all"));
        assert!(cookie.contains("useLocation=0"));
        assert!(cookie.contains("summarizer=0"));
    }

    #[test]
    fn build_cookie_header_with_params() {
        let params = SearchParams {
            safesearch: Some("strict".into()),
            country: Some("us".into()),
            ..Default::default()
        };
        let cookie = BraveEngine::build_cookie_header(&params).unwrap();
        assert!(cookie.contains("safesearch=strict"));
        assert!(cookie.contains("country=us"));
    }

    #[test]
    fn strip_date_prefix_with_date() {
        let raw = "January 7, 2025 - The elliptic curve only hash algorithm.".to_string();
        let date_str = Some("January 7, 2025 -".to_string());
        let (cleaned, date) = strip_date_prefix(raw, date_str);
        assert_eq!(date, Some(1736208000));
        assert!(!cleaned.contains("January 7, 2025"));
        assert!(cleaned.starts_with("The elliptic"));
    }

    #[test]
    fn strip_date_prefix_no_date() {
        let raw = "We describe a new explicit function.".to_string();
        let (cleaned, date) = strip_date_prefix(raw.clone(), None);
        assert!(date.is_none());
        assert_eq!(cleaned, raw);
    }

    #[test]
    fn strip_date_prefix_empty_date_str() {
        let raw = "Some content here.".to_string();
        let date_str = Some("".to_string());
        let (cleaned, date) = strip_date_prefix(raw.clone(), date_str);
        assert!(date.is_none());
        assert_eq!(cleaned, raw);
    }

    #[test]
    fn strip_date_prefix_august_date() {
        let raw = "August 7, 2025 - ResearchGate publication.".to_string();
        let date_str = Some("August 7, 2025 -".to_string());
        let (cleaned, date) = strip_date_prefix(raw, date_str);
        assert_eq!(date, Some(1754524800));
        assert!(!cleaned.contains("August 7, 2025"));
    }

    #[test]
    fn strip_date_prefix_no_dash_in_date() {
        let raw = "September 4, 2020 - Ethereum research post.".to_string();
        let date_str = Some("September 4, 2020".to_string());
        let (cleaned, date) = strip_date_prefix(raw, date_str);
        assert_eq!(date, Some(1599177600));
        assert!(!cleaned.contains("September 4, 2020"));
    }

}