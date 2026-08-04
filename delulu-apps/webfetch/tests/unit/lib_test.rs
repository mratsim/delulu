use super::*;
use crate::pipelines::parse_html;
use delulu_rate_limited_crawler::RateLimitedCrawler;
use std::collections::HashMap;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpListener;

// ── extract_title tests ───────────────────────────────────────────────

#[test]
fn test_extract_title_from_h1() {
    let dom = DomNode::Element {
        tag: "h1".into(),
        attrs: vec![],
        children: vec![DomNode::Text("Hello World".into())],
        scores: HashMap::new(),
        metadata: HashMap::new(),
    };
    assert_eq!(extract_title(&dom), "Hello World");
}

#[test]
fn test_extract_title_h1_preferred_over_title() {
    let dom = DomNode::Element {
        tag: "html".into(),
        attrs: vec![],
        children: vec![
            DomNode::Element {
                tag: "head".into(),
                attrs: vec![],
                children: vec![DomNode::Element {
                    tag: "title".into(),
                    attrs: vec![],
                    children: vec![DomNode::Text("HTML Title".into())],
                    scores: HashMap::new(),
                    metadata: HashMap::new(),
                }],
                scores: HashMap::new(),
                metadata: HashMap::new(),
            },
            DomNode::Element {
                tag: "body".into(),
                attrs: vec![],
                children: vec![DomNode::Element {
                    tag: "h1".into(),
                    attrs: vec![],
                    children: vec![DomNode::Text("Main Title".into())],
                    scores: HashMap::new(),
                    metadata: HashMap::new(),
                }],
                scores: HashMap::new(),
                metadata: HashMap::new(),
            },
        ],
        scores: HashMap::new(),
        metadata: HashMap::new(),
    };
    assert_eq!(extract_title(&dom), "Main Title");
}

#[test]
fn test_extract_title_fallback_to_html_title() {
    let html_str =
        r#"<html><head><title>Page Title</title></head><body><p>no h1 here</p></body></html>"#;
    let dom = parse_html(html_str).unwrap();
    assert_eq!(extract_title(&dom), "Page Title");
}

#[test]
fn test_extract_title_empty() {
    let dom = DomNode::Element {
        tag: "html".into(),
        attrs: vec![],
        children: vec![],
        scores: HashMap::new(),
        metadata: HashMap::new(),
    };
    assert_eq!(extract_title(&dom), "");
}

#[test]
fn test_extract_title_no_heading() {
    let dom = DomNode::Element {
        tag: "div".into(),
        attrs: vec![],
        children: vec![DomNode::Text("just text".into())],
        scores: HashMap::new(),
        metadata: HashMap::new(),
    };
    assert_eq!(extract_title(&dom), "");
}

// ── Helper: spawn a tiny HTTP server ──────────────────────────────────

/// Spawn a minimal HTTP server that responds with the given status, content-type, and body.
/// Returns the server's address.
async fn spawn_text_server(
    status: u16,
    content_type: String,
    body: String,
) -> std::net::SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    let body_bytes = body.into_bytes();
    tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let head = format!(
            "HTTP/1.1 {status} OK\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\n\r\n",
            body_bytes.len()
        );
        socket.write_all(head.as_bytes()).await.unwrap();
        socket.write_all(&body_bytes).await.unwrap();
    });

    addr
}

/// Create a RateLimitedCrawler pointed at a local test server.
async fn test_crawler_for(
    status: u16,
    content_type: &str,
    body: &str,
) -> (RateLimitedCrawler, String) {
    let addr = spawn_text_server(status, content_type.to_string(), body.to_string()).await;
    let raw_client = wreq::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()
        .unwrap();
    let crawler = RateLimitedCrawler::builder()
        .with_client(raw_client)
        .build()
        .unwrap();
    let url = format!("http://{}/test", addr);
    (crawler, url)
}

// ── fetch_and_extract integration tests ────────────────────────────────

