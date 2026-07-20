use serde::{Deserialize, Serialize};

use crate::core::types::{DiscoursePost, MarkdownDocument, WebbfetchError};

// ---------------------------------------------------------------------------
// DiscourseData
// ---------------------------------------------------------------------------

/// Represents a parsed Discourse topic with its metadata and posts.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscourseData {
    pub title: String,
    pub topic_id: u64,
    pub slug: String,
    pub posts: Vec<DiscoursePost>,
    pub post_count: u64,
}

// ---------------------------------------------------------------------------
// Internal API response types
// ---------------------------------------------------------------------------

/// Top-level JSON structure returned by Discourse `/t/slug/id.json`.
#[derive(Debug, Deserialize)]
struct DiscourseApiResponse {
    title: String,
    id: u64,
    slug: String,
    posts_count: u64,
    post_stream: PostStream,
}

/// Wrapper for the `post_stream` object.
#[derive(Debug, Deserialize)]
struct PostStream {
    posts: Vec<DiscoursePost>,
}

// ---------------------------------------------------------------------------
// DiscourseExtractor
// ---------------------------------------------------------------------------

/// Extracts structured data from a Discourse topic JSON response.
pub struct DiscourseExtractor;

impl DiscourseExtractor {
    /// Parse a Discourse `/t/slug/id.json` response body into [`DiscourseData`].
    ///
    /// # Errors
    ///
    /// Returns [`WebbfetchError::Parse`] when the JSON is malformed or missing
    /// required fields.
    pub fn extract(json_str: &str) -> Result<DiscourseData, WebbfetchError> {
        let api_response: DiscourseApiResponse = serde_json::from_str(json_str)
            .map_err(|e| WebbfetchError::Parse(format!("Failed to parse Discourse JSON: {e}")))?;

        let mut posts = api_response.post_stream.posts;
        let post_count = api_response.posts_count;

        if (posts.len() as u64) < post_count {
            tracing::warn!(
                "Discourse topic '{}' (id={}) has {} posts on server but only {} in this response (possibly paginated)",
                api_response.title,
                api_response.id,
                post_count,
                posts.len(),
            );
        }

        if posts.len() > 200 {
            tracing::warn!("Discourse: truncating {} posts to 200", posts.len());
            posts.truncate(200);
        }

        Ok(DiscourseData {
            title: api_response.title,
            topic_id: api_response.id,
            slug: api_response.slug,
            posts,
            post_count,
        })
    }
}

// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// From<DiscourseData> for MarkdownDocument
// ---------------------------------------------------------------------------

impl From<DiscourseData> for MarkdownDocument {
    fn from(data: DiscourseData) -> Self {
        // Build YAML frontmatter
        let frontmatter = format!(
            "title: \"{}\"\nsource_type: \"discourse\"\ntopic_id: {}",
            data.title, data.topic_id
        );

        // Build body: sequential numbered posts
        let mut body = String::new();

        for post in &data.posts {
            // Post header
            body.push_str(&format!("## Post #{}\n", post.post_number));

            // Metadata line
            body.push_str(&format!(
                "**{}** — {} — Post #{}\n\n",
                post.username, post.created_at, post.post_number,
            ));

            // Use raw Markdown from the API response
            let content_md = post.raw.trim().to_string();
            let trimmed = content_md.trim();
            if !trimmed.is_empty() {
                body.push_str(trimmed);
                body.push('\n');
            }

            body.push('\n');
        }

        MarkdownDocument { frontmatter, body }
    }
}

// ---------------------------------------------------------------------------
// From<DiscourseData> for serde_json::Value
// ---------------------------------------------------------------------------

