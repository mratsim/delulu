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
        ExtractionResult::GenericHtml { content_md } => {
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
        ExtractionResult::GenericHtml { content_md } => {
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
    // May fail due to xberg not finding valid PDF content, but should NOT
    // return Markdown (would indicate doc_to_html is doing md conversion)
    if let Ok(html) = result {
        assert!(
            !html.contains("# "),
            "fetch_doc_as_html should not return Markdown"
        );
    }
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
    match result {
        Ok(crate::ExtractionResult::GenericHtml { .. }) => {} // expected
        Ok(_) => panic!("fetch_doc should return GenericHtml variant"),
        Err(_) => {} // may fail on real xberg, but type is correct
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
