//! HTTP fetch layer for the delulu-webfetch agent.
//!
//! Provides `WebfetchClient` — an HTTP client with:
//! - Per-domain QPS rate limiting via `delulu-query-queues`
//! - Configurable retry logic (429 with exponential jitter, 5xx, connection errors)
//! - URL validation and platform detection (Reddit API, Discourse API)
//! - Bot-detection page recognition
//! - Total-operation timeout

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use delulu_query_queues::QueryQueue;
use rand::Rng;
use tokio::sync::Mutex;
use tokio::time;
use url::Url;
use wreq::redirect::Policy;
use wreq_util::Emulation;

use super::types::*;
use crate::core::detect::{
    detect_from_content, detect_source_type, is_bot_detected, reddit_url_to_api_url,
};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Maximum allowed URL length.
const MAX_URL_LENGTH: usize = 2048;

/// Maximum response body size (10 MB).
const MAX_BODY_SIZE: usize = 10 * 1024 * 1024;

/// Default per-domain QPS limit.
const DEFAULT_QPS: u64 = 2;

/// Default fetch timeout in seconds.
const DEFAULT_TIMEOUT_SECS: u64 = 30;

/// Retry-After cap in seconds (for 429 responses).
const RETRY_AFTER_CAP_SECS: u64 = 60;

/// Initial backoff for 429 retries (1 second).
const RATELIMIT_BACKOFF_INIT_MS: u64 = 1000;

/// Delay for 5xx retries (5 seconds).
const SERVER_ERROR_DELAY_SECS: u64 = 5;

/// Number of retries for connection errors.
const CONNECTION_RETRIES: u32 = 2;

/// Delay for connection error retries (1 second).
const CONNECTION_RETRY_DELAY_MS: u64 = 1000;

// ---------------------------------------------------------------------------
// Wreq-based HttpClient implementation
// ---------------------------------------------------------------------------

/// A concrete HTTP client backed by `wreq`.
struct WreqClient {
    inner: wreq::Client,
}

#[async_trait]
impl HttpClient for WreqClient {
    async fn get(&self, url: &str) -> Result<Response, WebbfetchError> {
        let resp = self
            .inner
            .get(url)
            .send()
            .await
            .map_err(|e| WebbfetchError::Fetch(format!("HTTP request failed: {e}")))?;

        let status = resp.status().as_u16();

        // TODO: fuzz/hardening — resp.text() buffers the entire body before the
        // size check runs, creating an OOM vector on large responses. Should stream
        // chunks with a running byte count and reject early via Content-Length.
        // See https://github.com/mratsim/delulu/pull/7
        let body = resp
            .text()
            .await
            .map_err(|e| WebbfetchError::Fetch(format!("Failed to read response body: {e}")))?;

        if body.len() > MAX_BODY_SIZE {
            return Err(WebbfetchError::Fetch(format!(
                "Response body too large: {} bytes (max {})",
                body.len(),
                MAX_BODY_SIZE
            )));
        }

        Ok(Response { status, body })
    }
}

// ---------------------------------------------------------------------------
// Helper: create_http_client
// ---------------------------------------------------------------------------

/// Create a wreq-based HTTP client with Safari 18.5 emulation and sensible defaults.
fn create_http_client(timeout_secs: u64) -> impl HttpClient {
    let client = wreq::Client::builder()
        .emulation(Emulation::Safari18_5)
        .redirect(Policy::limited(5))
        .timeout(Duration::from_secs(timeout_secs))
        .connect_timeout(Duration::from_secs(timeout_secs))
        .build()
        .expect("wreq client builder should always succeed");
    WreqClient { inner: client }
}

// ---------------------------------------------------------------------------
// WebbfetchClient
// ---------------------------------------------------------------------------

/// An HTTP fetch client with per-domain rate limiting, retry logic, and
/// automatic platform detection (Reddit, Discourse, Generic HTML).
pub struct WebbfetchClient {
    /// The underlying HTTP client (boxed for test injection).
    client: Arc<dyn HttpClient>,
    /// Per-domain rate-limit queues, keyed by domain name.
    queues: Arc<Mutex<HashMap<String, Arc<QueryQueue>>>>,
    /// Default QPS for the top-level  method.
    default_qps: u64,
}

impl WebbfetchClient {
    /// Create a new `WebbfetchClient` with a wreq-based HTTP client.
    ///
    /// The client is configured with Safari 18.5 emulation, up to 5 redirects,
    /// and the specified timeout.
    pub fn new(timeout_secs: u64, qps: u64) -> Self {
        let client = create_http_client(timeout_secs);
        Self {
            client: Arc::new(client),
            queues: Arc::new(Mutex::new(HashMap::new())),
            default_qps: qps,
        }
    }

