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

//! Unit tests for input validation and parsing (src/parsers.rs).

use crate::{parse_country, parse_max_results, parse_safesearch, parse_time_range, validate_query};
use crate::parsers::parse_page;

// --- safesearch ---

#[test]
fn safesearch_none_defaults_to_moderate() {
    assert_eq!(parse_safesearch(None).unwrap(), "moderate");
}

#[test]
fn safesearch_valid_values() {
    assert_eq!(parse_safesearch(Some("strict")).unwrap(), "strict");
    assert_eq!(parse_safesearch(Some("moderate")).unwrap(), "moderate");
    assert_eq!(parse_safesearch(Some("off")).unwrap(), "off");
}

#[test]
fn safesearch_invalid_rejected() {
    assert!(parse_safesearch(Some("")).is_err());
    assert!(parse_safesearch(Some("yes")).is_err());
    assert!(parse_safesearch(Some("strict\n")).is_err());
    assert!(parse_safesearch(Some("\"; malicious=1")).is_err());
}

// --- country ---

#[test]
fn country_none_defaults_to_all() {
    assert_eq!(parse_country(None).unwrap(), "all");
}

#[test]
fn country_valid_alpha2() {
    assert_eq!(parse_country(Some("us")).unwrap(), "us");
    assert_eq!(parse_country(Some("de")).unwrap(), "de");
    assert_eq!(parse_country(Some("jp")).unwrap(), "jp");
}

#[test]
fn country_valid_language_territory() {
    assert_eq!(parse_country(Some("en-US")).unwrap(), "en-US");
    assert_eq!(parse_country(Some("de-de")).unwrap(), "de-de");
    assert_eq!(parse_country(Some("ja-jp")).unwrap(), "ja-jp");
}

#[test]
fn country_explicit_all() {
    assert_eq!(parse_country(Some("all")).unwrap(), "all");
}

#[test]
fn country_invalid_rejected() {
    assert!(parse_country(Some("")).is_err());
    assert!(parse_country(Some("usa")).is_err());
    assert!(parse_country(Some("us!")).is_err());
    assert!(parse_country(Some("\"; malicious=1")).is_err());
    assert!(parse_country(Some("en_US")).is_err());
    assert!(parse_country(Some("en-US-extra")).is_err());
}

// --- page ---

#[test]
fn page_none_defaults_to_1() {
    assert_eq!(parse_page(None).unwrap(), 1);
}

#[test]
fn page_valid_values() {
    assert_eq!(parse_page(Some(1)).unwrap(), 1);
    assert_eq!(parse_page(Some(5)).unwrap(), 5);
    assert_eq!(parse_page(Some(100)).unwrap(), 100);
}

#[test]
fn page_zero_rejected() {
    assert!(parse_page(Some(0)).is_err());
}

// --- max_results ---

#[test]
fn max_results_none_defaults_to_20() {
    assert_eq!(parse_max_results(None).unwrap(), 20);
}

#[test]
fn max_results_capped_at_100() {
    assert_eq!(parse_max_results(Some(50)).unwrap(), 50);
    assert_eq!(parse_max_results(Some(100)).unwrap(), 100);
    assert_eq!(parse_max_results(Some(200)).unwrap(), 100);
    assert_eq!(parse_max_results(Some(u32::MAX)).unwrap(), 100);
}

#[test]
fn max_results_zero_rejected() {
    assert!(parse_max_results(Some(0)).is_err());
}

// --- query ---

#[test]
fn query_valid() {
    assert_eq!(validate_query("hello").unwrap(), "hello");
    assert_eq!(validate_query("  hello  ").unwrap(), "hello");
}

#[test]
fn query_empty_rejected() {
    assert!(validate_query("").is_err());
    assert!(validate_query("   ").is_err());
}

#[test]
fn query_too_long_rejected() {
    let long = "a".repeat(2049);
    assert!(validate_query(&long).is_err());
}

#[test]
fn query_at_boundary_accepted() {
    let exact = "a".repeat(2048);
    assert_eq!(validate_query(&exact).unwrap().len(), 2048);
}

#[test]
fn time_range_none_passthrough() {
    assert_eq!(parse_time_range(None).unwrap(), None);
}

#[test]
fn time_range_empty_is_none() {
    assert_eq!(parse_time_range(Some("")).unwrap(), None);
}

#[test]
fn time_range_valid_iso() {
    assert_eq!(parse_time_range(Some("2024-01-01to2024-12-31")).unwrap(), Some("2024-01-01to2024-12-31"));
}

#[test]
fn time_range_valid_short() {
    assert_eq!(parse_time_range(Some("past_month")).unwrap(), Some("past_month"));
}

#[test]
fn time_range_rejects_url_injection() {
    assert!(parse_time_range(Some("2024-01-01&extra=evil")).is_err());
    assert!(parse_time_range(Some("2024-01-01=1")).is_err());
    assert!(parse_time_range(Some("2024-01-01#fragment")).is_err());
    assert!(parse_time_range(Some("2024-01-01?query=1")).is_err());
    assert!(parse_time_range(Some("2024-01-01%00")).is_err());
    assert!(parse_time_range(Some("2024-01-01;x")).is_err());
}

#[test]
fn time_range_rejects_too_long() {
    let long = "a".repeat(65);
    assert!(parse_time_range(Some(&long)).is_err());
    let ok = "a".repeat(64);
    assert!(parse_time_range(Some(&ok)).is_ok());
}
