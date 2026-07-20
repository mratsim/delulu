use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Deserialize, Serialize)]
pub struct OpenApiSpec {
    pub openapi: String,
    pub info: Info,
    #[serde(default)]
    pub servers: Vec<Server>,
    pub paths: Paths,
    #[serde(default)]
    pub components: Component,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct Info {
    pub title: String,
    #[serde(default)]
    pub description: Option<String>,
    pub version: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct Server {
    pub url: String,
    #[serde(default)]
    pub description: Option<String>,
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
    #[serde(default)]
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
    #[serde(default)]
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
pub struct RequestBody {
    #[serde(default)]
    pub description: Option<String>,
    pub content: Option<HashMap<String, MediaType>>,
    #[serde(default)]
    pub required: bool,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct MediaType {
    #[serde(default)]
    pub schema: Option<ParameterSchema>,
}

#[derive(Debug, Default, Deserialize, Serialize)]
pub struct Component {
    #[serde(default)]
    pub schemas: Option<HashMap<String, ParameterSchema>>,
}

impl OpenApiSpec {
    pub fn from_json(json: &str) -> Result<Self> {
        serde_json::from_str(json).context("Failed to parse OpenAPI spec")
    }

    pub fn from_file(path: &str) -> Result<Self> {
        let json = std::fs::read_to_string(path).context("Failed to read OpenAPI file")?;
        Self::from_json(&json)
    }

    pub fn base_url(&self) -> Option<&str> {
        self.servers.first().map(|s| s.url.as_str())
    }

    pub fn get_operation(&self, path: &str, method: &str) -> Option<&Operation> {
        self.paths
            .0
            .get(path)
            .and_then(|path_item| match method.to_lowercase().as_str() {
                "get" => path_item.get.as_ref(),
                "post" => path_item.post.as_ref(),
                _ => None,
            })
    }
}
