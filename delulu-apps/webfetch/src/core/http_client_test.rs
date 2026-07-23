use super::*;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpListener;

/// A mock HTTP client that returns pre-configured responses for testing.
struct MockClient {
    responses: Arc<Mutex<HashMap<String, Response>>>,
    fail_count: Arc<Mutex<HashMap<String, u32>>>,
    mock_bytes: Arc<Mutex<HashMap<String, Vec<u8>>>>,
}

impl MockClient {
    fn new() -> Self {
        Self {
            responses: Arc::new(Mutex::new(HashMap::new())),
            fail_count: Arc::new(Mutex::new(HashMap::new())),
            mock_bytes: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    fn with_response(url: &str, status: u16, body: &str) -> Self {
        let mut map = HashMap::new();
        map.insert(
            url.to_string(),
            Response {
                status,
                body: body.to_string(),
                content_type: None,
            },
        );
        Self {
            responses: Arc::new(Mutex::new(map)),
            fail_count: Arc::new(Mutex::new(HashMap::new())),
            mock_bytes: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

impl MockClient {
    fn with_bytes(url: &str, bytes: &[u8]) -> Self {
        let mut map = HashMap::new();
        map.insert(url.to_string(), bytes.to_vec());
        Self {
            responses: Arc::new(Mutex::new(HashMap::new())),
            fail_count: Arc::new(Mutex::new(HashMap::new())),
            mock_bytes: Arc::new(Mutex::new(map)),
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

    async fn get_bytes(&self, url: &str) -> Result<Vec<u8>, WebbfetchError> {
        let bytes = self.mock_bytes.lock().await;
        if let Some(data) = bytes.get(url) {
            return Ok(data.clone());
        }
        // Fall back to converting string body to bytes
        let resp = self.get(url).await?;
        Ok(resp.body.into_bytes())
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

// -- get_bytes tests ----------------------------------------------------------

#[tokio::test]
async fn test_get_bytes_returns_body_bytes() {
    let mock = MockClient::with_response(
        "https://example.com/data",
        200,
        "hello world",
    );
    let bytes = mock
        .get_bytes("https://example.com/data")
        .await
        .unwrap();
    assert_eq!(bytes, b"hello world");
}

#[tokio::test]
async fn test_get_bytes_propagates_error() {
    let mock = MockClient::new();
    let err = mock
        .get_bytes("https://example.com/missing")
        .await
        .unwrap_err();
    assert!(matches!(err, WebbfetchError::Fetch(_)));
}

#[tokio::test]
async fn test_get_bytes_with_mock_bytes() {
    let mock = MockClient::with_bytes(
        "https://example.com/binary",
        &[0x00, 0x01, 0x02, 0xFF],
    );
    let bytes = mock
        .get_bytes("https://example.com/binary")
        .await
        .unwrap();
    assert_eq!(bytes, vec![0x00, 0x01, 0x02, 0xFF]);
}
// -- OOM prevention tests ----------------------------------------------------
//
// These tests verify that WreqClient rejects oversized responses BEFORE
// allocating the full body in memory, preventing OOM on malicious responses.

/// Helper: spawn a minimal HTTP server that responds with the given status line,
/// headers, and body. Returns the server's address.
/// The server sends headers first, then yields to let the client process them,
/// then sends the body in 64KB chunks with inter-chunk yields to avoid
/// overwhelming the TCP send buffer.
async fn spawn_test_server(
    status_line: String,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
) -> std::net::SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();

        // Build and send HTTP response head
        let mut head = format!("{}\r\n", status_line);
        for (name, value) in &headers {
            head.push_str(&format!("{}: {}\r\n", name, value));
        }
        head.push_str("\r\n");
        socket.write_all(head.as_bytes()).await.unwrap();

        // Small yield to let client process headers
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        // Send body in 64KB chunks with yields to avoid TCP buffer blocking
        const CHUNK_SIZE: usize = 64 * 1024;
        for chunk in body.chunks(CHUNK_SIZE) {
            if socket.write_all(chunk).await.is_err() {
                break; // client disconnected, stop writing
            }
            // Yield briefly to let the TCP buffer drain
            tokio::time::sleep(std::time::Duration::from_micros(100)).await;
        }
    });

    addr
}

#[tokio::test]
async fn test_get_rejects_oversized_via_content_length() {
    // Server sends headers with Content-Length > MAX_BODY_SIZE,
    // then closes (never sends body). Client must reject BEFORE reading body.
    let oversized = (MAX_BODY_SIZE + 1).to_string();
    let addr = spawn_test_server(
        "HTTP/1.1 200 OK".to_string(),
        vec![
            ("Content-Length".to_string(), oversized),
            ("Content-Type".to_string(), "text/plain".to_string()),
        ],
        vec![], // no body sent
    ).await;

    let client = WreqClient { inner: wreq::Client::new() };
    let url = format!("http://{}/test", addr);
    let result = client.get(&url).await;
    assert!(result.is_err(), "expected error for oversized Content-Length");
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("too large"),
        "expected 'too large' error, got: {err}"
    );
}

#[tokio::test]
async fn test_get_bytes_rejects_oversized_via_content_length() {
    let oversized = (MAX_BODY_SIZE + 1).to_string();
    let addr = spawn_test_server(
        "HTTP/1.1 200 OK".to_string(),
        vec![
            ("Content-Length".to_string(), oversized),
            ("Content-Type".to_string(), "application/octet-stream".to_string()),
        ],
        vec![],
    ).await;

    let client = WreqClient { inner: wreq::Client::new() };
    let url = format!("http://{}/test", addr);
    let result = client.get_bytes(&url).await;
    assert!(result.is_err(), "expected error for oversized Content-Length");
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("too large"),
        "expected 'too large' error, got: {err}"
    );
}

#[tokio::test]
async fn test_get_rejects_oversized_during_streaming() {
    // Server sends no Content-Length (or a small one), but the body exceeds
    // MAX_BODY_SIZE during streaming. Client must reject mid-stream.
    let chunk_size = 1024 * 1024; // 1 MB chunks
    let num_chunks = (MAX_BODY_SIZE / chunk_size) + 2; // exceed limit
    let mut large_body = Vec::with_capacity(num_chunks * chunk_size);
    for _ in 0..num_chunks {
        large_body.extend(std::iter::repeat(b'X').take(chunk_size));
    }

    let addr = spawn_test_server(
        "HTTP/1.1 200 OK".to_string(),
        vec![("Content-Type".to_string(), "text/plain".to_string())], // no Content-Length
        large_body,
    ).await;

    let client = WreqClient { inner: wreq::Client::new() };
    let url = format!("http://{}/test", addr);
    let result = client.get(&url).await;
    assert!(result.is_err(), "expected error for oversized body during streaming");
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("too large"),
        "expected 'too large' error, got: {err}"
    );
}

#[tokio::test]
async fn test_get_bytes_rejects_oversized_during_streaming() {
    let chunk_size = 1024 * 1024;
    let num_chunks = (MAX_BODY_SIZE / chunk_size) + 2;
    let mut large_body = Vec::with_capacity(num_chunks * chunk_size);
    for _ in 0..num_chunks {
        large_body.extend(std::iter::repeat(b'Y').take(chunk_size));
    }

    let addr = spawn_test_server(
        "HTTP/1.1 200 OK".to_string(),
        vec![("Content-Type".to_string(), "application/octet-stream".to_string())],
        large_body,
    ).await;

    let client = WreqClient { inner: wreq::Client::new() };
    let url = format!("http://{}/test", addr);
    let result = client.get_bytes(&url).await;
    assert!(result.is_err(), "expected error for oversized body during streaming");
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("too large"),
        "expected 'too large' error, got: {err}"
    );
}

#[tokio::test]
async fn test_get_happy_path_with_real_server() {
    // Small valid response — must succeed.
    let body = b"Hello, world!";
    let addr = spawn_test_server(
        "HTTP/1.1 200 OK".to_string(),
        vec![
            ("Content-Length".to_string(), "13".to_string()),
            ("Content-Type".to_string(), "text/plain".to_string()),
        ],
        body.to_vec(),
    ).await;

    let client = WreqClient { inner: wreq::Client::new() };
    let url = format!("http://{}/test", addr);
    let result = client.get(&url).await;
    assert!(result.is_ok(), "expected success for small response: {:?}", result.err());
    let response = result.unwrap();
    assert_eq!(response.status, 200);
    assert_eq!(response.body, "Hello, world!");
    assert_eq!(response.content_type.as_deref(), Some("text/plain"));
}

#[tokio::test]
async fn test_get_bytes_happy_path_with_real_server() {
    let body = b"\x00\x01\x02\xFF";
    let addr = spawn_test_server(
        "HTTP/1.1 200 OK".to_string(),
        vec![
            ("Content-Length".to_string(), "4".to_string()),
            ("Content-Type".to_string(), "application/octet-stream".to_string()),
        ],
        body.to_vec(),
    ).await;

    let client = WreqClient { inner: wreq::Client::new() };
    let url = format!("http://{}/test", addr);
    let result = client.get_bytes(&url).await;
    assert!(result.is_ok(), "expected success for small response: {:?}", result.err());
    assert_eq!(result.unwrap(), vec![0x00, 0x01, 0x02, 0xFF]);
}

#[tokio::test]
async fn test_get_rejects_oversized_no_content_length() {
    // Server sends no Content-Length header but body exceeds limit.
    // Must be caught by the streaming check.
    let chunk_size = 1024 * 1024;
    let num_chunks = (MAX_BODY_SIZE / chunk_size) + 2;
    let mut large_body = Vec::with_capacity(num_chunks * chunk_size);
    for _ in 0..num_chunks {
        large_body.extend(std::iter::repeat(b'Z').take(chunk_size));
    }

    let addr = spawn_test_server(
        "HTTP/1.1 200 OK".to_string(),
        vec![], // no Content-Length, no Content-Type
        large_body,
    ).await;

    let client = WreqClient { inner: wreq::Client::new() };
    let url = format!("http://{}/test", addr);
    let result = client.get(&url).await;
    assert!(result.is_err(), "expected error for oversized body without Content-Length");
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("too large"),
        "expected 'too large' error, got: {err}"
    );
}