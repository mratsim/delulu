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

//! Unit tests for MCP response serialization and engine name mapping.
//!
//! Tests for `McpSearchResponse` serialization format and `engine_name_to_id` mapping.

#[cfg(test)]
mod tests {
    use crate::engine::{EngineId, SearchResult};
    use crate::error::WebsearchError;
    use crate::mcp_serialization::{
        McpNextPageResponse, McpSearchResponse, engine_name_to_id, sanitize_error_for_client,
    };
    use serde_json::Value;
    use std::collections::HashMap;

    /// Verify that `McpSearchResponse` serializes to the expected JSON structure
    /// with all required fields and correct types.
    #[test]
    fn mcp_response_serialization() {
        let mut results = HashMap::new();
        results.insert(
            "brave".to_string(),
            vec![
                SearchResult {
                    title: "Example Title".to_string(),
                    url: "https://example.com".to_string(),
                    snippet: Some("An example snippet.".to_string()),
                    date: Some(1700000000),
                },
                SearchResult {
                    title: "Second Result".to_string(),
                    url: "https://example.org".to_string(),
                    snippet: None,
                    date: None,
                },
            ],
        );

        let response = McpSearchResponse {
            session_key: "brv-test12345".to_string(),
            results,
            has_next_page: true,
            continuation_engine: Some("brave".to_string()),
            engine_errors: None,
        };

        let json_str = serde_json::to_string_pretty(&response)
            .expect("McpSearchResponse should serialize to JSON");
        let parsed: Value =
            serde_json::from_str(&json_str).expect("Serialized JSON should be valid");

        // Verify top-level fields exist and have correct types
        assert!(
            parsed.get("session_key").and_then(|v| v.as_str()).is_some(),
            "session_key must be present and a string"
        );
        assert_eq!(parsed["session_key"].as_str().unwrap(), "brv-test12345");

        assert!(
            parsed.get("results").and_then(|v| v.as_object()).is_some(),
            "results must be present and an object"
        );

        assert!(
            parsed
                .get("has_next_page")
                .and_then(|v| v.as_bool())
                .is_some(),
            "has_next_page must be present and a boolean"
        );
        assert!(
            parsed["has_next_page"].as_bool().unwrap(),
            "has_next_page must be true"
        );

        assert!(
            parsed
                .get("continuation_engine")
                .and_then(|v| v.as_str())
                .is_some(),
            "continuation_engine must be present and a string"
        );
        assert_eq!(parsed["continuation_engine"].as_str().unwrap(), "brave");

        // Verify results structure
        let results_obj = parsed["results"].as_object().unwrap();
        assert!(
            results_obj.contains_key("brave"),
            "results must contain 'brave' key"
        );

        let brave_results = results_obj["brave"].as_array().unwrap();
        assert_eq!(brave_results.len(), 2, "brave should have 2 results");

        // Verify first result fields
        let first = &brave_results[0];
        assert_eq!(first["title"].as_str().unwrap(), "Example Title");
        assert_eq!(first["url"].as_str().unwrap(), "https://example.com");
        assert_eq!(first["snippet"].as_str().unwrap(), "An example snippet.");
        assert_eq!(first["date"].as_i64().unwrap(), 1700000000);

        // Verify second result has null snippet and date (absent or null)
        let second = &brave_results[1];
        assert_eq!(second["title"].as_str().unwrap(), "Second Result");
        assert_eq!(second["url"].as_str().unwrap(), "https://example.org");
        assert!(
            second.get("snippet").is_none() || second["snippet"].is_null(),
            "snippet should be absent or null when None"
        );
        assert!(
            second.get("date").is_none() || second["date"].is_null(),
            "date should be absent or null when None"
        );
    }

    /// Verify that `McpSearchResponse` serializes correctly when there is
    /// no continuation engine (has_next_page = false).
    #[test]
    fn mcp_response_no_continuation() {
        let results = HashMap::new();
        let response = McpSearchResponse {
            session_key: "ddg-test67890".to_string(),
            results,
            has_next_page: false,
            continuation_engine: None,
            engine_errors: None,
        };

        let json_str =
            serde_json::to_string(&response).expect("McpSearchResponse should serialize to JSON");
        let parsed: Value =
            serde_json::from_str(&json_str).expect("Serialized JSON should be valid");

        assert_eq!(parsed["session_key"].as_str().unwrap(), "ddg-test67890");
        assert!(
            !parsed["has_next_page"].as_bool().unwrap(),
            "has_next_page must be false"
        );
        assert!(
            parsed.get("continuation_engine").is_none(),
            "continuation_engine should be absent when None"
        );
        assert!(
            parsed.get("results").and_then(|v| v.as_object()).is_some(),
            "results must be present even when empty"
        );
    }

