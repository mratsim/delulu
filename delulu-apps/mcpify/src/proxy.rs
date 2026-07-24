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
            .redirect(wreq::redirect::Policy::none())
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
            Ok(response) => Self::process_response(response).await,
            Err(e) => ProxyResponse::error(format!("Request failed: {}", e)),
        }
    }

    /// Execute an HTTP POST request against the constructed URL.
    ///
    /// Path parameters are substituted into `{paramName}` placeholders (same as GET).
    /// Query parameters are appended to the URL query string.
    /// The designated `body_key` parameter (if present) is unwrapped and sent as JSON body.
    pub async fn post(
        &self,
        base_url: &str,
        path: &str,
        params: HashMap<String, Value>,
        path_param_names: &[String],
        query_param_names: &[String],
        body_key: Option<&str>,
    ) -> ProxyResponse {
        let mut url_params = HashMap::new();
        let mut json_body = None;

        for (k, v) in params {
            if path_param_names.contains(&k) || query_param_names.contains(&k) {
                url_params.insert(k, v);
            } else if Some(k.as_str()) == body_key {
                json_body = Some(v);
            }
        }

        let url = build_url(base_url, path, url_params, path_param_names);

        tracing::debug!(
            "Proxy POST: {} body={}",
            url,
            serde_json::to_string(&json_body).unwrap_or_default()
        );

        let req = self.client.post(url.as_str());
        let req = if let Some(b) = json_body {
            req.json(&b)
        } else {
            req
        };
        match req.send().await {
            Ok(response) => Self::process_response(response).await,
            Err(e) => ProxyResponse::error(format!("Request failed: {}", e)),
        }
    }

    /// Process an HTTP response: check size limits, read body incrementally,
    /// parse JSON, and return a ProxyResponse.
    async fn process_response(response: wreq::Response) -> ProxyResponse {
        let status = response.status();

        // Limit response to ~1M tokens (conservative 512KB) to fit in LLM context windows
        if let Some(cl) = response
            .headers()
            .get("content-length")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse::<usize>().ok())
        {
            if cl > 524_288 {
                return ProxyResponse::error(format!(
                    "Response too large: {} bytes (max 512KB)",
                    cl
                ));
            }
        }

        // Read body incrementally with size cap
        let mut body_bytes: Vec<u8> = Vec::new();
        use futures::StreamExt;
        let mut stream = response.bytes_stream();
        while let Some(chunk) = stream.next().await {
            match chunk {
                Ok(data) => {
                    body_bytes.extend_from_slice(&data);
                    if body_bytes.len() > 524_288 {
                        return ProxyResponse::error(format!(
                            "Response body too large: exceeded 512KB limit"
                        ));
                    }
                }
                Err(e) => return ProxyResponse::error(format!("Body read failed: {}", e)),
            }
        }

        // Safety check even without Content-Length header
        if body_bytes.len() > 524_288 {
            return ProxyResponse::error(format!(
                "Response body too large: {} bytes (max 512KB)",
                body_bytes.len()
            ));
        }
        let body = match String::from_utf8(body_bytes) {
            Ok(s) => s,
            Err(e) => {
                return ProxyResponse::error(format!(
                    "Response body is not valid UTF-8: {} at byte offset {}",
                    e.utf8_error(),
                    e.utf8_error().valid_up_to()
                ));
            }
        };

        if status.is_success() {
            match serde_json::from_str::<serde_json::Value>(&body) {
                Ok(json) => ProxyResponse::success(json),
                Err(e) => {
                    let truncated = if body.len() > 500 {
                        format!("{}...", truncate_error_body(&body, 500))
                    } else {
                        body.clone()
                    };
                    ProxyResponse::error(format!(
                        "Non-JSON response (HTTP {}): {} — parse error: {}",
                        status, truncated, e
                    ))
                }
            }
        } else {
            let truncated = if body.len() > 500 {
                format!("{}...", truncate_error_body(&body, 500))
            } else {
                body
            };
            ProxyResponse::error(format!("HTTP {}: {}", status, truncated))
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
            result.push_str(&urlencoding::encode(key));
            result.push('=');
            result.push_str(&urlencoding::encode(val));
        }
    }

    result
}

#[derive(Debug, serde::Serialize)]
pub struct ProxyResponse {
    pub(crate) success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) data: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) error: Option<String>,
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

/// Returns the longest valid UTF-8 prefix of `body` whose byte length is ≤ `max_bytes`.
/// Falls back to the full string if `body.len() <= max_bytes`.
/// Never panics on valid UTF-8 input.
fn truncate_error_body(body: &str, max_bytes: usize) -> &str {
    if body.len() > max_bytes {
        // Walk backward from max_bytes to find the last UTF-8 character boundary
        // at or before max_bytes. At most 4 iterations (max UTF-8 char width = 4 bytes,
        // so worst case: byte 500 is the 4th byte of a 4-byte char, walk back 3 steps).
        // This avoids panic when byte max_bytes falls in the middle of a multi-byte
        // character and guarantees the content before "..." is ≤ max_bytes bytes.
        let mut end = max_bytes;
        debug_assert!(end > 0, "max_bytes must be > 0 for the truncation path");
        while !body.is_char_boundary(end) {
            end -= 1;
        }
        &body[..end]
    } else {
        body
    }
}

#[cfg(test)]
#[path = "../tests/unit/proxy_test.rs"]
mod tests;
