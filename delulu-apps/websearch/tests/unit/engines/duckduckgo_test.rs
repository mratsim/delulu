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

//! Unit tests for DuckDuckGo search engine backend.

use super::{
    DuckDuckGoContinuation, DuckDuckGoEngine, extract_json_from_js, html_entity_decode,
    is_leap_year, parse_ddg_date, validate_n_token,
};
use crate::{Continuation, SearchParams, WebsearchError};

#[test]
fn extract_json_from_js_works() {
    let after_marker = " [{\"c\":\"https://example.com\",\"t\":\"Test\"}]);";
    let json = extract_json_from_js(after_marker).unwrap();
    assert_eq!(json, "[{\"c\":\"https://example.com\",\"t\":\"Test\"}]");
}

#[test]
fn extract_json_from_js_nested() {
    let after_marker = " [[{\"a\":[1,2,3]}]]);";
    let json = extract_json_from_js(after_marker).unwrap();
    assert_eq!(json, "[[{\"a\":[1,2,3]}]]");
}

#[test]
fn extract_json_from_js_no_bracket() {
    let result = extract_json_from_js("no brackets here");
    assert!(result.is_err());
}

#[test]
fn extract_json_from_js_unmatched() {
    let result = extract_json_from_js(" [[");
    assert!(result.is_err());
}

#[test]
fn build_search_url_basic() {
    let params = SearchParams::default();
    let url = DuckDuckGoEngine::build_search_url("rust", &params);
    assert!(url.contains("q=rust"));
    assert!(url.contains("kl=wt-wt"));
}

#[test]
fn build_search_url_with_safesearch() {
    let params = SearchParams {
        safesearch: Some("strict".into()),
        ..Default::default()
    };
    let url = DuckDuckGoEngine::build_search_url("rust", &params);
    assert!(url.contains("kp=1"));
}

#[test]
fn build_search_url_with_safesearch_off() {
    let params = SearchParams {
        safesearch: Some("off".into()),
        ..Default::default()
    };
    let url = DuckDuckGoEngine::build_search_url("rust", &params);
    assert!(url.contains("kp=-2"));
}

#[test]
fn build_search_url_with_country() {
    let params = SearchParams {
        country: Some("de-de".into()),
        ..Default::default()
    };
    let url = DuckDuckGoEngine::build_search_url("rust", &params);
    assert!(url.contains("kl=de-de"));
}

#[test]
fn build_search_url_with_time_range() {
    let params = SearchParams {
        time_range: Some("2024-01-01..2024-12-31".into()),
        ..Default::default()
    };
    let url = DuckDuckGoEngine::build_search_url("rust", &params);
    assert!(url.contains("df=2024-01-01..2024-12-31"));
}

#[test]
fn parse_djs_response_basic() {
    let body = r#"DDG.pageLayout.load('d', [{"c":"https://example.com","t":"Example","a":"An example site"},{"c":"https://test.org","t":"Test","a":"A test site","e":"2024-01-15T12:00:00"}]);"#;
    let (results, n_token) = DuckDuckGoEngine::parse_djs_response(body, 10).unwrap();
    assert_eq!(results.len(), 2);
    assert_eq!(results[0].title, "Example");
    assert_eq!(results[0].url, "https://example.com");
    assert_eq!(results[0].snippet.as_deref(), Some("An example site"));
    assert_eq!(results[1].title, "Test");
    assert_eq!(results[1].url, "https://test.org");
    assert!(results[1].date.is_some());
    assert!(n_token.is_none());
}

#[test]
fn parse_djs_response_with_n_token() {
    let body = r#"DDG.pageLayout.load('d', [{"c":"https://example.com","t":"Example","a":"An example site","n":"/d.js?q=test&vqd=abc"},{"c":"https://test.org","t":"Test","a":"A test site","n":"/d.js?q=test&vqd=abc&n=next"}]);"#;
    let (results, n_token) = DuckDuckGoEngine::parse_djs_response(body, 10).unwrap();
    assert_eq!(results.len(), 2);
    assert_eq!(n_token.as_deref(), Some("/d.js?q=test&vqd=abc&n=next"));
}

#[test]
fn parse_djs_response_jsa_challenge() {
    let body = "DDG.deep.initialize('some_token' + jsa";
    let result = DuckDuckGoEngine::parse_djs_response(body, 10);
    assert!(matches!(result, Err(WebsearchError::AccessDenied)));
}

#[test]
fn parse_djs_response_anomaly() {
    let body = "DDG.deep.anomalyDetectionBlock({";
    let result = DuckDuckGoEngine::parse_djs_response(body, 10);
    assert!(matches!(result, Err(WebsearchError::AccessDenied)));
}

#[test]
fn parse_djs_response_no_results() {
    let body = r#"DDG.pageLayout.load('d', [{"c":"","t":"DEEP_ERROR_NO_RESULTS"}]);"#;
    let (results, n_token) = DuckDuckGoEngine::parse_djs_response(body, 10).unwrap();
    assert!(results.is_empty());
    assert!(n_token.is_none());
}

#[test]
fn parse_djs_response_filters_malformed_urls() {
    let body = r#"DDG.pageLayout.load('d', [{"c":"not-a-valid-url","t":"Bad URL","a":"test"}]);"#;
    let (results, n_token) = DuckDuckGoEngine::parse_djs_response(body, 10).unwrap();
    assert!(results.is_empty());
    assert!(n_token.is_none());
}

