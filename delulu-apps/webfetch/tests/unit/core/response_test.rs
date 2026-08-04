use super::*;

use crate::core::page_status::{BlockedBy, PageStatus};
use crate::core::types::{ExtractionResult, MarkdownDocument};

fn generic_result() -> ExtractionResult {
    ExtractionResult::GenericHtml {
        content_md: MarkdownDocument {
            frontmatter: "title: Test\nsource_type: generic_html\n".to_string(),
            body: "Some body".to_string(),
        },
        raw_html_len: 100,
        filtered_html_len: 9,
    }
}

// sibling page_status key for Blocked{CloudflareTurnstile} + GenericHtml.
#[test]
fn webfetch_raw_response_blocked_cloudflare_with_generic_html() {
    let result = generic_result();
    let status = PageStatus::Blocked {
        by: BlockedBy::CloudflareTurnstile,
    };
    let json = webfetch_raw_response(&result, &status);
    let value: serde_json::Value = serde_json::from_str(&json).unwrap();
    let obj = value.as_object().expect("top-level must be an object");
    let mut keys: Vec<&String> = obj.keys().collect();
    keys.sort();
    assert_eq!(keys, vec!["GenericHtml", "page_status"]);
    assert_eq!(
        obj["page_status"],
        serde_json::json!({"status": "blocked", "by": "cloudflare_turnstile"})
    );
    // The GenericHtml key stays at top level.
    assert!(obj["GenericHtml"].is_object());
}

// Article status with a Reddit result (additional sibling form).
#[test]
fn webfetch_raw_response_article_with_reddit() {
    let result = ExtractionResult::Reddit {
        title: "Hello".to_string(),
        selftext: "body".to_string(),
        author: "a".to_string(),
        score: 1,
        permalink: "/r/x/comments/1/".to_string(),
        source_url: "https://reddit.com/r/x/comments/1/".to_string(),
        comments: vec![],
        comment_count: 0,
    };
    let json = webfetch_raw_response(&result, &PageStatus::Article);
    let value: serde_json::Value = serde_json::from_str(&json).unwrap();
    let obj = value.as_object().expect("top-level must be an object");
    assert!(
        obj.contains_key("Reddit"),
        "Reddit key must stay at top level"
    );
    assert_eq!(obj["page_status"], serde_json::json!({"status": "article"}));
}

// Blocked{CookieConsent} sibling form.
#[test]
fn webfetch_raw_response_blocked_cookie_consent() {
    let result = generic_result();
    let status = PageStatus::Blocked {
        by: BlockedBy::CookieConsent,
    };
    let json = webfetch_raw_response(&result, &status);
    let value: serde_json::Value = serde_json::from_str(&json).unwrap();
    let obj = value.as_object().unwrap();
    assert_eq!(
        obj["page_status"],
        serde_json::json!({"status": "blocked", "by": "cookie_consent"})
    );
    assert!(obj.contains_key("GenericHtml"));
}

// every PageStatus serialized form appears as a valid sibling.
#[test]
fn webfetch_raw_response_every_page_status_form() {
    let result = generic_result();
    let statuses = [
        PageStatus::Article,
        PageStatus::Partial,
        PageStatus::JSHeavy,
        PageStatus::Gallery,
        PageStatus::Blocked {
            by: BlockedBy::CloudflareTurnstile,
        },
        PageStatus::Blocked {
            by: BlockedBy::Captcha,
        },
        PageStatus::Blocked {
            by: BlockedBy::Anubis,
        },
        PageStatus::Blocked {
            by: BlockedBy::CookieConsent,
        },
        PageStatus::Blocked {
            by: BlockedBy::Unknown,
        },
        PageStatus::Empty,
    ];
    for status in statuses {
        let json = webfetch_raw_response(&result, &status);
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        let obj = value.as_object().unwrap();
        assert!(
            obj.contains_key("page_status"),
            "missing page_status for {status:?}"
        );
        assert!(
            obj.contains_key("GenericHtml"),
            "missing GenericHtml for {status:?}"
        );
    }
}
