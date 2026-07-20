pub mod core;
pub use crate::core::types;

pub mod generators;
pub mod pipeline;
pub mod sources;

pub use crate::core::detect::detect_source_type;
pub use crate::core::http_client::WebbfetchClient;
pub use crate::core::types::{ExtractionResult, MarkdownDocument, RedditComment};

use crate::core::types::{SourceType, WebbfetchError};
use crate::pipeline::DomNode;
use crate::pipeline::PassFn;
// ---------------------------------------------------------------------------
// fetch_and_extract
// ---------------------------------------------------------------------------

/// Fetch a URL and extract content based on the detected source type.
///
/// Uses a two-step fetch for Discourse URLs:
/// 1. First fetch: raw HTML (from the original URL)
/// 2. Content detection via `detect_from_content()` checks for Discourse markers
/// 3. If Discourse detected, second fetch to `.json` API endpoint for structured data
/// 4. If JSON fetch fails, falls back to GenericHtml extraction
///
/// Dispatching:
/// - Reddit: URL-based detection → immediate dispatch (no content detection needed)
/// - Discourse: URL returns GenericHtml → content detection → second fetch → parse JSON
/// - GenericHtml: URL returns GenericHtml → content detection → pipeline → lower
pub async fn fetch_and_extract(
    url: &str,
    client: &crate::core::http_client::WebbfetchClient,
    pipeline: &[PassFn],
) -> Result<ExtractionResult, WebbfetchError> {
    // Step 1: URL-based detection (primary dispatch — only returns Reddit or GenericHtml now)
    let url_source_type = detect_source_type(url);

    // Step 2: Fetch from HTTP layer (Reddit URLs already transformed at HTTP layer)
    // Note: The HTTP layer always stores the raw response body as GenericHtml,
    // regardless of the actual source type. The body is extracted here and
    // dispatched to the appropriate parser.
    let fetch_result = client.fetch(url).await?;

    // Step 3: If Reddit, dispatch immediately — no content detection needed
    if url_source_type == SourceType::Reddit {
        match &fetch_result.content {
            ExtractionResult::GenericHtml { content_md } => {
                let body = content_md.body.clone();
                let data = sources::reddit::RedditExtractor::extract(&body)?;
                return Ok(ExtractionResult::Reddit {
                    title: data.title,
                    selftext: data.selftext,
                    author: data.author,
                    score: data.score,
                    permalink: data.permalink,
                    comments: data.comments,
                });
            }
            ExtractionResult::Reddit { .. } => return Ok(fetch_result.content),
            other => {
                tracing::warn!(
                    "fetch_and_extract: unexpected content type {:?} for Reddit URL, falling back to GenericHtml",
                    other
                );
                // Fall through to non-Reddit processing below
            }
        }
    }

    // Step 4: For non-Reddit URLs, extract body and run content detection
    let body = match &fetch_result.content {
        ExtractionResult::GenericHtml { content_md } => content_md.body.clone(),
        other => {
            tracing::warn!(
                "fetch_and_extract: unexpected content type {:?} for non-Reddit URL, falling back to GenericHtml",
                other
            );
            return Err(WebbfetchError::Pass(format!(
                "fetch_and_extract: unexpected content type {:?} for non-Reddit URL",
                other
            )));
        }
    };

    // Step 5: Detect from content (checks for Discourse markers in HTML)
    let content_type = crate::core::detect::detect_from_content(&body);

    match content_type {
        Some(SourceType::Discourse) => {
            // Step 5a: Second fetch — get Discourse JSON API
            let api_url = crate::core::detect::discourse_url_to_api_url(url);

            let api_result = match client.fetch(&api_url).await {
                Ok(r) => r,
                Err(e) => {
                    tracing::warn!(
                        "Discourse JSON API fetch failed: {e}; falling back to GenericHtml"
                    );
                    return fallback_to_generic_html(url, body, pipeline);
                }
            };

            let api_body = match &api_result.content {
                ExtractionResult::GenericHtml { content_md } => content_md.body.clone(),
                other => {
                    tracing::warn!(
                        "Discourse API returned unexpected content type: {:?}; falling back to GenericHtml",
                        other
                    );
                    return fallback_to_generic_html(url, body, pipeline);
                }
            };

            // Step 5b: Parse Discourse JSON
            let data = sources::discourse::DiscourseExtractor::extract(&api_body)?;
            Ok(ExtractionResult::Discourse {
                title: data.title,
                topic_id: data.topic_id,
                posts: data.posts,
            })
        }
        // No Discourse detected — treat as GenericHtml
        _ => fallback_to_generic_html(url, body, pipeline),
    }
}

// ---------------------------------------------------------------------------
// extract
// ---------------------------------------------------------------------------