#[test]
fn html_entity_decode_works() {
    assert_eq!(html_entity_decode("Tom &amp; Jerry"), "Tom & Jerry");
    assert_eq!(html_entity_decode("&lt;script&gt;"), "<script>");
}

#[test]
fn html_entity_decode_no_double_decode() {
    // &amp;lt; should decode to &lt;, not <
    assert_eq!(html_entity_decode("&amp;lt;"), "&lt;");
}

#[test]
fn parse_ddg_date_rfc3339() {
    let ts = parse_ddg_date("2024-01-15T12:00:00+00:00");
    assert!(ts.is_some(), "RFC 3339 should parse");
    assert_eq!(ts.unwrap(), 1705320000);
}

#[test]
fn parse_ddg_date_utc_z() {
    let ts = parse_ddg_date("2024-01-15T12:00:00Z");
    assert!(ts.is_some(), "UTC Z should parse");
    assert_eq!(ts.unwrap(), 1705320000);
}

#[test]
fn parse_ddg_date_naive() {
    let ts = parse_ddg_date("2024-01-15T12:00:00");
    assert!(ts.is_some(), "naive should parse");
    assert_eq!(ts.unwrap(), 1705320000);
}

#[test]
fn parse_ddg_date_invalid() {
    let ts = parse_ddg_date("not a date");
    assert!(ts.is_none());
}

#[test]
fn is_leap_year_divisible_by_4() {
    assert!(is_leap_year(2024));
}

#[test]
fn is_leap_year_century() {
    assert!(!is_leap_year(1900));
}

#[test]
fn is_leap_year_400() {
    assert!(is_leap_year(2000));
}

// ---- build_djs_url tests ----

#[test]
fn build_djs_url_path_only() {
    let url = DuckDuckGoEngine::build_djs_url("/d.js?q=test&vqd=abc");
    assert_eq!(url, "https://links.duckduckgo.com/d.js?q=test&vqd=abc");
}

#[test]
fn build_djs_url_full_url() {
    let url = DuckDuckGoEngine::build_djs_url("https://links.duckduckgo.com/d.js?q=test&vqd=abc");
    assert_eq!(url, "https://links.duckduckgo.com/d.js?q=test&vqd=abc");
}

// ---- NEW TESTS for extract_djs_url ----

#[test]
fn extract_djs_url_success() {
    let html = r#"<html><head><link id="deep_preload_link" href="/d.js?t=test"/></head></html>"#;
    let result = DuckDuckGoEngine::extract_djs_url(html).unwrap();
    assert_eq!(result, "/d.js?t=test");
}

#[test]
fn extract_djs_url_captcha_detected() {
    let html = r#"<html><body><form id="challenge-form"><input type="text"/></form></body></html>"#;
    let result = DuckDuckGoEngine::extract_djs_url(html);
    assert!(matches!(result, Err(WebsearchError::AccessDenied)));
}

#[test]
fn extract_djs_url_missing_link() {
    let html = r#"<html><head></head><body>no link here</body></html>"#;
    let result = DuckDuckGoEngine::extract_djs_url(html);
    assert!(matches!(result, Err(WebsearchError::ParseFailed { .. })));
}

#[test]
fn extract_djs_url_missing_href() {
    let html = r#"<html><head><link id="deep_preload_link" rel="preload"/></head></html>"#;
    let result = DuckDuckGoEngine::extract_djs_url(html);
    assert!(matches!(result, Err(WebsearchError::MissingField { .. })));
}

#[test]
fn duckduckgo_continuation_roundtrip() {
    let cont = DuckDuckGoContinuation {
        n_token: "/d.js?q=test&vqd=abc".into(),
    };
    let json = serde_json::to_string(&cont).unwrap();
    let back: DuckDuckGoContinuation = serde_json::from_str(&json).unwrap();
    assert_eq!(back.n_token, "/d.js?q=test&vqd=abc");
}

#[test]
fn duckduckgo_continuation_from_json() {
    let cont: DuckDuckGoContinuation =
        serde_json::from_str(r#"{"n_token":"/d.js?q=test&vqd=xyz"}"#).unwrap();
    assert_eq!(cont.n_token, "/d.js?q=test&vqd=xyz");
}

#[test]
fn validate_n_token_accepts_valid_paths() {
    assert!(validate_n_token("/d.js?q=rust&vqd=abc"));
    assert!(validate_n_token("d.js?o=jsonp&q=test"));
    assert!(validate_n_token("/d.js"));
    assert!(validate_n_token("abc123/def"));
}

#[test]
fn validate_n_token_rejects_path_traversal() {
    assert!(!validate_n_token("../../../etc/passwd"));
    assert!(!validate_n_token("..\\..\\windows\\system32"));
    assert!(!validate_n_token("/d.js?q=../escape"));
}

#[test]
fn validate_n_token_rejects_protocol_injection() {
    assert!(!validate_n_token("https://internal.service/admin"));
    assert!(!validate_n_token("http://evil.com/payload"));
}

#[test]
fn validate_n_token_rejects_empty() {
    assert!(!validate_n_token(""));
}