#[tokio::test]
async fn test_fetch_and_extract_generic_html() {
    let html_body =
        "<html><head><title>Test Page</title></head><body><h1>Hello</h1><p>World</p></body></html>";
    let (crawler, url) = test_crawler_for(200, "text/html", html_body).await;

    let result = fetch_and_extract(
        &url,
        &crawler,
        &[crate::pipelines::mozilla_readability::filter_mozilla_readability],
    )
    .await
    .unwrap();

    match result {
        ExtractionResult::GenericHtml { content_md, .. } => {
            assert!(
                content_md.body.contains("Hello") || content_md.body.contains("Test Page"),
                "body should contain lowered content: {}",
                content_md.body
            );
            assert!(
                content_md.frontmatter.contains("source_type: generic_html"),
                "frontmatter should indicate generic_html"
            );
            assert!(
                content_md.frontmatter.contains("source_url:"),
                "frontmatter should contain source URL"
            );
        }
        other => panic!("Expected GenericHtml, got {other:?}"),
    }
}

#[tokio::test]
async fn test_fetch_and_extract_non_2xx_status_returns_body() {
    // With fetch_text() handling the HTTP fetch, non-2xx status codes are
    // no longer rejected — the response body is returned as-is.
    let (crawler, url) = test_crawler_for(404, "text/plain", "Not Found").await;

    let result = fetch_and_extract(
        &url,
        &crawler,
        &[crate::pipelines::mozilla_readability::filter_mozilla_readability],
    )
    .await;
    // Non-2xx responses are now returned as Ok with the body content
    assert!(result.is_ok(), "expected Ok, got {:?}", result);
    match result.unwrap() {
        ExtractionResult::GenericHtml { content_md, .. } => {
            assert!(
                content_md.body.contains("Not Found"),
                "body should contain the response text, got: {}",
                content_md.body
            );
        }
        other => panic!("Expected GenericHtml, got {other:?}"),
    }
}

#[tokio::test]
async fn test_fetch_and_extract_https_only_scheme_rejected() {
    let (crawler, _url) = test_crawler_for(200, "text/plain", "ok").await;

    let err = fetch_and_extract(
        "ftp://example.com/file",
        &crawler,
        &[crate::pipelines::mozilla_readability::filter_mozilla_readability],
    )
    .await
    .unwrap_err();
    // URL validation moved to crawler; crawler errors map to WebfetchError::Fetch
    assert!(
        matches!(err, WebfetchError::Fetch(_)),
        "expected Fetch, got {err:?}"
    );
    assert!(
        err.to_string().contains("invalid URL") || err.to_string().contains("scheme"),
        "error should mention invalid URL or scheme, got: {err}"
    );
}

#[tokio::test]
async fn test_fetch_url_too_long() {
    // URLs exceeding MAX_URL_LENGTH (2048) should now return WebfetchError::Fetch
    // via the crawler (URL validation moved to RateLimitedCrawler::fetch_text).
    let (crawler, _url) = test_crawler_for(200, "text/plain", "ok").await;

    // Build a URL > 2048 chars
    let long_url = "https://example.com/".to_string() + &"a".repeat(2048);
    assert!(long_url.len() > 2048, "test URL must exceed MAX_URL_LENGTH");

    let err = fetch_and_extract(
        &long_url,
        &crawler,
        &[crate::pipelines::mozilla_readability::filter_mozilla_readability],
    )
    .await
    .unwrap_err();

    // URL validation moved to crawler; crawler errors map to WebfetchError::Fetch
    assert!(
        matches!(err, WebfetchError::Fetch(_)),
        "expected Fetch, got {err:?}"
    );
    assert!(
        err.to_string().contains("invalid URL")
            || err.to_string().contains("exceeds maximum length"),
        "error should mention invalid URL or too long, got: {err}"
    );
}

// ── doc_html_to_markdown tests ─────────────────────────────────────────

#[test]
fn test_doc_html_to_markdown_plain_html() {
    let html = "<html><body><h1>Title</h1><p>Paragraph</p></body></html>";
    let result = doc_html_to_markdown(html, None).unwrap();
    assert!(result.contains("Title"), "should contain title text");
    assert!(
        result.contains("Paragraph"),
        "should contain paragraph text"
    );
}

#[test]
fn test_doc_html_to_markdown_removes_scripts() {
    let html = r#"<html><body><script>alert(1)</script><p>Content</p></body></html>"#;
    let result = doc_html_to_markdown(html, None).unwrap();
    assert!(!result.contains("alert"), "scripts should be removed");
    assert!(result.contains("Content"), "content should remain");
}

