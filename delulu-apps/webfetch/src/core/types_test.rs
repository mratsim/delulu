use super::*;

// -- WebbfetchError construction ---------------------------------------

#[test]
fn test_error_fetch() {
    let e = WebbfetchError::Fetch("connection refused".into());
    assert_eq!(e.to_string(), "HTTP fetch error: connection refused");
}

#[test]
fn test_error_parse() {
    let e = WebbfetchError::Parse("invalid JSON".into());
    assert_eq!(e.to_string(), "Parse error: invalid JSON");
}

#[test]
fn test_error_pass() {
    let e = WebbfetchError::Pass("DOM walk failed".into());
    assert_eq!(e.to_string(), "DOM pass error: DOM walk failed");
}

#[test]
fn test_error_invalid_url() {
    let e = WebbfetchError::InvalidUrl("bad scheme".into());
    assert_eq!(e.to_string(), "Invalid URL: bad scheme");
}

#[test]
fn test_error_timeout() {
    let e = WebbfetchError::Timeout("timed out after 30s".into());
    assert_eq!(e.to_string(), "Request timed out: timed out after 30s");
}

#[test]
fn test_error_retry_exhausted() {
    let e = WebbfetchError::RetryExhausted(3);
    assert_eq!(e.to_string(), "Retry exhausted after 3 attempts");
}

#[test]
fn test_error_auth_required() {
    let e = WebbfetchError::AuthRequired("API key missing".into());
    assert_eq!(e.to_string(), "Authentication required: API key missing");
}

// -- SourceType Display / FromStr round-trip ----------------------------

#[test]
fn test_source_type_display_reddit() {
    assert_eq!(SourceType::Reddit.to_string(), "reddit");
}

#[test]
fn test_source_type_display_discourse() {
    assert_eq!(SourceType::Discourse.to_string(), "discourse");
}

#[test]
fn test_source_type_display_generic_html() {
    assert_eq!(SourceType::GenericHtml.to_string(), "generic_html");
}

#[test]
fn test_source_type_from_str_reddit() {
    assert_eq!("reddit".parse::<SourceType>().unwrap(), SourceType::Reddit);
}

#[test]
fn test_source_type_from_str_discourse() {
    assert_eq!(
        "discourse".parse::<SourceType>().unwrap(),
        SourceType::Discourse
    );
}

#[test]
fn test_source_type_from_str_generic() {
    assert_eq!(
        "generic".parse::<SourceType>().unwrap(),
        SourceType::GenericHtml
    );
}

#[test]
fn test_source_type_from_str_html() {
    assert_eq!(
        "html".parse::<SourceType>().unwrap(),
        SourceType::GenericHtml
    );
}

#[test]
fn test_source_type_from_str_generic_html() {
    assert_eq!(
        "generic_html".parse::<SourceType>().unwrap(),
        SourceType::GenericHtml
    );
}

#[test]
fn test_source_type_from_str_case_insensitive() {
    assert_eq!("Reddit".parse::<SourceType>().unwrap(), SourceType::Reddit);
    assert_eq!(
        "DISCOURSE".parse::<SourceType>().unwrap(),
        SourceType::Discourse
    );
    assert_eq!(
        "GENERIC_HTML".parse::<SourceType>().unwrap(),
        SourceType::GenericHtml
    );
}

#[test]
fn test_source_type_from_str_unknown() {
    let err = "unknown".parse::<SourceType>().unwrap_err();
    assert!(err.contains("unknown source type"));
}

// -- SourceType round-trip ----------------------------------------------

#[test]
fn test_source_type_round_trip() {
    for variant in [
        SourceType::Reddit,
        SourceType::Discourse,
        SourceType::GenericHtml,
    ] {
        let display = variant.to_string();
        let parsed: SourceType = display.parse().unwrap();
        assert_eq!(parsed, variant);
    }
}

// -- MarkdownDocument construction --------------------------------------

#[test]
fn test_markdown_document_construction() {
    let doc = MarkdownDocument {
        frontmatter: "title: Test".into(),
        body: "# Hello".into(),
    };
    assert_eq!(doc.frontmatter, "title: Test");
    assert_eq!(doc.body, "# Hello");
}

// -- ExtractionResult variant construction -------------------------------

#[test]
fn test_extraction_result_reddit() {
    let result = ExtractionResult::Reddit {
        title: "Post title".into(),
        selftext: "Body text".into(),
        author: "user".into(),
        score: 42,
        permalink: "/r/test/123/".into(),
        comments: vec![],
    };
    match result {
        ExtractionResult::Reddit { title, score, .. } => {
            assert_eq!(title, "Post title");
            assert_eq!(score, 42);
        }
        _ => panic!("expected Reddit variant"),
    }
}

#[test]
fn test_extraction_result_discourse() {
    let result = ExtractionResult::Discourse {
        title: "Topic".into(),
        topic_id: 99,
        posts: vec![DiscoursePost {
            post_number: 1,
            username: "alice".into(),
            raw: "<p>hi</p>".into(),
            created_at: "2024-01-01T00:00:00Z".into(),
            reply_to_post_number: None,
        }],
    };
    match result {
        ExtractionResult::Discourse {
            title, topic_id, ..
        } => {
            assert_eq!(title, "Topic");
            assert_eq!(topic_id, 99);
        }
        _ => panic!("expected Discourse variant"),
    }
}

#[test]
fn test_extraction_result_generic_html() {
    let md = MarkdownDocument {
        frontmatter: String::new(),
        body: "<p>hello</p>".into(),
    };
    let result = ExtractionResult::GenericHtml { content_md: md };
    match result {
        ExtractionResult::GenericHtml { content_md } => {
            assert_eq!(content_md.body, "<p>hello</p>");
        }
        _ => panic!("expected GenericHtml variant"),
    }
}
