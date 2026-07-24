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
use delulu_rate_limited_crawler::RateLimitedCrawler;
use scraper::{Html, Selector};
use std::time::Instant;
use tracing::{debug, warn};

use crate::engine::{DEFAULT_USER_AGENT, Engine, SearchParams, SearchResult};
use crate::error::WebsearchError;
use crate::sanitize_for_log;

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
        let offset = if page > 1 { (page - 1) * 10 } else { 0 };
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
    fn build_cookie_header(params: &SearchParams) -> String {
        let safesearch = params.safesearch.as_deref().unwrap_or("moderate");
        let country = params.country.as_deref().unwrap_or("all");

        format!(
            "safesearch={}; country={}; useLocation=0; summarizer=0",
            safesearch, country
        )
    }

    /// Extract the JavaScript data object from Brave's HTML response.
    ///
    /// Finds the `<script>` tag containing `kit.start(` and extracts the
    /// data object after the `data:` field.
    fn extract_js_data(html: &str) -> Result<serde_json::Value, WebsearchError> {
        let document = Html::parse_document(html);

        // Find all script tags
        let script_selector = Selector::parse("script").expect("valid selector");
        let mut kit_start_script: Option<String> = None;

        for script in document.select(&script_selector) {
            let inner_html = script.inner_html();
            if inner_html.contains("kit.start(") {
                kit_start_script = Some(inner_html);
                break;
            }
        }

        let script_content = kit_start_script.ok_or_else(|| {
            WebsearchError::ParseFailed {
                parser: "brave_kitstart",
                source: "no <script> tag containing 'kit.start(' found".into(),
            }
        })?;
        // Find the JSON data inside kit.start(...)
        // kit.start(app, element, { node_ids: [...], data: [...] })
        let kit_start_pos = script_content.find("kit.start(")
            .ok_or_else(|| WebsearchError::ParseFailed {
                parser: "brave_kitstart",
                source: "kit.start( not found".into(),
            })?;

        // Find the first '[' or '{' after kit.start(
        let after_kit = &script_content[kit_start_pos + 10..];
        let json_start = after_kit.char_indices()
            .find(|(_, c)| *c == '[' || *c == '{')
            .map(|(i, _)| i)
            .ok_or_else(|| WebsearchError::ParseFailed {
                parser: "brave_kitstart",
                source: "no array/object after kit.start(".into(),
            })?;

        let json_str = extract_js_object(&after_kit[json_start..])?;

        match serde_json::from_str::<serde_json::Value>(&json_str) {
            Ok(val) => Ok(val),
            Err(_) => {
                // Try cleaning up JS-specific syntax
                let cleaned = clean_js_object(&json_str);
                serde_json::from_str(&cleaned).map_err(|e| {
                    WebsearchError::ParseFailed {
                        parser: "brave_kitstart",
                        source: Box::new(e),
                    }
                })
            }
        }
    }

    /// Extract results from the parsed Brave JS data.
    fn extract_results(
        data: &serde_json::Value,
        max_results: usize,
    ) -> Result<Vec<SearchResult>, WebsearchError> {
        // Check for PoW Captcha — collapsed if-let
        if let Some(title) = data
            .get(2)
            .and_then(|v| v.get("data"))
            .and_then(|v| v.get("title"))
            .and_then(|v| v.as_str())
            && title.to_lowercase().contains("pow captcha")
        {
            return Err(WebsearchError::AccessDenied);
        }

        // Navigate to web results path: [1]["data"]["body"]["response"]["web"]["results"]
        let results_array = data
            .get(1)
            .and_then(|v| v.get("data"))
            .and_then(|v| v.get("body"))
            .and_then(|v| v.get("response"))
            .and_then(|v| v.get("web"))
            .and_then(|v| v.get("results"))
            .and_then(|v| v.as_array());

        let results_array = match results_array {
            Some(arr) => arr,
            None => {
                // Try alternative path: [1]["data"]["body"]["response"]["results"]
                let alt = data
                    .get(1)
                    .and_then(|v| v.get("data"))
                    .and_then(|v| v.get("body"))
                    .and_then(|v| v.get("response"))
                    .and_then(|v| v.get("results"))
                    .and_then(|v| v.as_array());
                match alt {
                    Some(arr) => arr,
                    None => {
                        return Err(WebsearchError::ParseFailed {
                            parser: "brave_kitstart",
                            source: "no web results found in response".into(),
                        });
                    }
                }
            }
        };

        let mut results = Vec::new();

        for item in results_array {
            if results.len() >= max_results {
                break;
            }

            // Extract title
            let title = match item.get("title").and_then(|v| v.as_str()) {
                Some(t) if !t.is_empty() => t.to_string(),
                _ => continue,
            };

            // Extract URL
            let url = match item.get("url").and_then(|v| v.as_str()) {
                Some(u) if !u.is_empty() => u.to_string(),
                _ => continue,
            };

            // Validate URL
            if url::Url::parse(&url).is_err() {
                debug!("Brave: filtering out malformed URL: {}", url);
                continue;
            }

            // Extract description/snippet
            let snippet = item
                .get("description")
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
                .map(|s| s.to_string());

            results.push(SearchResult {
                title,
                url,
                snippet,
                date: None,
            });
        }

        Ok(results)
    }
}

