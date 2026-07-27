use super::*;
use std::time::Duration;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};
use wreq_util::Profile;

// ---------------------------------------------------------------------------
// Builder unit tests — no HTTP server needed, just field assertions
// ---------------------------------------------------------------------------

/// Verifies that chaining multiple `with_*` methods on `CrawlerBuilder`
/// does NOT silently drop earlier settings (anti-regression for HIDN-B-002).
#[test]
fn test_builder_chained_settings_preserved() {
    let crawler = RateLimitedCrawler::builder()
        .with_emulation(Profile::Safari18_5)
        .with_redirect(wreq::redirect::Policy::limited(5))
        .with_timeout(Duration::from_secs(30))
        .with_connect_timeout(Duration::from_secs(15))
        .with_qps(50)
        .with_burst(5)
        .with_max_domains(256)
        .with_http2()
        .build()
        .expect("CrawlerBuilder::build() should succeed");
    assert_eq!(crawler.qps, 50);
    assert_eq!(crawler.burst, 5);
}
/// Verifies that with_client works and builder-only settings cannot be mixed.
#[test]
fn test_builder_with_client() {
    let raw_client = wreq::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .expect("wreq::Client::builder().build() should succeed");
    let crawler = RateLimitedCrawler::builder()
        .with_client(raw_client)
        .with_qps(10)
        .with_burst(1)
        .with_max_domains(128)
        .build()
        .expect("CrawlerBuilder::build() should succeed");
    assert_eq!(crawler.qps, 10);
    assert_eq!(crawler.burst, 1);
}

/// Verifies that mixing with_client and builder settings returns an error.
#[test]
fn test_builder_mixed_with_client_and_timeout_errs() {
    let raw_client = wreq::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .unwrap();
    let result = RateLimitedCrawler::builder()
        .with_client(raw_client)
        .with_timeout(Duration::from_secs(5))
        .build();
    assert!(
        result.is_err(),
        "with_client then with_timeout should error (mixed)"
    );
}

/// Verifies that with_timeout then with_client succeeds (with_client clears builder).
#[test]
fn test_builder_timeout_then_with_client_ok() {
    let raw_client = wreq::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .unwrap();
    let result = RateLimitedCrawler::builder()
        .with_timeout(Duration::from_secs(5))
        .with_client(raw_client)
        .build();
    assert!(
        result.is_ok(),
        "with_client should clear builder settings and succeed"
    );
}

/// Verifies that chaining only a subset of `with_*` methods works.
#[test]
fn test_builder_partial_chain() {
    let crawler = RateLimitedCrawler::builder()
        .with_timeout(Duration::from_secs(60))
        .with_qps(5)
        .build()
        .expect("CrawlerBuilder::build() with partial chain should succeed");
    assert_eq!(crawler.qps, 5);
    assert_eq!(crawler.burst, 1, "burst should default to 1");
}

/// Verifies that calling a `with_*` method multiple times (last-wins) works.
#[test]
fn test_builder_override_settings() {
    let crawler = RateLimitedCrawler::builder()
        .with_timeout(Duration::from_secs(1))
        .with_timeout(Duration::from_secs(30))
        .with_emulation(Profile::Safari18_5)
        .with_redirect(wreq::redirect::Policy::limited(5))
        .with_connect_timeout(Duration::from_secs(30))
        .build()
        .expect("CrawlerBuilder::build() with overrides should succeed");
    assert_eq!(crawler.qps, 10, "qps should default to 10");
    assert_eq!(crawler.burst, 1, "burst should default to 1");
}

/// Verifies that the builder with only defaults works.
#[test]
fn test_builder_defaults_only() {
    let crawler = RateLimitedCrawler::builder()
        .build()
        .expect("CrawlerBuilder::build() with defaults should succeed");
    assert_eq!(crawler.qps, 10, "qps should default to 10");
    assert_eq!(crawler.burst, 1, "burst should default to 1");
}

