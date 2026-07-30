use once_cell::sync::Lazy;
use regex::Regex;

use crate::pipelines::DomNode;

// ---------------------------------------------------------------------------
// Utility: text collection helpers
// ---------------------------------------------------------------------------

/// Get text content from visible elements only, excluding <script> and <style>.
pub(crate) fn get_visible_text(node: &DomNode) -> String {
    match node {
        DomNode::Text(t) => t.clone(),
        DomNode::Element { tag, children, .. } if matches!(
            tag.as_str(), "script" | "style"
        ) => String::new(),
        DomNode::Element { children, .. } => {
            let mut result = String::new();
            for child in children {
                result.push_str(&get_visible_text(child));
            }
            result
        }
        _ => String::new(),
    }
}

/// Collect all text content from a subtree, excluding comments and doctypes.
/// Note: No direct Python trafilatura equivalent — Rust-specific.
pub(crate) fn collect_text(nodes: &[DomNode]) -> String {
    let mut result = String::new();
    for node in nodes {
        match node {
            DomNode::Text(t) => result.push_str(t),
            DomNode::Element { children, .. } => result.push_str(&collect_text(children)),
            _ => {} // Skip comments, doctypes
        }
    }
    result
}

/// Collect all text content from a single node's subtree.
///
/// Unlike `collect_text` which takes a slice, this operates on a single
/// Note: No direct Python trafilatura equivalent — Rust-specific.
/// `&DomNode`. Returns the concatenated text of all descendant Text nodes.
pub(crate) fn get_inner_text(node: &DomNode) -> String {
    match node {
        DomNode::Text(t) => t.clone(),
        DomNode::Element { children, .. } => collect_text(children),
        _ => String::new(),
    }
}

/// Count total text length from all `<p>` elements in the subtree (any depth).
///
/// Assumes cleaned tags (script, style) have been removed by earlier pipeline steps.
/// Uses byte length (`String::len()`). O(N) traversal; called per matching container.
/// Note: No direct Python trafilatura equivalent — Rust-specific.
pub(crate) fn count_p_text(nodes: &[DomNode]) -> usize {
    nodes
        .iter()
        .map(|node| match node {
            DomNode::Element { tag, children, .. } if tag == "p" => collect_text(children).len(),
            DomNode::Element { children, .. } => count_p_text(children),
            _ => 0,
        })
        .sum()
}

/// Count total text length from all `<a>` descendant elements (recursive, any depth).
///
/// Recursively traverses the node tree. For each `<a>` element found, adds its
/// inner text length. Continues descending into non-`<a>` elements to find
/// nested anchors (e.g., `<div><span><a>link</a></span></div>`).
///
/// This is used by `tf_filter_by_link_density` to compute the link-to-text ratio.
/// The recursive search ensures nested `<a>` tags (inside `<span>`, `<p>`, `<li>`,
/// etc.) are counted, matching Readability's behavior.
pub(crate) fn count_link_text(nodes: &[DomNode]) -> usize {
    nodes
        .iter()
        .map(|node| match node {
            DomNode::Element { tag, children, .. } if tag == "a" => get_inner_text(node).len(),
            DomNode::Element { children, .. } => count_link_text(children),
            _ => 0,
        })
        .sum()
}

// ---------------------------------------------------------------------------
// Pass-level helpers: paragraph recovery, JSON-LD extraction, text measurement
// ---------------------------------------------------------------------------

// Regex for detecting any HTML tag in a string (case-insensitive).
// Used by `extract_jsonld_article_body` to determine if articleBody contains HTML markup.
static HTML_TAG_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r#"(?i)<[a-z][a-z0-9]*\b[^>]*>"#).expect("invalid HTML_TAG_RE")
});

/// Extract `articleBody` from JSON-LD scripts in the DOM tree.
/// Returns `None` if no JSON-LD script with `articleBody` is found.
///
/// Pre: DOM tree is fully parsed (may contain `<script>` elements).
/// Post: Returns the article body text if found and >= 100 chars.
///
/// Note: This function is recursive. Stack overflow may occur on DOM trees deeper than ~1000 nodes.
///
/// Reference: Trafilatura `baseline()` in `baseline.py:24-58` (JSON-LD articleBody extraction portion, lines 41-58)

