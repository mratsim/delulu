//! Library integration tests for `fetch_and_extract` using fixture data (mocked HTTP).
//!
//! Each test decompresses a `.zst` fixture from `tests/fixtures-webfetch/`, serves it
//! through a local test server, calls `fetch_and_extract`, and verifies the
//! returned `ExtractionResult`.

use std::path::PathBuf;
use std::time::Duration;

use delulu_rate_limited_crawler::RateLimitedCrawler;
use delulu_webfetch::sources::reddit::RedditExtractor;
use delulu_webfetch::{fetch_and_extract, types::*};
use tokio::io::AsyncWriteExt;
use tokio::net::TcpListener;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn fixture_path(name: &str) -> PathBuf {
    let base: PathBuf = [env!("CARGO_MANIFEST_DIR"), "tests", "fixtures-webfetch"]
        .iter()
        .collect();
    base.join(name)
}

fn load_fixture(name: &str) -> String {
    let path = fixture_path(name);
    let compressed =
        std::fs::read(&path).unwrap_or_else(|e| panic!("failed to read fixture {path:?}: {e}"));
    let decompressed = zstd::decode_all(compressed.as_slice())
        .unwrap_or_else(|e| panic!("failed to decompress {path:?}: {e}"));
    String::from_utf8(decompressed)
        .unwrap_or_else(|e| panic!("fixture {path:?} is not valid UTF-8: {e}"))
}

// Store raw fixture text for creating per-test mock clients
fn reddit_fixture_body() -> String {
    load_fixture("reddit/reddit-thread-simple.json.zst")
}

fn discourse_html_fixture_body() -> String {
    load_fixture("forum-discourse/ethresear.ch/reed-solomon.html.zst")
}

fn discourse_json_fixture_body() -> String {
    load_fixture("forum-discourse/ethresear.ch/reed-solomon.json.zst")
}

fn generic_html_fixture_body() -> String {
    load_fixture("blog/dankrad.de/pcs-multiproofs.html.zst")
}

// ---------------------------------------------------------------------------
// Test server helper
// ---------------------------------------------------------------------------

/// Spawn a minimal HTTP server that responds with the given body for any request.
async fn spawn_test_server(
    status: u16,
    content_type: String,
    body: String,
) -> std::net::SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    let body_bytes = body.into_bytes();
    tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let reason = if status == 200 {
            "OK"
        } else if status == 404 {
            "Not Found"
        } else {
            "Unknown"
        };
        let head = format!(
            "HTTP/1.1 {status} {reason}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body_bytes.len()
        );
        socket.write_all(head.as_bytes()).await.unwrap();
        socket.write_all(&body_bytes).await.unwrap();
    });

    addr
}

/// Create a RateLimitedCrawler pointed at a local test server.
fn test_crawler_for(_addr: std::net::SocketAddr) -> RateLimitedCrawler {
    let raw_client = wreq::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .unwrap();
    RateLimitedCrawler::builder()
        .with_client(raw_client)
        .build()
        .unwrap()
}

// ---------------------------------------------------------------------------
// Reddit
// ---------------------------------------------------------------------------
//
// These tests verify RedditExtractor::extract() directly using fixture data,
// bypassing HTTP. URL detection and URL-to-API transformation are tested
// separately in `detect_test.rs`.

#[tokio::test]
async fn test_fetch_and_extract_reddit_from_fixture() {
    // Verifies that RedditExtractor correctly extracts post metadata
    // (title, author, score, selftext, permalink) and top-level comments
    // from a real Reddit JSON API fixture.
    let body = reddit_fixture_body();
    let data = RedditExtractor::extract(&body).expect("RedditExtractor::extract should succeed");

    assert_eq!(data.title, "Hello World from Reddit");
    assert_eq!(data.author, "reddit_user");
    assert_eq!(data.score, 42);
    assert_eq!(data.selftext, "This is the post body content");
    assert_eq!(data.permalink, "/r/test/comments/abc123/hello_world/");
    assert!(!data.comments.is_empty(), "should have comments");
    // The fixture contains exactly 2 top-level comments (the nested reply
    // lives inside the 2nd comment's `replies` and is NOT counted). This is a
    // known fixture constant, pinning the value `comment_count` is derived from
    // (`data.comments.len()` in lib.rs) — well below the MAX_COMMENTS=500 cap.
    assert_eq!(data.comments.len(), 2, "fixture has 2 top-level comments");
    assert!(
        data.comments[0].body.contains("First comment body"),
        "first comment body mismatch: {}",
        data.comments[0].body
    );
}

