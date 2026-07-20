use super::*;

#[test]
fn test_from_yaml_minimal_spec() {
    let yaml = r#"
openapi: "3.0.0"
info:
  title: Test API
  version: "1.0"
paths:
  /users:
    get:
      operationId: listUsers
      parameters: []
servers:
  - url: https://example.com
"#;
    let spec = OpenApiSpec::from_yaml(yaml).expect("valid YAML spec");
    assert_eq!(spec.info.title, "Test API");
    assert_eq!(spec.base_url(), Some("https://example.com"));
}

#[test]
fn test_from_file_json() {
    // from_file should still accept JSON (no extension = treated as JSON)
    let json = r#"{"openapi":"3.0.0","info":{"title":"JSON","version":"1.0"},"paths":{},"servers":[{"url":"https://json.com"}]}"#;
    // Write to a temp file with no extension
    let p = std::env::temp_dir().join("test_openapi_no_ext.json");
    std::fs::write(&p, json).unwrap();
    let spec = OpenApiSpec::from_file(p.to_str().unwrap()).expect("JSON file");
    assert_eq!(spec.info.title, "JSON");
    std::fs::remove_file(&p).ok();
}

#[test]
fn test_from_invalid_yaml() {
    let result = OpenApiSpec::from_yaml("not: valid: openapi: [[[");
    assert!(result.is_err(), "invalid YAML should return error");
}
#[test]
fn test_from_file_yaml_uppercase_extension() {
    let yaml = r#"
openapi: "3.0.0"
info:
  title: YAML Test
  version: "1.0"
paths: {}
servers:
  - url: https://example.com
"#;
    let p = std::env::temp_dir().join("test_openapi_upper.YAML");
    std::fs::write(&p, yaml).unwrap();
    let spec = OpenApiSpec::from_file(p.to_str().unwrap())
        .expect(".YAML file should be parsed as YAML");
    assert_eq!(spec.info.title, "YAML Test");
    std::fs::remove_file(&p).ok();
}

#[test]
fn test_from_file_yml_lowercase_extension() {
    let yaml = r#"
openapi: "3.0.0"
info:
  title: Yml Test
  version: "1.0"
paths: {}
servers:
  - url: https://example.com
"#;
    let p = std::env::temp_dir().join("test_openapi_mixed.yml");
    std::fs::write(&p, yaml).unwrap();
    let spec = OpenApiSpec::from_file(p.to_str().unwrap())
        .expect(".yml file should be parsed as YAML");
    assert_eq!(spec.info.title, "Yml Test");
    std::fs::remove_file(&p).ok();
}
