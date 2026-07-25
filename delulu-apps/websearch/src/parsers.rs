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

//! Input validation and parsing for external parameters.
//!
//! Following the hexagonal architecture pattern, all inputs that cross the
//! boundary from external callers (CLI, MCP, HTTP) into the engine domain
//! must be validated here. This ensures that engines always receive
//! well-formed parameters and that injection vectors (cookie injection,
//! URL manipulation) are blocked at the boundary.

use crate::error::WebsearchError;

/// Validated safesearch level.
///
/// Accepts: `"strict"`, `"moderate"`, `"off"`, or `None` (defaults to `"moderate"`).
/// Returns an error for any other value, preventing cookie injection via
/// unsanitized safesearch strings.
pub fn parse_safesearch(value: Option<&str>) -> Result<&str, WebsearchError> {
    match value {
        Some("strict") | Some("moderate") | Some("off") => Ok(value.unwrap()),
        None => Ok("moderate"),
        Some(_other) => Err(WebsearchError::InvalidQuery {
            reason: "safesearch must be 'strict', 'moderate', or 'off'",
        }),
    }
}

/// Validated country / region code.
///
/// Accepts: `None` (defaults to `"all"`), ISO alpha-2 (`"us"`, `"de"`, `"jp"`),
/// or language- territory format (`"en-US"`, `"de-de"`, `"ja-jp"`).
/// Returns an error for any other value, preventing cookie injection via
/// unsanitized country strings.
pub fn parse_country(value: Option<&str>) -> Result<&str, WebsearchError> {
    match value {
        None => Ok("all"),
        Some(c) if c == "all" => Ok("all"),
        Some(c) if c.len() == 2 && c.chars().all(|ch| ch.is_ascii_alphabetic()) => Ok(c),
        Some(c) if c.len() == 5 => {
            let parts: Vec<&str> = c.split('-').collect();
            if parts.len() == 2
                && parts[0].len() == 2
                && parts[1].len() == 2
                && parts[0].chars().all(|ch| ch.is_ascii_alphabetic())
                && parts[1].chars().all(|ch| ch.is_ascii_alphabetic())
            {
                Ok(c)
            } else {
                Err(WebsearchError::InvalidQuery {
                    reason: "country must be ISO alpha-2 (e.g. 'us') or language-territory (e.g. 'en-US')",
                })
            }
        }
        _ => Err(WebsearchError::InvalidQuery {
            reason: "country must be ISO alpha-2 (e.g. 'us') or language-territory (e.g. 'en-US')",
        }),
    }
}

/// Validated page number (1-indexed).
///
/// Accepts: `None` (defaults to 1) or any `u32 >= 1`.
/// Returns an error for `Some(0)`.
pub fn parse_page(value: Option<u32>) -> Result<u32, WebsearchError> {
    match value {
        None => Ok(1),
        Some(0) => Err(WebsearchError::InvalidQuery {
            reason: "page must be >= 1",
        }),
        Some(p) => Ok(p),
    }
}

/// Validated max_results, clamped to `1..=100`.
///
/// Accepts: `None` (defaults to 20), any `u32`.
/// Values > 100 are silently capped. Returns an error for `Some(0)`.
pub fn parse_max_results(value: Option<u32>) -> Result<usize, WebsearchError> {
    match value {
        None => Ok(20),
        Some(0) => Err(WebsearchError::InvalidQuery {
            reason: "max_results must be >= 1",
        }),
        Some(m) => Ok(m.min(100) as usize),
    }
}

/// Validated search query.
///
/// Accepts: non-empty strings up to 2048 bytes (after trimming).
/// Returns an error for empty or whitespace-only queries.
pub fn validate_query(query: &str) -> Result<&str, WebsearchError> {
    let trimmed = query.trim();
    if trimmed.is_empty() {
        return Err(WebsearchError::InvalidQuery {
            reason: "query is empty",
        });
    }
    if trimmed.len() > 2048 {
        return Err(WebsearchError::InvalidQuery {
            reason: "query too long",
        });
    }
    Ok(trimmed)
}


#[cfg(test)]
#[path = "../tests/unit/parsers_test.rs"]
mod parsers_test;