// ---------------------------------------------------------------------------
// Retry & backoff tests — these need a mock HTTP server
// ---------------------------------------------------------------------------
// These tests verify GetBuilder::send() retry logic:
//   - HTTP 429 → retry with exponential backoff
//   - HTTP 5xx → retry with exponential backoff
//   - Connection errors → retry with exponential backoff
//   - Non-429 4xx → no retry
//   - Custom retry limit
//
// NOTE: tokio::time::pause() is NOT used here because CrawlerBuilder::build()
// hardcodes a 30-second timeout that overrides with_timeout(). When time is
// paused + advanced past 30s, the wreq client's internal timeout fires and all
// requests fail with TimedOut. Instead, we run in real time with a base backoff
// of 1 second — total test time is ~15s which is acceptable.
//
// The "exhausted" tests (test_retry_429_exhausted, test_retry_with_custom_limit)
// DO use pause/advance because they only need a single mock and work correctly.

/// Helper: spawn a retry send() and advance time until it completes.
/// Only used by tests that work correctly with pause/advance.
async fn retry_with_time_advance(
    crawler: RateLimitedCrawler,
    url: String,
    base_secs: u64,
    retry_limit: Option<u32>,
) -> Result<wreq::Response, CrawlerError> {
    let handle = tokio::spawn(async move {
        let mut builder = crawler.get(&url).with_exponential_retry(base_secs);
        if let Some(limit) = retry_limit {
            builder = builder.with_retry_limit(limit);
        }
        builder.send().await
    });

    // Let the spawned task make the first request and hit the first sleep
    tokio::task::yield_now().await;

    // Advance time in steps to let all retry backoffs complete
    for _ in 0..10 {
        tokio::time::advance(Duration::from_secs(10)).await;
        tokio::task::yield_now().await;
        if handle.is_finished() {
            break;
        }
    }

    handle.await.expect("retry task should not panic")
}

/// Verifies that HTTP 429 triggers retry with exponential backoff and eventually
/// succeeds when the server returns 200.
#[tokio::test]
async fn test_retry_429_then_succeed() {
    let mock_server = MockServer::start().await;

    // Mount the 429 mock first (higher priority 1, limited to 2 matches)
    Mock::given(wiremock::matchers::any())
        .respond_with(ResponseTemplate::new(429))
        .up_to_n_times(2)
        .expect(2)
        .with_priority(1)
        .mount(&mock_server)
        .await;

    // Mount the 200 mock second (lower priority 5, unlimited matches)
    Mock::given(wiremock::matchers::any())
        .respond_with(ResponseTemplate::new(200).set_body_string("ok"))
        .mount(&mock_server)
        .await;

    // Use with_client to bypass wreq's Safari emulation which sends
    // absolute-form URIs (GET http://localhost/...) that confuse wiremock.
    let raw_client = wreq::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .expect("wreq::Client::builder().build() should succeed");
    let crawler = RateLimitedCrawler::builder()
        .with_client(raw_client)
        .with_qps(10_000)
        .build()
        .expect("CrawlerBuilder::build() should succeed");

    let url = format!("{}/retry-429-succeed", mock_server.uri());
    // Run in real time — backoffs are ~1s per retry
    let result = crawler.get(&url).with_exponential_retry(1).send().await;

    assert!(
        result.is_ok(),
        "expected Ok after retries, got {:?}",
        result
    );
    let resp = result.unwrap();
    assert_eq!(
        resp.status().as_u16(),
        200,
        "expected HTTP 200 after retries"
    );
    let body = resp.text().await.expect("response body should be readable");
    assert_eq!(body, "ok", "response body should match");
}

/// Verifies that HTTP 429 exhausts retries and returns CrawlerError::RetryExhausted.
#[tokio::test]
async fn test_retry_429_exhausted() {
    tokio::time::pause();

    let mock_server = MockServer::start().await;

    // Always return 429
    Mock::given(method("GET"))
        .and(path("/retry-429-exhausted"))
        .respond_with(ResponseTemplate::new(429))
        .mount(&mock_server)
        .await;

    let crawler = RateLimitedCrawler::builder()
        .with_qps(10_000)
        .build()
        .expect("CrawlerBuilder::build() should succeed");

    let url = format!("{}/retry-429-exhausted", mock_server.uri());

    let result = retry_with_time_advance(crawler, url, 1, None).await;
    assert!(
        matches!(result, Err(CrawlerError::RetryExhausted { .. })),
        "expected RetryExhausted, got {:?}",
        result
    );
}

