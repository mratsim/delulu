// KNOWN-DEFERRED: Pre-existing error-swallowing patterns in mcpify crate.
// These are NOT fixed by this session. Tests must NOT accidentally rely on
// their silent-fallback behavior.
//
// 1. server.rs:113 — `serde_json::to_value(&result).unwrap_or(json!({"error":
//    "Serialization failed"}))`: If ProxyResponse serialization fails, returns
//    hardcoded error JSON instead of panicking.
// 2. server.rs:115 — `serde_json::to_string(&json).unwrap_or_default()`: If
//    error JSON itself can't serialize, returns empty string.
// 3. proxy.rs:38 — `response.text().await.unwrap_or_default()`: If HTTP body
//    read fails, returns empty string.
// 4. proxy.rs:42-43 — Non-JSON 2xx response silently wrapped as
//    `ProxyResponse::success(Value::String(body))`.
use delulu_mcpify::{McpifyServer, OpenApiSpec};
use serde_json::json;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn make_spec(json_value: serde_json::Value) -> OpenApiSpec {
    serde_json::from_value(json_value).expect("Test fixture is not a valid OpenApiSpec")
}

// ---------------------------------------------------------------------------
// Minimal spec registers tool with correct name
// ---------------------------------------------------------------------------

#[test]
fn test_minimal_spec_registers_tool() {
    let spec = make_spec(json!({
        "openapi": "3.0.0",
        "info": { "title": "Test", "version": "1.0.0" },
        "paths": {
            "/users": {
                "get": {
                    "operationId": "listUsers",
                    "description": "List all users",
                    "parameters": []
                }
            }
        },
        "servers": [{"url": "https://example.com"}]
    }));
    let server = McpifyServer::from_openapi(&spec).expect("Server should build");
    let tools = server.list_tools();

    assert_eq!(tools.len(), 1, "expected exactly 1 tool");
    assert_eq!(
        tools[0].name, "listUsers",
        "tool name should match operationId"
    );
    assert_eq!(
        tools[0].description.as_deref(),
        Some("List all users"),
        "tool description should match spec description"
    );
}

// ---------------------------------------------------------------------------
// Tool input schema has correct top-level structure
// ---------------------------------------------------------------------------

#[test]
fn test_input_schema_has_object_type() {
    let spec = make_spec(json!({
        "openapi": "3.0.0",
        "info": { "title": "Test", "version": "1.0.0" },
        "paths": {
            "/items/{id}": {
                "get": {
                    "operationId": "getItem",
                    "parameters": [
                        {
                            "name": "id",
                            "in": "path",
                            "required": true,
                            "schema": { "type": "integer" }
                        }
                    ]
                }
            }
        },
        "servers": [{"url": "https://example.com"}]
    }));
    let server = McpifyServer::from_openapi(&spec).expect("Server should build");
    let tools = server.list_tools();

    assert_eq!(tools.len(), 1);
    assert!(
        tools[0].input_schema.contains_key("properties"),
        "input_schema must have a 'properties' key"
    );
    assert_eq!(
        tools[0].input_schema["type"].as_str(),
        Some("object"),
        "input_schema type must be 'object'"
    );
    assert!(
        tools[0].input_schema["properties"].is_object(),
        "input_schema['properties'] must be a JSON object"
    );
}

// ---------------------------------------------------------------------------
// Required parameters appear in required array
// ---------------------------------------------------------------------------

#[test]
fn test_required_params_in_required_array() {
    let spec = make_spec(json!({
        "openapi": "3.0.0",
        "info": { "title": "Test", "version": "1.0.0" },
        "paths": {
            "/users/{userId}": {
                "get": {
                    "operationId": "getUser",
                    "parameters": [
                        {
                            "name": "userId",
                            "in": "path",
                            "required": true,
                            "schema": { "type": "string" }
                        },
                        {
                            "name": "filter",
                            "in": "query",
                            "required": false,
                            "schema": { "type": "string" }
                        }
                    ]
                }
            }
        },
        "servers": [{"url": "https://example.com"}]
    }));
    let server = McpifyServer::from_openapi(&spec).expect("Server should build");
    let tools = server.list_tools();

    assert_eq!(tools.len(), 1);

    let required = tools[0].input_schema["required"]
        .as_array()
        .expect("input_schema should have a 'required' array");

    let required_names: Vec<&str> = required
        .iter()
        .map(|v| v.as_str().expect("required entry must be a string"))
        .collect();

    assert!(
        required_names.contains(&"userId"),
        "required array must contain 'userId'"
    );
    assert!(
        !required_names.contains(&"filter"),
        "required array must NOT contain 'filter' (optional param)"
    );
}

// ---------------------------------------------------------------------------
// Path and query parameters both appear in properties
// ---------------------------------------------------------------------------

