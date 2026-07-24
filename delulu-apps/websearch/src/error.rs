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

//! Typed error enum for web search operations.

use delulu_rate_limited_crawler::error::CrawlerError;
use thiserror::Error;

/// Typed error enum for all web search operations.
///
/// Each variant has a doc comment with recovery guidance.
/// Variants NOT included:
/// - `RateLimited` — the `RateLimitedCrawler` delays rather than rejects requests;
///   429 responses from engines are mapped to `HttpStatus { code: 429 }`.
/// - `Timeout` — timeouts are configured on `RateLimitedCrawler` and returned as
///   `Http(CrawlerError)`. Per-engine timeouts in "all" mode are handled at the
///   MCP server level.
/// - `Captcha` — merged into `AccessDenied`.
/// - `Parse(String)` — split into `ParseFailed` + `MissingField`.
#[derive(Debug, Error)]
pub enum WebsearchError {
    /// Transport-level HTTP error from the rate-limited crawler (network failure, DNS, timeout).
    /// This variant wraps CrawlerError which covers connection errors, timeouts, and
    /// retry exhaustion. It does NOT cover HTTP status-level errors (see HttpStatus).
    /// Recovery: Retry the request.
    #[error("HTTP transport error: {0}")]
    Http(CrawlerError),

    /// Application-level HTTP status error (e.g., 429 Too Many Requests, 403 Forbidden).
    /// Checked manually via response.status() after crawling (CrawlerError does not
    /// surface status codes). The engine name helps identify which engine returned it.
    /// Recovery: Depends on status code. 429 → back off and retry later. 403 → use a
    /// different engine or query.
    #[error("HTTP status {code} from {engine}")]
    HttpStatus {
        /// The HTTP status code (e.g., 429, 403).
        code: u16,
        /// The engine name that returned this status.
        engine: &'static str,
    },

    /// Response body could not be parsed as the expected format (JSON, HTML, JS data object).
    /// `parser` identifies which engine/parser failed (e.g., "duckduckgo_djs", "brave_kitstart").
    /// `source` is the underlying library error, if any.
    /// Recovery: The engine format may have changed; log the source for debugging.
    #[error("Parse failed in {parser}: {source}")]
    ParseFailed {
        /// Name of the parser that failed.
        parser: &'static str,
        /// The underlying error, if any.
        source: Box<dyn std::error::Error + Send + Sync>,
    },

    /// Expected field is missing from a structurally-valid response.
    /// Example: d.js response lacks the `"t"` (title) field.
    /// Recovery: The engine format may have changed; log the field name for debugging.
    #[error("Missing field '{field}' in {engine} response")]
    MissingField {
        /// Name of the missing field.
        field: &'static str,
        /// The engine name.
        engine: &'static str,
    },

    /// The search engine blocked the request — either a captcha challenge or access denial.
    /// Covers: DuckDuckGo JSA challenge, Brave PoW captcha, Baidu wappass/antiFlag,
    /// Startpage /sp/captcha. All captcha types are permanently unsolvable under the
    /// project's "no headless browser" constraint.
    /// Recovery: Use a different engine. Retrying the same engine is unlikely to succeed.
    #[error("Access denied by search engine (captcha / bot challenge)")]
    AccessDenied,

    /// The query was rejected by the search engine (e.g., contains characters the engine
    /// cannot process, query too long, or blocked terms). This is distinct from an empty
    /// query, which is caught at the input boundary.
    /// `reason` describes the specific rejection.
    /// Recovery: Modify the query and retry.
    #[error("Invalid query: {reason}")]
    InvalidQuery {
        /// Reason for the rejection.
        reason: &'static str,
    },

    /// The specified engine name was not found in the registry.
    /// Recovery: Use a valid engine name from `list_engines()`.
    #[error("Engine '{name}' not found")]
    EngineNotFound {
        /// The engine name that was not found.
        name: String,
    },
}

impl From<CrawlerError> for WebsearchError {
    fn from(e: CrawlerError) -> Self {
        WebsearchError::Http(e)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_display_http() {
        let err = WebsearchError::Http(CrawlerError::QpsZero);
        let msg = err.to_string();
        assert!(msg.contains("HTTP transport error"));
    }

    #[test]
    fn error_display_http_status() {
        let err = WebsearchError::HttpStatus {
            code: 429,
            engine: "duckduckgo",
        };
        let msg = err.to_string();
        assert!(msg.contains("HTTP status 429 from duckduckgo"));
    }

    #[test]
    fn error_display_parse_failed() {
        let err = WebsearchError::ParseFailed {
            parser: "test_parser",
            source: Box::new(std::io::Error::new(std::io::ErrorKind::Other, "bad data")),
        };
        let msg = err.to_string();
        assert!(msg.contains("Parse failed in test_parser"));
    }

    #[test]
    fn error_display_missing_field() {
        let err = WebsearchError::MissingField {
            field: "title",
            engine: "duckduckgo",
        };
        let msg = err.to_string();
        assert!(msg.contains("Missing field 'title' in duckduckgo response"));
    }

    #[test]
    fn error_display_access_denied() {
        let err = WebsearchError::AccessDenied;
        let msg = err.to_string();
        assert!(msg.contains("Access denied"));
    }

    #[test]
    fn error_display_invalid_query() {
        let err = WebsearchError::InvalidQuery {
            reason: "query too long",
        };
        let msg = err.to_string();
        assert!(msg.contains("Invalid query: query too long"));
    }

    #[test]
    fn error_display_engine_not_found() {
        let err = WebsearchError::EngineNotFound {
            name: "nonexistent".into(),
        };
        let msg = err.to_string();
        assert!(msg.contains("Engine 'nonexistent' not found"));
    }

    #[test]
    fn error_from_crawler() {
        let crawler_err = CrawlerError::QpsZero;
        let web_err: WebsearchError = crawler_err.into();
        assert!(matches!(web_err, WebsearchError::Http(_)));
    }

    #[test]
    fn error_send_sync() {
        fn assert_send<T: Send>() {}
        fn assert_sync<T: Sync>() {}
        assert_send::<WebsearchError>();
        assert_sync::<WebsearchError>();
    }
}
