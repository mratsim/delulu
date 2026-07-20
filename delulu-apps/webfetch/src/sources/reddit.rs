use crate::core::types::{MarkdownDocument, RedditComment, WebbfetchError};
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Intermediate serde types for Reddit JSON API
// ---------------------------------------------------------------------------

/// Top-level Reddit API response: array of two Listings (post + comments).
#[derive(Deserialize)]
struct RedditApiResponse(Vec<RedditListing>);

#[derive(Deserialize)]
struct RedditListing {
    // Reserved — consumed by serde during deserialization to match the Reddit API response shape.
    // Kept for future use (e.g. distinguishing Listing vs. t1/t3 kinds without looping children).
    #[allow(dead_code)]
    #[serde(default)]
    kind: String,
    #[serde(default)]
    data: RedditListingData,
}

#[derive(Deserialize, Default)]
struct RedditListingData {
    #[serde(default)]
    children: Vec<RedditChild>,
}

#[derive(Deserialize)]
struct RedditChild {
    #[serde(default)]
    kind: String,
    #[serde(default)]
    data: serde_json::Value,
}

// ---------------------------------------------------------------------------
// RedditData
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RedditData {
    pub title: String,
    pub author: String,
    pub score: i64,
    pub selftext: String,
    pub created_utc: f64,
    pub permalink: String,
    pub comments: Vec<RedditComment>,
    pub comments_truncated: bool,
}

// ---------------------------------------------------------------------------
// RedditExtractor
// ---------------------------------------------------------------------------

pub struct RedditExtractor;

impl RedditExtractor {
    const MAX_DEPTH: u32 = 10;
    const MAX_COMMENTS: usize = 500;

    /// Extract structured Reddit data from Reddit's JSON API response.
    ///
    /// The Reddit API returns a top-level array:
    ///   `[post_listing, comments_listing]`
    ///
    /// - `post_listing["data"]["children"][0]["data"]` contains post metadata.
    /// - `comments_listing["data"]["children"]` is the comments array.
    pub fn extract(json_str: &str) -> Result<RedditData, WebbfetchError> {
        // Pre-check for known error shapes before full parse.
        if let Ok(val) = serde_json::from_str::<serde_json::Value>(json_str)
            && let Some(err) = val.get("error").and_then(|e| e.as_i64())
            && err == 403
        {
            return Err(WebbfetchError::AuthRequired(
                "Reddit API returned HTTP 403 — content may be private, removed, or quarantined"
                    .into(),
            ));
        }

        let response: RedditApiResponse = serde_json::from_str(json_str).map_err(|e| {
            WebbfetchError::Parse(format!("Failed to parse Reddit JSON response: {e}"))
        })?;

        // --- Extract post info ---
        let post_listing = response.0.first().ok_or_else(|| {
            WebbfetchError::Parse("Reddit response: missing post listing (index 0)".into())
        })?;
        let post_child = post_listing.data.children.first().ok_or_else(|| {
            WebbfetchError::Parse("Reddit response: missing post child in listing".into())
        })?;

        let post_data = &post_child.data;
        let title = post_data
            .get("title")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string();
        let author = post_data
            .get("author")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string();
        let score = post_data.get("score").and_then(|v| v.as_i64()).unwrap_or(0);
        let selftext = post_data
            .get("selftext")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string();
        let created_utc = post_data
            .get("created_utc")
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0);
        let permalink = post_data
            .get("permalink")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string();

        // --- Extract comments ---
        let mut current_count = 0usize;
        let mut truncated = false;

        let comments = if let Some(comments_listing) = response.0.get(1) {
            Self::parse_children(
                &comments_listing.data.children,
                Self::MAX_DEPTH,
                Self::MAX_COMMENTS,
                &mut current_count,
                &mut truncated,
            )
        } else {
            Vec::new()
        };