/// Extract content from a URL using the default Mozilla Readability pipeline.
pub async fn extract(
    url: &str,
    client: &crate::core::http_client::WebbfetchClient,
) -> Result<ExtractionResult, WebbfetchError> {
    fetch_and_extract(
        url,
        client,
        &[crate::pipeline::mozilla_readability::filter_mozilla_readability],
    )
    .await
}

// ---------------------------------------------------------------------------
// fallback_to_generic_html
// ---------------------------------------------------------------------------

/// Shared logic for GenericHtml extraction.
///
/// # Precondition
/// - `url` is the original source URL (used for frontmatter `source_url`).
/// - `body` is the raw HTML string.
/// - `pipeline` is a slice of `PassFn` passes.
///
/// # Postcondition
/// - Returns `Ok(ExtractionResult::GenericHtml { content_md })` with frontmatter.
/// - Pipeline passes may panic on logic bugs (intentional pre-alpha behavior).
fn fallback_to_generic_html(
    url: &str,
    body: String,
    pipeline: &[PassFn],
) -> Result<ExtractionResult, WebbfetchError> {
    let mut dom = pipeline::parse_html(&body)?;
    for pass in pipeline {
        pass(&mut dom);
    }
    let content_md = generators::gen_md::MarkdownLowerer::lower(&dom, None);
    let title = extract_title(&dom);

    Ok(ExtractionResult::GenericHtml {
        content_md: MarkdownDocument {
            frontmatter: format!(
                "title: {}\nsource_type: generic_html\nsource_url: {}",
                title, url
            ),
            body: content_md,
        },
    })
}

// ---------------------------------------------------------------------------
// extract_title
// ---------------------------------------------------------------------------

/// Extract the page title from a DOM tree.
///
/// Searches for the first `<h1>` element and returns its text content.
/// Falls back to the first `<title>` element if no `<h1>` is found.
/// Returns an empty string if neither is present.
pub fn extract_title(dom: &DomNode) -> String {
    // First pass: search for <h1>
    if let Some(title) = find_first_heading(dom, "h1") {
        return title;
    }
    // Fallback: search for <title>
    find_first_heading(dom, "title").unwrap_or_default()
}

/// Recursively find the first element with the given tag and return its text content.
fn find_first_heading(node: &DomNode, tag: &str) -> Option<String> {
    match node {
        DomNode::Element {
            tag: t, children, ..
        } if t == tag => {
            let text = collect_text_from_nodes(children);
            let trimmed = text.trim().to_string();
            if !trimmed.is_empty() {
                return Some(trimmed);
            }
        }
        DomNode::Element { children, .. } => {
            for child in children {
                if let found @ Some(_) = find_first_heading(child, tag) {
                    return found;
                }
            }
        }
        _ => {}
    }
    None
}

/// Collect all descendant text nodes into a single string.
fn collect_text_from_nodes(nodes: &[DomNode]) -> String {
    let mut buf = String::new();
    for node in nodes {
        match node {
            DomNode::Text(t) => buf.push_str(t),
            DomNode::Element { children, .. } => {
                buf.push_str(&collect_text_from_nodes(children));
            }
            _ => {}
        }
    }
    buf
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::http_client::WebbfetchClient;
    use crate::core::types::Response;
    use crate::pipeline::parse_html;
    use async_trait::async_trait;
    use std::collections::HashMap;
    use std::sync::Arc;
    use tokio::sync::Mutex;

    // ── Mock client for integration tests ─────────────────────────────────

    struct MockClient {
        responses: Arc<Mutex<HashMap<String, Response>>>,
    }

    impl MockClient {
        fn new() -> Self {
            Self {
                responses: Arc::new(Mutex::new(HashMap::new())),
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
                },
            );
            Self {
                responses: self.responses,
            }
        }

        fn with_response(url: &str, status: u16, body: &str) -> Self {
            Self::new().add_response(url, status, body)
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
        let html_body = "<html><head><title>Test Page</title></head><body><h1>Hello</h1><p>World</p></body></html>";
        let mock = MockClient::with_response("https://example.com/page", 200, html_body);
        let client = WebbfetchClient::with_client(mock);

        let result = fetch_and_extract(
            "https://example.com/page",
            &client,
            &[crate::pipeline::mozilla_readability::filter_mozilla_readability],
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
            &[crate::pipeline::mozilla_readability::filter_mozilla_readability],
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
        let api_url =
            "https://forum.example.com/t/discourse-topic/12345.json?raw_json=1&include_raw=1";

        // Two-step mock: first responds to original URL with HTML, second to .json with JSON
        let mock = MockClient::new()
            .add_response(original_url, 200, &html_body)
            .add_response(api_url, 200, &discourse_json);
        let client = WebbfetchClient::with_client(mock);

        let result = fetch_and_extract(
            original_url,
            &client,
            &[crate::pipeline::mozilla_readability::filter_mozilla_readability],
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
}