#[test]
fn test_path_and_query_params_in_properties() {
    let spec = make_spec(json!({
        "openapi": "3.0.0",
        "info": { "title": "Test", "version": "1.0.0" },
        "paths": {
            "/pets/{petId}": {
                "get": {
                    "operationId": "getPet",
                    "parameters": [
                        {
                            "name": "petId",
                            "in": "path",
                            "required": true,
                            "schema": { "type": "string" }
                        },
                        {
                            "name": "status",
                            "in": "query",
                            "required": false,
                            "schema": { "type": "string" }
                        }
                    ]
                }
            }
        },
        "servers": [{"url": "https://example.com"}]
    }));
    let server = McpifyServer::from_openapi(&spec).expect("Server should build");
    let tools = server.list_tools();

    assert_eq!(tools.len(), 1);

    let properties = tools[0].input_schema["properties"]
        .as_object()
        .expect("input_schema must have a 'properties' object");

    assert!(
        properties.contains_key("petId"),
        "properties must contain path param 'petId'"
    );
    assert!(
        properties.contains_key("status"),
        "properties must contain query param 'status'"
    );

    // Each property must be a schema object with a 'type' field
    for (name, prop) in properties {
        let prop_obj = prop
            .as_object()
            .unwrap_or_else(|| panic!("property '{}' must be a JSON object", name));
        assert!(
            prop_obj.contains_key("type"),
            "property '{}' must have a 'type' field",
            name
        );
    }
}

// ---------------------------------------------------------------------------
// Operations without operationId do not crash / empty operationId skipped
// ---------------------------------------------------------------------------

#[test]
fn test_operation_without_operation_id_skipped() {
    let spec = make_spec(json!({
        "openapi": "3.0.0",
        "info": { "title": "Test", "version": "1.0.0" },
        "paths": {
            "/users": {
                "get": {
                    "operationId": "listUsers",
                    "parameters": []
                }
            },
            "/items": {
                "get": {
                    "description": "No operationId here",
                    "parameters": []
                }
            }
        },
        "servers": [{"url": "https://example.com"}]
    }));
    let server = McpifyServer::from_openapi(&spec).expect("Server should build");
    let tools = server.list_tools();

    assert_eq!(
        tools.len(),
        1,
        "only the operation WITH operationId should register"
    );
    assert_eq!(
        tools[0].name, "listUsers",
        "tool name should match the valid operationId"
    );
}

#[test]
fn test_operation_with_empty_operation_id_skipped() {
    let spec = make_spec(json!({
        "openapi": "3.0.0",
        "info": { "title": "Test", "version": "1.0.0" },
        "paths": {
            "/valid": {
                "get": {
                    "operationId": "valid",
                    "parameters": []
                }
            },
            "/empty": {
                "get": {
                    "operationId": "",
                    "parameters": []
                }
            }
        },
        "servers": [{"url": "https://example.com"}]
    }));
    let server = McpifyServer::from_openapi(&spec).expect("Server should build");
    let tools = server.list_tools();

    assert_eq!(tools.len(), 1, "empty operationId should be skipped");
    assert_eq!(
        tools[0].name, "valid",
        "only the non-empty operationId should register"
    );
}

// ---------------------------------------------------------------------------
// Empty spec returns empty tool list
// ---------------------------------------------------------------------------

#[test]
fn test_empty_paths_empty_tools() {
    let spec = make_spec(json!({
        "openapi": "3.0.0",
        "info": { "title": "Test", "version": "1.0.0" },
        "paths": {},
        "servers": [{"url": "https://example.com"}]
    }));
    let server = McpifyServer::from_openapi(&spec).expect("Server should build");
    let tools = server.list_tools();

    assert_eq!(tools.len(), 0, "empty paths should produce zero tools");
}

#[test]
fn test_missing_paths_key_empty_tools() {
    // `paths` field in OpenApiSpec does NOT have #[serde(default)],
    // so a missing key causes deser error instead of zero tools.
    let result: Result<OpenApiSpec, _> = serde_json::from_value(json!({
        "openapi": "3.0.0",
        "info": { "title": "Test", "version": "1.0.0" },
        "servers": [{"url": "https://example.com"}]
    }));
    assert!(
        result.is_err(),
        "missing paths key should cause deserialization error"
    );
}

// ---------------------------------------------------------------------------
// Spec missing servers returns error
// ---------------------------------------------------------------------------

#[test]
fn test_missing_servers_returns_error() {
    // "servers" key absent entirely
    let spec = make_spec(json!({
        "openapi": "3.0.0",
        "info": { "title": "Test", "version": "1.0.0" },
        "paths": {
            "/users": {
                "get": {
                    "operationId": "listUsers",
                    "parameters": []
                }
            }
        }
    }));
    let result = McpifyServer::from_openapi(&spec);

    assert!(result.is_err(), "expected Err when servers key is absent");
    let err = format!("{}", result.err().expect("already checked is_err"));
    assert!(
        err.contains("No servers defined"),
        "error message should contain 'No servers defined', got: {}",
        err
    );
    // TODO: Replace string match with typed error variant once McpifyError enum is defined
}