        Ok(RedditData {
            title,
            author,
            score,
            selftext,
            created_utc,
            permalink,
            comments,
            comments_truncated: truncated,
        })
    }

    /// Recursively parse a slice of `RedditChild` items into `RedditComment`s.
    fn parse_children(
        children: &[RedditChild],
        max_depth: u32,
        max_total: usize,
        current_count: &mut usize,
        truncated: &mut bool,
    ) -> Vec<RedditComment> {
        let mut comments = Vec::new();

        for child in children {
            if *current_count >= max_total {
                *truncated = true;
                break;
            }

            match child.kind.as_str() {
                "more" => {
                    let count = child
                        .data
                        .get("count")
                        .and_then(|v| v.as_i64())
                        .unwrap_or(0);
                    if count > 0 {
                        tracing::warn!("Reddit: skipping {} more comments", count);
                    }
                    *truncated = true;
                }
                "t1" => {
                    let depth = child
                        .data
                        .get("depth")
                        .and_then(|v| v.as_u64())
                        .unwrap_or(0) as u32;
                    if depth >= max_depth {
                        // Comment is deeper than max depth — skip entirely.
                        *truncated = true;
                        continue;
                    }

                    let comment = RedditComment {
                        author: child
                            .data
                            .get("author")
                            .and_then(|v| v.as_str())
                            .unwrap_or_default()
                            .to_string(),
                        body: child
                            .data
                            .get("body")
                            .and_then(|v| v.as_str())
                            .unwrap_or_default()
                            .to_string(),
                        score: child
                            .data
                            .get("score")
                            .and_then(|v| v.as_i64())
                            .unwrap_or(0),
                        depth,
                        replies: Self::parse_replies(
                            &child.data,
                            max_depth,
                            max_total,
                            current_count,
                            truncated,
                        ),
                    };
                    *current_count += 1;
                    comments.push(comment);
                }
                _ => {
                    // Unknown kind (e.g. "t3" for posts in cross-post listings) — skip.
                }
            }
        }

        comments
    }

    /// Parse the `replies` field of a comment, which can be a Listing object
    /// or an empty string.
    fn parse_replies(
        data: &serde_json::Value,
        max_depth: u32,
        max_total: usize,
        current_count: &mut usize,
        truncated: &mut bool,
    ) -> Vec<RedditComment> {
        if let Some(replies) = data.get("replies")
            && replies.is_object()
            && let Ok(listing) = serde_json::from_value::<RedditListing>(replies.clone())
        {
            return Self::parse_children(
                &listing.data.children,
                max_depth,
                max_total,
                current_count,
                truncated,
            );
        }
        Vec::new()
    }
}

// ---------------------------------------------------------------------------
// From<RedditData> for MarkdownDocument
// ---------------------------------------------------------------------------

