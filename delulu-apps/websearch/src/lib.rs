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

#![forbid(unsafe_code)]

//! Multi-engine web search library providing a unified `Engine` trait, search
//! result types, error types, and backend implementations for DuckDuckGo,
//! Brave, Baidu, Yahoo Japan, and Startpage.

pub mod engine;
pub mod engines;
pub mod error;
#[cfg(feature = "mcp")]
pub mod mcp_serialization;
pub mod parsers;
pub mod session_cache;
pub mod session_key;

#[cfg(feature = "mcp")]
pub mod lib_mcp;
#[cfg(feature = "mcp")]
pub use lib_mcp::WebsearchServer;

pub use engine::{
    Continuation, Engine, EngineId, EngineRef, SearchParams, SearchResponse, SearchResult,
};
pub use engines::EngineRegistry;
pub use error::WebsearchError;
pub use parsers::{
    parse_country, parse_max_results, parse_safesearch, parse_time_range, validate_query,
};
pub use session_cache::SessionCache;
pub use session_key::SessionKey;

/// Sanitize a string for logging: strip control characters, truncate at 2048 bytes.
///
/// This is shared across all engine backends to ensure consistent log output.
/// Truncation is at the UTF-8 character boundary (not byte-slicing) to avoid panics.
pub fn sanitize_for_log(s: &str) -> String {
    // Strip control characters (except newline and tab), then truncate to 2048 bytes at char boundary
    let cleaned: String = s
        .chars()
        .filter(|&c| !c.is_control() || c == '\n' || c == '\t')
        .collect();

    // Truncate to 2048 bytes at a UTF-8 boundary
    if cleaned.len() <= 2048 {
        cleaned
    } else {
        let end = cleaned.floor_char_boundary(2048);
        cleaned[..end].to_string()
    }
}

#[cfg(test)]
#[path = "../tests/unit/lib_test.rs"]
mod lib_test;
