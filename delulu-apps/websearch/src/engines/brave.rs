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
use serde::{Deserialize, Serialize};
use std::any::Any;
use std::time::Instant;
use tracing::{debug, warn};

use crate::engine::{Continuation, Engine, SearchParams, SearchResponse, SearchResult};
use crate::error::WebsearchError;

/// Case-insensitive contains check — zero allocation.
/// Scans the haystack bytes without copying or allocating.
fn contains_ignore_ascii_case(haystack: &str, needle: &str) -> bool {
    let haystack = haystack.as_bytes();
    let needle = needle.as_bytes();
    if needle.is_empty() {
        return true;
    }
    haystack.windows(needle.len()).any(|w| {
        w.iter()
            .zip(needle.iter())
            .all(|(a, b)| a.eq_ignore_ascii_case(b))
    })
}

/// Brave search engine implementation.
///
/// # Precondition
/// - The internal `RateLimitedCrawler` must be configured with QPS 2,
///   10s timeout, and 5MB max response size.
///
/// # Postcondition
/// - Returns `Ok(SearchResponse)` with up to `max_results` results.
/// - Returns `Err(WebsearchError::AccessDenied)` if a PoW captcha is detected.
/// - Returns `Err(WebsearchError::ParseFailed)` if the response cannot
///   be parsed.
///
/// # Panic-if
/// - This function MUST NOT panic. All error paths return Err.
pub struct BraveEngine {
    crawler: RateLimitedCrawler,
}

/// Continuation token for Brave pagination.
/// Brave uses a simple page-number-based pagination scheme.
/// The page number is 1-indexed and capped at 999.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BraveContinuation {
    /// The next page number to fetch (1-indexed).
    pub page: u32,
}

impl Continuation for BraveContinuation {
    fn as_any(&self) -> &dyn Any {
        self
    }
}

impl BraveEngine {
    /// Create a new Brave engine with the given crawler.
    pub fn new(crawler: RateLimitedCrawler) -> Self {
        Self { crawler }
    }

    /// Build the search URL with query parameters.
    pub fn build_search_url(query: &str, params: &SearchParams, page: u32) -> String {
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
    pub fn build_cookie_header(params: &SearchParams) -> Result<String, WebsearchError> {
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
        continuation: Option<&dyn Continuation>,
    ) -> Result<SearchResponse, WebsearchError> {
        crate::parsers::validate_query(query)?;
        // TODO side-effect to push to main: Instant::now() timing (logs only)
        let start = Instant::now();
        let max_results = crate::parsers::parse_max_results(params.max_results)?;

        // Determine the page number from continuation or params
        let page = match continuation {
            None => crate::parsers::parse_page(params.page)?,
            Some(c) => {
                let brave_cont =
                    c.as_any()
                        .downcast_ref::<BraveContinuation>()
                        .ok_or_else(|| WebsearchError::ContinuationTypeMismatch {
                            expected: "BraveContinuation",
                            received: std::any::type_name_of_val(c),
                        })?;
                if brave_cont.page == 0 {
                    return Err(WebsearchError::ContinuationInvalidValue {
                        reason: "Brave page cannot be 0",
                    });
                }
                brave_cont.page
            }
        };

        // Check if we've reached the page limit
        if page >= 1000 {
            return Ok(SearchResponse {
                results: Vec::new(),
                continuation: None,
            });
        }

        // Build URL and cookie header
        let search_url = Self::build_search_url(query, &params, page);
        let cookie = Self::build_cookie_header(&params)?;

        debug!("Brave: fetching search page (query hidden, status=?, duration=?)");

        let response = self
            .crawler
            .get(&search_url)
            .merge_with_headers(vec![("Cookie".into(), cookie)])
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
            }
            return Err(WebsearchError::HttpStatus {
                code: status,
                engine: "brave",
            });
        }

        let body = response
            .text()
            .await
            .map_err(|e| WebsearchError::ParseFailed {
                parser: "brave_response_body",
                source: Box::new(e),
            })?;
        // Parse HTML for search results
        let results = parse_search_results(&body, max_results)?;

        // Only return continuation if there are results (no point paginating empty sets)
        let next_page = page + 1;
        let continuation = if results.is_empty() || next_page >= 1000 {
            None
        } else {
            Some(Box::new(BraveContinuation { page: next_page }) as Box<dyn Continuation>)
        };

        Ok(SearchResponse {
            results,
            continuation,
        })
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
        if results.len() >= max_results {
            break;
        }
        // Extract URL from the first <a> href
        let url = snippet
            .select(&url_selector)
            .next()
            .and_then(|a| a.value().attr("href").map(|s| s.to_string()))
            .unwrap_or_default();
        if url.is_empty() {
            continue;
        }
        // Skip Brave-internal navigation links (search.brave.com),
        // but NOT legitimate external results (brave.com, brave.blog, etc.)
        if url::Url::parse(&url)
            .map(|u| u.host_str() == Some("search.brave.com"))
            .unwrap_or(false)
        {
            continue;
        }

        // Extract title from <div class="title">
        let title = snippet
            .select(&title_selector)
            .next()
            .map(|t| t.text().collect::<Vec<_>>().join(" ").trim().to_string())
            .unwrap_or_default();
        if title.is_empty() {
            continue;
        }

        // Extract snippet from <div class="content">
        let content_elem = snippet.select(&content_selector).next();

        // Extract date from <span class="t-secondary"> inside content
        let (date, snippet_text) = if let Some(content) = &content_elem {
            let date_str = content
                .select(&date_selector)
                .next()
                .map(|d| d.text().collect::<String>().trim().to_string());
            let raw_text = content
                .text()
                .collect::<Vec<_>>()
                .join(" ")
                .trim()
                .to_string();
            let (cleaned, parsed_date) = strip_date_prefix(raw_text, date_str);
            (parsed_date, Some(cleaned))
        } else {
            (None, None)
        };

        results.push(SearchResult {
            title,
            url,
            snippet: snippet_text,
            date,
        });
    }

    // Check for PoW captcha regardless of whether results were found.
    // A captcha page may contain HTML elements matching div.snippet selectors,
    // producing zero results without triggering the empty-results path.
    // Only check for "pow captcha" (not generic "captcha") to avoid false
    // positives from Brave's built-in i18n translation dictionary.
    if contains_ignore_ascii_case(body, "pow captcha") {
        return Err(WebsearchError::AccessDenied);
    }

    Ok(results)
}

/// Extract a date string from Brave's HTML and clean it from the snippet text.
///
/// Brave renders dates inside `<span class="t-secondary">` inside the content div.
/// The span text includes a trailing " -" separator (e.g. "January 7, 2025 -").
pub(crate) fn strip_date_prefix(
    raw_text: String,
    date_str: Option<String>,
) -> (String, Option<i64>) {
    let raw = match date_str {
        Some(ref d) if !d.is_empty() => d.trim().to_string(),
        _ => return (raw_text, None),
    };

    // Strip trailing " -" separator from the date span text
    let clean_date = raw.trim_end_matches(['-', ' ']).trim().to_string();

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
#[path = "../../tests/unit/engines/brave_test.rs"]
mod brave_test;