impl From<RedditData> for MarkdownDocument {
    fn from(data: RedditData) -> Self {
        let frontmatter = format!(
            r#"---
title: "{}"
source_type: "reddit"
author: "{}"
score: {}
comments_truncated: {}
---"#,
            data.title.replace('"', r#"\""#),
            data.author.replace('"', r#"\""#),
            data.score,
            data.comments_truncated,
        );

        let mut body = String::new();

        // Selftext
        if !data.selftext.is_empty() {
            body.push_str(&data.selftext);
            body.push('\n');
            body.push('\n');
        }

        // Threaded comments
        for comment in &data.comments {
            format_comment(comment, 0, &mut body);
        }

        MarkdownDocument { frontmatter, body }
    }
}

/// Append a threaded comment (and its replies) to the body string, using `>`
/// indentation for nesting.
fn format_comment(comment: &RedditComment, depth: u32, out: &mut String) {
    // Build the prefix: depth 0 => ">", depth 1 => "> >", etc.
    let prefix: String = (0..=depth).map(|_| "> ").collect::<String>();

    // Trim trailing space from prefix so output looks clean.
    let prefix = prefix.trim_end().to_string();

    if !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }
    out.push_str(&prefix);
    out.push(' ');
    out.push_str("**");
    out.push_str(&comment.author);
    out.push_str("** (");
    // Push score as decimal string
    out.push_str(&comment.score.to_string());
    out.push_str("): ");
    out.push_str(&comment.body);
    out.push('\n');

    // Recurse into replies
    for reply in &comment.replies {
        format_comment(reply, depth + 1, out);
    }
}

// ---------------------------------------------------------------------------
// From<RedditData> for serde_json::Value
// ---------------------------------------------------------------------------

impl From<RedditData> for serde_json::Value {
    fn from(data: RedditData) -> Self {
        serde_json::json!({
            "title": data.title,
            "author": data.author,
            "score": data.score,
            "selftext": data.selftext,
            "created_utc": data.created_utc,
            "permalink": data.permalink,
            "comments": data.comments,
            "comments_truncated": data.comments_truncated,
        })
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // ── Helper: build a Reddit API JSON string ────────────────────────────

    fn make_thread_json(
        title: &str,
        author: &str,
        score: i64,
        selftext: &str,
        permalink: &str,
        created_utc: f64,
        comments: Vec<serde_json::Value>,
    ) -> String {
        let post_listing = json!({
            "kind": "Listing",
            "data": {
                "children": [{
                    "kind": "t3",
                    "data": {
                        "title": title,
                        "author": author,
                        "score": score,
                        "selftext": selftext,
                        "created_utc": created_utc,
                        "permalink": permalink,
                        "subreddit": "test",
                        "subreddit_id": "t5_123",
                        "id": "abc123"
                    }
                }]
            }
        });

        let comments_listing = json!({
            "kind": "Listing",
            "data": {
                "children": comments
            }
        });

        serde_json::to_string(&vec![post_listing, comments_listing]).unwrap()
    }

    fn make_comment_value(
        body: &str,
        author: &str,
        score: i64,
        depth: u32,
        replies: serde_json::Value,
    ) -> serde_json::Value {
        json!({
            "kind": "t1",
            "data": {
                "body": body,
                "author": author,
                "score": score,
                "depth": depth,
                "replies": replies,
                "subreddit": "test",
                "id": "comment_id"
            }
        })
    }

    fn make_more_value(count: i64) -> serde_json::Value {
        json!({
            "kind": "more",
            "data": {
                "count": count,
                "name": "t1_more_id",
                "id": "more_id"
            }
        })
    }

    fn make_empty_replies() -> serde_json::Value {
        serde_json::Value::String(String::new())
    }

    fn make_listing_replies(children: Vec<serde_json::Value>) -> serde_json::Value {
        json!({
            "kind": "Listing",
            "data": {
                "children": children
            }
        })
    }

    // ── test_extract_basic_thread ──────────────────────────────────────
    #[test]
    fn test_extract_basic_thread() {
        let comment1 = make_comment_value("First comment", "user1", 10, 0, make_empty_replies());
        let comment2 = make_comment_value("Second comment", "user2", 5, 0, make_empty_replies());

        let json = make_thread_json(
            "Hello World",
            "op_user",
            42,
            "This is the post body",
            "/r/test/comments/abc123/hello_world/",
            1234567890.0,
            vec![comment1, comment2],
        );

        let result = RedditExtractor::extract(&json).unwrap();
        assert_eq!(result.title, "Hello World");
        assert_eq!(result.author, "op_user");
        assert_eq!(result.score, 42);
        assert_eq!(result.selftext, "This is the post body");
        assert_eq!(result.permalink, "/r/test/comments/abc123/hello_world/");
        assert!((result.created_utc - 1234567890.0).abs() < f64::EPSILON);
        assert!(!result.comments_truncated);
        assert_eq!(result.comments[0].body, "First comment");
        assert_eq!(result.comments[0].author, "user1");
        assert_eq!(result.comments[0].score, 10);
        assert_eq!(result.comments[0].depth, 0);
        assert!(result.comments[0].replies.is_empty());
        assert_eq!(result.comments[1].body, "Second comment");
        assert_eq!(result.comments[1].author, "user2");
        assert_eq!(result.comments[1].score, 5);
    }

    // ── test_extract_more_comments ─────────────────────────────────────
    #[test]
    fn test_extract_more_comments() {
        let comment = make_comment_value("Visible comment", "user1", 10, 0, make_empty_replies());
        let more = make_more_value(23);

        let json = make_thread_json(
            "More test",
            "op",
            1,
            "",
            "/r/test/comments/abc/",
            100.0,
            vec![comment, more],
        );

        let result = RedditExtractor::extract(&json).unwrap();
        assert!(result.comments_truncated);
        assert_eq!(result.comments.len(), 1);
    }

    // ── test_extract_error_403 ─────────────────────────────────────────
    #[test]
    fn test_extract_error_403() {
        let json = r#"{"error": 403, "reason": "private", "message": "Forbidden"}"#;
        let err = RedditExtractor::extract(json).unwrap_err();
        match err {
            WebbfetchError::AuthRequired(msg) => {
                assert!(msg.contains("403") || msg.contains("private"));
            }
            other => panic!("Expected AuthRequired, got {other:?}"),
        }
    }

    // ── test_extract_max_depth ─────────────────────────────────────────
    #[test]
    fn test_extract_max_depth() {
        // Create a chain: comment at depth 0 has a reply at depth 1 ...
        // We'll go up to depth 11 and verify depth 10+ is truncated.
        let mut replies = make_empty_replies();
        // depth 0..9 = 10 comments expected
        for depth in (0..=11).rev() {
            let c = make_comment_value(
                &format!("depth {depth}"),
                &format!("user{depth}"),
                1,
                depth,
                replies,
            );
            replies = make_listing_replies(vec![c]);
        }

        // Now `replies` is a listing wrapping the full chain.
        // We need a top-level comment at depth 0 that has this chain as its replies.
        let top_comment = make_comment_value("top", "top_user", 5, 0, replies);

        let json = make_thread_json(
            "Depth test",
            "op",
            1,
            "",
            "/r/test/",
            100.0,
            vec![top_comment],
        );

        let result = RedditExtractor::extract(&json).unwrap();
        assert!(result.comments_truncated);
        // The depth 10 and depth 11 comments should be truncated.
        // "top" (depth 0) + depth 0..9 = 11 comments should be present.
        fn count_comments(comments: &[RedditComment]) -> usize {
            let mut total = 0;
            for c in comments {
                total += 1 + count_comments(&c.replies);
            }
            total
        }
        let total = count_comments(&result.comments);
        assert_eq!(
            total, 11,
            "should have exactly 11 comments (top + depths 0-9)"
        );
    }

    // ── test_extract_comment_limit ─────────────────────────────────────
    #[test]
    fn test_extract_comment_limit() {
        // Create 510 top-level comments (all at depth 0).
        let mut comments = Vec::new();
        for i in 0..510 {
            comments.push(make_comment_value(
                &format!("comment {i}"),
                &format!("user{i}"),
                i,
                0,
                make_empty_replies(),
            ));
        }

        let json = make_thread_json("Limit test", "op", 1, "", "/r/test/", 100.0, comments);

        let result = RedditExtractor::extract(&json).unwrap();
        assert!(result.comments_truncated);
        assert_eq!(result.comments.len(), 500, "should cap at MAX_COMMENTS");
    }

    // ── test_to_markdown ──────────────────────────────────────────────
    #[test]
    fn test_to_markdown() {
        let reply = RedditComment {
            author: "reply_user".into(),
            body: "A nested reply".into(),
            score: 3,
            depth: 1,
            replies: vec![],
        };

        let comment = RedditComment {
            author: "top_user".into(),
            body: "Top level comment".into(),
            score: 10,
            depth: 0,
            replies: vec![reply],
        };

        let data = RedditData {
            title: "Test Post".into(),
            author: "op_user".into(),
            score: 42,
            selftext: "Post body text".into(),
            created_utc: 1000000.0,
            permalink: "/r/test/123/".into(),
            comments: vec![comment],
            comments_truncated: false,
        };

        let md: MarkdownDocument = data.into();

        assert!(md.frontmatter.contains("title: \"Test Post\""));
        assert!(md.frontmatter.contains("source_type: \"reddit\""));
        assert!(md.frontmatter.contains("author: \"op_user\""));
        assert!(md.frontmatter.contains("score: 42"));
        assert!(md.frontmatter.contains("comments_truncated: false"));

        assert!(md.body.contains("Post body text"));
        assert!(md.body.contains("**top_user** (10): Top level comment"));
        // Nested reply should have additional `> ` indentation
        assert!(md.body.contains("> **reply_user** (3): A nested reply"));
    }

    // ── test_to_json ──────────────────────────────────────────────────
    #[test]
    fn test_to_json() {
        let comment = RedditComment {
            author: "user_a".into(),
            body: "A comment".into(),
            score: 7,
            depth: 0,
            replies: vec![],
        };

        let data = RedditData {
            title: "JSON Post".into(),
            author: "op".into(),
            score: 100,
            selftext: "".into(),
            created_utc: 5000.0,
            permalink: "/r/j/1/".into(),
            comments: vec![comment],
            comments_truncated: false,
        };

        let val: serde_json::Value = data.into();
        assert_eq!(val["title"], "JSON Post");
        assert_eq!(val["author"], "op");
        assert_eq!(val["score"], 100);
        assert_eq!(val["comments_truncated"], false);
        assert_eq!(val["comments"][0]["author"], "user_a");
        assert_eq!(val["permalink"], "/r/j/1/");
    }

    // ── test_extract_empty_comments ───────────────────────────────────
    #[test]
    fn test_extract_empty_comments() {
        let json = make_thread_json("No Comments", "op", 0, "", "/r/test/", 1.0, vec![]);

        let result = RedditExtractor::extract(&json).unwrap();
        assert!(result.comments.is_empty());
        assert!(!result.comments_truncated);
    }
}
