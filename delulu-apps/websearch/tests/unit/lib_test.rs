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

//! Unit tests for lib-level functions (sanitize_for_log).

use crate::sanitize_for_log;

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
    assert!(result.len() <= 2048, "len={} > 2048", result.len());
    assert_eq!(
        result.len() % 3,
        0,
        "len={} not at char boundary",
        result.len()
    );
    assert!(result.chars().all(|c| c == '中'));
    assert_eq!(
        result.len(),
        2046,
        "expected 2046 bytes (682 CJK), got {}",
        result.len()
    );
    assert_eq!(
        result.chars().count(),
        682,
        "expected 682 CJK chars, got {}",
        result.chars().count()
    );
}

#[test]
fn sanitize_for_log_emoji_at_boundary() {
    // Emoji are 4 bytes each. 2048 / 4 = 512
    let long: String = "😀".repeat(600); // 2400 bytes
    let result = sanitize_for_log(&long);
    assert!(result.len() <= 2048, "len={} > 2048", result.len());
    assert_eq!(
        result.len() % 4,
        0,
        "len={} not at char boundary",
        result.len()
    );
    assert!(result.chars().all(|c| c == '😀'));
    assert_eq!(
        result.len(),
        2048,
        "expected 2048 bytes (512 emoji), got {}",
        result.len()
    );
    assert_eq!(
        result.chars().count(),
        512,
        "expected 512 emoji, got {}",
        result.chars().count()
    );
}