    /// Create a `WebbfetchClient` with an injected HTTP client (useful for tests).
    pub fn with_client(client: impl HttpClient + 'static) -> Self {
        Self {
            client: Arc::new(client),
            queues: Arc::new(Mutex::new(HashMap::new())),
            default_qps: DEFAULT_QPS,
        }
    }

    /// Fetch a single URL with auto-detection and default configuration.
    ///
    /// This method:
    /// 1. Validates the URL
    /// 2. Detects source type from the URL
    /// 3. Transforms known platform URLs to their API endpoints
    /// 4. Gets or creates a per-domain rate-limit queue (QPS=2)
    /// 5. Executes the fetch with retry + timeout
    /// 6. Returns a `FetchResult` with URL info and extracted content
    pub async fn fetch(&self, url: &str) -> Result<FetchResult, WebbfetchError> {
        let config = FetchConfig {
            timeout_secs: DEFAULT_TIMEOUT_SECS,
            qps: self.default_qps,
        };
        self.fetch_with_config_inner(url, &config).await
    }

    /// Fetch a single URL with per-request configuration overrides.
    pub async fn fetch_with_config(
        &self,
        url: &str,
        config: &FetchConfig,
    ) -> Result<FetchResult, WebbfetchError> {
        self.fetch_with_config_inner(url, config).await
    }

    /// Internal implementation shared by `fetch` and `fetch_with_config`.
    async fn fetch_with_config_inner(
        &self,
        url: &str,
        config: &FetchConfig,
    ) -> Result<FetchResult, WebbfetchError> {
        // 1. Validate URL
        let url = url.trim();
        if url.len() > MAX_URL_LENGTH {
            return Err(WebbfetchError::InvalidUrl(format!(
                "URL exceeds maximum length of {} characters",
                MAX_URL_LENGTH
            )));
        }

        let parsed = Url::parse(url)
            .map_err(|e| WebbfetchError::InvalidUrl(format!("Failed to parse URL: {e}")))?;

        let scheme = parsed.scheme();
        if scheme != "http" && scheme != "https" {
            return Err(WebbfetchError::InvalidUrl(format!(
                "Unsupported URL scheme: '{scheme}' (only http/https allowed)"
            )));
        }

        let domain = parsed
            .host_str()
            .ok_or_else(|| WebbfetchError::InvalidUrl("URL has no host".to_string()))?
            .to_string();

        // 2. Detect source type from URL
        let source_type = detect_source_type(url);

        // 3. Transform URL if it points to a known platform
        let fetch_url = match source_type {
            SourceType::Reddit => reddit_url_to_api_url(url),
            SourceType::Discourse => url.to_string(), // unreachable from URL detection; Discourse is detected from content in lib.rs
            SourceType::GenericHtml => url.to_string(),
        };

        // 4. Get or create per-domain queue
        let queue = self.get_or_create_queue(&domain, config.qps).await;

        // 5. Execute fetch with retry + timeout
        let result = self.execute_fetch(&queue, &fetch_url, config).await?;

        let url_info = UrlInfo {
            url: url.to_string(),
            source_type: source_type.clone(),
            domain,
        };

        Ok(FetchResult {
            url: url_info,
            content: result,
        })
    }

    /// Get an existing per-domain queue or create a new one with QPS limit.
    async fn get_or_create_queue(&self, domain: &str, qps: u64) -> Arc<QueryQueue> {
        let mut queues = self.queues.lock().await;
        queues
            .entry(domain.to_string())
            .or_insert_with(|| Arc::new(QueryQueue::with_qps_limit(qps.max(1))))
            .clone()
    }

    /// Execute the HTTP fetch with retry logic and a total-operation timeout.
    async fn execute_fetch(
        &self,
        queue: &QueryQueue,
        url: &str,
        config: &FetchConfig,
    ) -> Result<ExtractionResult, WebbfetchError> {
        // Wrap the entire operation in a timeout.
        let timeout_dur = Duration::from_secs(config.timeout_secs);

        time::timeout(timeout_dur, self.fetch_with_retry(queue, url, config))
            .await
            .map_err(|_| {
                WebbfetchError::Timeout(format!(
                    "Request timed out after {} seconds for URL: {url}",
                    config.timeout_secs
                ))
            })?
    }

