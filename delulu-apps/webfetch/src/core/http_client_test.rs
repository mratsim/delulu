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
