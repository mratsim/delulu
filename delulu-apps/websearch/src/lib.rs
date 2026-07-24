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

//! Multi-engine web search library providing a unified `Engine` trait, search
//! result types, error types, and backend implementations for DuckDuckGo,
//! Brave, Baidu, Yahoo Japan, and Startpage.

pub mod engine;
pub mod error;
pub mod engines;

pub use engine::{Engine, SearchParams, SearchResult};
pub use error::WebsearchError;
pub use engines::EngineRegistry;

/// Sanitize a string for logging: strip control characters, truncate at 2048 bytes.
///
/// This is shared across all engine backends to ensure consistent log output.
/// Truncation is at the UTF-8 character boundary (not byte-slicing) to avoid panics.
pub(crate) fn sanitize_for_log(s: &str) -> String {
    // First filter out control characters, then truncate to 2048 bytes at char boundary
    let cleaned: String = s
        .chars()
        .filter(|&c| c.is_ascii_graphic() || c == ' ' || c == '\n' || c == '\t')
        .collect();

    // Truncate to 2048 bytes at a UTF-8 boundary
    let byte_limit = 2048;
    if cleaned.len() <= byte_limit {
        return cleaned;
    }
    let end = cleaned.floor_char_boundary(byte_limit);
    cleaned[..end].to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_for_log_strips_control_chars() {
        let input = "hello\x00world\x1btest\nnewline";
        let result = sanitize_for_log(input);
        assert!(!result.contains('\x00'));
        assert!(!result.contains('\x1b'));
        assert!(result.contains("hello"));
        assert!(result.contains("world"));
        assert!(result.contains("test"));
        assert!(result.contains("newline"));
    }

    #[test]
    fn sanitize_for_log_truncates_at_2048_bytes() {
        // Create a string of ~3000 ASCII bytes (1 char = 1 byte)
        let long = "a".repeat(3000);
        let result = sanitize_for_log(&long);
        assert_eq!(result.len(), 2048);
        assert!(result.chars().all(|c| c == 'a'));
    }

    #[test]
    fn sanitize_for_log_short_string_unchanged() {
        let input = "hello world";
        let result = sanitize_for_log(input);
        assert_eq!(result, "hello world");
    }

    #[test]
    fn sanitize_for_log_cjk_at_boundary() {
        // CJK chars are 3 bytes each. 2048 / 3 = 682.66, so at most 682 chars
        let long: String = "中".repeat(1000); // 3000 bytes
        let result = sanitize_for_log(&long);
        // Should be at most 2048 bytes, at a char boundary (so 682 * 3 = 2046)
        assert!(result.len() <= 2048);
        assert_eq!(result.len() % 3, 0); // must be at char boundary
        assert!(result.chars().all(|c| c == '中'));
    }

    #[test]
    fn sanitize_for_log_emoji_at_boundary() {
        // Emoji are 4 bytes each. 2048 / 4 = 512
        let long: String = "😀".repeat(600); // 2400 bytes
        let result = sanitize_for_log(&long);
        assert!(result.len() <= 2048);
        assert_eq!(result.len() % 4, 0); // must be at char boundary
        assert!(result.chars().all(|c| c == '😀'));
    }
}