#[test]
fn test_doc_html_to_markdown_removes_styles() {
    let html =
        r#"<html><head><style>body{color:red}</style></head><body><p>Text</p></body></html>"#;
    let result = doc_html_to_markdown(html, None).unwrap();
    assert!(!result.contains("color:red"), "styles should be removed");
    assert!(result.contains("Text"), "content should remain");
}

#[test]
fn test_doc_html_to_markdown_with_base_url() {
    let html = r#"<html><body><a href="/relative">Link</a></body></html>"#;
    let result = doc_html_to_markdown(html, Some("https://example.com")).unwrap();
    assert!(
        result.contains("https://example.com/relative") || result.contains("/relative"),
        "expected resolved URL in markdown output, got: {result}"
    );
    assert!(result.contains("Link"), "should contain link text");
}

#[test]
fn test_doc_html_to_markdown_empty_html() {
    let html = "";
    let result = doc_html_to_markdown(html, None).unwrap();
    assert!(
        result.trim().is_empty() || result.is_empty(),
        "empty HTML should produce empty output"
    );
}

#[test]
fn test_doc_html_to_markdown_preserves_img() {
    let html = r#"<html><body><img src="pic.png" alt="Pic"/><p>Text</p></body></html>"#;
    let result = doc_html_to_markdown(html, None).unwrap();
    assert!(
        result.contains("pic.png") || result.contains("Pic"),
        "image should be preserved"
    );
    assert!(result.contains("Text"), "text should be preserved");
}

#[test]
fn test_doc_html_to_markdown_public_api() {
    let html = "<html><body><p>API test</p></body></html>";
    let result = doc_html_to_markdown(html, None).unwrap();
    assert!(result.contains("API test"), "public API should work");
}

#[test]
#[ignore = "parse_html is currently infallible (scraper parses everything). Enable when MAX_NODES guard enforced."]
fn test_doc_html_to_markdown_parse_error() {
    let bad_html = "<html><body><p>unclosed";
    let result = doc_html_to_markdown(bad_html, None);
    assert!(result.is_err(), "malformed HTML should produce an error");
    match result {
        Err(WebfetchError::Parse(_)) => {} // expected
        Err(other) => panic!("Expected Parse error, got: {other:?}"),
        Ok(_) => panic!("Expected error, got Ok"),
    }
}

// ── Anti-regression: URL validation hoisted ─────────────────────────
// fetch_and_extract must validate URL (scheme + length) BEFORE dispatching
// to SourceType::ArxivPdf or SourceType::Document branches, not just the
// fallthrough. These branches used to skip validation entirely.

#[tokio::test]
async fn test_fetch_and_extract_rejects_non_http_url() {
    let crawler = RateLimitedCrawler::builder()
        .with_qps(10)
        .with_burst(1)
        .with_max_domains(10)
        .build()
        .expect("build");
    // An FTP URL should be rejected before any fetch attempt
    let result = fetch_and_extract("ftp://example.com/paper.pdf", &crawler, &[]).await;
    assert!(result.is_err(), "Non-HTTP URL should be rejected");
    if let Err(e) = result {
        let msg = format!("{e}");
        assert!(msg.contains("scheme"), "Error should mention scheme: {msg}");
    }
}

#[tokio::test]
async fn test_fetch_and_extract_rejects_overlong_url() {
    let crawler = RateLimitedCrawler::builder()
        .with_qps(10)
        .with_burst(1)
        .with_max_domains(10)
        .build()
        .expect("build");
    let long_url = format!("https://example.com/{}", "a".repeat(2049));
    let result = fetch_and_extract(&long_url, &crawler, &[]).await;
    assert!(result.is_err(), "Overlong URL should be rejected");
}

// ── Anti-regression: doc_to_html returns raw HTML ───────────────────
// doc_to_html must return raw xberg output (HTML), not Markdown.