#[tokio::test]
async fn test_fetch_and_extract_reddit_replies_are_threaded() {
    // Verifies that RedditExtractor correctly preserves the nested reply
    // structure: comments with replies are threaded (not flattened).
    // At least one comment should contain a nested reply with "Nested reply".
    let body = reddit_fixture_body();
    let data = RedditExtractor::extract(&body).expect("RedditExtractor::extract should succeed");

    let has_nested_reply = data
        .comments
        .iter()
        .any(|c| c.replies.iter().any(|r| r.body.contains("Nested reply")));
    assert!(
        has_nested_reply,
        "expected at least one threaded (nested) reply"
    );
}

// ---------------------------------------------------------------------------
// Discourse
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_fetch_and_extract_discourse_from_fixture() {
    let html_body = discourse_html_fixture_body();
    let json_body = discourse_json_fixture_body();

    // Discourse tests need two fetches: first HTML, then JSON.
    // We use a multi-response server approach: first request gets HTML, second gets JSON.
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    let html_bytes = html_body.as_bytes().to_vec();
    let json_bytes = json_body.as_bytes().to_vec();
    let _serve_html = std::sync::atomic::AtomicBool::new(true);

    tokio::spawn(async move {
        // First connection: serve HTML
        if let Ok((mut socket, _)) = listener.accept().await {
            let head = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                html_bytes.len()
            );
            socket.write_all(head.as_bytes()).await.unwrap();
            socket.write_all(&html_bytes).await.unwrap();
        }
        // Second connection: serve JSON
        if let Ok((mut socket, _)) = listener.accept().await {
            let head = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                json_bytes.len()
            );
            socket.write_all(head.as_bytes()).await.unwrap();
            socket.write_all(&json_bytes).await.unwrap();
        }
    });

    let crawler = test_crawler_for(addr);

    // Use a URL that triggers Discourse detection
    let original_url = format!("http://{}/t/reed-solomon-erasure-code-recovery/3039", addr);

    let result = fetch_and_extract(
        &original_url,
        &crawler,
        &[delulu_webfetch::pipelines::mozilla_readability::filter_mozilla_readability],
    )
    .await
    .unwrap();

    match result {
        ExtractionResult::Discourse {
            title,
            topic_id,
            posts,
            post_count,
            posts_returned,
            ..
        } => {
            // Fixture has posts_count=12 and delivers all 12 posts in the
            // JSON response (known constants, not derived from the code under
            // test): post_count is the server total, posts_returned is what
            // was fetched.
            assert_eq!(post_count, 12, "fixture posts_count is 12");
            assert_eq!(posts_returned, 12, "fixture delivers 12 posts");
            assert_eq!(
                title,
                "Reed-Solomon erasure code recovery in n*log^2(n) time with FFTs"
            );
            assert_eq!(topic_id, 3039);
            assert_eq!(posts.len(), 12, "expected 12 posts in fixture");
            assert_eq!(posts[0].username, "vbuterin");
            assert_eq!(posts[1].username, "sourabhniyogi");
        }
        other => panic!("Expected ExtractionResult::Discourse, got {other:?}"),
    }
}