    /// Core fetch with per-error retry logic.
    async fn fetch_with_retry(
        &self,
        queue: &QueryQueue,
        url: &str,
        _config: &FetchConfig,
    ) -> Result<ExtractionResult, WebbfetchError> {
        let mut retry_count_429 = 0u32;
        let max_retries_429 = 3u32;
        let mut retry_count_5xx = 0u32;
        let max_retries_5xx = 1u32;
        let mut retry_count_conn = 0u32;
        let max_retries_conn = CONNECTION_RETRIES;

        loop {
            // Use the queue for QPS rate limiting. We use max_retries=0
            // internally (handling our own retry logic above) but the queue
            // still acquires the semaphore permit and QPS token.
            //
            // We clone the Arc<dyn HttpClient> so it can be moved into the closure.
            let client_arc = Arc::clone(&self.client);
            let url_for_attempt = url.to_string();

            let response = queue
                .with_retry::<Response, _, _>(move || {
                    let url = url_for_attempt.clone();
                    let client = client_arc.clone();
                    async move { client.get(&url).await.map_err(|e| anyhow::anyhow!("{e}")) }
                })
                .await;

            let response = match response {
                Ok(r) => r,
                Err(e) => {
                    // Connection / transport error — treat as retryable
                    retry_count_conn += 1;
                    if retry_count_conn > max_retries_conn {
                        return Err(WebbfetchError::Fetch(format!(
                            "Connection failed after {max_retries_conn} retries: {e}",
                        )));
                    }
                    let jitter_ms = rand::thread_rng().gen_range(0..=CONNECTION_RETRY_DELAY_MS);
                    time::sleep(Duration::from_millis(jitter_ms)).await;
                    continue;
                }
            };

            // Check for bot detection
            if is_bot_detected(&response.body) {
                return Err(WebbfetchError::Fetch(
                    "Blocked by bot detection".to_string(),
                ));
            }

            let status = response.status;

            if (200..300).contains(&status) {
                // Attempt platform detection from content (overrides URL-based type)
                let _content_type =
                    detect_from_content(&response.body).unwrap_or(SourceType::GenericHtml);

                // Build the extraction result.
                // Store the raw response body as GenericHtml;
                // structured parsing (Reddit JSON, Discourse JSON) is deferred.
                return Ok(ExtractionResult::GenericHtml {
                    content_md: MarkdownDocument {
                        frontmatter: String::new(),
                        body: response.body,
                    },
                });
            }

            // Handle non-2xx status codes
            match status {
                429 => {
                    retry_count_429 += 1;
                    if retry_count_429 > max_retries_429 {
                        return Err(WebbfetchError::RetryExhausted(retry_count_429));
                    }
                    // Full jitter exponential backoff
                    let base = RATELIMIT_BACKOFF_INIT_MS * 2u64.pow(retry_count_429 - 1);
                    let capped = base.min(RETRY_AFTER_CAP_SECS * 1000);
                    let delay_ms = rand::thread_rng().gen_range(0..=capped);
                    time::sleep(Duration::from_millis(delay_ms)).await;
                    continue;
                }
                500..=599 => {
                    retry_count_5xx += 1;
                    if retry_count_5xx > max_retries_5xx {
                        return Err(WebbfetchError::Fetch(format!(
                            "Server error after {max_retries_5xx} retry: HTTP {status}"
                        )));
                    }
                    time::sleep(Duration::from_secs(SERVER_ERROR_DELAY_SECS)).await;
                    continue;
                }
                _ => {
                    return Err(WebbfetchError::Fetch(format!("HTTP error {status}")));
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// A mock HTTP client that returns pre-configured responses for testing.
    struct MockClient {
        responses: Arc<Mutex<HashMap<String, Response>>>,
        fail_count: Arc<Mutex<HashMap<String, u32>>>,
    }

    impl MockClient {
        fn new() -> Self {
            Self {
                responses: Arc::new(Mutex::new(HashMap::new())),
                fail_count: Arc::new(Mutex::new(HashMap::new())),
            }
        }

        fn with_response(url: &str, status: u16, body: &str) -> Self {
            let mut map = HashMap::new();
            map.insert(
                url.to_string(),
                Response {
                    status,
                    body: body.to_string(),
                },
            );
            Self {
                responses: Arc::new(Mutex::new(map)),
                fail_count: Arc::new(Mutex::new(HashMap::new())),
            }
        }
    }

    #[async_trait]
    impl HttpClient for MockClient {
        async fn get(&self, url: &str) -> Result<Response, WebbfetchError> {
            let mut fails = self.fail_count.lock().await;
            let fail_key = url.to_string();
            let fail_count = fails.entry(fail_key.clone()).or_insert(0);
            if *fail_count > 0 {
                *fail_count -= 1;
                return Err(WebbfetchError::Fetch(format!("Mock failure for {url}")));
            }

            let responses = self.responses.lock().await;
            responses
                .get(url)
                .cloned()
                .ok_or_else(|| WebbfetchError::Fetch(format!("No mock response for {url}")))
        }
    }

    #[tokio::test]
    async fn test_fetch_success() {
        let mock = MockClient::with_response(
            "https://example.com/page",
            200,
            "<html><body>Hello</body></html>",
        );
        let client = WebbfetchClient::with_client(mock);
        let result = client.fetch("https://example.com/page").await.unwrap();
        assert_eq!(result.url.source_type, SourceType::GenericHtml);
    }

    #[tokio::test]
    async fn test_fetch_invalid_scheme() {
        let mock = MockClient::new();
        let client = WebbfetchClient::with_client(mock);
        let err = client.fetch("ftp://example.com/file").await.unwrap_err();
        assert!(matches!(err, WebbfetchError::InvalidUrl(_)));
    }

    #[tokio::test]
    async fn test_fetch_url_too_long() {
        let mock = MockClient::new();
        let client = WebbfetchClient::with_client(mock);
        let long_url = format!("https://example.com/{}", "a".repeat(2048));
        let err = client.fetch(&long_url).await.unwrap_err();
        assert!(matches!(err, WebbfetchError::InvalidUrl(_)));
    }

    #[tokio::test]
    async fn test_fetch_non_2xx() {
        let mock = MockClient::with_response("https://example.com/notfound", 404, "Not Found");
        let client = WebbfetchClient::with_client(mock);
        let err = client
            .fetch("https://example.com/notfound")
            .await
            .unwrap_err();
        assert!(matches!(err, WebbfetchError::Fetch(_)));
    }

    #[tokio::test]
    async fn test_fetch_429_retry_exhausted() {
        let mock =
            MockClient::with_response("https://example.com/ratelimited", 429, "Too Many Requests");
        let client = WebbfetchClient::with_client(mock);
        let err = client
            .fetch("https://example.com/ratelimited")
            .await
            .unwrap_err();
        assert!(matches!(err, WebbfetchError::RetryExhausted(_)));
    }

    #[tokio::test]
    async fn test_fetch_bot_detection() {
        let mock = MockClient::with_response(
            "https://example.com/challenge",
            200,
            "Just a moment... <div>cf-browser-verification</div>",
        );
        let client = WebbfetchClient::with_client(mock);
        let err = client
            .fetch("https://example.com/challenge")
            .await
            .unwrap_err();
        assert!(matches!(err, WebbfetchError::Fetch(_)));
        assert!(err.to_string().contains("bot detection") || err.to_string().contains("Blocked"));
    }

    #[tokio::test]
    async fn test_fetch_reddit_url_detection() {
        let api_url = "https://www.reddit.com/r/rust/comments/abc123/hello_world.json?raw_json=1";
        let mock = MockClient::with_response(
            api_url,
            200,
            r#"{"kind": "Listing", "data": {"children": []}}"#,
        );
        let client = WebbfetchClient::with_client(mock);
        let result = client
            .fetch("https://www.reddit.com/r/rust/comments/abc123/hello_world/")
            .await
            .unwrap();
        assert_eq!(result.url.source_type, SourceType::Reddit);
    }

    #[tokio::test]
    async fn test_discourse_url_detected_as_generic_html() {
        let mock = MockClient::with_response(
            "https://forum.example.com/t/some-topic/12345",
            200,
            "<html><body>Hello</body></html>",
        );
        let client = WebbfetchClient::with_client(mock);
        let result = client
            .fetch("https://forum.example.com/t/some-topic/12345")
            .await
            .unwrap();
        assert_eq!(result.url.source_type, SourceType::GenericHtml);
    }

    #[tokio::test]
    async fn test_fetch_with_config_override() {
        let mock = MockClient::with_response("https://example.com/page", 200, "content");
        let client = WebbfetchClient::with_client(mock);
        let config = FetchConfig {
            timeout_secs: 5,
            qps: 1,
        };
        let result = client
            .fetch_with_config("https://example.com/page", &config)
            .await
            .unwrap();
        assert_eq!(result.url.source_type, SourceType::GenericHtml);
    }
}