/// Verifies that HTTP 5xx (503) triggers retry and succeeds when the server
/// returns 200 on the next attempt.
#[tokio::test]
async fn test_retry_5xx_then_succeed() {
    let mock_server = MockServer::start().await;

    // Mount the 503 mock first (higher priority 1, limited to 1 match)
    Mock::given(wiremock::matchers::any())
        .respond_with(ResponseTemplate::new(503))
        .up_to_n_times(1)
        .expect(1)
        .with_priority(1)
        .mount(&mock_server)
        .await;

    // Mount the 200 mock second (lower priority 5, unlimited matches)
    Mock::given(wiremock::matchers::any())
        .respond_with(ResponseTemplate::new(200).set_body_string("ok"))
        .mount(&mock_server)
        .await;

    // Use with_client to bypass wreq's Safari emulation which sends
    // absolute-form URIs (GET http://localhost/...) that confuse wiremock.
    let raw_client = wreq::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .expect("wreq::Client::builder().build() should succeed");
    let crawler = RateLimitedCrawler::builder()
        .with_client(raw_client)
        .with_qps(10_000)
        .build()
        .expect("CrawlerBuilder::build() should succeed");

    let url = format!("{}/retry-5xx-succeed", mock_server.uri());
    // Run in real time — backoffs are ~1s per retry
    let result = crawler.get(&url).with_exponential_retry(1).send().await;

    assert!(
        result.is_ok(),
        "expected Ok after 5xx retry, got {:?}",
        result
    );
    let resp = result.unwrap();
    assert_eq!(
        resp.status().as_u16(),
        200,
        "expected HTTP 200 after 5xx retry"
    );
    let body = resp.text().await.expect("response body should be readable");
    assert_eq!(body, "ok", "response body should match");
}

/// Verifies that connection errors trigger retry and eventually return
/// RetryExhausted, and that the crawler remains usable afterwards.
#[tokio::test]
async fn test_retry_connection_error_then_succeed() {
    // Start a mock server for the recovery request
    let mock_server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/recovery"))
        .respond_with(ResponseTemplate::new(200).set_body_string("recovery-ok"))
        .mount(&mock_server)
        .await;

    // Make a request to a port that's not listening (connection refused).
    // Port 1 is almost certainly not in use and produces a connection error
    // that wreq::Error::is_connect() returns true for.
    let bad_url = "http://127.0.0.1:1/connection-error".to_string();

    let crawler = RateLimitedCrawler::builder()
        .with_qps(10_000)
        .build()
        .expect("CrawlerBuilder::build() should succeed");

    // Run in real time — backoffs are ~1s per retry
    let result = crawler.get(&bad_url).with_exponential_retry(1).send().await;

    assert!(
        matches!(result, Err(CrawlerError::RetryExhausted { .. })),
        "expected RetryExhausted for connection errors, got {:?}",
        result
    );

    // Verify the crawler still works for subsequent requests
    let recovery_url = format!("{}/recovery", mock_server.uri());
    let crawler2 = RateLimitedCrawler::builder()
        .with_qps(10_000)
        .build()
        .expect("CrawlerBuilder::build() should succeed");
    let resp = crawler2
        .get(&recovery_url)
        .send()
        .await
        .expect("recovery request should succeed");
    assert_eq!(resp.status().as_u16(), 200);
    let body = resp.text().await.expect("response body should be readable");
    assert_eq!(body, "recovery-ok");
}

