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

//! Unit tests for WebsearchError display and conversion.

use crate::WebsearchError;
use delulu_rate_limited_crawler::error::CrawlerError;

#[test]
fn error_display_http() {
    let err = WebsearchError::Http(CrawlerError::QpsZero);
    let msg = err.to_string();
    assert!(msg.contains("HTTP transport error"));
}

#[test]
fn error_display_http_status() {
    let err = WebsearchError::HttpStatus {
        code: 429,
        engine: "duckduckgo",
    };
    let msg = err.to_string();
    assert!(msg.contains("HTTP status 429 from duckduckgo"));
}

#[test]
fn error_display_parse_failed() {
    let err = WebsearchError::ParseFailed {
        parser: "test_parser",
        source: Box::new(std::io::Error::new(std::io::ErrorKind::Other, "bad data")),
    };
    let msg = err.to_string();
    assert!(msg.contains("Parse failed in test_parser"));
}

#[test]
fn error_display_missing_field() {
    let err = WebsearchError::MissingField {
        field: "title",
        engine: "duckduckgo",
    };
    let msg = err.to_string();
    assert!(msg.contains("Missing field 'title' in duckduckgo response"));
}

#[test]
fn error_display_access_denied() {
    let err = WebsearchError::AccessDenied;
    let msg = err.to_string();
    assert!(msg.contains("Access denied"));
}

#[test]
fn error_display_invalid_query() {
    let err = WebsearchError::InvalidQuery {
        reason: "query too long",
    };
    let msg = err.to_string();
    assert!(msg.contains("Invalid query: query too long"));
}

#[test]
fn error_display_engine_not_found() {
    let err = WebsearchError::EngineNotFound {
        name: "nonexistent".into(),
    };
    let msg = err.to_string();
    assert!(msg.contains("Engine 'nonexistent' not found"));
}

#[test]
fn error_from_crawler() {
    let crawler_err = CrawlerError::QpsZero;
    let web_err: WebsearchError = crawler_err.into();
    assert!(matches!(web_err, WebsearchError::Http(_)));
}
