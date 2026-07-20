use super::*;
use serde_json::json;

#[test]
fn test_path_param_substitution() {
    let params = HashMap::from([("userId".to_string(), json!("42"))]);
    let path_params = vec!["userId".to_string()];
    let result = build_url(
        "http://example.com",
        "/users/{userId}",
        params,
        &path_params,
    );
    assert_eq!(result, "http://example.com/users/42");
}

#[test]
fn test_multiple_path_params() {
    let params = HashMap::from([
        ("from".to_string(), json!("JFK")),
        ("to".to_string(), json!("LAX")),
    ]);
    let path_params = vec!["from".to_string(), "to".to_string()];
    let result = build_url(
        "http://example.com",
        "/flights/{from}/{to}",
        params,
        &path_params,
    );
    assert_eq!(result, "http://example.com/flights/JFK/LAX");
}

#[test]
fn test_mixed_path_and_query_params() {
    let params = HashMap::from([
        ("userId".to_string(), json!("42")),
        ("include".to_string(), json!("profile")),
    ]);
    let path_params = vec!["userId".to_string()];
    let result = build_url(
        "http://example.com",
        "/users/{userId}",
        params,
        &path_params,
    );
    assert_eq!(result, "http://example.com/users/42?include=profile");
}

#[test]
fn test_query_params_only() {
    let params = HashMap::from([
        ("q".to_string(), json!("test")),
        ("page".to_string(), json!(1)),
    ]);
    let path_params: Vec<String> = vec![];
    let result = build_url("http://example.com", "/search", params, &path_params);
    assert_eq!(result, "http://example.com/search?page=1&q=test");
}

#[test]
fn test_no_params() {
    let params = HashMap::new();
    let path_params: Vec<String> = vec![];
    let result = build_url("http://example.com", "/users", params, &path_params);
    assert_eq!(result, "http://example.com/users");
}

#[test]
fn test_double_slash_prevention() {
    let params = HashMap::new();
    let path_params: Vec<String> = vec![];
    let result = build_url("http://example.com/api/", "/users", params, &path_params);
    assert_eq!(result, "http://example.com/api/users");
}

#[test]
fn test_numeric_path_param() {
    let params = HashMap::from([("id".to_string(), json!(123))]);
    let path_params = vec!["id".to_string()];
    let result = build_url("http://example.com", "/items/{id}", params, &path_params);
    assert_eq!(result, "http://example.com/items/123");
}

#[test]
fn test_query_param_url_encoding() {
    let params = HashMap::from([("q".to_string(), json!("hello world"))]);
    let path_params: Vec<String> = vec![];
    let result = build_url("http://example.com", "/search", params, &path_params);
    assert_eq!(result, "http://example.com/search?q=hello%20world");
}

#[test]
fn test_empty_string_query_param_filtered() {
    let params = HashMap::from([("q".to_string(), json!(""))]);
    let path_params: Vec<String> = vec![];
    let result = build_url("http://example.com", "/search", params, &path_params);
    assert_eq!(result, "http://example.com/search");
}

#[test]
fn test_path_param_no_placeholder_match() {
    // Param declared as path param but no matching {placeholder} in path template
    let params = HashMap::from([("unknown".to_string(), json!("val"))]);
    let path_params = vec!["unknown".to_string()];
    let result = build_url(
        "http://example.com",
        "/users/{userId}",
        params,
        &path_params,
    );
    // The param is consumed (not in query string) but placeholder remains
    assert_eq!(result, "http://example.com/users/{userId}");
}

// ---------------------------------------------------------------------------
// UTF-8 truncation helper tests
// ---------------------------------------------------------------------------

#[test]
fn test_utf8_truncation_at_boundary() {
    // 499 ASCII bytes + 1 two-byte char (é) = 501 bytes.
    // Byte 500 is the continuation byte of é, so truncation must back up to byte 499.
    let body = format!("{}{}", "a".repeat(499), "é");
    assert_eq!(body.len(), 501, "precondition: body is 501 bytes");
    let result = format!("{}...", truncate_error_body(&body, 500));
    assert_eq!(
        result.len(),
        502,
        "output should be 499 a's + ... = 502 bytes"
    );
    assert!(
        result.starts_with(&"a".repeat(499)),
        "should start with 499 a's"
    );
    assert!(result.ends_with("..."), "should end with ...");
    // Verify the content before ... is valid UTF-8 and ≤500 bytes
    let prefix = truncate_error_body(&body, 500);
    assert!(prefix.len() <= 500, "prefix must be ≤500 bytes");
    assert_eq!(prefix, &"a".repeat(499), "prefix should be exactly 499 a's");
}

#[test]
fn test_ascii_truncation_at_500() {
    // 501 ASCII bytes — truncation at exactly byte 500
    let body = "a".repeat(501);
    assert_eq!(body.len(), 501);
    let result = format!("{}...", truncate_error_body(&body, 500));
    assert_eq!(
        result.len(),
        503,
        "output should be 500 a's + ... = 503 bytes"
    );
    assert!(result.starts_with(&"a".repeat(500)));
    assert!(result.ends_with("..."));
}

