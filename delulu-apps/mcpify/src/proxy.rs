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

    pub async fn get(
        &self,
        base_url: &str,
        path: &str,
        params: HashMap<String, Value>,
    ) -> ProxyResponse {
        let url = match build_url(base_url, path, params) {
            Ok(u) => u,
            Err(e) => return ProxyResponse::error(format!("Failed to build URL: {}", e)),
        };

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

fn build_url(base_url: &str, path: &str, params: HashMap<String, Value>) -> Result<String, String> {
    let mut url = base_url.to_string();
    if !url.ends_with('/') && !path.starts_with('/') {
        url.push('/');
    }
    url.push_str(path);

    let mut query_params: Vec<(String, String)> = Vec::new();
    for (key, value) in params {
        if let Some(v) = value.as_str() {
            if !v.is_empty() {
                query_params.push((key.clone(), v.to_string()));
            }
        } else if let Some(v) = value.as_i64() {
            query_params.push((key.clone(), v.to_string()));
        } else if let Some(v) = value.as_f64() {
            query_params.push((key.clone(), v.to_string()));
        } else if let Some(v) = value.as_bool() {
            query_params.push((key.clone(), v.to_string()));
        } else {
            query_params.push((key.clone(), value.to_string()));
        }
    }

    if !query_params.is_empty() {
        url.push('?');
        for (i, (key, val)) in query_params.iter().enumerate() {
            if i > 0 {
                url.push('&');
            }
            url.push_str(key);
            url.push('=');
            url.push_str(&urlencoding::encode(val));
        }
    }

    Ok(url)
}

#[derive(Debug, serde::Serialize)]
pub struct ProxyResponse {
    pub success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl ProxyResponse {
    pub fn success<T: Serialize>(data: T) -> Self {
        Self {
            success: true,
            data: Some(serde_json::to_value(data).unwrap_or(Value::Null)),
            error: None,
        }
    }

    pub fn error<T: Into<String>>(msg: T) -> Self {
        Self {
            success: false,
            data: None,
            error: Some(msg.into()),
        }
    }
}
