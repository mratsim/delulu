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

//! Unit tests for Brave search engine backend.

use super::{BraveContinuation, BraveEngine, strip_date_prefix};
use crate::SearchParams;

#[test]
fn build_search_url_basic() {
    let params = SearchParams::default();
    let url = BraveEngine::build_search_url("rust", &params, 1);
    assert!(url.contains("q=rust"));
    assert!(url.contains("source=web"));
    assert!(!url.contains("offset"));
}

#[test]
fn build_search_url_page_2() {
    let params = SearchParams::default();
    let url = BraveEngine::build_search_url("rust", &params, 2);
    assert!(url.contains("offset=1"));
}

#[test]
fn build_search_url_page_3() {
    let params = SearchParams::default();
    let url = BraveEngine::build_search_url("rust", &params, 3);
    assert!(url.contains("offset=2"));
}

#[test]
fn build_cookie_header_default() {
    let params = SearchParams::default();
    let cookie = BraveEngine::build_cookie_header(&params).unwrap();
    assert!(cookie.contains("safesearch=moderate"));
    assert!(cookie.contains("country=all"));
    assert!(cookie.contains("useLocation=0"));
    assert!(cookie.contains("summarizer=0"));
}

#[test]
fn build_cookie_header_with_params() {
    let params = SearchParams {
        safesearch: Some("strict".into()),
        country: Some("us".into()),
        ..Default::default()
    };
    let cookie = BraveEngine::build_cookie_header(&params).unwrap();
    assert!(cookie.contains("safesearch=strict"));
    assert!(cookie.contains("country=us"));
}

#[test]
fn strip_date_prefix_with_date() {
    let raw = "January 7, 2025 - The elliptic curve only hash algorithm.".to_string();
    let date_str = Some("January 7, 2025 -".to_string());
    let (cleaned, date) = strip_date_prefix(raw, date_str);
    assert_eq!(date, Some(1736208000));
    assert!(!cleaned.contains("January 7, 2025"));
    assert!(cleaned.starts_with("The elliptic"));
}

#[test]
fn strip_date_prefix_no_date() {
    let raw = "We describe a new explicit function.".to_string();
    let (cleaned, date) = strip_date_prefix(raw.clone(), None);
    assert!(date.is_none());
    assert_eq!(cleaned, raw);
}

#[test]
fn strip_date_prefix_empty_date_str() {
    let raw = "Some content here.".to_string();
    let date_str = Some("".to_string());
    let (cleaned, date) = strip_date_prefix(raw.clone(), date_str);
    assert!(date.is_none());
    assert_eq!(cleaned, raw);
}

#[test]
fn strip_date_prefix_august_date() {
    let raw = "August 7, 2025 - ResearchGate publication.".to_string();
    let date_str = Some("August 7, 2025 -".to_string());
    let (cleaned, date) = strip_date_prefix(raw, date_str);
    assert_eq!(date, Some(1754524800));
    assert!(!cleaned.contains("August 7, 2025"));
}

#[test]
fn strip_date_prefix_no_dash_in_date() {
    let raw = "September 4, 2020 - Ethereum research post.".to_string();
    let date_str = Some("September 4, 2020".to_string());
    let (cleaned, date) = strip_date_prefix(raw, date_str);
    assert_eq!(date, Some(1599177600));
    assert!(!cleaned.contains("September 4, 2020"));
}

#[test]
fn brave_continuation_roundtrip() {
    let cont = BraveContinuation { page: 5 };
    let json = serde_json::to_string(&cont).unwrap();
    let back: BraveContinuation = serde_json::from_str(&json).unwrap();
    assert_eq!(back.page, 5);
}

#[test]
fn brave_continuation_from_json() {
    let cont: BraveContinuation = serde_json::from_str(r#"{"page":7}"#).unwrap();
    assert_eq!(cont.page, 7);
}