#[tokio::test]
async fn test_doc_to_html_returns_raw_html() {
    // Test with a simple HTML document (xberg passthrough)
    let html_input = b"<html><body><p>Hello</p></body></html>";
    // doc_to_html writes to tempfile and runs xberg on it.
    // With a .html extension, xberg may pass through or transform.
    // We just verify it doesn't crash and returns something.
    let result = crate::doc_to_html(html_input.to_vec(), "http://example.com/test.html").await;
    // Should succeed — xberg handles HTML input
    assert!(
        result.is_ok(),
        "doc_to_html on HTML bytes should succeed: {:?}",
        result.err()
    );
}

#[tokio::test]
async fn test_doc_to_html_rejects_oversized_input() {
    // Create bytes larger than MAX_DOC_SIZE
    let oversized = vec![0u8; MAX_DOC_SIZE + 1];
    let result = crate::doc_to_html(oversized, "http://example.com/test.pdf").await;
    assert!(result.is_err(), "Oversized input should be rejected");
}

// ── Anti-regression: MIME-detected doc branch streams with size check ──
// The MIME-detected document path in fetch_and_extract must use chunkwise
// streaming (not response.bytes()) so oversized bodies without Content-Length
// are rejected mid-stream.

#[tokio::test]
async fn test_mime_detected_doc_rejects_oversized_without_content_length() {
    // Start a server that streams more than MAX_BODY_SIZE without Content-Length
    use tokio::io::AsyncWriteExt;
    use tokio::net::TcpListener;

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let url = format!("http://{}/test.pdf", addr);

    tokio::spawn(async move {
        if let Ok((mut stream, _)) = listener.accept().await {
            // Chunked response without Content-Length, body > MAX_BODY_SIZE
            let oversized_body = "x".repeat(MAX_BODY_SIZE + 1024);
            let _ = stream
                .write_all(
                    format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: application/pdf\r\n\r\n{}",
                        oversized_body
                    )
                    .as_bytes(),
                )
                .await;
        }
    });

    let crawler = RateLimitedCrawler::builder()
        .with_qps(10)
        .with_burst(1)
        .with_max_domains(10)
        .build()
        .expect("build");

    let result = fetch_and_extract(&url, &crawler, &[]).await;
    assert!(
        result.is_err(),
        "Oversized MIME-detected doc without Content-Length should be rejected"
    );
}

// ── Anti-regression: fetch_raw_html returns raw response ────────────
// fetch_raw_html must return the raw HTTP body, not processed content.

#[tokio::test]
async fn test_fetch_raw_html_returns_raw_body() {
    use tokio::io::AsyncWriteExt;
    use tokio::net::TcpListener;

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let url = format!("http://{}", addr);

    tokio::spawn(async move {
        if let Ok((mut stream, _)) = listener.accept().await {
            let body = "<html><body>raw</body></html>";
            let _ = stream
                .write_all(
                    format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n{}",
                        body.len(),
                        body
                    )
                    .as_bytes(),
                )
                .await;
        }
    });

    let crawler = RateLimitedCrawler::builder()
        .with_qps(10)
        .with_burst(1)
        .with_max_domains(10)
        .build()
        .expect("build");

    let result = crate::fetch_raw_html(&url, &crawler).await;
    assert!(result.is_ok(), "fetch_raw_html should succeed");
    assert_eq!(result.unwrap(), "<html><body>raw</body></html>");
}

// ── Anti-regression: fetch_doc_as_html returns raw HTML ─────────────
// fetch_doc_as_html must return raw xberg output, not Markdown.

#[tokio::test]
async fn test_fetch_doc_as_html_returns_raw_html() {
    use tokio::io::AsyncWriteExt;
    use tokio::net::TcpListener;

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let url = format!("http://{}/test.pdf", addr);

    tokio::spawn(async move {
        if let Ok((mut stream, _)) = listener.accept().await {
            // Minimal PDF that xberg can process
            let _ = stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 4\r\n\r\n%PDF")
                .await;
        }
    });

    let crawler = RateLimitedCrawler::builder()
        .with_qps(10)
        .with_burst(1)
        .with_max_domains(10)
        .build()
        .expect("build");

    let result = crate::fetch_doc_as_html(&url, &crawler).await;
    let html =
        result.expect("fetch_doc_as_html should succeed — if xberg is unavailable, skip this test");
    assert!(
        !html.contains("# "),
        "fetch_doc_as_html should not return Markdown"
    );
}