    /// Verify that engine_name_to_id maps correctly.
    #[test]
    fn engine_name_to_id_brave() {
        assert_eq!(engine_name_to_id("brave"), Some(EngineId::Brave));
    }

    #[test]
    fn engine_name_to_id_duckduckgo() {
        assert_eq!(engine_name_to_id("duckduckgo"), Some(EngineId::DuckDuckGo));
    }

    #[test]
    fn engine_name_to_id_unknown() {
        assert_eq!(engine_name_to_id("unknown"), None);
    }

    #[test]
    fn engine_name_to_id_empty() {
        assert_eq!(engine_name_to_id(""), None);
    }

    #[test]
    fn engine_name_to_id_case_sensitive() {
        assert_eq!(engine_name_to_id("Brave"), None);
    }

    /// Verify that `McpNextPageResponse` serializes correctly.
    #[test]
    fn mcp_next_page_response_serialization() {
        let results = vec![SearchResult {
            title: "Next Page Result".to_string(),
            url: "https://example.com/next".to_string(),
            snippet: Some("Next page snippet.".to_string()),
            date: Some(1700000001),
        }];

        let response = McpNextPageResponse {
            results: results.clone(),
            has_next_page: true,
        };

        let json_str =
            serde_json::to_string(&response).expect("McpNextPageResponse should serialize to JSON");
        let parsed: Value =
            serde_json::from_str(&json_str).expect("Serialized JSON should be valid");

        assert!(parsed.get("results").is_some(), "Missing 'results' field");
        assert!(
            parsed.get("has_next_page").is_some(),
            "Missing 'has_next_page' field"
        );
        assert!(
            parsed["has_next_page"].as_bool().unwrap(),
            "has_next_page must be true"
        );

        let results_arr = parsed["results"].as_array().unwrap();
        assert_eq!(results_arr.len(), 1, "Expected 1 result");
        assert_eq!(
            results_arr[0]["title"].as_str().unwrap(),
            "Next Page Result"
        );
    }

    /// Verify that `sanitize_error_for_client` produces sanitized messages.
    #[test]
    fn mcp_error_sanitization() {
        // SessionNotFound
        let err = WebsearchError::SessionNotFound;
        let msg = sanitize_error_for_client(&err);
        assert_eq!(msg, "Session not found or expired");

        // EngineNotFound
        let err = WebsearchError::EngineNotFound {
            name: "unknown".to_string(),
        };
        let msg = sanitize_error_for_client(&err);
        assert_eq!(msg, "Session engine not available: unknown");

        // HttpStatus
        let err = WebsearchError::HttpStatus {
            code: 403,
            engine: "brave",
        };
        let msg = sanitize_error_for_client(&err);
        assert_eq!(msg, "Search engine error: brave");

        // Http (transport)
        let err = WebsearchError::Http(delulu_rate_limited_crawler::error::CrawlerError::QpsZero);
        let msg = sanitize_error_for_client(&err);
        assert_eq!(msg, "Search engine error");

        // AccessDenied
        let err = WebsearchError::AccessDenied;
        let msg = sanitize_error_for_client(&err);
        assert_eq!(msg, "Access denied by search engine");

        // Internal errors
        let err = WebsearchError::ContinuationTypeMismatch {
            expected: "A",
            received: "B",
        };
        let msg = sanitize_error_for_client(&err);
        assert_eq!(msg, "Internal session error");

        let err = WebsearchError::ContinuationDeserializationFailed {
            engine: "brave".to_string(),
            detail: "some internal detail".to_string(),
        };
        let msg = sanitize_error_for_client(&err);
        assert_eq!(msg, "Internal session error");

        // InvalidQuery
        let err = WebsearchError::InvalidQuery {
            reason: "bad chars",
        };
        let msg = sanitize_error_for_client(&err);
        assert_eq!(msg, "Invalid query: bad chars");

        // ParseFailed
        let err = WebsearchError::ParseFailed {
            parser: "duckduckgo_djs",
            source: Box::new(std::io::Error::new(
                std::io::ErrorKind::Other,
                "parse error",
            )),
        };
        let msg = sanitize_error_for_client(&err);
        assert_eq!(msg, "Search engine parse error (duckduckgo_djs)");

        // MissingField
        let err = WebsearchError::MissingField {
            field: "title",
            engine: "brave",
        };
        let msg = sanitize_error_for_client(&err);
        assert_eq!(msg, "Search engine error: brave");
    }
}
