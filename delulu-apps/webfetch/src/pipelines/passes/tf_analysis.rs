use once_cell::sync::Lazy;
use regex::Regex;

use crate::pipelines::DomNode;

// ---------------------------------------------------------------------------
// Utility: text collection helpers
// ---------------------------------------------------------------------------

/// Get text content from visible elements only, excluding <script> and <style>.
#[allow(dead_code)]
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
                let text = children.iter().map(DomNode::text_content).collect::<String>();
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


/// Minimum extracted content size in characters (in `<p>` text at any depth within a container).
/// Matches Trafilatura's `min_extracted_size` default of 250 chars.
/// A container that matches BODY_XPATH patterns must have at least this many
/// characters of `<p>` text to be accepted.
///
/// Uses byte length (`String::len()`), consistent with ASCII-dominated web content.
/// For CJK content, byte length may overestimate vs UTF-8 char count, making the
/// threshold slightly more lenient — acceptable for precision mode.
pub const MIN_EXTRACTED_SIZE: usize = 250;
