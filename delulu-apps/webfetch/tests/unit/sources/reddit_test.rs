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
        WebfetchError::AuthRequired(msg) => {
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

// ── test_extract_empty_comments ───────────────────────────────────
#[test]
fn test_extract_empty_comments() {
    let json = make_thread_json("No Comments", "op", 0, "", "/r/test/", 1.0, vec![]);

    let result = RedditExtractor::extract(&json).unwrap();
    assert!(result.comments.is_empty());
    assert!(!result.comments_truncated);
}