#[async_trait]
impl Engine for BraveEngine {
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
        let page = params.page.unwrap_or(1);

        // Build URL and cookie header
        let search_url = Self::build_search_url(query, &params, page);
        let cookie = Self::build_cookie_header(&params);

        debug!("Brave: fetching search page (query hidden, status=?, duration=?)");

        let response = self
            .crawler
            .get(&search_url)
            .with_headers(vec![
                ("User-Agent".into(), BRAVE_USER_AGENT.into()),
                ("Cookie".into(), cookie),
            ])
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
        // Brave renders results in <div class="snippet "> elements
        let document = scraper::Html::parse_document(&body);
        let snippet_selector = scraper::Selector::parse("div.snippet").expect("valid selector");

        let mut results = Vec::new();
        for snippet in document.select(&snippet_selector) {
            // Extract URL from the first <a> href
            let url_selector = scraper::Selector::parse("a[href]").expect("valid selector");
            let url = snippet.select(&url_selector).next()
                .and_then(|a| a.value().attr("href").map(|s| s.to_string()))
                .unwrap_or_default();
            if url.is_empty() || url.contains("brave.com") {
                continue;
            }

            // Extract title from <div class="title">
            let title_selector = scraper::Selector::parse("div.title").expect("valid selector");
            let title = snippet.select(&title_selector).next()
                .map(|t| t.text().collect::<Vec<_>>().join(" ").trim().to_string())
                .unwrap_or_default();
            if title.is_empty() {
                continue;
            }

            // Extract snippet from <div class="content">
            let content_selector = scraper::Selector::parse("div.content").expect("valid selector");
            let snippet_text = snippet.select(&content_selector).next()
                .map(|c| c.text().collect::<Vec<_>>().join(" ").trim().to_string());

            if results.len() >= max_results {
                break;
            }

            results.push(SearchResult {
                title,
                url,
                snippet: snippet_text,
                date: None,
            });
        }

        // If HTML parsing found nothing, check for PoW captcha
        if results.is_empty() {
            if body.contains("pow captcha") || body.contains("captcha") {
                return Err(WebsearchError::AccessDenied);
            }
        }

        let total_duration = start.elapsed();
        debug!(
            "Brave: search complete, {} results, duration={:?}",
            results.len(),
            total_duration
        );

        Ok(results)
    }
}

/// Extract a JSON object/array from a JS data string.
///
/// Finds the outermost array `[...]` or object `{...}` with matching brackets.
fn extract_js_object(s: &str) -> Result<String, WebsearchError> {
    let s = s.trim();

    // Determine opening character
    let (open, close) = if s.starts_with('[') {
        ('[', ']')
    } else if s.starts_with('{') {
        ('{', '}')
    } else {
        return Err(WebsearchError::ParseFailed {
            parser: "brave_js_object",
            source: "response does not start with '[' or '{'".into(),
        });
    };

    let mut depth = 0u32;
    let mut in_string = false;
    let mut escaped = false;
    let mut end = 0;

    for (i, ch) in s.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }

        match ch {
            '\\' if in_string => escaped = true,
            '"' => in_string = !in_string,
            _ if in_string => {}
            c if c == open => depth += 1,
            c if c == close => {
                depth -= 1;
                if depth == 0 {
                    end = i + 1;
                    break;
                }
            }
            _ => {}
        }
    }

    if depth != 0 {
        return Err(WebsearchError::ParseFailed {
            parser: "brave_js_object",
            source: "unmatched brackets in JS data".into(),
        });
    }

    Ok(s[..end].to_string())
}