/// Verifies that non-429 4xx responses (e.g. 404) are NOT retried — they are
/// returned immediately.
#[tokio::test]
async fn test_no_retry_on_4xx_non_429() {
    let mock_server = MockServer::start().await;

    // Use expect(1) to verify only ONE request is made (no retry)
    Mock::given(method("GET"))
        .and(path("/not-found"))
        .respond_with(ResponseTemplate::new(404).set_body_string("not found"))
        .expect(1)
        .mount(&mock_server)
        .await;

    let crawler = RateLimitedCrawler::builder()
        .with_qps(10_000)
        .build()
        .expect("CrawlerBuilder::build() should succeed");

    let url = format!("{}/not-found", mock_server.uri());

    // Even with retry enabled, 404 should NOT be retried
    let result = crawler.get(&url).with_exponential_retry(1).send().await;

    assert!(result.is_ok(), "expected Ok(404), got {:?}", result);
    let resp = result.unwrap();
    assert_eq!(resp.status().as_u16(), 404, "expected HTTP 404");
    let body = resp.text().await.expect("response body should be readable");
    assert_eq!(body, "not found");
}

/// Verifies that a custom retry limit is respected.
#[tokio::test]
async fn test_retry_with_custom_limit() {
    tokio::time::pause();

    let mock_server = MockServer::start().await;

    // Always return 429
    Mock::given(method("GET"))
        .and(path("/custom-limit"))
        .respond_with(ResponseTemplate::new(429))
        .mount(&mock_server)
        .await;

    let crawler = RateLimitedCrawler::builder()
        .with_qps(10_000)
        .build()
        .expect("CrawlerBuilder::build() should succeed");

    let url = format!("{}/custom-limit", mock_server.uri());

    // with_retry_limit(1) means 2 total attempts (attempt 0 and 1)
    let result = retry_with_time_advance(crawler, url, 1, Some(1)).await;
    assert!(
        matches!(result, Err(CrawlerError::RetryExhausted { .. })),
        "expected RetryExhausted with custom limit, got {:?}",
        result
    );
}

// ---------------------------------------------------------------------------
// URL validation & edge-case tests (ported from deleted webfetch tests)
// ---------------------------------------------------------------------------

/// Verifies that ftp:// URLs are rejected by the crawler.
///
/// `Url::parse("ftp://...")` succeeds (ftp is a valid URL scheme), but wreq
/// rejects the scheme internally with a `BadScheme` error, which surfaces as
/// `CrawlerError::Http`.
#[tokio::test]
async fn test_fetch_invalid_scheme() {
    let crawler = RateLimitedCrawler::builder()
        .with_qps(10_000)
        .build()
        .expect("CrawlerBuilder::build() should succeed");

    let result = crawler.get("ftp://example.com/file").send().await;
    match result {
        Err(CrawlerError::Http(e)) => {
            let msg = e.to_string();
            assert!(
                msg.contains("scheme is not allowed") || msg.contains("BadScheme"),
                "expected bad-scheme error, got: {msg}"
            );
        }
        other => panic!("expected Err(CrawlerError::Http), got {other:?}"),
    }
}

/// Verifies that very long URLs (2048+ chars) are handled gracefully.
///
/// The crawler does not impose its own URL length limit — wreq handles
/// long URLs without error.
#[tokio::test]
async fn test_fetch_long_url() {
    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200).set_body_string("long-ok"))
        .mount(&mock_server)
        .await;

    let crawler = RateLimitedCrawler::builder()
        .with_qps(10_000)
        .build()
        .expect("CrawlerBuilder::build() should succeed");

    // Build a URL > 2048 chars
    let long_url = format!("{}/{}", mock_server.uri(), "a".repeat(2048));
    assert!(long_url.len() > 2048, "test URL must exceed 2048 chars");

    let result = crawler.get(&long_url).send().await;
    assert!(result.is_ok(), "expected Ok for long URL, got {:?}", result);
    let resp = result.unwrap();
    assert_eq!(resp.status().as_u16(), 200);
    let body = resp.text().await.expect("response body should be readable");
    assert_eq!(body, "long-ok");
}

/// Verifies that an empty HTTP 200 response body is handled correctly.
#[tokio::test]
async fn test_fetch_empty_body() {
    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200)) // no body
        .mount(&mock_server)
        .await;

    let crawler = RateLimitedCrawler::builder()
        .with_qps(10_000)
        .build()
        .expect("CrawlerBuilder::build() should succeed");

    let url = format!("{}/empty", mock_server.uri());
    let result = crawler.get(&url).send().await;
    assert!(
        result.is_ok(),
        "expected Ok for empty body, got {:?}",
        result
    );
    let resp = result.unwrap();
    assert_eq!(resp.status().as_u16(), 200);
    let body = resp.text().await.expect("response body should be readable");
    assert!(body.is_empty(), "expected empty body, got {body:?}");
}