pub(crate) fn extract_jsonld_article_body(node: &DomNode) -> Option<String> {
    match node {
        DomNode::Text(_) => None,
        DomNode::Element { tag, attrs, children, .. } if tag == "script" => {
            // Check if type attribute is exactly "application/ld+json"
            let is_jsonld = attrs.iter().any(|(k, v)| {
                k.eq_ignore_ascii_case("type")
                    && v == "application/ld+json"
            });
            if is_jsonld {
                let text = collect_text(children);
                if text.contains("articleBody") {
                    if let Ok(val) = serde_json::from_str::<serde_json::Value>(&text) {
                        if let Some(body) = val.get("articleBody").and_then(|v| v.as_str()) {
                            let trimmed = body.trim();
                            // Minimum article body length threshold (pre-existing, matches Trafilatura behavior)
                            if trimmed.len() >= 100 {
                                let text = if HTML_TAG_RE.is_match(trimmed) {
                                    let fragment = scraper::Html::parse_fragment(trimmed);
                                    fragment.root_element().text().collect::<String>().trim().to_string()
                                } else {
                                    trimmed.to_string()
                                };
                                return Some(text);
                            }
                        }
                    }
                }
                // JSON-LD script was processed (even if no articleBody found) — skip child recursion
                return None;
            }
            // Not a JSON-LD script — recurse into children
            for child in children {
                if let Some(body) = extract_jsonld_article_body(child) {
                    return Some(body);
                }
            }
            None
        }
        DomNode::Element { children, .. } => {
            for child in children {
                if let Some(body) = extract_jsonld_article_body(child) {
                    return Some(body);
                }
            }
            None
        }
        _ => None,
    }
}

/// Count total text characters in a DOM tree recursively.
///
/// Pre: DOM tree is fully parsed.
/// Post: Returns the total byte length of all text nodes in the tree.
///
/// Note: This function is recursive. Stack overflow may occur on DOM trees deeper than ~1000 nodes.
///
/// Reference: Trafilatura `len(tree.text_content())` in `htmlprocessing.py:95,106`
pub(crate) fn count_text_chars(node: &DomNode) -> usize {
    match node {
        DomNode::Text(t) => t.len(),
        DomNode::Element { children, .. } => children.iter().map(count_text_chars).sum(),
        _ => 0,
    }
}

/// Count non-whitespace text characters in a DOM tree recursively.
/// This gives a better estimate of actual useful content than raw text length,
/// which includes whitespace from HTML formatting (newlines, indentation).
///
/// Pre: DOM tree is fully parsed.
/// Post: Returns the count of non-whitespace characters in all text nodes.
///
/// Note: This function is recursive. Stack overflow may occur on DOM trees deeper than ~1000 nodes.
///
/// Note: No direct Python trafilatura equivalent — Rust-specific utility for estimating useful content.
pub(crate) fn count_non_ws_chars(node: &DomNode) -> usize {
    match node {
        DomNode::Text(t) => t.chars().filter(|c| !c.is_whitespace()).count(),
        DomNode::Element { children, .. } => children.iter().map(count_non_ws_chars).sum(),
        _ => 0,
    }
}

/// Check if a container element has enough content to be considered the main article.
///
/// Navigates the path to the matched element and checks:
/// 1. If `<p>` text content is >= `MIN_EXTRACTED_SIZE`
/// 2. If total text content (all tags) is >= `MIN_EXTRACTED_SIZE`
///
/// This is the content threshold check that matches Trafilatura's behavior:
/// a BODY_XPATH match is only accepted if the container has enough text.
///
/// Pre: `root_children` is the children of the root `<html>` element.
///      `path` is a valid path to a child element found by `find_first_match`.
/// Post: Returns `true` if the container has enough content.
///
/// Note: No direct Python trafilatura equivalent — Rust-specific content check.
pub(crate) fn container_has_content(root_children: &[DomNode], path: &[usize]) -> bool {
    if path.is_empty() {
        return false;
    }
    // Navigate to the parent of the matched element (all indices except last)
    let mut current = root_children;
    let parent_len = path.len() - 1;
    for &idx in &path[..parent_len] {
        if idx >= current.len() {
            return false;
        }
        if let DomNode::Element { children, .. } = &current[idx] {
            current = children;
        } else {
            return false;
        }
    }
    // Get the matched element at the last index
    let last_idx = path[parent_len];
    if last_idx >= current.len() {
        return false;
    }
    if let DomNode::Element { children, .. } = &current[last_idx] {
        let p_text = count_p_text(children);
        if p_text >= MIN_EXTRACTED_SIZE {
            return true;
        }
        if collect_text(children).len() >= MIN_EXTRACTED_SIZE {
            return true;
        }
    }
    false
}

/// Minimum extracted content size in characters (in `<p>` text at any depth within a container).
/// Matches Trafilatura's `min_extracted_size` default of 250 chars.
/// A container that matches BODY_XPATH patterns must have at least this many
/// characters of `<p>` text to be accepted.
///
/// Uses byte length (`String::len()`), consistent with ASCII-dominated web content.
/// For CJK content, byte length may overestimate vs UTF-8 char count, making the
/// threshold slightly more lenient — acceptable for precision mode.
pub const MIN_EXTRACTED_SIZE: usize = 250;