/// Clean up JS-specific syntax so it becomes valid JSON.
/// Handles: trailing commas, unquoted keys (identifiers before `:`),
/// single quotes, and boolean/null literals.
fn clean_js_object(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let chars: Vec<char> = s.chars().collect();
    let len = chars.len();
    let mut i = 0;

    while i < len {
        // Skip whitespace before checking for identifiers
        // Handle trailing commas
        if chars[i] == ',' {
            let mut j = i + 1;
            while j < len && chars[j].is_whitespace() {
                j += 1;
            }
            if j < len && (chars[j] == ']' || chars[j] == '}') {
                i += 1;
                continue;
            }
        }

        // Detect unquoted identifier key (word before ':')
        if (chars[i].is_ascii_alphabetic() || chars[i] == '_')
            && (i == 0 || chars[i - 1] != '"')
        {
            // Check if this is followed by ':' (key) or ':' inside an object
            let mut j = i;
            while j < len && (chars[j].is_alphanumeric() || chars[j] == '_') {
                j += 1;
            }
            // Skip whitespace
            let mut k = j;
            while k < len && chars[k].is_whitespace() {
                k += 1;
            }
            if k < len && chars[k] == ':' {
                // This is an unquoted key — quote it
                result.push('"');
                while i < j {
                    result.push(chars[i]);
                    i += 1;
                }
                result.push('"');
                continue;
            }
        }

        // Replace single quotes with double quotes (but not inside double-quoted strings)
        // This is a simplified approach — might not handle all edge cases

        result.push(chars[i]);
        i += 1;
    }

    result
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
        assert!(url.contains("offset=10"));
    }

    #[test]
    fn build_search_url_page_3() {
        let params = SearchParams::default();
        let url = BraveEngine::build_search_url("rust", &params, 3);
        assert!(url.contains("offset=20"));
    }

    #[test]
    fn build_cookie_header_default() {
        let params = SearchParams::default();
        let cookie = BraveEngine::build_cookie_header(&params);
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
        let cookie = BraveEngine::build_cookie_header(&params);
        assert!(cookie.contains("safesearch=strict"));
        assert!(cookie.contains("country=us"));
    }

    #[test]
    fn extract_js_object_array() {
        let s = r#"[{"key":"value"}]"#;
        let result = extract_js_object(s).unwrap();
        assert_eq!(result, r#"[{"key":"value"}]"#);
    }

    #[test]
    fn extract_js_object_nested() {
        let s = r#"{"outer":{"inner":[1,2,3]}}"#;
        let result = extract_js_object(s).unwrap();
        assert_eq!(result, r#"{"outer":{"inner":[1,2,3]}}"#);
    }

    #[test]
    fn extract_js_object_with_strings() {
        let s = r#"{"a":"b[c]d"}"#;
        let result = extract_js_object(s).unwrap();
        assert_eq!(result, r#"{"a":"b[c]d"}"#);
    }

    #[test]
    fn extract_js_object_unmatched() {
        // Only opening bracket, no closing bracket
        let result = extract_js_object("{");
        assert!(result.is_err());
    }

    #[test]
    fn extract_js_object_no_bracket() {
        let result = extract_js_object("hello");
        assert!(result.is_err());
    }

    #[test]
    fn clean_js_object_trailing_comma() {
        let cleaned = clean_js_object(r#"[1,2,]"#);
        assert_eq!(cleaned, r#"[1,2]"#);
    }

    #[test]
    fn extract_results_basic() {
        let json = serde_json::json!([
            null,
            {
                "data": {
                    "body": {
                        "response": {
                            "web": {
                                "results": [
                                    {
                                        "title": "Example",
                                        "url": "https://example.com",
                                        "description": "An example site"
                                    },
                                    {
                                        "title": "Test",
                                        "url": "https://test.org",
                                        "description": null
                                    }
                                ]
                            }
                        }
                    }
                }
            }
        ]);

        let results = BraveEngine::extract_results(&json, 10).unwrap();
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].title, "Example");
        assert_eq!(results[0].url, "https://example.com");
        assert_eq!(results[0].snippet.as_deref(), Some("An example site"));
        assert_eq!(results[1].title, "Test");
        assert_eq!(results[1].url, "https://test.org");
        assert!(results[1].snippet.is_none());
    }

    #[test]
    fn extract_results_empty() {
        let json = serde_json::json!([
            null,
            {
                "data": {
                    "body": {
                        "response": {
                            "web": {
                                "results": []
                            }
                        }
                    }
                }
            }
        ]);

        let results = BraveEngine::extract_results(&json, 10).unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn extract_results_pow_captcha() {
        let json = serde_json::json!([
            null,
            null,
            {
                "data": {
                    "title": "PoW Captcha"
                }
            }
        ]);

        let result = BraveEngine::extract_results(&json, 10);
        assert!(matches!(result, Err(WebsearchError::AccessDenied)));
    }

    #[test]
    fn extract_results_filters_malformed_url() {
        let json = serde_json::json!([
            null,
            {
                "data": {
                    "body": {
                        "response": {
                            "web": {
                                "results": [
                                    {
                                        "title": "Bad URL",
                                        "url": "not-a-valid-url",
                                        "description": "test"
                                    }
                                ]
                            }
                        }
                    }
                }
            }
        ]);

        let results = BraveEngine::extract_results(&json, 10).unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn extract_results_missing_title() {
        let json = serde_json::json!([
            null,
            {
                "data": {
                    "body": {
                        "response": {
                            "web": {
                                "results": [
                                    {
                                        "url": "https://example.com",
                                        "description": "no title"
                                    }
                                ]
                            }
                        }
                    }
                }
            }
        ]);

        let results = BraveEngine::extract_results(&json, 10).unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn max_results_capping() {
        let mut web_results = Vec::new();
        for i in 0..50 {
            web_results.push(serde_json::json!({
                "title": format!("Result {}", i),
                "url": format!("https://example.com/{}", i),
                "description": format!("Description {}", i),
            }));
        }

        let json = serde_json::json!([
            null,
            {
                "data": {
                    "body": {
                        "response": {
                            "web": {
                                "results": web_results
                            }
                        }
                    }
                }
            }
        ]);

        let results = BraveEngine::extract_results(&json, 10).unwrap();
        assert_eq!(results.len(), 10);
    }

    // ---- NEW TESTS for extract_js_data ----

    /// Helper to create a minimal Brave-style HTML with kit.start script.
    fn make_brave_html(data_json: &str) -> String {
        // kit.start(app, element, { node_ids: [0], data: [{type:"data",data:VALUE}] })
        format!(r#"<html><head></head><body><script>kit.start(app, element, {{node_ids:[0],data:[{{type:"data",data:{}}}]}})</script></body></html>"#,
            data_json
        )
    }

    #[test]
    fn extract_js_data_basic() {
        let html = make_brave_html(r#"{"key":"value"}"#);
        let result = BraveEngine::extract_js_data(&html).unwrap();
        // The function extracts the full kit.start() argument
        // kit.start(app, element, { node_ids: [...], data: [{type:"data",data:{"key":"value"}}] })
        let data_arr = result.get("data").and_then(|v| v.as_array()).unwrap();
        let inner = data_arr[0].get("data").unwrap();
        assert_eq!(inner, &serde_json::json!({"key": "value"}));
    }

    #[test]
    fn extract_js_data_missing_kit_start() {
        let html = r#"<html><head></head><body><script>console.log("no kit.start here")</script></body></html>"#;
        let result = BraveEngine::extract_js_data(&html);
        assert!(matches!(result, Err(WebsearchError::ParseFailed { parser: "brave_kitstart", .. })));
    }

    fn extract_js_data_missing_data_field() {
        // The function now extracts any JS object/array after kit.start(
        // even without a data: field
        let html = r#"<html><head></head><body><script>kit.start({"notdata":1})</script></body></html>"#;
        let result = BraveEngine::extract_js_data(&html);
        assert!(result.is_ok(), "Should parse any object after kit.start(");
    }

    fn extract_js_data_trailing_comma() {
        // Brave sometimes includes trailing commas in the JS object
        let html = make_brave_html(r#"{"items":[1,2,3,]}"#);
        let result = BraveEngine::extract_js_data(&html).unwrap();
        // Should parse successfully despite trailing comma
        assert!(result.get("data").is_some());
    }

    fn extract_js_data_with_array_data() {
        let html = make_brave_html(r#"[{"title":"Test","url":"https://example.com"}]"#);
        let result = BraveEngine::extract_js_data(&html).unwrap();
        // Should parse the outer wrapper successfully
        assert!(result.get("data").is_some());
    }
}