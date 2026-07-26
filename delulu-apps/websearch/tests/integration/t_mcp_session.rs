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

//! Integration tests for MCP session management.
//!
//! These tests verify that:
//! - `SessionCache` correctly stores, retrieves, and updates sessions.
//! - `McpSearchResponse` serializes to the expected JSON format.
//!
//! No HTTP requests are made — all tests use in-memory state.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use delulu_websearch::engine::{EngineId, SearchParams, SearchResult};
use delulu_websearch::mcp_serialization::McpSearchResponse;
use delulu_websearch::SessionCache;
use serde_json::Value;

// ---------------------------------------------------------------------------
// SessionCache integration tests
// ---------------------------------------------------------------------------

#[test]
fn session_cache_store_and_retrieve() {
    let cache = SessionCache::new(100, Duration::from_secs(3600));
    let now = Instant::now();
    let random_id = [0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08];

    let key = cache.store(
        EngineId::Brave,
        "test query",
        SearchParams::default(),
        None,
        now,
        random_id,
    );

    let entry = cache.get(&key, now);
    assert!(entry.is_some(), "Entry should be retrievable immediately after store");
    let entry = entry.unwrap();
    assert_eq!(entry.engine, EngineId::Brave);
    assert_eq!(entry.query, "test query");
    assert!(entry.continuation.is_none(), "Continuation should be None");
}

#[test]
fn session_cache_update_continuation() {
    let cache = SessionCache::new(100, Duration::from_secs(3600));
    let now = Instant::now();
    let random_id = [0x10, 0x20, 0x30, 0x40, 0x50, 0x60, 0x70, 0x80];

    let key = cache.store(
        EngineId::DuckDuckGo,
        "update test",
        SearchParams::default(),
        None,
        now,
        random_id,
    );

    // Update with a continuation
    cache
        .update_continuation(&key, None, now)
        .expect("update_continuation should succeed");

    let entry = cache.get(&key, now);
    assert!(entry.is_some(), "Entry should still be retrievable after update");
}

#[test]
fn session_cache_expired_entry_not_returned() {
    let cache = SessionCache::new(100, Duration::from_secs(1));
    let now = Instant::now();
    let random_id = [0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff, 0x11, 0x22];

    let key = cache.store(
        EngineId::Brave,
        "expired query",
        SearchParams::default(),
        None,
        now,
        random_id,
    );

    // Entry should be present now
    assert!(cache.get(&key, now).is_some(), "Entry should be present before expiry");

    // Advance time past TTL
    let later = now + Duration::from_secs(2);
    let entry = cache.get(&key, later);
    assert!(entry.is_none(), "Entry should be None after TTL expiry");
}

#[test]
fn session_cache_evict_expired() {
    let cache = SessionCache::new(100, Duration::from_secs(1));
    let now = Instant::now();
    let random_id = [0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88];

    let key = cache.store(
        EngineId::Brave,
        "evict test",
        SearchParams::default(),
        None,
        now,
        random_id,
    );

    // Advance time and call evict_expired
    let later = now + Duration::from_secs(2);
    cache.evict_expired(later);

    // Entry should no longer be accessible
    let entry = cache.get(&key, later);
    assert!(entry.is_none(), "Entry should be gone after evict_expired");
}

// ---------------------------------------------------------------------------
// McpSearchResponse serialization tests
// ---------------------------------------------------------------------------