#[test]
fn test_utf8_truncation_multi_byte_heavy() {
    // 251 two-byte chars (é) = 502 bytes
    let body = "é".repeat(251);
    assert_eq!(body.len(), 502);
    let result = format!("{}...", truncate_error_body(&body, 500));
    // Content before ... should be ≤500 bytes, total ≤503
    assert!(result.len() <= 503, "output must be ≤503 bytes");
    // The prefix should be 250 é's = 500 bytes
    let prefix = truncate_error_body(&body, 500);
    assert!(prefix.len() <= 500, "prefix must be ≤500 bytes");
    assert_eq!(
        prefix.len(),
        500,
        "prefix should be exactly 500 bytes (250 é's)"
    );
    assert!(result.ends_with("..."));
    // Verify output is valid UTF-8 by converting back (no panic)
    let _ = String::from_utf8(result.into_bytes()).expect("output must be valid UTF-8");
}

#[test]
fn test_utf8_truncation_4byte_char() {
    // 499 ASCII bytes + 1 four-byte emoji (🔥) = 503 bytes
    let body = format!("{}{}", "a".repeat(499), "🔥");
    assert_eq!(body.len(), 503);
    let result = format!("{}...", truncate_error_body(&body, 500));
    // Content before ... should be ≤500 bytes
    let prefix = truncate_error_body(&body, 500);
    assert!(prefix.len() <= 500, "prefix must be ≤500 bytes");
    assert_eq!(
        prefix,
        &"a".repeat(499),
        "prefix should be 499 a's (emoji dropped)"
    );
    assert!(result.ends_with("..."));
    // Verify output is valid UTF-8
    let _ = String::from_utf8(result.into_bytes()).expect("output must be valid UTF-8");
}

#[test]
fn test_utf8_truncation_exactly_500() {
    // Exactly 500 bytes — no truncation
    let body = "a".repeat(500);
    assert_eq!(body.len(), 500);
    let result = truncate_error_body(&body, 500);
    assert_eq!(result, body, "should return original string unchanged");
    assert_eq!(result.len(), 500);
}

#[test]
fn test_utf8_truncation_empty() {
    // Empty string
    let body = String::new();
    let result = truncate_error_body(&body, 500);
    assert_eq!(result, "", "empty string should return empty");
}

#[test]
fn test_utf8_truncation_table_driven() {
    // Table-driven test with 10+ cases verifying the invariant:
    // For any UTF-8 body >500 bytes, content before "..." is a valid UTF-8 prefix ≤500 bytes.
    struct Case {
        name: &'static str,
        body: String,
        expected_prefix_len: usize,
    }

    let cases: Vec<Case> = vec![
        Case {
            name: "pure ascii >500",
            body: "b".repeat(600),
            expected_prefix_len: 500,
        },
        Case {
            name: "pure ascii =501",
            body: "c".repeat(501),
            expected_prefix_len: 500,
        },
        Case {
            name: "pure ascii =500",
            body: "d".repeat(500),
            expected_prefix_len: 500,
        },
        Case {
            name: "pure ascii =499",
            body: "e".repeat(499),
            expected_prefix_len: 499,
        },
        Case {
            name: "2-byte char heavy",
            body: "é".repeat(251),
            expected_prefix_len: 500,
        },
        Case {
            name: "2-byte char at boundary",
            body: format!("{}{}", "f".repeat(499), "é"),
            expected_prefix_len: 499,
        },
        Case {
            name: "3-byte char at boundary",
            body: format!("{}{}", "g".repeat(499), "€"),
            expected_prefix_len: 499,
        },
        Case {
            name: "4-byte char at boundary",
            body: format!("{}{}", "h".repeat(499), "🔥"),
            expected_prefix_len: 499,
        },
        Case {
            name: "empty string",
            body: String::new(),
            expected_prefix_len: 0,
        },
        Case {
            name: "exactly 1 byte",
            body: "i".to_string(),
            expected_prefix_len: 1,
        },
        Case {
            name: "mixed multi-byte",
            body: format!("{}{}{}", "a".repeat(100), "é".repeat(100), "🔥".repeat(50)),
            expected_prefix_len: 500,
        },
        Case {
            name: "3-byte char heavy",
            body: "€".repeat(167),
            expected_prefix_len: 498,
        },
    ];

    for case in &cases {
        let prefix = truncate_error_body(&case.body, 500);
        assert!(
            prefix.len() <= 500,
            "case '{}': prefix length {} exceeds 500 bytes",
            case.name,
            prefix.len()
        );
        if case.body.len() > 500 {
            // Prefix must be a valid prefix of the original body
            assert!(
                case.body.starts_with(prefix),
                "case '{}': prefix is not a prefix of the body",
                case.name
            );
        } else {
            assert_eq!(
                prefix, case.body,
                "case '{}': body ≤500 bytes should return unchanged",
                case.name
            );
        }
        assert_eq!(
            prefix.len(),
            case.expected_prefix_len,
            "case '{}': expected prefix length {} but got {}",
            case.name,
            case.expected_prefix_len,
            prefix.len()
        );
    }
}