impl TryFrom<DiscourseData> for serde_json::Value {
    type Error = serde_json::Error;
    fn try_from(data: DiscourseData) -> Result<Self, Self::Error> {
        serde_json::to_value(&data)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // ── Test data ─────────────────────────────────────────────────────────

    fn basic_topic_json() -> &'static str {
        r#"{
            "title": "Test Topic",
            "id": 42,
            "slug": "test-topic",
            "posts_count": 2,
            "post_stream": {
                "posts": [
                    {
                        "post_number": 1,
                        "username": "alice",
                        "raw": "First post content",
                        "created_at": "2024-01-01T00:00:00Z",
                        "reply_to_post_number": null
                    },
                    {
                        "post_number": 2,
                        "username": "bob",
                        "raw": "Second post with **bold** text",
                        "created_at": "2024-01-02T00:00:00Z",
                        "reply_to_post_number": 1
                    }
                ]
            }
        }"#
    }

    fn reply_topic_json() -> &'static str {
        r#"{
            "title": "Reply Topic",
            "id": 99,
            "slug": "reply-topic",
            "posts_count": 3,
            "post_stream": {
                "posts": [
                    {
                        "post_number": 1,
                        "username": "charlie",
                        "raw": "<p>Original post</p>",
                        "created_at": "2024-03-01T00:00:00Z",
                        "reply_to_post_number": null
                    },
                    {
                        "post_number": 2,
                        "username": "dave",
                        "raw": "<p>First reply</p>",
                        "created_at": "2024-03-02T00:00:00Z",
                        "reply_to_post_number": 1
                    },
                    {
                        "post_number": 3,
                        "username": "charlie",
                        "raw": "<p>Follow-up reply</p>",
                        "created_at": "2024-03-03T00:00:00Z",
                        "reply_to_post_number": 2
                    }
                ]
            }
        }"#
    }

    fn partial_topic_json() -> &'static str {
        r#"{
            "title": "Partial Topic",
            "id": 100,
            "slug": "partial-topic",
            "posts_count": 10,
            "post_stream": {
                "posts": [
                    {
                        "post_number": 1,
                        "username": "eve",
                        "raw": "<p>Only page 1</p>",
                        "created_at": "2024-06-01T00:00:00Z",
                        "reply_to_post_number": null
                    },
                    {
                        "post_number": 2,
                        "username": "frank",
                        "raw": "<p>Second post</p>",
                        "created_at": "2024-06-02T00:00:00Z",
                        "reply_to_post_number": 1
                    }
                ]
            }
        }"#
    }

    // ── test_extract_basic_topic ─────────────────────────────────────────

    #[test]
    fn test_extract_basic_topic() {
        let data =
            DiscourseExtractor::extract(basic_topic_json()).expect("should parse basic topic JSON");

        assert_eq!(data.title, "Test Topic");
        assert_eq!(data.topic_id, 42);
        assert_eq!(data.slug, "test-topic");
        assert_eq!(data.post_count, 2);
        assert_eq!(data.posts.len(), 2);

        // First post
        assert_eq!(data.posts[0].post_number, 1);
        assert_eq!(data.posts[0].username, "alice");
        assert_eq!(data.posts[0].created_at, "2024-01-01T00:00:00Z");
        assert!(data.posts[0].raw.contains("First post content"));

        // Second post
        assert_eq!(data.posts[1].post_number, 2);
        assert_eq!(data.posts[1].username, "bob");
        assert_eq!(data.posts[1].created_at, "2024-01-02T00:00:00Z");
    }

    // ── test_extract_reply_to ────────────────────────────────────────────

    #[test]
    fn test_extract_reply_to() {
        let data =
            DiscourseExtractor::extract(reply_topic_json()).expect("should parse reply topic JSON");

        // Post 1: no reply_to
        assert_eq!(data.posts[0].post_number, 1);
        assert!(data.posts[0].reply_to_post_number.is_none());

        // Post 2: reply to post 1
        assert_eq!(data.posts[1].post_number, 2);
        assert_eq!(data.posts[1].reply_to_post_number, Some(1));

        // Post 3: reply to post 2
        assert_eq!(data.posts[2].post_number, 3);
        assert_eq!(data.posts[2].reply_to_post_number, Some(2));
    }

    // ── test_extract_partial_data ───────────────────────────────────────

    #[test]
    fn test_extract_partial_data() {
        // Set up a tracing subscriber so the `tracing::warn!` call
        // does not output to stderr in an unconfigured state.
        let subscriber = tracing_subscriber::fmt().with_test_writer().finish();
        let _guard = tracing::subscriber::set_default(subscriber);

        let data = DiscourseExtractor::extract(partial_topic_json())
            .expect("should parse partial data without error");

        assert_eq!(data.post_count, 10);
        assert_eq!(data.posts.len(), 2);
        assert_eq!(data.title, "Partial Topic");
        // The warning is emitted — with --nocapture it will be visible in test output.
        // We verify that the function handles partial data gracefully.
    }

    // ── test_extract_invalid_json ───────────────────────────────────────

    #[test]
    fn test_extract_invalid_json() {
        let result = DiscourseExtractor::extract("this is not valid JSON");
        match result {
            Err(WebbfetchError::Parse(msg)) => {
                assert!(!msg.is_empty(), "parse error message should not be empty");
            }
            other => panic!("expected WebbfetchError::Parse, got {:?}", other),
        }
    }

    #[test]
    fn test_extract_invalid_json_empty() {
        let result = DiscourseExtractor::extract("");
        match result {
            Err(WebbfetchError::Parse(_)) => {} // expected
            other => panic!("expected WebbfetchError::Parse, got {:?}", other),
        }
    }

    #[test]
    fn test_extract_invalid_json_missing_fields() {
        let result = DiscourseExtractor::extract(r#"{"title":"no posts"}"#);
        match result {
            Err(WebbfetchError::Parse(_)) => {} // expected
            other => panic!("expected WebbfetchError::Parse, got {:?}", other),
        }
    }

    // ── test_to_markdown ────────────────────────────────────────────────

    #[test]
    fn test_to_markdown() {
        let data =
            DiscourseExtractor::extract(basic_topic_json()).expect("should parse basic topic JSON");

        let doc: MarkdownDocument = data.into();

        // Frontmatter checks
        assert!(
            doc.frontmatter.starts_with("title: \""),
            "frontmatter should start with title"
        );
        assert!(
            doc.frontmatter.contains("source_type: \"discourse\""),
            "frontmatter should contain source_type"
        );
        assert!(
            doc.frontmatter.contains("topic_id: 42"),
            "frontmatter should contain topic_id"
        );

        // Body checks — Post #1 section
        assert!(
            doc.body.contains("## Post #1"),
            "body should contain Post #1 header"
        );
        assert!(
            doc.body.contains("**alice**"),
            "body should contain alice username"
        );
        assert!(
            doc.body.contains("2024-01-01T00:00:00Z"),
            "body should contain created_at timestamp"
        );

        // Body checks — Post #2 section
        assert!(
            doc.body.contains("## Post #2"),
            "body should contain Post #2 header"
        );
        assert!(
            doc.body.contains("**bob**"),
            "body should contain bob username"
        );

        // Content from raw Markdown should appear in the output
        assert!(
            doc.body.contains("First post content"),
            "body should contain lowered content from post 1"
        );
        assert!(
            doc.body.contains("**bold**"),
            "body should contain **bold** from post 2 raw Markdown"
        );
    }

    // ── test_to_json ────────────────────────────────────────────────────

    #[test]
    fn test_to_json() {
        let data =
            DiscourseExtractor::extract(basic_topic_json()).expect("should parse basic topic JSON");

        let json_value: serde_json::Value = data.try_into().expect("serialization should succeed");

        // Verify it's an object with the expected fields
        let obj = json_value.as_object().expect("should be a JSON object");
        assert!(obj.contains_key("title"), "should contain 'title' key");
        assert!(
            obj.contains_key("topic_id"),
            "should contain 'topic_id' key"
        );
        assert!(obj.contains_key("slug"), "should contain 'slug' key");
        assert!(obj.contains_key("posts"), "should contain 'posts' key");
        assert!(
            obj.contains_key("post_count"),
            "should contain 'post_count' key"
        );

        assert_eq!(obj["title"].as_str(), Some("Test Topic"));
        assert_eq!(obj["topic_id"].as_u64(), Some(42));
        assert_eq!(obj["slug"].as_str(), Some("test-topic"));
        assert_eq!(obj["post_count"].as_u64(), Some(2));

        // Posts array
        let posts = obj["posts"].as_array().expect("posts should be an array");
        assert_eq!(posts.len(), 2);
        assert_eq!(posts[0]["username"].as_str(), Some("alice"));
        assert_eq!(posts[1]["username"].as_str(), Some("bob"));
    }
}
