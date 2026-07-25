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

    /// The provided continuation type does not match the engine's expected type.
    /// Recovery: Use the correct continuation type for the engine.
    #[error("Continuation type mismatch: expected {expected}, received {received}")]
    ContinuationTypeMismatch {
        /// Expected continuation type name.
        expected: &'static str,
        /// Received continuation type name.
        received: &'static str,
    },

    /// The continuation contains an invalid value.
    /// Recovery: Re-run the search without a continuation.
    #[error("Invalid continuation value: {reason}")]
    ContinuationInvalidValue {
        /// Reason the value is invalid.
        reason: &'static str,
    },

    /// Failed to deserialize a continuation from JSON.
    /// Recovery: Check the serialization format and retry.
    #[error("Failed to deserialize continuation for '{engine}': {detail}")]
    ContinuationDeserializationFailed {
        /// The engine name.
        engine: String,
        /// Details of the deserialization failure.
        detail: String,
    },
    /// Session key not found or expired.
    /// Recovery: Re-run the search without a continuation.
    #[error("Session not found or expired")]
    SessionNotFound,
}

impl From<CrawlerError> for WebsearchError {
    fn from(e: CrawlerError) -> Self {
        WebsearchError::Http(e)
    }
}

#[cfg(test)]
#[path = "../tests/unit/error_test.rs"]
mod error_test;
