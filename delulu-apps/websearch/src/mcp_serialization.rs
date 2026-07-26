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

//! MCP response serialization for the `web_search` tool.
//!
//! This module defines the JSON response format returned by the MCP
//! `web_search` tool. It is a **library module** so that unit tests
//! are discoverable via `cargo test --lib`.
//!
//! # Response Format
//!
//! ```json
//! {
//!   "session_key": "brv-Pyz8q4fVDuL",
//!   "results": {
//!     "brave": [
//!       {
//!         "title": "...",
//!         "url": "https://...",
//!         "snippet": "...",
//!         "date": 1234567890
//!       }
//!     ]
//!   },
//!   "has_next_page": true,
//!   "continuation_engine": "brave"
//! }
//! ```

use std::collections::HashMap;

use serde::Serialize;

use crate::engine::{EngineId, SearchResult};

/// The JSON response returned by the MCP `web_search` tool.
///
/// # Fields
///
/// - `session_key` — A session key string for pagination (e.g. `"brv-Pyz8q4fVDuL"`).
/// - `results` — A map from engine name to a vector of search results.
/// - `has_next_page` — Whether there are more pages available for any engine.
/// - `continuation_engine` — The engine that has a continuation token, if any.
/// - `engine_errors` — Per-engine error messages (absent when all engines succeeded).
#[derive(Debug, Clone, Serialize)]
pub struct McpSearchResponse {
    /// Session key for pagination (serialized form of `SessionKey`).
    pub session_key: String,
    /// Results grouped by engine name (e.g. `"brave"`, `"duckduckgo"`).
    pub results: HashMap<String, Vec<SearchResult>>,
    /// Whether there are more pages available for any engine.
    pub has_next_page: bool,
    /// The engine that has a continuation token, if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub continuation_engine: Option<String>,
    /// Per-engine error messages, if any engines failed.
    /// Absent when all engines succeeded.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub engine_errors: Option<Vec<String>>,
}

/// Map an engine name string to its `EngineId` variant.
pub fn engine_name_to_id(name: &str) -> Option<EngineId> {
    match name {
        "brave" => Some(EngineId::Brave),
        "duckduckgo" => Some(EngineId::DuckDuckGo),
        _ => None,
    }
}

/// The JSON response returned by the MCP `web_search_next_page` tool.
#[derive(Debug, Clone, Serialize)]
pub struct McpNextPageResponse {
    /// Search results for this page.
    pub results: Vec<SearchResult>,
    /// Whether there are more pages available.
    pub has_next_page: bool,
}

/// Sanitize a `WebsearchError` into a client-safe error message.
///
/// Internal error details (downcast errors, deserialization internals) are
/// stripped to prevent leaking implementation details to the MCP client.
pub fn sanitize_error_for_client(err: &crate::error::WebsearchError) -> String {
    match err {
        crate::error::WebsearchError::SessionNotFound => {
            "Session not found or expired".to_string()
        }
        crate::error::WebsearchError::Http(_) => {
            "Search engine error".to_string()
        }
        crate::error::WebsearchError::HttpStatus { engine, .. } => {
            format!("Search engine error: {engine}")
        }
        crate::error::WebsearchError::ParseFailed { .. } => {
            "Search engine error".to_string()
        }
        crate::error::WebsearchError::MissingField { engine, .. } => {
            format!("Search engine error: {engine}")
        }
        crate::error::WebsearchError::AccessDenied => {
            "Search engine error".to_string()
        }
        crate::error::WebsearchError::InvalidQuery { .. } => {
            "Search engine error".to_string()
        }
        crate::error::WebsearchError::EngineNotFound { name } => {
            format!("Session engine not available: {name}")
        }
        crate::error::WebsearchError::ContinuationTypeMismatch { .. }
        | crate::error::WebsearchError::ContinuationInvalidValue { .. }
        | crate::error::WebsearchError::ContinuationDeserializationFailed { .. } => {
            "Internal session error".to_string()
        }
    }
}

#[cfg(test)]
#[path = "../tests/unit/mcp_serialization_test.rs"]
mod mcp_serialization_test;