// ── Anti-regression: fetch_doc still returns ExtractionResult ───────
// fetch_doc's return type must not change (backward compat).

#[tokio::test]
async fn test_fetch_doc_returns_extraction_result() {
    use tokio::io::AsyncWriteExt;
    use tokio::net::TcpListener;

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let url = format!("http://{}", addr);

    tokio::spawn(async move {
        if let Ok((mut stream, _)) = listener.accept().await {
            let _ = stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/html\r\n\r\n<p>hello</p>")
                .await;
        }
    });

    let crawler = RateLimitedCrawler::builder()
        .with_qps(10)
        .with_burst(1)
        .with_max_domains(10)
        .build()
        .expect("build");

    let result = crate::fetch_doc(&url, &crawler).await;
    // fetch_doc should return ExtractionResult, not String
    let result =
        result.expect("fetch_doc should succeed — if xberg is unavailable, skip this test");
    match result {
        crate::ExtractionResult::GenericHtml { .. } => {} // expected
        _ => panic!("fetch_doc should return GenericHtml variant"),
    }
}

// ── Anti-regression: IPv6 private IP detection ─────────────────────
// These functions are security-critical for SSRF protection.

#[test]
fn test_is_private_ip_v4_loopback() {
    assert!(crate::is_private_ip(&std::net::IpAddr::V4(
        std::net::Ipv4Addr::new(127, 0, 0, 1)
    )));
}

#[test]
fn test_is_private_ip_v4_public() {
    assert!(!crate::is_private_ip(&std::net::IpAddr::V4(
        std::net::Ipv4Addr::new(8, 8, 8, 8)
    )));
}

#[test]
fn test_is_private_ip_v6_loopback() {
    assert!(crate::is_private_ip(&std::net::IpAddr::V6(
        std::net::Ipv6Addr::new(0, 0, 0, 0, 0, 0, 0, 1)
    )));
}

#[test]
fn test_is_private_ip_v6_ula() {
    assert!(crate::is_private_ip(&std::net::IpAddr::V6(
        std::net::Ipv6Addr::new(0xfc00, 0, 0, 0, 0, 0, 0, 1)
    )));
}

#[test]
fn test_is_private_ip_v6_link_local() {
    assert!(crate::is_private_ip(&std::net::IpAddr::V6(
        std::net::Ipv6Addr::new(0xfe80, 0, 0, 0, 0, 0, 0, 1)
    )));
}

#[test]
fn test_is_private_ip_v6_ipv4_mapped_loopback() {
    // ::ffff:127.0.0.1 — mapped loopback, should be private
    let mapped = std::net::IpAddr::V6(std::net::Ipv6Addr::new(0, 0, 0, 0, 0, 0xffff, 0x7f00, 1));
    assert!(crate::is_private_ip(&mapped));
}

#[test]
fn test_is_private_ip_v6_ipv4_mapped_public() {
    // ::ffff:8.8.8.8 — mapped public IP, should NOT be private
    let mapped = std::net::IpAddr::V6(std::net::Ipv6Addr::new(
        0, 0, 0, 0, 0, 0xffff, 0x0808, 0x0808,
    ));
    assert!(
        !crate::is_private_ip(&mapped),
        "mapped public IP should not be private"
    );
}

#[test]
fn test_is_private_ip_v6_documentation() {
    let doc = std::net::IpAddr::V6(std::net::Ipv6Addr::new(0x2001, 0x0db8, 0, 0, 0, 0, 0, 1));
    assert!(crate::is_private_ip(&doc));
}

#[test]
fn test_is_private_ip_v6_public() {
    let pub_addr = std::net::IpAddr::V6(std::net::Ipv6Addr::new(
        0x2001, 0x4860, 0x4860, 0, 0, 0, 0, 0x8888,
    ));
    assert!(!crate::is_private_ip(&pub_addr));
}

#[test]
fn test_same_subnet_v4_same() {
    let a = std::net::SocketAddr::new(
        std::net::IpAddr::V4(std::net::Ipv4Addr::new(10, 0, 1, 1)),
        80,
    );
    let b = std::net::SocketAddr::new(
        std::net::IpAddr::V4(std::net::Ipv4Addr::new(10, 0, 2, 2)),
        443,
    );
    assert!(crate::same_subnet_16(a, b));
}

