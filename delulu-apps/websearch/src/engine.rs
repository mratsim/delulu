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

//! Unified engine trait, search result types, and search parameters.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::error::WebsearchError;

/// A single search result from any engine.
///
/// Fields `position` and `engine` are intentionally NOT stored per-result.
/// The engine is tracked at the response level (HashMap key).
/// Position is implicit in the Vec ordering.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    /// Result title. MUST be non-empty.
    pub title: String,
    /// Result URL. MUST be a valid URL.
    pub url: String,
    /// Optional snippet / description text.
    pub snippet: Option<String>,
    /// Optional Unix timestamp (seconds since epoch) of the result.
    pub date: Option<i64>,
}

/// Search parameters passed to an engine's `search()` method.
///
/// All fields are optional — engines apply defaults when a field is `None`.
/// Raw strings are used for `safesearch`, `time_range`, and `country`
/// — no intermediary enums. Each engine maps these raw strings to its
/// own query parameters.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SearchParams {
    /// Page number (1-indexed). Defaults to 1.
    pub page: Option<u32>,
    /// Country / region code (e.g. "us", "de", "jp"). Engine-specific.
    pub country: Option<String>,
    /// Safesearch level (e.g. "strict", "moderate", "off"). Engine-specific.
    pub safesearch: Option<String>,
    /// Time range filter (e.g. "2024-01-01to2024-12-31"). Engine-specific.
    pub time_range: Option<String>,
    /// Maximum number of results to return. Defaults to 20, hard limit 100.
    pub max_results: Option<u32>,
}

/// Unified search engine trait.
///
/// Each backend (DuckDuckGo, Brave, Baidu, Yahoo Japan, Startpage) implements
/// this trait. The trait is async via `#[async_trait]` and requires `Send + Sync`
/// so engines can be stored in an `Arc<dyn Engine + Send + Sync>` registry.
///
/// # Precondition
/// - `query` MUST be non-empty after trimming whitespace.
/// - `params` MAY have all fields as None (defaults used).
///
/// # Postcondition
/// - Returns `Ok(Vec<SearchResult>)` with 0..=max_results results on success.
/// - Returns `Err(WebsearchError)` on any failure.
///
/// # Panic-if
/// - This function MUST NOT panic. All error paths return Err.
#[async_trait]
pub trait Engine: Send + Sync {
    async fn search(
        &self,
        query: &str,
        params: SearchParams,
    ) -> Result<Vec<SearchResult>, WebsearchError>;
}

/// Type alias for an engine stored in the registry.
pub type EngineRef = Arc<dyn Engine + Send + Sync>;

/// Realistic desktop browser User-Agent shared across all engines.
///
/// Matches the Chrome-on-Linux pattern used by the scrapers.
pub const DEFAULT_USER_AGENT: &str =
    "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_result_serialize_roundtrip() {
        let result = SearchResult {
            title: "Test".into(),
            url: "https://example.com".into(),
            snippet: Some("A test result".into()),
            date: Some(1234567890),
        };
        let json = serde_json::to_string(&result).unwrap();
        let back: SearchResult = serde_json::from_str(&json).unwrap();
        assert_eq!(back.title, "Test");
        assert_eq!(back.url, "https://example.com");
        assert_eq!(back.snippet, Some("A test result".into()));
        assert_eq!(back.date, Some(1234567890));
    }

    #[test]
    fn search_params_default() {
        let params = SearchParams::default();
        assert!(params.page.is_none());
        assert!(params.country.is_none());
        assert!(params.safesearch.is_none());
        assert!(params.time_range.is_none());
        assert!(params.max_results.is_none());
    }

    #[test]
    fn search_result_no_panic_fields() {
        // Verify no `position` or `engine` fields exist by round-tripping
        let json = r#"{"title":"X","url":"https://x.com","snippet":null,"date":null}"#;
        let result: SearchResult = serde_json::from_str(json).unwrap();
        assert_eq!(result.title, "X");
        assert_eq!(result.url, "https://x.com");
    }
}
