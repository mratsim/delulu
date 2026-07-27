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
use std::any::Any;
use std::sync::Arc;

use crate::error::WebsearchError;

/// Known search engine identifiers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum EngineId {
    /// Brave search engine.
    Brave,
    /// DuckDuckGo search engine.
    DuckDuckGo,
}

impl EngineId {
    /// Return the 3-letter abbreviation for this engine.
    pub fn abbreviation(&self) -> &'static str {
        match self {
            EngineId::Brave => "brv",
            EngineId::DuckDuckGo => "ddg",
        }
    }
}

impl std::fmt::Display for EngineId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EngineId::Brave => write!(f, "brave"),
            EngineId::DuckDuckGo => write!(f, "duckduckgo"),
        }
    }
}

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

/// Typed continuation token for pagination.
///
/// Each engine defines its own concrete type implementing this trait.
/// Adding a new engine requires implementing Continuation for the new type
/// — NO existing code is modified (solves the expression problem).
///
/// # Downcasting
/// Engines use `as_any().downcast_ref::<ConcreteType>()` to recover
/// their concrete continuation type from `&dyn Continuation`.
///
/// # Object Safety
/// This trait is object-safe: `as_any()` can be called on `&dyn Continuation`.
pub trait Continuation: Send + Sync {
    /// Downcast to `&dyn Any` for type-safe downcasting.
    fn as_any(&self) -> &dyn Any;
}

/// The response from a search engine, containing results and an optional
/// continuation token for pagination.
///
/// This struct does NOT derive `Serialize` because `Box<dyn Continuation>`
/// is not serializable.
pub struct SearchResponse {
    /// The search results for this page.
    pub results: Vec<SearchResult>,
    /// An optional continuation token for fetching the next page.
    /// `None` indicates no more pages are available.
    pub continuation: Option<Box<dyn Continuation>>,
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
/// - `continuation` MAY be None (first page) or Some with the engine's
///   continuation type for pagination.
///
/// # Postcondition
/// - Returns `Ok(SearchResponse)` with 0..=max_results results on success.
/// - The `SearchResponse.continuation` field contains the next-page token
///   if more pages are available, or `None` if this was the last page.
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
        continuation: Option<&dyn Continuation>,
    ) -> Result<SearchResponse, WebsearchError>;
}

/// Type alias for an engine stored in the registry.
pub type EngineRef = Arc<dyn Engine + Send + Sync>;

#[cfg(test)]
#[path = "../tests/unit/engine_test.rs"]
mod engine_test;