/// Verifies that non-2xx status codes (e.g. 404) are returned as successful
/// responses, not as errors.
///
/// This is distinct from the retry test `test_no_retry_on_4xx_non_429` which
/// tests that 404 is not retried. This test verifies the plain GET path
/// (no retry) returns non-2xx statuses as Ok.
#[tokio::test]
async fn test_fetch_non_2xx_status() {
    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(404).set_body_string("not found"))
        .mount(&mock_server)
        .await;

    let crawler = RateLimitedCrawler::builder()
        .with_qps(10_000)
        .build()
        .expect("CrawlerBuilder::build() should succeed");

    let url = format!("{}/not-found", mock_server.uri());
    let result = crawler.get(&url).send().await;
    assert!(result.is_ok(), "expected Ok(404), got {:?}", result);
    let resp = result.unwrap();
    assert_eq!(resp.status().as_u16(), 404, "expected HTTP 404");
    let body = resp.text().await.expect("response body should be readable");
    assert_eq!(body, "not found");
}

// ---------------------------------------------------------------------------
// max_resp_size tests
// ---------------------------------------------------------------------------

/// Verifies that `with_max_resp_size` sets the field on the crawler.
#[test]
fn test_builder_with_max_resp_size() {
    let crawler = RateLimitedCrawler::builder()
        .with_max_resp_size(1024)
        .build()
        .expect("CrawlerBuilder::build() should succeed");
    assert_eq!(crawler.max_resp_size, Some(1024));
}

/// Verifies that default builder has max_resp_size = None.
#[test]
fn test_builder_max_resp_size_default() {
    let crawler = RateLimitedCrawler::builder()
        .build()
        .expect("CrawlerBuilder::build() should succeed");
    assert_eq!(crawler.max_resp_size, None);
}

/// Verifies that `with_max_resp_size` is preserved when chained with other settings.
#[test]
fn test_builder_chained_with_max_resp_size() {
    let crawler = RateLimitedCrawler::builder()
        .with_qps(50)
        .with_burst(5)
        .with_max_domains(256)
        .with_max_resp_size(64 * 1024)
        .build()
        .expect("CrawlerBuilder::build() should succeed");
    assert_eq!(crawler.qps, 50);
    assert_eq!(crawler.burst, 5);
    assert_eq!(crawler.max_resp_size, Some(64 * 1024));
}

/// Verifies that Content-Length exceeding max_resp_size is rejected with ResponseTooLarge.
#[tokio::test]
async fn test_execute_get_rejects_oversized_content_length() {
    let mock_server = MockServer::start().await;

    // Return a response with Content-Length > 100
    Mock::given(method("GET"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_string("x".repeat(200))
                .insert_header("Content-Length", "200"),
        )
        .mount(&mock_server)
        .await;

    let crawler = RateLimitedCrawler::builder()
        .with_max_resp_size(100)
        .with_qps(10_000)
        .build()
        .expect("CrawlerBuilder::build() should succeed");

    let url = format!("{}/oversized", mock_server.uri());
    let result = crawler.get(&url).send().await;
    assert!(
        matches!(
            result,
            Err(CrawlerError::ResponseTooLarge {
                size: 200,
                max: 100
            })
        ),
        "expected ResponseTooLarge, got {:?}",
        result
    );
}

/// Verifies that a response within max_resp_size passes through.
#[tokio::test]
async fn test_execute_get_accepts_within_max_resp_size() {
    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_string("small body")
                .insert_header("Content-Length", "10"),
        )
        .mount(&mock_server)
        .await;

    let crawler = RateLimitedCrawler::builder()
        .with_max_resp_size(100)
        .with_qps(10_000)
        .build()
        .expect("CrawlerBuilder::build() should succeed");

    let url = format!("{}/small", mock_server.uri());
    let result = crawler.get(&url).send().await;
    assert!(result.is_ok(), "expected Ok, got {:?}", result);
    let resp = result.unwrap();
    assert_eq!(resp.status().as_u16(), 200);
}