#[test]
fn test_empty_servers_returns_error() {
    // "servers" key present but empty array
    let spec = make_spec(json!({
        "openapi": "3.0.0",
        "info": { "title": "Test", "version": "1.0.0" },
        "paths": {
            "/users": {
                "get": {
                    "operationId": "listUsers",
                    "parameters": []
                }
            }
        },
        "servers": []
    }));
    let result = McpifyServer::from_openapi(&spec);

    assert!(result.is_err(), "expected Err when servers is empty array");
    let err = format!("{}", result.err().expect("already checked is_err"));
    assert!(
        err.contains("No servers defined"),
        "error message should contain 'No servers defined', got: {}",
        err
    );
    // TODO: Replace string match with typed error variant once McpifyError enum is defined
}

// ---------------------------------------------------------------------------
// POST operation registers tool correctly
// ---------------------------------------------------------------------------

#[test]
fn test_post_operation_registers_tool() {
    let spec = make_spec(json!({
        "openapi": "3.0.0",
        "info": { "title": "Test", "version": "1.0.0" },
        "paths": {
            "/resources": {
                "post": {
                    "operationId": "createResource",
                    "parameters": [
                        {
                            "name": "name",
                            "in": "query",
                            "required": true,
                            "schema": { "type": "string" }
                        }
                    ]
                }
            }
        },
        "servers": [{"url": "https://example.com"}]
    }));
    let server = McpifyServer::from_openapi(&spec).expect("Server should build");
    let tools = server.list_tools();

    assert_eq!(
        tools.len(),
        1,
        "POST operation should register exactly one tool"
    );
    assert_eq!(
        tools[0].name, "createResource",
        "tool name should match POST operationId"
    );
    assert_eq!(
        tools[0].input_schema["type"].as_str(),
        Some("object"),
        "input_schema type must be 'object'"
    );
    assert!(
        tools[0].input_schema.contains_key("properties"),
        "input_schema must have a 'properties' key"
    );
}

// ---------------------------------------------------------------------------
// Negative test: Unsupported HTTP methods produce zero tools
// ---------------------------------------------------------------------------

#[test]
fn test_unsupported_methods_produce_zero_tools() {
    // PUT and DELETE operations are silently dropped by PathItem struct.
    // Use separate path entries to avoid JSON key collision.
    let spec = make_spec(json!({
        "openapi": "3.0.0",
        "info": { "title": "Test", "version": "1.0.0" },
        "paths": {
            "/resource-put": {
                "put": {
                    "operationId": "updateResource",
                    "parameters": []
                }
            },
            "/resource-delete": {
                "delete": {
                    "operationId": "deleteResource",
                    "parameters": []
                }
            }
        },
        "servers": [{"url": "https://example.com"}]
    }));
    let server = McpifyServer::from_openapi(&spec).expect("Server should build");
    let tools = server.list_tools();

    assert_eq!(tools.len(), 0,);
}

// ---------------------------------------------------------------------------
// POST operation with requestBody includes body property in input schema
// ---------------------------------------------------------------------------

#[test]
fn test_post_with_request_body_includes_body_property() {
    let spec = make_spec(json!({
        "openapi": "3.0.0",
        "info": { "title": "Test", "version": "1.0.0" },
        "paths": {
            "/resources": {
                "post": {
                    "operationId": "createResource",
                    "parameters": [
                        {
                            "name": "name",
                            "in": "query",
                            "required": true,
                            "schema": { "type": "string" }
                        }
                    ],
                    "requestBody": {
                        "description": "Resource data",
                        "content": {
                            "application/json": {
                                "schema": {
                                    "type": "object",
                                    "properties": {
                                        "title": { "type": "string" },
                                        "price": { "type": "number" }
                                    }
                                }
                            }
                        },
                        "required": true
                    }
                }
            }
        },
        "servers": [{"url": "https://example.com"}]
    }));
    let server = McpifyServer::from_openapi(&spec).expect("Server should build");
    let tools = server.list_tools();

    assert_eq!(
        tools.len(),
        1,
        "POST operation should register exactly one tool"
    );
    assert_eq!(tools[0].name, "createResource");

    let properties = tools[0].input_schema["properties"]
        .as_object()
        .expect("input_schema must have a 'properties' object");

    // The request body should appear as a 'body' property
    assert!(
        properties.contains_key("body"),
        "input schema properties must contain 'body' from requestBody"
    );

    let body_prop = properties["body"]
        .as_object()
        .expect("'body' property must be a JSON object");
    assert_eq!(
        body_prop["type"].as_str(),
        Some("object"),
        "body schema type should be 'object'"
    );

    // The required array should include 'body' since requestBody.required = true
    let required = tools[0].input_schema["required"]
        .as_array()
        .expect("input_schema should have a 'required' array");
    let required_names: Vec<&str> = required
        .iter()
        .map(|v| v.as_str().expect("required entry must be a string"))
        .collect();
    assert!(
        required_names.contains(&"body"),
        "required array must contain 'body' when requestBody.required is true"
    );
    assert!(required_names.contains(&"name"),);
}