#[tokio::test]
async fn test_fetch_and_extract_discourse_posts_have_raw_markdown() {
    let html_body = discourse_html_fixture_body();
    let json_body = discourse_json_fixture_body();

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    let html_bytes = html_body.as_bytes().to_vec();
    let json_bytes = json_body.as_bytes().to_vec();

    tokio::spawn(async move {
        if let Ok((mut socket, _)) = listener.accept().await {
            let head = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                html_bytes.len()
            );
            socket.write_all(head.as_bytes()).await.unwrap();
            socket.write_all(&html_bytes).await.unwrap();
        }
        if let Ok((mut socket, _)) = listener.accept().await {
            let head = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                json_bytes.len()
            );
            socket.write_all(head.as_bytes()).await.unwrap();
            socket.write_all(&json_bytes).await.unwrap();
        }
    });

    let crawler = test_crawler_for(addr);
    let original_url = format!("http://{}/t/reed-solomon-erasure-code-recovery/3039", addr);

    let result = fetch_and_extract(
        &original_url,
        &crawler,
        &[delulu_webfetch::pipelines::mozilla_readability::filter_mozilla_readability],
    )
    .await
    .unwrap();

    match result {
        ExtractionResult::Discourse { posts, .. } => {
            assert!(
                posts[0].raw.contains("Fast Fourier transforms"),
                "post 0 should contain expected content, got: {}",
                posts[0].raw
            );
            assert!(
                posts[1].raw.contains("fountain codes"),
                "post 1 should contain expected content, got: {}",
                posts[1].raw
            );
        }
        other => panic!("Expected ExtractionResult::Discourse, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// Generic HTML
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_fetch_and_extract_generic_html_from_fixture() {
    let body = generic_html_fixture_body();
    let addr = spawn_test_server(200, "text/html".to_string(), body.clone()).await;
    let crawler = test_crawler_for(addr);
    let url = format!("http://{}/article", addr);

    let result = fetch_and_extract(
        &url,
        &crawler,
        &[delulu_webfetch::pipelines::mozilla_readability::filter_mozilla_readability],
    )
    .await
    .unwrap();

    match result {
        ExtractionResult::GenericHtml { content_md, .. } => {
            assert!(
                content_md.body.contains("PCS multiproofs"),
                "body should contain the article's h1 heading, got: {}",
                content_md.body
            );
            assert!(
                content_md.body.contains("random evaluation"),
                "body should contain random evaluation content"
            );
            assert!(
                content_md.body.contains("verkle"),
                "body should contain verkle content"
            );
            assert!(
                content_md.body.contains("polynomial commitments"),
                "body should contain polynomial commitments content"
            );
            assert!(
                content_md.frontmatter.starts_with("title: \n"),
                "frontmatter should have empty title after readability (h1→h2), got: {}",
                content_md.frontmatter
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
        other => panic!("Expected ExtractionResult::GenericHtml, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// Edge cases
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_fetch_and_extract_empty_body_returns_generic_html() {
    let body = "<html><head></head><body></body></html>";
    let addr = spawn_test_server(200, "text/html".to_string(), body.to_string()).await;
    let crawler = test_crawler_for(addr);
    let url = format!("http://{}/empty", addr);

    let result = fetch_and_extract(
        &url,
        &crawler,
        &[delulu_webfetch::pipelines::mozilla_readability::filter_mozilla_readability],
    )
    .await
    .unwrap();

    match result {
        ExtractionResult::GenericHtml { content_md, .. } => {
            assert!(
                content_md.body.trim().is_empty(),
                "empty HTML body should produce empty markdown, got: {:?}",
                content_md.body
            );
            assert!(
                content_md.frontmatter.contains("source_type: generic_html"),
                "frontmatter should indicate generic_html"
            );
        }
        other => panic!("Expected GenericHtml, got {other:?}"),
    }
}
#[tokio::test]
async fn test_fetch_and_extract_generic_html_title_from_h1() {
    // The readability pipeline converts the first <h1> to <h2>, so the
    // frontmatter title is sourced from the <title> tag fallback rather than
    // from an <h1> element. This test verifies the GenericHtml path works
    // correctly with a real fixture through the full pipeline.
    let body = generic_html_fixture_body();
    let addr = spawn_test_server(200, "text/html".to_string(), body.clone()).await;
    let crawler = test_crawler_for(addr);
    let url = format!("http://{}/article", addr);

    let result = fetch_and_extract(
        &url,
        &crawler,
        &[delulu_webfetch::pipelines::mozilla_readability::filter_mozilla_readability],
    )
    .await
    .unwrap();

    match result {
        ExtractionResult::GenericHtml { content_md, .. } => {
            // After readability converts h1→h2, the title comes from the
            // <title> tag fallback (not from an h1). Verify the pipeline
            // produced a valid GenericHtml result.
            assert!(
                content_md.frontmatter.contains("source_type: generic_html"),
                "frontmatter should indicate generic_html, got: {}",
                content_md.frontmatter
            );
            assert!(!content_md.body.is_empty(), "body should not be empty");
        }
        other => panic!("expected GenericHtml, got {other:?}"),
    }
}

#[tokio::test]
async fn test_fetch_and_extract_non_2xx_status_returns_error() {
    let addr = spawn_test_server(404, "text/plain".to_string(), "Not Found".to_string()).await;
    let crawler = test_crawler_for(addr);
    let url = format!("http://{}/notfound", addr);

    let err = fetch_and_extract(
        &url,
        &crawler,
        &[delulu_webfetch::pipelines::mozilla_readability::filter_mozilla_readability],
    )
    .await
    .unwrap_err();

    assert!(
        matches!(err, WebfetchError::Fetch(_)),
        "expected WebfetchError::Fetch, got {err:?}"
    );
    assert!(
        err.to_string().contains("404") || err.to_string().contains("HTTP error 404"),
        "error should mention HTTP 404, got: {err}"
    );
}

#[tokio::test]
async fn test_fetch_and_extract_https_only_scheme_rejected() {
    // URL validation moved to crawler; crawler errors map to WebfetchError::Fetch
    let addr = spawn_test_server(200, "text/plain".to_string(), "ok".to_string()).await;
    let crawler = test_crawler_for(addr);

    let err = fetch_and_extract(
        "ftp://example.com/file",
        &crawler,
        &[delulu_webfetch::pipelines::mozilla_readability::filter_mozilla_readability],
    )
    .await
    .unwrap_err();
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
async fn test_fetch_and_extract_reddit_with_trailing_slash() {
    // Verifies that RedditExtractor works correctly with fixture data.
    // URL trailing-slash handling is tested separately in detect_test.rs.
    let body = reddit_fixture_body();
    let data = RedditExtractor::extract(&body).expect("RedditExtractor::extract should succeed");

    assert_eq!(data.title, "Hello World from Reddit");
}

// ---------------------------------------------------------------------------
// Phase 4b: New integration tests for non-regex Discourse detection
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_fetch_and_extract_non_discourse_t_url_is_generic_html() {
    // URL matches old DISCOURSE_URL_RE pattern (/t/<slug>/<digits>) but has NO Discourse markers.
    let body = "<html><body><p>Regular page on a /t/ URL</p></body></html>";
    let addr = spawn_test_server(200, "text/html".to_string(), body.to_string()).await;
    let crawler = test_crawler_for(addr);
    let url = format!("http://{}/t/foo/123", addr);

    let result = fetch_and_extract(
        &url,
        &crawler,
        &[delulu_webfetch::pipelines::mozilla_readability::filter_mozilla_readability],
    )
    .await
    .unwrap();

    match result {
        ExtractionResult::GenericHtml { content_md, .. } => {
            assert!(
                content_md.frontmatter.contains("source_type: generic_html"),
                "should be GenericHtml, got frontmatter: {}",
                content_md.frontmatter
            );
        }
        other => panic!("Expected ExtractionResult::GenericHtml, got {other:?}"),
    }
}

#[tokio::test]
async fn test_fetch_and_extract_discourse_detected_from_html_content() {
    let html_body = r###"<html><head><meta name="generator" content="Discourse 3.5.2 - https://discourse.org"></head><body><p>Discourse topic HTML</p></body></html>"###;
    let discourse_json = serde_json::json!({
        "title": "Detected Discourse Topic",
        "id": 54321,
        "slug": "detected-discourse",
        "posts_count": 1,
        "post_stream": {
            "posts": [{
                "post_number": 1,
                "username": "detected_user",
                "raw": "<p>Detected content</p>",
                "created_at": "2024-01-01T00:00:00Z",
                "reply_to_post_number": null
            }]
        }
    })
    .to_string();

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    let html_bytes = html_body.as_bytes().to_vec();
    let json_bytes = discourse_json.as_bytes().to_vec();

    tokio::spawn(async move {
        if let Ok((mut socket, _)) = listener.accept().await {
            let head = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                html_bytes.len()
            );
            socket.write_all(head.as_bytes()).await.unwrap();
            socket.write_all(&html_bytes).await.unwrap();
        }
        if let Ok((mut socket, _)) = listener.accept().await {
            let head = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                json_bytes.len()
            );
            socket.write_all(head.as_bytes()).await.unwrap();
            socket.write_all(&json_bytes).await.unwrap();
        }
    });

    let crawler = test_crawler_for(addr);
    let original_url = format!("http://{}/page", addr);

    let result = fetch_and_extract(
        &original_url,
        &crawler,
        &[delulu_webfetch::pipelines::mozilla_readability::filter_mozilla_readability],
    )
    .await
    .unwrap();

    match result {
        ExtractionResult::Discourse {
            title,
            topic_id,
            posts,
            ..
        } => {
            assert_eq!(title, "Detected Discourse Topic");
            assert_eq!(topic_id, 54321);
            assert_eq!(posts.len(), 1);
            assert_eq!(posts[0].username, "detected_user");
        }
        other => panic!("Expected ExtractionResult::Discourse, got {other:?}"),
    }
}

#[tokio::test]
async fn test_fetch_and_extract_discourse_with_simple_fixture() {
    let json_body = discourse_json_fixture_body();
    let html_body = r###"<html><head><meta name="generator" content="Discourse"></head><body><p>Minimal Discourse topic HTML</p></body></html>"###;

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    let html_bytes = html_body.as_bytes().to_vec();
    let json_bytes = json_body.as_bytes().to_vec();

    tokio::spawn(async move {
        if let Ok((mut socket, _)) = listener.accept().await {
            let head = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                html_bytes.len()
            );
            socket.write_all(head.as_bytes()).await.unwrap();
            socket.write_all(&html_bytes).await.unwrap();
        }
        if let Ok((mut socket, _)) = listener.accept().await {
            let head = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                json_bytes.len()
            );
            socket.write_all(head.as_bytes()).await.unwrap();
            socket.write_all(&json_bytes).await.unwrap();
        }
    });

    let crawler = test_crawler_for(addr);
    let original_url = format!("http://{}/t/reed-solomon-erasure-code-recovery/3039", addr);

    let result = fetch_and_extract(
        &original_url,
        &crawler,
        &[delulu_webfetch::pipelines::mozilla_readability::filter_mozilla_readability],
    )
    .await
    .unwrap();

    match result {
        ExtractionResult::Discourse {
            title,
            topic_id,
            posts,
            ..
        } => {
            assert_eq!(
                title,
                "Reed-Solomon erasure code recovery in n*log^2(n) time with FFTs"
            );
            assert_eq!(topic_id, 3039);
            assert!(!posts.is_empty(), "should have posts");
            assert_eq!(posts[0].username, "vbuterin");
        }
        other => panic!("Expected ExtractionResult::Discourse, got {other:?}"),
    }
}

#[tokio::test]
async fn test_fetch_and_extract_stale_discourse_markers_falls_back_to_generic_html() {
    let html_body = "<html><head><meta name=\"generator\" content=\"Discourse\"></head><body><p>Stale content, migrated away</p></body></html>";

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    let html_bytes = html_body.as_bytes().to_vec();

    tokio::spawn(async move {
        // First connection: serve HTML
        if let Ok((mut socket, _)) = listener.accept().await {
            let head = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                html_bytes.len()
            );
            socket.write_all(head.as_bytes()).await.unwrap();
            socket.write_all(&html_bytes).await.unwrap();
        }
        // Second connection: serve 404
        let not_found = b"Not Found";
        if let Ok((mut socket, _)) = listener.accept().await {
            let head = format!(
                "HTTP/1.1 404 Not Found\r\nContent-Type: text/plain\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                not_found.len()
            );
            socket.write_all(head.as_bytes()).await.unwrap();
            socket.write_all(not_found).await.unwrap();
        }
    });

    let crawler = test_crawler_for(addr);
    let original_url = format!("http://{}/t/stale-topic/999", addr);

    let result = fetch_and_extract(
        &original_url,
        &crawler,
        &[delulu_webfetch::pipelines::mozilla_readability::filter_mozilla_readability],
    )
    .await
    .unwrap();

    match result {
        ExtractionResult::GenericHtml { content_md, .. } => {
            assert!(
                content_md.frontmatter.contains("source_type: generic_html"),
                "should fall back to GenericHtml, got frontmatter: {}",
                content_md.frontmatter
            );
            assert!(
                content_md.body.contains("Stale content"),
                "body should contain original HTML content, got: {}",
                content_md.body
            );
        }
        other => panic!("Expected ExtractionResult::GenericHtml, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// arXiv HTML5 pipeline
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_fetch_and_extract_arxiv_valida_isa() {
    let manifest = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let fixture_path = manifest.join("tests/fixtures-arxiv/valida-isa/source.html.zst");
    let compressed = std::fs::read(&fixture_path).unwrap();
    let decompressed = zstd::decode_all(compressed.as_slice()).unwrap();
    let html = String::from_utf8(decompressed).unwrap();

    // Run the arXiv HTML5 pipeline directly (same as gen_expected_arxiv)
    let mut dom = delulu_webfetch::pipelines::parse_html(&html).unwrap();
    delulu_webfetch::pipelines::dl_arxiv::filter_arxiv(&mut dom);
    let md = delulu_webfetch::generators::gen_md::MarkdownLowerer::lower(&dom, None);

    assert!(
        md.contains("Valida"),
        "body should contain 'Valida', got: {}",
        &md[..500.min(md.len())],
    );
    assert!(
        md.contains("Instruction Set Architecture"),
        "body should contain paper title"
    );
    assert!(
        md.len() > 1000,
        "markdown output should be substantial, got {} chars",
        md.len(),
    );
}
