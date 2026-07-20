use crate::pipeline::DomNode;
use once_cell::sync::Lazy;
use regex::Regex;
use std::collections::HashMap;

// ---------------------------------------------------------------------------
// Class/ID weight computation (from Mozilla Readability)
// ---------------------------------------------------------------------------

static POSITIVE_CANDIDATES_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        "article|body|content|entry|hentry|h-entry|main|page|pagination|post|text|blog|story",
    )
    .expect("invalid positive-candidate regex")
});

static NEGATIVE_CANDIDATES_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        "-ad-|hidden|^hid$| hid$| hid |^hid |banner|combx|comment|com-|contact|footer|gdpr|masthead|media|meta|outbrain|promo|related|scroll|share|shoutbox|sidebar|skyscraper|sponsor|shopping|tags|widget"
    )
    .expect("invalid negative-candidate regex")
});

/// Compute the class/ID weight for a node's attributes.
///
/// Matches positive candidates (+25) and negative candidates (-25)
/// against class and id attributes, returning the sum.
/// This is the same logic used by `compute_mozilla_readability_score`.
///
/// Returns 0 if no class/id attributes match.
pub fn get_class_weight(attrs: &[(String, String)]) -> i32 {
    let mut weight: i32 = 0;
    for (key, value) in attrs {
        if key == "class" || key == "id" {
            if POSITIVE_CANDIDATES_RE.is_match(value) {
                weight += 25;
            }
            if NEGATIVE_CANDIDATES_RE.is_match(value) {
                weight -= 25;
            }
        }
    }
    weight
}

// ---------------------------------------------------------------------------
// Metadata helpers
// ---------------------------------------------------------------------------

/// Safely parse a metadata string as f64.
/// Returns `tracing::warn!` + None on non-numeric input.
/// Returns None on NaN or Infinity.
pub fn meta_parse_f64(val: &str) -> Option<f64> {
    let parsed: f64 = match val.parse() {
        Ok(v) => v,
        Err(_) => {
            tracing::warn!("meta_parse_f64: non-numeric value '{}'", val);
            return None;
        }
    };
    if parsed.is_nan() || parsed.is_infinite() {
        tracing::warn!("meta_parse_f64: got {} from '{}'", parsed, val);
        return None;
    }
    Some(parsed)
}

// ---------------------------------------------------------------------------
// normalize_spaces
// ---------------------------------------------------------------------------

/// Collapse all whitespace runs to a single space and trim leading/trailing.
///
/// Replaces `\s+` runs (including newlines, tabs) with a single space,
/// then strips leading/trailing whitespace.
///
/// Returns an empty string if `text` is empty or all-whitespace.
pub fn normalize_spaces(text: &str) -> String {
    let mut parts = text.split_whitespace();
    match parts.next() {
        None => String::new(),
        Some(first) => {
            let mut result = String::with_capacity(text.len());
            result.push_str(first);
            for part in parts {
                result.push(' ');
                result.push_str(part);
            }
            result
        }
    }
}

// ---------------------------------------------------------------------------
// get_inner_text
// ---------------------------------------------------------------------------

/// Recursively collect text content from all descendant Text nodes,
/// normalizing whitespace via [`normalize_spaces`].
///
/// Equivalent to JS Readability's `_getInnerText(e, normalizeSpaces=true)`.
///
/// Whitespace runs in the raw concatenated text are collapsed to single spaces,
/// and leading/trailing whitespace is removed.
pub fn get_inner_text(node: &DomNode) -> String {
    fn collect(node: &DomNode, buf: &mut String) {
        match node {
            DomNode::Text(t) => buf.push_str(t),
            DomNode::Element { children, .. } => {
                for child in children {
                    collect(child, buf);
                }
            }
            _ => {}
        }
    }
    let mut raw = String::new();
    collect(node, &mut raw);
    normalize_spaces(&raw)
}

// ---------------------------------------------------------------------------
// has_descendant_tag
// ---------------------------------------------------------------------------

/// Recursively check if any descendant Element has the given tag.
///
/// Returns `true` if the node itself matches the tag.
pub fn has_descendant_tag(node: &DomNode, tag: &str) -> bool {
    match node {
        DomNode::Element {
            tag: t, children, ..
        } => {
            if t == tag {
                return true;
            }
            for child in children {
                if has_descendant_tag(child, tag) {
                    return true;
                }
            }
            false
        }
        _ => false,
    }
}

// ---------------------------------------------------------------------------
// meta_get_f64
// ---------------------------------------------------------------------------

/// Parse a metadata key as `f64`, returning `None` on missing or unparseable.
///
/// NaN and Infinity produce `None` with a warning.
///
/// Callers must handle the `None` case (panicking or fallback as appropriate).
pub fn meta_get_f64(metadata: &HashMap<String, String>, key: &str) -> Option<f64> {
    metadata
        .get(key)
        .and_then(|s| meta_parse_f64(s))
}

// ---------------------------------------------------------------------------
// Phrasing content detection
// ---------------------------------------------------------------------------

/// Phrasing-content tags (inline / text-level semantics).
/// Same as the existing `PHRASING_TAGS` in mozilla_readability.rs, exported for reuse.
pub const PHRASING_TAGS: &[&str] = &[
    "span", "a", "strong", "em", "b", "i", "u", "small", "sub", "sup", "code", "kbd", "var", "s",
    "q", "abbr", "cite", "mark", "time", "output", "br", "img", "input", "label", "button",
    "select", "textarea", "wbr", "bdi", "bdo", "dfn", "samp", "data", "acronym", "big", "tt",
    "strike", "nobr",
];

/// Returns `true` when the node is phrasing content — either a text node or an
/// element whose tag is in the phrasing set.
pub fn is_phrasing(node: &DomNode) -> bool {
    match node {
        DomNode::Text(_) => true,
        DomNode::Element { tag, .. } => PHRASING_TAGS.contains(&tag.as_str()),
        _ => false,
    }
}


// ---------------------------------------------------------------------------
// is_body_or_html
// ---------------------------------------------------------------------------

/// Returns true if the tag is "body" or "html" — these are structural
/// wrappers that should never be selected as content candidates.
pub fn is_body_or_html(tag: &str) -> bool {
    tag == "body" || tag == "html"
}



/// Check if all children of an element are phrasing content.
pub fn all_phrasing(children: &[DomNode]) -> bool {
    children.iter().all(is_phrasing)
}

#[cfg(test)]
mod tests {
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
    fn test_meta_get_f64_unparseable() {
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
        assert!(!is_body_or_html("BODY"));   // case-sensitive
        assert!(!is_body_or_html("HTML"));   // case-sensitive
        assert!(!is_body_or_html(""));       // empty string
        assert!(!is_body_or_html("body "));  // trailing space
    }
}
