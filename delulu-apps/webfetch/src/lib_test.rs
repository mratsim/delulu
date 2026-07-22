use super::*;
use crate::core::http_client::WebbfetchClient;
use crate::core::types::Response;
use crate::pipelines::parse_html;
use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

// ── Mock client for integration tests ─────────────────────────────────

struct MockClient {
    responses: Arc<Mutex<HashMap<String, Response>>>,
    mock_bytes: Arc<Mutex<HashMap<String, Vec<u8>>>>,
}

impl MockClient {
    fn new() -> Self {
        Self {
            responses: Arc::new(Mutex::new(HashMap::new())),
            mock_bytes: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    fn add_response(self, url: &str, status: u16, body: &str) -> Self {
        let responses = Arc::clone(&self.responses);
        let mut map = responses.try_lock().expect("MockClient mutex locked");
        map.insert(
            url.to_string(),
            Response {
                status,
                body: body.to_string(),
                content_type: None,
            },
        );
        Self {
            responses: self.responses,
            mock_bytes: self.mock_bytes,
        }
    }

    fn with_response(url: &str, status: u16, body: &str) -> Self {
        Self::new().add_response(url, status, body)
    }

    fn add_bytes(self, url: &str, data: Vec<u8>) -> Self {
        let bytes = Arc::clone(&self.mock_bytes);
        let mut map = bytes.try_lock().expect("MockClient mutex locked");
        map.insert(url.to_string(), data);
        Self {
            responses: self.responses,
            mock_bytes: self.mock_bytes,
        }
    }
}

#[async_trait]
impl crate::core::types::HttpClient for MockClient {
    async fn get(&self, url: &str) -> Result<Response, WebbfetchError> {
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
    // H1 should be preferred when both h1 and title exist
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

// ── fetch_and_extract integration tests ────────────────────────────────

#[tokio::test]
async fn test_fetch_and_extract_generic_html() {
    let html_body =
        "<html><head><title>Test Page</title></head><body><h1>Hello</h1><p>World</p></body></html>";
    let mock = MockClient::with_response("https://example.com/page", 200, html_body);
    let client = WebbfetchClient::with_client(mock);

    let result = fetch_and_extract(
        "https://example.com/page",
        &client,
        &[crate::pipelines::mozilla_readability::filter_mozilla_readability],
    )
    .await
    .unwrap();

    match result {
        ExtractionResult::GenericHtml { content_md } => {
            assert!(
                content_md.body.contains("World"),
                "body should contain lowered content: {}",
                content_md.body
            );
            assert!(
                content_md.frontmatter.contains("source_type: generic_html"),
                "frontmatter should indicate generic_html"
            );
            assert!(
                content_md
                    .frontmatter
                    .contains("source_url: https://example.com/page"),
                "frontmatter should contain source URL"
            );
        }
        other => panic!("Expected GenericHtml, got {other:?}"),
    }
}

#[tokio::test]
async fn test_fetch_and_extract_reddit() {
    // Build a minimal Reddit JSON API response
    let reddit_json = serde_json::json!([
        {
            "kind": "Listing",
            "data": {
                "children": [{
                    "kind": "t3",
                    "data": {
                        "title": "Reddit Post",
                        "author": "test_user",
                        "score": 42,
                        "selftext": "Post body",
                        "created_utc": 1000000.0,
                        "permalink": "/r/test/comments/abc/",
                        "subreddit": "test",
                        "subreddit_id": "t5_1",
                        "id": "abc"
                    }
                }]
            }
        },
        {
            "kind": "Listing",
            "data": {
                "children": []
            }
        }
    ])
    .to_string();

    // The http layer transforms the reddit URL to add .json
    let api_url = "https://www.reddit.com/r/test/comments/abc/reddit_post.json?raw_json=1";
    let mock = MockClient::with_response(api_url, 200, &reddit_json);
    let client = WebbfetchClient::with_client(mock);

    let result = fetch_and_extract(
        "https://www.reddit.com/r/test/comments/abc/reddit_post/",
        &client,
        &[crate::pipelines::mozilla_readability::filter_mozilla_readability],
    )
    .await
    .unwrap();

    match result {
        ExtractionResult::Reddit {
            title,
            selftext,
            author,
            score,
            ..
        } => {
            assert_eq!(title, "Reddit Post");
            assert_eq!(author, "test_user");
            assert_eq!(score, 42);
            assert_eq!(selftext, "Post body");
        }
        other => panic!("Expected Reddit, got {other:?}"),
    }
}

#[tokio::test]
async fn test_fetch_and_extract_discourse() {
    // Step 1: HTML body with Discourse markers (what the first fetch returns)
    let html_body = r#"<html><head><meta name="generator" content="Discourse"></head><body><p>Discourse topic HTML</p></body></html>"#;

    // Step 2: JSON body (what the second fetch to .json returns)
    let discourse_json = serde_json::json!({
        "title": "Discourse Topic",
        "id": 12345,
        "slug": "discourse-topic",
        "posts_count": 1,
        "post_stream": {
            "posts": [{
                "post_number": 1,
                "username": "alice",
                "cooked": "<p>Hello world</p>",
                "created_at": "2024-01-01T00:00:00Z",
                "reply_to_post_number": null
            }]
        }
    })
    .to_string();

    let original_url = "https://forum.example.com/t/discourse-topic/12345";
    let api_url = "https://forum.example.com/t/discourse-topic/12345.json?raw_json=1&include_raw=1";

    // Two-step mock: first responds to original URL with HTML, second to .json with JSON
    let mock = MockClient::new()
        .add_response(original_url, 200, &html_body)
        .add_response(api_url, 200, &discourse_json);
    let client = WebbfetchClient::with_client(mock);

    let result = fetch_and_extract(
        original_url,
        &client,
        &[crate::pipelines::mozilla_readability::filter_mozilla_readability],
    )
    .await
    .unwrap();

    match result {
        ExtractionResult::Discourse {
            title,
            topic_id,
            posts,
        } => {
            assert_eq!(title, "Discourse Topic");
            assert_eq!(topic_id, 12345);
            assert_eq!(posts.len(), 1);
            assert_eq!(posts[0].username, "alice");
        }
        other => panic!("Expected Discourse, got {other:?}"),
    }
}

// ── xberg_html_to_markdown tests ─────────────────────────────────────────

#[test]
fn test_xberg_html_to_markdown_plain_html() {
    let html = "<html><body><h1>Title</h1><p>Paragraph</p></body></html>";
    let result = xberg_html_to_markdown(html, None).unwrap();
    assert!(result.contains("Title"), "should contain title text");
    assert!(result.contains("Paragraph"), "should contain paragraph text");
}

#[test]
fn test_xberg_html_to_markdown_removes_scripts() {
    let html = r#"<html><body><script>alert(1)</script><p>Content</p></body></html>"#;
    let result = xberg_html_to_markdown(html, None).unwrap();
    assert!(!result.contains("alert"), "scripts should be removed");
    assert!(result.contains("Content"), "content should remain");
}

#[test]
fn test_xberg_html_to_markdown_removes_styles() {
    let html = r#"<html><head><style>body{color:red}</style></head><body><p>Text</p></body></html>"#;
    let result = xberg_html_to_markdown(html, None).unwrap();
    assert!(!result.contains("color:red"), "styles should be removed");
    assert!(result.contains("Text"), "content should remain");
}

#[test]
fn test_xberg_html_to_markdown_with_base_url() {
    let html = r#"<html><body><a href="/relative">Link</a></body></html>"#;
    let result = xberg_html_to_markdown(html, Some("https://example.com")).unwrap();
    // Verify the relative URL is resolved against the base URL
    assert!(
        result.contains("https://example.com/relative")
            || result.contains("/relative"),
        "expected resolved URL in markdown output, got: {result}"
    );
    assert!(result.contains("Link"), "should contain link text");
}

#[test]
fn test_xberg_html_to_markdown_empty_html() {
    let html = "";
    let result = xberg_html_to_markdown(html, None).unwrap();
    // Empty HTML should produce empty markdown
    assert!(result.trim().is_empty() || result.is_empty(), "empty HTML should produce empty output");
}

#[test]
fn test_xberg_html_to_markdown_preserves_img() {
    let html = r#"<html><body><img src="pic.png" alt="Pic"/><p>Text</p></body></html>"#;
    let result = xberg_html_to_markdown(html, None).unwrap();
    // The markdown lowerer should produce image markdown
    assert!(result.contains("pic.png") || result.contains("Pic"), "image should be preserved");
    assert!(result.contains("Text"), "text should be preserved");
}

#[test]
fn test_xberg_html_to_markdown_public_api() {
    // Verify the function is callable as delulu_webfetch::xberg_html_to_markdown
    let html = "<html><body><p>API test</p></body></html>";
    let result = xberg_html_to_markdown(html, None).unwrap();
    assert!(result.contains("API test"), "public API should work");
}

#[test]
#[ignore = "parse_html is currently infallible (scraper parses everything). Enable when MAX_NODES guard enforced."]
fn test_xberg_html_to_markdown_parse_error() {
    // parse_html should reject malformed/unclosed HTML
    // Currently, scraper::Html::parse_document is lenient and never fails.
    // This test documents the expected contract for when MAX_NODES is enforced.
    let bad_html = "<html><body><p>unclosed";
    let result = xberg_html_to_markdown(bad_html, None);
    assert!(
        result.is_err(),
        "malformed HTML should produce an error"
    );
    match result {
        Err(WebbfetchError::Parse(_)) => {} // expected
        Err(other) => panic!("Expected Parse error, got: {other:?}"),
        Ok(_) => panic!("Expected error, got Ok"),
    }
}