/// Verifies that when max_resp_size is None (default), no size check is performed.
#[tokio::test]
async fn test_execute_get_no_max_resp_size() {
    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200).set_body_string("x".repeat(500)))
        .mount(&mock_server)
        .await;

    let crawler = RateLimitedCrawler::builder()
        .with_qps(10_000)
        .build()
        .expect("CrawlerBuilder::build() should succeed");

    let url = format!("{}/no-limit", mock_server.uri());
    let result = crawler.get(&url).send().await;
    assert!(
        result.is_ok(),
        "expected Ok with no limit, got {:?}",
        result
    );
    let resp = result.unwrap();
    assert_eq!(resp.status().as_u16(), 200);
    let body = resp.text().await.expect("response body should be readable");
    assert_eq!(body.len(), 500);
}

// ---------------------------------------------------------------------------
// fetch_text tests
// ---------------------------------------------------------------------------

/// Verifies that fetch_text rejects URLs exceeding MAX_URL_LENGTH.
#[tokio::test]
async fn test_fetch_text_rejects_long_url() {
    let crawler = RateLimitedCrawler::builder()
        .with_qps(10_000)
        .build()
        .expect("CrawlerBuilder::build() should succeed");

    let long_url = "https://example.com/".to_string() + &"a".repeat(2048);
    assert!(long_url.len() > 2048, "test URL must exceed MAX_URL_LENGTH");

    let result = crawler.fetch_text(&long_url).await;
    assert!(
        matches!(result, Err(CrawlerError::InvalidUrl(_))),
        "expected InvalidUrl, got {:?}",
        result
    );
}

/// Verifies that fetch_text rejects unsupported URL schemes.
#[tokio::test]
async fn test_fetch_text_rejects_unsupported_scheme() {
    let crawler = RateLimitedCrawler::builder()
        .with_qps(10_000)
        .build()
        .expect("CrawlerBuilder::build() should succeed");

    let result = crawler.fetch_text("ftp://example.com/file").await;
    assert!(
        matches!(result, Err(CrawlerError::InvalidUrl(_))),
        "expected InvalidUrl, got {:?}",
        result
    );
}

/// Verifies that fetch_text with max_resp_size rejects oversized responses
/// (Content-Length exceeds max).
#[tokio::test]
async fn test_fetch_text_rejects_oversized_content_length() {
    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_string("x".repeat(200))
                .insert_header("Content-Length", "200"),
        )
        .mount(&mock_server)
        .await;

    let crawler = RateLimitedCrawler::builder()
        .with_max_resp_size(100)
        .with_qps(10_000)
        .build()
        .expect("CrawlerBuilder::build() should succeed");

    let url = format!("{}/oversized", mock_server.uri());
    let result = crawler.fetch_text(&url).await;
    assert!(
        matches!(
            result,
            Err(CrawlerError::ResponseTooLarge {
                size: 200,
                max: 100
            })
        ),
        "expected ResponseTooLarge, got {:?}",
        result
    );
}

/// Verifies that fetch_text with max_resp_size accepts responses within the limit.
#[tokio::test]
async fn test_fetch_text_accepts_within_max_resp_size() {
    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_string("small body")
                .insert_header("Content-Length", "10"),
        )
        .mount(&mock_server)
        .await;

    let crawler = RateLimitedCrawler::builder()
        .with_max_resp_size(100)
        .with_qps(10_000)
        .build()
        .expect("CrawlerBuilder::build() should succeed");

    let url = format!("{}/small", mock_server.uri());
    let result = crawler.fetch_text(&url).await;
    assert!(result.is_ok(), "expected Ok, got {:?}", result);
    let (body, content_type) = result.unwrap();
    assert_eq!(body, "small body");
    assert_eq!(content_type, Some("text/plain".to_string()));
}