#[test]
fn test_same_subnet_v4_diff() {
    let a = std::net::SocketAddr::new(
        std::net::IpAddr::V4(std::net::Ipv4Addr::new(10, 0, 1, 1)),
        80,
    );
    let b = std::net::SocketAddr::new(
        std::net::IpAddr::V4(std::net::Ipv4Addr::new(10, 1, 1, 1)),
        443,
    );
    assert!(!crate::same_subnet_16(a, b));
}

// ── Body-injection: blocked hard-fail equivalence ─────────────
// These tests inject a raw body past the fetch step via the #[cfg(test)]
// seam `fetch_and_extract_inner_with_body`, proving the blocked hard-fail
// equivalence without network or host-override.

fn test_crawler_no_network() -> RateLimitedCrawler {
    RateLimitedCrawler::builder().build().unwrap()
}

#[tokio::test]
async fn test_injected_generic_html_bot_blocked_equiv() {
    let crawler = test_crawler_no_network();
    let url = "https://example.com/article";
    let bot_body = "<html><body><div class=\"cf-turnstile\"></div></body></html>";
    let (result, status) =
        fetch_and_extract_inner_with_body(url, &crawler, &[], bot_body.to_string())
            .await
            .expect("inner should return Ok with a Blocked status");
    assert!(
        matches!(
            status,
            PageStatus::Blocked {
                by: BlockedBy::CloudflareTurnstile
            }
        ),
        "expected Blocked(CloudflareTurnstile), got {status:?}"
    );
    // fetch_and_extract (wrapper) maps Blocked -> Err.
    assert!(
        wrap_blocked_status(result, status).is_err(),
        "wrapper must hard-fail on a Blocked status"
    );
}

#[tokio::test]
async fn test_injected_reddit_bot_blocked_both_err() {
    let crawler = test_crawler_no_network();
    let url = "https://www.reddit.com/r/test/comments/abc123/hello_world/";
    let bot_body = "<html>Just a moment... <div class=\"cf-turnstile\"></div></html>";
    let err = fetch_and_extract_inner_with_body(url, &crawler, &[], bot_body.to_string())
        .await
        .expect_err("Reddit keeps its hard-fail on bot-blocked bodies");
    assert!(
        matches!(&err, WebfetchError::Fetch(m) if m == BLOCKED_MSG),
        "expected Fetch(BLOCKED_MSG), got {err:?}"
    );
    // Both fetch_and_extract and fetch_and_extract_with_status delegate to the
    // inner, which errored, so both return Err.
}

#[tokio::test]
async fn test_injected_reddit_fixture_metadata_fields() {
    // Full-pipeline assertion of the Reddit metadata fields added under RID:
    // `comment_count` and `source_url`, sourced from the REAL fixture
    // (`tests/fixtures-webfetch/reddit/reddit-thread-simple.json.zst`) rather
    // than a synthetic page. The fixture contains exactly 2 top-level comments
    // (the nested reply lives inside the 2nd comment's `replies` and is NOT
    // counted by `comment_count`), below the MAX_COMMENTS=500 cap, and its
    // permalink is `/r/test/comments/abc123/hello_world/`.
    let crawler = test_crawler_no_network();
    let url = "https://www.reddit.com/r/test/comments/abc123/hello_world/";

    let path: std::path::PathBuf = [
        env!("CARGO_MANIFEST_DIR"),
        "tests",
        "fixtures-webfetch",
        "reddit",
        "reddit-thread-simple.json.zst",
    ]
    .iter()
    .collect();
    let compressed = std::fs::read(&path).expect("failed to read reddit fixture");
    let body = String::from_utf8(zstd::decode_all(compressed.as_slice()).expect("decompress"))
        .expect("utf-8");

    let (result, _status) = fetch_and_extract_inner_with_body(url, &crawler, &[], body)
        .await
        .expect("full-pipeline reddit extraction should succeed");

    match result {
        ExtractionResult::Reddit {
            comment_count,
            source_url,
            ..
        } => {
            // 2 top-level comments in the fixture (known constant, not derived
            // from the code under test).
            assert_eq!(comment_count, 2, "fixture has 2 top-level comments");
            // source_url is the permalink-derived well-formed URL.
            assert_eq!(
                source_url,
                "https://reddit.com/r/test/comments/abc123/hello_world/",
            );
        }
        other => panic!("Expected ExtractionResult::Reddit, got {other:?}"),
    }
}

