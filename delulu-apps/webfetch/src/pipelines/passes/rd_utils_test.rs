use super::*;

// ── normalize_spaces ──────────────────────────────────────────────────

#[test]
fn test_normalize_spaces_empty() {
    assert_eq!(normalize_spaces(""), "");
}

#[test]
fn test_normalize_spaces_all_whitespace() {
    assert_eq!(normalize_spaces("   \n\t  "), "");
}

#[test]
fn test_normalize_spaces_single_word() {
    assert_eq!(normalize_spaces("hello"), "hello");
}

#[test]
fn test_normalize_spaces_simple() {
    assert_eq!(normalize_spaces("hello   world"), "hello world");
}

#[test]
fn test_normalize_spaces_with_newlines() {
    assert_eq!(normalize_spaces("hello\nworld\n\nfoo"), "hello world foo");
}

#[test]
fn test_normalize_spaces_with_tabs() {
    assert_eq!(normalize_spaces("hello\tworld\t\tfoo"), "hello world foo");
}

#[test]
fn test_normalize_spaces_trim_leading() {
    assert_eq!(normalize_spaces("   hello world"), "hello world");
}

#[test]
fn test_normalize_spaces_trim_trailing() {
    assert_eq!(normalize_spaces("hello world   "), "hello world");
}

#[test]
fn test_normalize_spaces_trim_both() {
    assert_eq!(normalize_spaces("  hello   world  "), "hello world");
}

#[test]
fn test_normalize_spaces_unicode() {
    assert_eq!(normalize_spaces("élève   garçon"), "élève garçon");
}

// ── get_inner_text ────────────────────────────────────────────────────

#[test]
fn test_get_inner_text_single_text() {
    let node = DomNode::Text("hello world".into());
    assert_eq!(get_inner_text(&node), "hello world");
}

#[test]
fn test_get_inner_text_element_with_text() {
    let node = DomNode::Element {
        tag: "p".into(),
        attrs: vec![],
        children: vec![DomNode::Text("hello world".into())],
        scores: Default::default(),
        metadata: Default::default(),
    };
    assert_eq!(get_inner_text(&node), "hello world");
}

#[test]
fn test_get_inner_text_nested_elements() {
    let node = DomNode::Element {
        tag: "div".into(),
        attrs: vec![],
        children: vec![
            DomNode::Text("hello ".into()),
            DomNode::Element {
                tag: "b".into(),
                attrs: vec![],
                children: vec![DomNode::Text("world".into())],
                scores: Default::default(),
                metadata: Default::default(),
            },
        ],
        scores: Default::default(),
        metadata: Default::default(),
    };
    assert_eq!(get_inner_text(&node), "hello world");
}

#[test]
fn test_get_inner_text_normalizes_whitespace() {
    let node = DomNode::Element {
        tag: "p".into(),
        attrs: vec![],
        children: vec![DomNode::Text("hello   world\n\nfoo".into())],
        scores: Default::default(),
        metadata: Default::default(),
    };
    assert_eq!(get_inner_text(&node), "hello world foo");
}

// ── has_descendant_tag ────────────────────────────────────────────────

#[test]
fn test_has_descendant_tag_self() {
    let node = DomNode::Element {
        tag: "p".into(),
        attrs: vec![],
        children: vec![],
        scores: Default::default(),
        metadata: Default::default(),
    };
    assert!(has_descendant_tag(&node, "p"));
    assert!(!has_descendant_tag(&node, "div"));
}

#[test]
fn test_has_descendant_tag_child() {
    let node = DomNode::Element {
        tag: "div".into(),
        attrs: vec![],
        children: vec![DomNode::Element {
            tag: "p".into(),
            attrs: vec![],
            children: vec![],
            scores: Default::default(),
            metadata: Default::default(),
        }],
        scores: Default::default(),
        metadata: Default::default(),
    };
    assert!(has_descendant_tag(&node, "p"));
    assert!(!has_descendant_tag(&node, "table"));
}

