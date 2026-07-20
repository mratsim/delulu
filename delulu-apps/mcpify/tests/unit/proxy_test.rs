use super::*;
use serde_json::json;

#[test]
fn test_path_param_substitution() {
    let params = HashMap::from([("userId".to_string(), json!("42"))]);
    let path_params = vec!["userId".to_string()];
    let result = build_url("http://example.com", "/users/{userId}", params, &path_params);
    assert_eq!(result, "http://example.com/users/42");
}

#[test]
fn test_multiple_path_params() {
    let params = HashMap::from([
        ("from".to_string(), json!("JFK")),
        ("to".to_string(), json!("LAX")),
    ]);
    let path_params = vec!["from".to_string(), "to".to_string()];
    let result =
        build_url("http://example.com", "/flights/{from}/{to}", params, &path_params);
    assert_eq!(result, "http://example.com/flights/JFK/LAX");
}

#[test]
fn test_mixed_path_and_query_params() {
    let params = HashMap::from([
        ("userId".to_string(), json!("42")),
        ("include".to_string(), json!("profile")),
    ]);
    let path_params = vec!["userId".to_string()];
    let result =
        build_url("http://example.com", "/users/{userId}", params, &path_params);
    assert_eq!(
        result,
        "http://example.com/users/42?include=profile"
    );
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
    let result =
        build_url("http://example.com", "/users/{userId}", params, &path_params);
    // The param is consumed (not in query string) but placeholder remains
    assert_eq!(result, "http://example.com/users/{userId}");
}