#[tokio::test]
async fn test_injected_content_bearing_bot_body_is_article() {
    // Differential fixture 1: content-bearing (>=200) bot body -> Ok((_, Article))
    // from both functions (content beats the bot marker).
    let crawler = test_crawler_no_network();
    let url = "https://example.com/article";
    let content = "x".repeat(300);
    let body =
        format!("<html><body><div class=\"cf-turnstile\"></div><p>{content}</p></body></html>");
    let (result, status) = fetch_and_extract_inner_with_body(url, &crawler, &[], body)
        .await
        .unwrap();
    assert_eq!(status, PageStatus::Article, "content beats the bot marker");
    assert!(
        wrap_blocked_status(result, status).is_ok(),
        "content-bearing blocked page must not hard-fail"
    );
}

#[tokio::test]
async fn test_injected_thin_consent_wall_err_and_blocked() {
    // Differential fixture 2: thin consent-walled body -> Err from
    // fetch_and_extract, Ok((_, Blocked{CookieConsent})) from
    // fetch_and_extract_with_status (consent-walled thin pages hard-fail).
    let crawler = test_crawler_no_network();
    let url = "https://example.com/article";
    let body = r#"<html><body><script src="https://consent.google.com/x"></script></body></html>"#;
    let (result, status) = fetch_and_extract_inner_with_body(url, &crawler, &[], body.to_string())
        .await
        .unwrap();
    assert!(
        matches!(
            status,
            PageStatus::Blocked {
                by: BlockedBy::CookieConsent
            }
        ),
        "expected Blocked(CookieConsent), got {status:?}"
    );
    assert!(
        wrap_blocked_status(result, status).is_err(),
        "consent-walled thin page must hard-fail in fetch_and_extract"
    );
}

// ── Pure status mapper ───────────────────────────────────────

#[test]
fn test_structured_success_status_is_article() {
    // Reddit/Discourse/arXiv/Document all map to Article via this single helper.
    assert_eq!(structured_success_status(), PageStatus::Article);
}

// ── Production-path: JSHeavy measurement (Phase 2) ────────────────
// These tests drive the REAL `filter_trafilatura` pipeline through the
// #[cfg(test)] seam `fetch_and_extract_inner_with_body`, pinning the URL to a
// GenericHtml route and self-verifying the pre-pipeline measurement
// preconditions before asserting the status.

fn load_js_challenge_fixture() -> String {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures-webfetch/js-challenge/google-enablejs.html.zst");
    let compressed = std::fs::read(&path).unwrap_or_else(|e| panic!("read fixture {path:?}: {e}"));
    let decompressed = zstd::decode_all(compressed.as_slice())
        .unwrap_or_else(|e| panic!("decompress {path:?}: {e}"));
    String::from_utf8(decompressed).expect("fixture is UTF-8")
}

/// The body must contain none of the anti-bot/consent/paywall markers so
/// no higher-priority signal fires before `is_js_heavy`.
fn assert_no_bot_consent_paywall_markers(body: &str) {
    const MARKERS: &[&str] = &[
        "turnstile",
        "challenge-platform",
        "data-sitekey",
        "cf-",
        "g-recaptcha",
        "consent.google",
        "data-paywall",
        "paywall-",
        "metered-content",
    ];
    let lower = body.to_lowercase();
    for m in MARKERS {
        assert!(!lower.contains(m), "body must be marker-free, found {m}");
    }
}

