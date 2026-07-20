use anyhow::Result;
use serde::Serialize;
use serde_json::Value;
use std::collections::HashMap;

pub struct ProxyClient {
    client: wreq::Client,
}

impl ProxyClient {
    pub fn new() -> Result<Self> {
        let client = wreq::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()?;
        Ok(Self { client })
    }

    /// Execute an HTTP GET request against the constructed URL.
    ///
    /// Parameters named in `path_param_names` are substituted into `{paramName}`
    /// placeholders in the path template and excluded from the query string.
    /// All other parameters are appended as query string values, sorted by key
    /// for deterministic output.
    pub async fn get(
        &self,
        base_url: &str,
        path: &str,
        params: HashMap<String, Value>,
        path_param_names: &[String],
    ) -> ProxyResponse {
        let url = build_url(base_url, path, params, path_param_names);

        tracing::debug!("Proxy GET: {}", url);

        match self.client.get(url.as_str()).send().await {
            Ok(response) => {
                let status = response.status();
                let body = response.text().await.unwrap_or_default();

                if status.is_success() || status.is_redirection() {
                    match serde_json::from_str::<serde_json::Value>(&body) {
                        Ok(json) => ProxyResponse::success(json),
                        Err(_) => ProxyResponse::success(Value::String(body)),
                    }
                } else {
                    let truncated = if body.len() > 500 {
                        format!("{}...", &body[..500])
                    } else {
                        body
                    };
                    ProxyResponse::error(format!("HTTP {}: {}", status, truncated))
                }
            }
            Err(e) => ProxyResponse::error(format!("Request failed: {}", e)),
        }
    }
}

/// Convert a JSON value to its string representation for URL usage.
fn param_value_to_string(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        Value::Bool(b) => b.to_string(),
        other => other.to_string(),
    }
}

fn build_url(
    base_url: &str,
    path: &str,
    params: HashMap<String, Value>,
    path_param_names: &[String],
) -> String {
    // Join base URL and path, avoiding double slashes
    let base = base_url.trim_end_matches('/');
    let mut result = format!("{}/", base);

    // Substitute path parameters into the path template
    let mut resolved_path = path.to_string();
    let mut query_params: Vec<(String, String)> = Vec::new();

    for (key, value) in params {
        if path_param_names.contains(&key) {
            // Path parameter: substitute {key} placeholder in the path template
            let placeholder = format!("{{{}}}", key);
            let val_str = param_value_to_string(&value);
            let encoded = urlencoding::encode(&val_str);
            resolved_path = resolved_path.replace(&placeholder, &encoded);
        } else {
            // Query parameter: collect for query string construction
            if let Some(v) = value.as_str() {
                if !v.is_empty() {
                    query_params.push((key, v.to_string()));
                }
            } else {
                let val_str = param_value_to_string(&value);
                query_params.push((key, val_str));
            }
        }
    }

    result.push_str(resolved_path.trim_start_matches('/'));

    // Sort query params by key for deterministic output
    query_params.sort_by(|a, b| a.0.cmp(&b.0));

    if !query_params.is_empty() {
        result.push('?');
        for (i, (key, val)) in query_params.iter().enumerate() {
            if i > 0 {
                result.push('&');
            }
            result.push_str(key);
            result.push('=');
            result.push_str(&urlencoding::encode(val));
        }
    }

    result
}

#[derive(Debug, serde::Serialize)]
pub struct ProxyResponse {
    success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    data: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

impl ProxyResponse {
    fn success<T: Serialize>(data: T) -> Self {
        Self {
            success: true,
            data: Some(serde_json::to_value(data).unwrap_or(Value::Null)),
            error: None,
        }
    }

    fn error<T: Into<String>>(msg: T) -> Self {
        Self {
            success: false,
            data: None,
            error: Some(msg.into()),
        }
    }
}

#[cfg(test)]
#[path = "../tests/unit/proxy_test.rs"]
mod tests;