#[test]
fn test_has_descendant_tag_nested_deep() {
    let node = DomNode::Element {
        tag: "html".into(),
        attrs: vec![],
        children: vec![DomNode::Element {
            tag: "body".into(),
            attrs: vec![],
            children: vec![DomNode::Element {
                tag: "div".into(),
                attrs: vec![],
                children: vec![DomNode::Element {
                    tag: "span".into(),
                    attrs: vec![],
                    children: vec![DomNode::Element {
                        tag: "img".into(),
                        attrs: vec![],
                        children: vec![],
                        scores: Default::default(),
                        metadata: Default::default(),
                    }],
                    scores: Default::default(),
                    metadata: Default::default(),
                }],
                scores: Default::default(),
                metadata: Default::default(),
            }],
            scores: Default::default(),
            metadata: Default::default(),
        }],
        scores: Default::default(),
        metadata: Default::default(),
    };
    assert!(has_descendant_tag(&node, "img"));
    assert!(!has_descendant_tag(&node, "table"));
}

#[test]
fn test_has_descendant_tag_text_node() {
    let node = DomNode::Text("hello".into());
    assert!(!has_descendant_tag(&node, "p"));
}

#[test]
fn test_has_descendant_tag_multiple_matches() {
    let node = DomNode::Element {
        tag: "div".into(),
        attrs: vec![],
        children: vec![
            DomNode::Element {
                tag: "p".into(),
                attrs: vec![],
                children: vec![],
                scores: Default::default(),
                metadata: Default::default(),
            },
            DomNode::Element {
                tag: "p".into(),
                attrs: vec![],
                children: vec![],
                scores: Default::default(),
                metadata: Default::default(),
            },
        ],
        scores: Default::default(),
        metadata: Default::default(),
    };
    assert!(has_descendant_tag(&node, "p"));
}

// ── meta_get_f64 ──────────────────────────────────────────────────────

#[test]
fn test_meta_get_f64_missing_key() {
    let metadata = HashMap::new();
    assert_eq!(meta_get_f64(&metadata, "score"), None);
}

#[test]
fn test_meta_get_f64_valid() {
    let mut metadata = HashMap::new();
    metadata.insert("score".into(), "42.5".into());
    assert_eq!(meta_get_f64(&metadata, "score"), Some(42.5));
}

#[test]
fn test_meta_get_f64_integer() {
    let mut metadata = HashMap::new();
    metadata.insert("count".into(), "10".into());
    assert_eq!(meta_get_f64(&metadata, "count"), Some(10.0));
}

#[test]
fn test_meta_get_f64_nan() {
    let mut metadata = HashMap::new();
    metadata.insert("score".into(), "NaN".into());
    assert_eq!(meta_get_f64(&metadata, "score"), None);
}

#[test]
fn test_meta_get_f64_infinity() {
    let mut metadata = HashMap::new();
    metadata.insert("score".into(), "inf".into());
    assert_eq!(meta_get_f64(&metadata, "score"), None);
}

#[test]
fn test_meta_get_f64_unparsable() {
    let mut metadata = HashMap::new();
    metadata.insert("score".into(), "not-a-number".into());
    assert_eq!(meta_get_f64(&metadata, "score"), None);
}

// ── is_body_or_html ──────────────────────────────────────────────────

#[test]
fn test_is_body_or_html() {
    // Structural wrappers — should return true
    assert!(is_body_or_html("body"));
    assert!(is_body_or_html("html"));

    // Content elements — should return false
    assert!(!is_body_or_html("div"));
    assert!(!is_body_or_html("article"));
    assert!(!is_body_or_html("section"));
    assert!(!is_body_or_html("p"));
    assert!(!is_body_or_html("span"));
    assert!(!is_body_or_html("main"));

    // Edge cases
    assert!(!is_body_or_html("BODY")); // case-sensitive
    assert!(!is_body_or_html("HTML")); // case-sensitive
    assert!(!is_body_or_html("")); // empty string
    assert!(!is_body_or_html("body ")); // trailing space
}
