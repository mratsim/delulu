use super::*;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

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
    async fn get(&self, url: &str) -> Result<Response, WebfetchError> {
        let mut fails = self.fail_count.lock().await;
        let fail_key = url.to_string();
        let fail_count = fails.entry(fail_key.clone()).or_insert(0);
        if *fail_count > 0 {
            *fail_count -= 1;
            return Err(WebfetchError::Fetch(format!("Mock failure for {url}")));
        }

        let responses = self.responses.lock().await;
        responses
            .get(url)
            .cloned()
            .ok_or_else(|| WebfetchError::Fetch(format!("No mock response for {url}")))
    }

    async fn get_bytes(&self, url: &str) -> Result<Vec<u8>, WebfetchError> {
        let bytes = self.mock_bytes.lock().await;
        if let Some(data) = bytes.get(url) {
            return Ok(data.clone());
        }
        // Fall back to converting string body to bytes
        let resp = self.get(url).await?;
        Ok(resp.body.into_bytes())
    }
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
    assert!(matches!(err, WebfetchError::Fetch(_)));
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