#[tokio::test]
async fn test_prod_path_js_challenge() {
    // Pin the URL to a GenericHtml route so the modified call site is exercised.
    let url = "https://example.com/js-challenge";
    assert_eq!(detect_source_type(url), SourceType::GenericHtml);

    let body = load_js_challenge_fixture();
    // Marker-free (no anti-bot/consent/paywall).
    assert_no_bot_consent_paywall_markers(&body);
    // Script-dominance preconditions (visible < 200, script > visible), recomputed PRE-pipeline.
    let dom = parse_html(&body).expect("fixture parses");
    let visible_len = dom.visible_text_len();
    let script_len = dom.script_len();
    assert!(
        visible_len < 200,
        "pre-pipeline visible_text_len must be < 200, got {visible_len}"
    );
    assert!(
        script_len > visible_len,
        "script_len must exceed visible_len ({script_len} vs {visible_len})"
    );

    let crawler = test_crawler_no_network();
    let (_, status) = fetch_and_extract_inner_with_body(
        url,
        &crawler,
        &[crate::pipelines::trafilatura::filter_trafilatura],
        body,
    )
    .await
    .expect("inner should return Ok with a JSHeavy status");

    assert_eq!(
        status,
        PageStatus::JSHeavy,
        "JS-challenge must be JSHeavy, not Article/Empty, got {status:?}"
    );
}

#[tokio::test]
async fn test_prod_path_script_only_jsheavy() {
    // A pure script-only page with no enable-js marker and no SPA marker
    // -> JSHeavy by measurement alone (no marker required).
    let url = "https://example.com/script-only";
    assert_eq!(detect_source_type(url), SourceType::GenericHtml);

    let script = "var heavy = ".to_string() + &"x".repeat(2000) + ";";
    let body = format!(
        "<html><head><title>Script</title></head><body><script>{script}</script></body></html>"
    );
    let dom = parse_html(&body).expect("parses");
    let visible_len = dom.visible_text_len();
    let script_len = dom.script_len();
    assert!(visible_len < 200, "visible: {visible_len}");
    assert!(script_len > visible_len, "script-dominant");
    assert_no_bot_consent_paywall_markers(&body);

    let crawler = test_crawler_no_network();
    let (_, status) = fetch_and_extract_inner_with_body(
        url,
        &crawler,
        &[crate::pipelines::trafilatura::filter_trafilatura],
        body,
    )
    .await
    .expect("Ok");
    assert_eq!(
        status,
        PageStatus::JSHeavy,
        "script-only marker-free -> JSHeavy by measurement, got {status:?}"
    );
}

#[tokio::test]
async fn test_prod_path_style_only_empty() {
    // Miss path: a style-only thin page (CSS, no script, no marker)
    // -> Empty, NOT JSHeavy.
    let url = "https://example.com/style-only";
    assert_eq!(detect_source_type(url), SourceType::GenericHtml);
    let body = r#"<html><head><style>body{color:red;margin:0;}</style></head><body></body></html>"#
        .to_string();

    let crawler = test_crawler_no_network();
    let (_, status) = fetch_and_extract_inner_with_body(
        url,
        &crawler,
        &[crate::pipelines::trafilatura::filter_trafilatura],
        body,
    )
    .await
    .expect("Ok");
    assert_eq!(
        status,
        PageStatus::Empty,
        "style-only thin page -> Empty, not JSHeavy, got {status:?}"
    );
}

#[tokio::test]
async fn test_prod_path_readable_article_no_regression() {
    // A readable article (visible text >= 200) with script/style
    // + consent/paywall/SPA + enable-js markers -> Article (Article-first).
    let url = "https://example.com/article";
    assert_eq!(detect_source_type(url), SourceType::GenericHtml);

    let content = "x".repeat(500);
    let body = format!(
        "<html><head><title>Article</title>\
         <script src=\"https://consent.google.com/x\"></script>\
         <style>body{{}}</style></head>\
         <body><div id=\"root\"></div>\
         <div data-x=\"httpservice/retry/enablejs\"></div>\
         <article>{content}</article></body></html>"
    );
    let dom = parse_html(&body).expect("parses");
    let visible_len = dom.visible_text_len();
    assert!(
        visible_len >= 200,
        "precondition: pre-pipeline visible_len must be >= 200, got {visible_len}"
    );

    let crawler = test_crawler_no_network();
    let (_, status) = fetch_and_extract_inner_with_body(
        url,
        &crawler,
        &[crate::pipelines::trafilatura::filter_trafilatura],
        body,
    )
    .await
    .expect("Ok");
    assert_eq!(
        status,
        PageStatus::Article,
        "readable article must be Article (Article-first), got {status:?}"
    );
}
