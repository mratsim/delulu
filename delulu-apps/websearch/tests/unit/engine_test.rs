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

//! Unit tests for engine module types (SearchResult, SearchParams, etc.)

use crate::{SearchParams, SearchResult};

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
fn search_result_no_panic_fields() {
    // Verify no `position` or `engine` fields exist by round-tripping
    let json = r#"{"title":"X","url":"https://x.com","snippet":null,"date":null}"#;
    let result: SearchResult = serde_json::from_str(json).unwrap();
    assert_eq!(result.title, "X");
    assert_eq!(result.url, "https://x.com");
}