#[test]
fn mcp_response_has_all_expected_fields() {
    let mut results = HashMap::new();
    results.insert(
        "brave".to_string(),
        vec![SearchResult {
            title: "Test Title".to_string(),
            url: "https://test.example.com".to_string(),
            snippet: Some("Test snippet.".to_string()),
            date: Some(1700000000),
        }],
    );

    let response = McpSearchResponse {
        session_key: "brv-ABCDEFGHIJK".to_string(),
        results,
        has_next_page: true,
        continuation_engine: Some("brave".to_string()),
            engine_errors: None,
    };

    let json_str = serde_json::to_string(&response)
        .expect("McpSearchResponse should serialize to JSON");
    let parsed: Value = serde_json::from_str(&json_str)
        .expect("Serialized JSON should be valid");

    // Verify all expected fields
    assert!(parsed.get("session_key").is_some(), "Missing 'session_key' field");
    assert!(parsed.get("results").is_some(), "Missing 'results' field");
    assert!(parsed.get("has_next_page").is_some(), "Missing 'has_next_page' field");
    assert!(parsed.get("continuation_engine").is_some(), "Missing 'continuation_engine' field");

    // Verify types
    assert!(parsed["session_key"].is_string(), "'session_key' must be a string");
    assert!(parsed["results"].is_object(), "'results' must be an object");
    assert!(parsed["has_next_page"].is_boolean(), "'has_next_page' must be a boolean");
    assert!(parsed["continuation_engine"].is_string(), "'continuation_engine' must be a string");
}

#[test]
fn mcp_response_no_continuation_omits_field() {
    let results = HashMap::new();
    let response = McpSearchResponse {
        session_key: "ddg-ZYXWVUTSRQP".to_string(),
        results,
        has_next_page: false,
        continuation_engine: None,
            engine_errors: None,
    };

    let json_str = serde_json::to_string(&response)
        .expect("McpSearchResponse should serialize to JSON");
    let parsed: Value = serde_json::from_str(&json_str)
        .expect("Serialized JSON should be valid");

    // continuation_engine should be absent when None
    assert!(
        parsed.get("continuation_engine").is_none(),
        "continuation_engine should be absent when None"
    );

    // Other fields should still be present
    assert!(parsed.get("session_key").is_some(), "Missing 'session_key' field");
    assert!(parsed.get("results").is_some(), "Missing 'results' field");
    assert!(parsed.get("has_next_page").is_some(), "Missing 'has_next_page' field");
}

#[test]
fn mcp_response_results_structure() {
    let mut results = HashMap::new();
    results.insert(
        "brave".to_string(),
        vec![
            SearchResult {
                title: "Result 1".to_string(),
                url: "https://example.com/1".to_string(),
                snippet: Some("Snippet 1".to_string()),
                date: Some(1700000000),
            },
            SearchResult {
                title: "Result 2".to_string(),
                url: "https://example.com/2".to_string(),
                snippet: None,
                date: None,
            },
        ],
    );
    results.insert(
        "duckduckgo".to_string(),
        vec![SearchResult {
            title: "DDG Result".to_string(),
            url: "https://ddg.example.com".to_string(),
            snippet: Some("DDG snippet.".to_string()),
            date: None,
        }],
    );

    let response = McpSearchResponse {
        session_key: "brv-TESTKEY12345".to_string(),
        results,
        has_next_page: true,
        continuation_engine: None,
            engine_errors: None,
    };

    let json_str = serde_json::to_string(&response)
        .expect("McpSearchResponse should serialize to JSON");
    let parsed: Value = serde_json::from_str(&json_str)
        .expect("Serialized JSON should be valid");

    let results_obj = parsed["results"].as_object().unwrap();

    // Both engines should be present
    assert!(results_obj.contains_key("brave"), "Missing 'brave' in results");
    assert!(results_obj.contains_key("duckduckgo"), "Missing 'duckduckgo' in results");

    // Brave should have 2 results
    let brave_results = results_obj["brave"].as_array().unwrap();
    assert_eq!(brave_results.len(), 2, "Expected 2 brave results");

    // DuckDuckGo should have 1 result
    let ddg_results = results_obj["duckduckgo"].as_array().unwrap();
    assert_eq!(ddg_results.len(), 1, "Expected 1 duckduckgo result");

    // Verify result field types
    for result in brave_results.iter().chain(ddg_results.iter()) {
        assert!(result.get("title").and_then(|v| v.as_str()).is_some(), "Result missing 'title' string");
        assert!(result.get("url").and_then(|v| v.as_str()).is_some(), "Result missing 'url' string");
        // snippet and date are optional
    }
}
