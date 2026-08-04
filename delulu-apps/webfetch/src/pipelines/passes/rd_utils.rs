use crate::pipelines::DomNode;
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
    normalize_spaces(&node.text_content())
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

/// Parse a metadata key as `f64`, returning `None` on missing or unparsable.
///
/// NaN and Infinity produce `None` with a warning.
///
/// Callers must handle the `None` case (panicking or fallback as appropriate).
pub fn meta_get_f64(metadata: &HashMap<String, String>, key: &str) -> Option<f64> {
    metadata.get(key).and_then(|s| meta_parse_f64(s))
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
#[path = "../../../tests/unit/pipelines/passes/rd_utils_test.rs"]
mod tests;
