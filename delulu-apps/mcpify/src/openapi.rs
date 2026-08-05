use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

#[derive(Debug, Deserialize, Serialize)]
pub struct OpenApiSpec {
    openapi: String,
    pub info: Info,
    #[serde(default)]
    servers: Vec<Server>,
    pub paths: Paths,
    #[serde(default)]
    components: Component,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct Info {
    pub title: String,
    #[serde(default)]
    description: Option<String>,
    version: String,
}

#[derive(Debug, Deserialize, Serialize)]
struct Server {
    url: String,
    #[serde(default)]
    description: Option<String>,
}

#[derive(Debug, Deserialize, Default, Serialize)]
pub struct Paths(pub HashMap<String, PathItem>);

impl Paths {
    pub fn iter(&self) -> impl Iterator<Item = (&str, &PathItem)> {
        self.0.iter().map(|(k, v)| (k.as_str(), v))
    }
}

#[derive(Debug, Deserialize, Serialize)]
pub struct PathItem {
    #[serde(default)]
    pub get: Option<Operation>,
    #[serde(default)]
    pub post: Option<Operation>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct Operation {
    #[serde(rename = "operationId")]
    pub operation_id: Option<String>,
    #[serde(default)]
    pub summary: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub parameters: Vec<Parameter>,
    #[serde(default, rename = "requestBody")]
    pub request_body: Option<RequestBody>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct Parameter {
    pub name: String,
    #[serde(rename = "in")]
    pub location: ParameterLocation,
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub description: Option<String>,
    pub schema: Option<ParameterSchema>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ParameterLocation {
    Query,
    Path,
    Header,
    Cookie,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct ParameterSchema {
    #[serde(rename = "type")]
    pub param_type: Option<String>,
    #[serde(default)]
    pub format: Option<String>,
    #[serde(rename = "enum", default)]
    pub enum_values: Option<Vec<serde_json::Value>>,
    #[serde(default)]
    pub default: Option<serde_json::Value>,
    #[serde(default)]
    pub items: Option<Box<ParameterSchema>>,
    #[serde(default)]
    pub properties: Option<HashMap<String, Box<ParameterSchema>>>,
    #[serde(default)]
    pub required: Option<Vec<String>>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub examples: Option<serde_json::Value>,
    #[serde(default)]
    pub nullable: Option<bool>,
    #[serde(default)]
    pub minimum: Option<serde_json::Value>,
    #[serde(default)]
    pub maximum: Option<serde_json::Value>,
}
#[derive(Debug, Deserialize, Serialize)]
/// Describes an OpenAPI 3.x Request Body Object.
///
/// `content` maps media type strings (e.g., `"application/json"`) to their
/// schema definitions. `required` indicates whether the body is mandatory
/// in the request.
pub struct RequestBody {
    #[serde(default)]
    pub description: Option<String>,
    pub content: Option<HashMap<String, MediaType>>,
    #[serde(default)]
    pub required: bool,
}
#[derive(Debug, Deserialize, Serialize)]
/// An OpenAPI 3.x Media Type Object.
///
/// Wraps the schema definition associated with a specific media type
/// (e.g., `application/json`) in a request or response body.
pub struct MediaType {
    #[serde(default)]
    pub schema: Option<ParameterSchema>,
}

#[derive(Debug, Default, Deserialize, Serialize)]
struct Component {
    #[serde(default)]
    schemas: Option<HashMap<String, ParameterSchema>>,
}

impl OpenApiSpec {
    fn from_json(json: &str) -> Result<Self> {
        serde_json::from_str(json).context("Failed to parse OpenAPI spec")
    }

    fn from_yaml(yaml: &str) -> Result<Self> {
        let value: serde_json::Value =
            serde_yaml_neo::from_str(yaml).context("Failed to parse YAML OpenAPI spec")?;
        serde_json::from_value(value).context("Failed to convert YAML to OpenAPI spec")
    }

    pub fn from_file(path: &str) -> Result<Self> {
        // TODO side-effect to push to main: fs::read_to_string (explicit input arg)
        let content = std::fs::read_to_string(path).context("Failed to read OpenAPI file")?;
        let p = Path::new(path);
        match p
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_lowercase())
            .as_deref()
        {
            Some("yaml") | Some("yml") => Self::from_yaml(&content),
            _ => Self::from_json(&content),
        }
    }

    pub fn base_url(&self) -> Option<&str> {
        self.servers.first().map(|s| s.url.as_str())
    }
}

#[cfg(test)]
#[path = "../tests/unit/openapi_test.rs"]
mod tests;