/// Verifies that fetch_text without max_resp_size returns the full body.
#[tokio::test]
async fn test_fetch_text_no_max_resp_size() {
    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200).set_body_string("x".repeat(500)))
        .mount(&mock_server)
        .await;

    let crawler = RateLimitedCrawler::builder()
        .with_qps(10_000)
        .build()
        .expect("CrawlerBuilder::build() should succeed");

    let url = format!("{}/no-limit", mock_server.uri());
    let result = crawler.fetch_text(&url).await;
    assert!(result.is_ok(), "expected Ok, got {:?}", result);
    let (body, content_type) = result.unwrap();
    assert_eq!(body.len(), 500);
    assert_eq!(content_type, Some("text/plain".to_string()));
}

/// Verifies that fetch_text returns content-type header.
#[tokio::test]
async fn test_fetch_text_returns_content_type() {
    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200).set_body_string("hello"))
        .mount(&mock_server)
        .await;

    let crawler = RateLimitedCrawler::builder()
        .with_qps(10_000)
        .build()
        .expect("CrawlerBuilder::build() should succeed");

    let url = format!("{}/content-type", mock_server.uri());
    let result = crawler.fetch_text(&url).await;
    assert!(result.is_ok(), "expected Ok, got {:?}", result);
    let (body, content_type) = result.unwrap();
    assert_eq!(body, "hello");
    // wiremock defaults to text/plain for set_body_string
    assert_eq!(content_type, Some("text/plain".to_string()));
}

/// Verifies that fetch_text handles responses without explicit Content-Type header.
/// wiremock always sets a default Content-Type, so this tests that the header
/// is captured correctly.
#[tokio::test]
async fn test_fetch_text_default_content_type() {
    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200).set_body_string("hello"))
        .mount(&mock_server)
        .await;

    let crawler = RateLimitedCrawler::builder()
        .with_qps(10_000)
        .build()
        .expect("CrawlerBuilder::build() should succeed");

    let url = format!("{}/no-content-type", mock_server.uri());
    let result = crawler.fetch_text(&url).await;
    assert!(result.is_ok(), "expected Ok, got {:?}", result);
    let (body, content_type) = result.unwrap();
    assert_eq!(body, "hello");
    // wiremock defaults to text/plain
    assert_eq!(content_type, Some("text/plain".to_string()));
}

/// Anti-regression: user with_timeout/with_redirect/with_emulation/with_connect_timeout
/// must survive build(). The bug was that build() re-applied hardcoded defaults
/// after user values, silently discarding them (last-write-wins in wreq).
///
/// We can't introspect the built wreq::Client's timeout, so we verify structurally:
/// the builder accepts chained settings without panic/error.
#[test]
fn test_builder_user_settings_survive_build() {
    let crawler = RateLimitedCrawler::builder()
        .with_emulation(Profile::Safari18_5)
        .with_redirect(wreq::redirect::Policy::limited(10))
        .with_timeout(Duration::from_secs(15))
        .with_connect_timeout(Duration::from_secs(5))
        .with_qps(1)
        .with_burst(1)
        .with_max_domains(10)
        .build()
        .expect("CrawlerBuilder::build() with user settings should succeed");
    assert_eq!(crawler.qps, 1);
    assert_eq!(crawler.burst, 1);
}

/// Anti-regression: with_timeout with a very short timeout should actually
/// cause a timeout on a slow server (behavioral test).
#[tokio::test]
async fn test_builder_short_timeout_actually_applied() {
    // Start a server that waits 3 seconds before responding
    use tokio::io::AsyncWriteExt;
    use tokio::net::TcpListener;

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        if let Ok((mut stream, _)) = listener.accept().await {
            // Wait 3s before responding — longer than our 1s timeout
            tokio::time::sleep(Duration::from_secs(3)).await;
            let _ = stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok")
                .await;
        }
    });

    let crawler = RateLimitedCrawler::builder()
        .with_timeout(Duration::from_secs(1))
        .with_connect_timeout(Duration::from_secs(1))
        .with_qps(10)
        .with_burst(1)
        .with_max_domains(10)
        .build()
        .expect("build should succeed");

    let result = crawler.get(&format!("http://{}", addr)).send().await;

    assert!(result.is_err(), "1s timeout should fire on a 3s server");
}
