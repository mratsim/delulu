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
        Err(WebfetchError::Parse(msg)) => {
            assert!(!msg.is_empty(), "parse error message should not be empty");
        }
        other => panic!("expected WebfetchError::Parse, got {:?}", other),
    }
}

#[test]
fn test_extract_invalid_json_empty() {
    let result = DiscourseExtractor::extract("");
    match result {
        Err(WebfetchError::Parse(_)) => {} // expected
        other => panic!("expected WebfetchError::Parse, got {:?}", other),
    }
}

#[test]
fn test_extract_invalid_json_missing_fields() {
    let result = DiscourseExtractor::extract(r#"{"title":"no posts"}"#);
    match result {
        Err(WebfetchError::Parse(_)) => {} // expected
        other => panic!("expected WebfetchError::Parse, got {:?}", other),
    }
}

