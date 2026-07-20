use once_cell::sync::Lazy;
use regex::Regex;

use crate::pipelines::DomNode;
use crate::pipelines::walkers::WalkerAction;

// ---------------------------------------------------------------------------
// MANUALLY_CLEANED tags — removed entirely
// ---------------------------------------------------------------------------

/// Tags from Trafilatura's `MANUALLY_CLEANED` list (39 tags).
///
/// Elements with these tags are removed from the DOM tree.
pub const TF_CLEANED_TAGS: &[&str] = &[
    "aside",
    "embed",
    "fencedframe",
    "footer",
    "form",
    "head",
    "iframe",
    "menu",
    "object",
    "script",
    "applet",
    "audio",
    "canvas",
    "figure",
    "map",
    "picture",
    "svg",
    "video",
    "area",
    "blink",
    "button",
    "datalist",
    "dialog",
    "frame",
    "frameset",
    "fieldset",
    "link",
    "input",
    "ins",
    "label",
    "legend",
    "marquee",
    "math",
    "menuitem",
    "nav",
    "noindex",
    "noscript",
    "optgroup",
    "option",
    "output",
    "param",
    "progress",
    "rp",
    "rt",
    "rtc",
    "select",
    "source",
    "style",
    "track",
    "textarea",
    "time",
    "use",
];

/// Remove elements whose tag is in the `MANUALLY_CLEANED` list.
///
/// Returns `WalkerAction::Remove` if the node's tag is in `TF_CLEANED_TAGS`,
/// `WalkerAction::Continue` otherwise.
pub fn tf_remove_cleaned(node: &mut DomNode) -> WalkerAction {
    match node {
        // Preserve <head> elements with a rend attribute (converted headings like <head rend=\"h1\">).
        // Use case-insensitive check for robustness.
        DomNode::Element { tag, attrs, .. }
            if tag == "head" && attrs.iter().any(|(k, _)| k.eq_ignore_ascii_case("rend")) =>
        {
            WalkerAction::Continue
        }
        DomNode::Element { tag, .. } if TF_CLEANED_TAGS.contains(&tag.as_str()) => {
            WalkerAction::Remove
        }
        _ => WalkerAction::Continue,
    }
}

// ---------------------------------------------------------------------------
// TEASER_DISCARD — remove teaser/duplicate content containers
// ---------------------------------------------------------------------------

/// Remove elements whose `class` or `id` attribute contains "teaser" (case-insensitive ASCII).
///
/// Maps to Trafilatura's `TEASER_DISCARD_XPATH`:
/// ```xpath
/// .//*[self::div or self::item or self::list or self::p or self::section or self::span]
///   [contains(translate(@id, 'T', 't'), 'teaser')
///    or contains(translate(@class, 'T', 't'), 'teaser')]
/// ```
///
/// Only `class` and `id` attributes are checked (matches Trafilatura behavior).
/// Other attributes like `role`, `aria-*`, `data-*` are intentionally excluded.
///
/// Risk: Legitimate content with "teaser" in class/id will be removed.
/// This matches Trafilatura's behavior — a known trade-off.
///
/// Returns `WalkerAction::Remove` if the element's tag is in the allowed list
/// AND its `class` or `id` (case-insensitive) contains "teaser".
/// `WalkerAction::Continue` otherwise.
pub fn tf_remove_teaser(node: &mut DomNode) -> WalkerAction {
    match node {
        DomNode::Element { tag, attrs, .. }
            if matches!(
                tag.as_str(),
                "div" | "item" | "list" | "p" | "section" | "span"
            ) =>
        {
            let has_teaser = attrs.iter().any(|(key, val)| {
                matches!(key.as_str(), "class" | "id")
                    && val.to_ascii_lowercase().contains("teaser")
            });
            if has_teaser {
                WalkerAction::Remove
            } else {
                WalkerAction::Continue
            }
        }
        _ => WalkerAction::Continue,
    }
}

// ---------------------------------------------------------------------------
// OVERALL_DISCARD_XPATH — remove unlikely-candidate elements
// ---------------------------------------------------------------------------
//
// Three regex patterns matching Trafilatura's OVERALL_DISCARD_XPATH
// Source: trafilatura/xpaths.py lines 118-148
//
// Pattern 1: Shared id|class — matches `re:test(@id|@class, ...)`
static OVERALL_DISCARD_SHARED_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        "(?i)^shar|viral|social|syndication|newsletter|cookie|tags|sidebar|banner|bread-?crumb|author|button"
    ).expect("invalid OVERALL_DISCARD_SHARED_RE")
});

/// Pattern 2: ID-only — matches `re:test(@id, ...)`
static OVERALL_DISCARD_ID_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        "(?i)^(?:jp-|dpsp-content)|footer|Footer|share|Share|nav|Nav|related|menu|message-container|bmdh|premium"
    ).expect("invalid OVERALL_DISCARD_ID_RE")
});

/// Pattern 3: Class-only — matches `re:test(@class, ...)`
static OVERALL_DISCARD_CLASS_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        "(?i)^(?:nav|post-nav|ZendeskForm)| ad |footer|Footer|byline|Byline|elated|share-|sociable|embedded|embed|subnav|tag-list|bar|meta|menu|avigation|navbar|navbox|rating|widget|attachment|timestamp|user-info|user-profile|-ad-|-icon|article-infos|nfoline|outbrain|taboola|criteo|options|expand|consent|modal-content|permission|next-|-stories|most-popular|mol-factbox|message-container|yin|zlylin|xg1|slide|viewport|overlay|paid-?content|obfuscated|blurred"
    ).expect("invalid OVERALL_DISCARD_CLASS_RE")
});

// ---------------------------------------------------------------------------
// Pattern 2 (scope-unrestricted) — Trafilatura OVERALL_DISCARD_XPATH[1]
// Source: trafilatura/xpaths.py lines 131-151
// ---------------------------------------------------------------------------
//
// Scope-unrestricted: matches ALL elements, not just div|item|list|p|section|span.
// Matches class patterns: ^hide-, ^reply-, comments-title, nocomments, -reply-,
//   message, akismet, suggest-links, -hide-, hide-print,  hidden,  hide, noprint, notloaded
// Matches id patterns: hidden, reader-comments, akismet

/// Pattern 2: ID-only — matches `re:test(@id, 'reader-comments|akismet')` + `re:test(@id|@style, 'hidden')` (id part)
static OVERALL_DISCARD_P2_ID_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new("(?i)hidden|reader-comments|akismet").expect("invalid OVERALL_DISCARD_P2_ID_RE")
});

/// Pattern 2: Class-only — matches `re:test(@class, ...)` for Trafilatura Pattern 2
static OVERALL_DISCARD_P2_CLASS_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        "(?i)^hide-|^reply-|comments-title|nocomments|-reply-|message|akismet|suggest-links|-hide-|hide-print| hidden| hide|noprint|notloaded").expect("invalid OVERALL_DISCARD_P2_CLASS_RE")
});

/// Remove elements whose `class` or `id` attribute matches Trafilatura's
/// `OVERALL_DISCARD_XPATH` patterns.
///
/// Maps to Trafilatura's OVERALL_DISCARD_XPATH (xpaths.py:118-148):
/// ```xpath
/// .//*[self::div or self::item or self::list or self::p or self::section or self::span][
///   re:test(@id|@class, '^shar|...') or
///   re:test(@id, '^(?:jp-|...') or
///   re:test(@class, '^(?:nav|...')]
/// ```
///
/// Scope restriction: Only elements whose tag is one of `div`, `item`, `list`,
/// `p`, `section`, `span` are checked. This matches Trafilatura's XPath.
///
/// Case sensitivity: Uses `(?i)` flag in regex (Rust) vs Trafilatura's
/// `translate()` per-pattern approach. Our implementation is equivalent or
/// more permissive — acceptable for Trafilatura parity.
///
/// Known minor deviation: Namespace-prefixed HTML (e.g., `<xhtml:div>`) is
/// not handled — the tag match is exact. Rare in practice.
///
/// Role check: Uses Trafilatura's exact `contains(translate(@role, 'N', 'n'), 'nav')`
/// rather than Readability's broader `UNLIKELY_ROLES` list.
///
/// NOTE: Unlike Readability's `strip_unlikely_candidates`, this pass has
/// no `has_likely_content` guard. Elements match → removed (within scope).
/// This matches Trafilatura's unconditional OVERALL_DISCARD_XPATH behavior.
///
/// Pattern 2 (scope-unrestricted) is now fully implemented, providing full
///
/// Pre: DOM tree is fully parsed, cleaned tags already removed.
/// Post: Elements with unlikely-candidate class/id/role patterns are removed.
pub fn tf_remove_unlikely_candidates(node: &mut DomNode) -> WalkerAction {
    match node {
        DomNode::Element { tag, attrs, .. } => {
            // Never strip <html>, <body>, <head>, <base>.
            if matches!(tag.as_str(), "html" | "body" | "head" | "base") {
                return WalkerAction::Continue;
            }

            // === Pattern 2: Scope-unrestricted discard (BEFORE scope check) ===
            // Trafilatura OVERALL_DISCARD_XPATH[1] — matches ALL elements
            // regardless of tag. Covers noprint, hide-, notloaded, comments-title,
            // reply-, akismet, message, suggest-links, hidden, aria-hidden.

            let class_val = attrs
                .iter()
                .find(|(k, _)| k == "class")
                .map(|(_, v)| v.as_str())
                .unwrap_or("");
            let id_val = attrs
                .iter()
                .find(|(k, _)| k == "id")
                .map(|(_, v)| v.as_str())
                .unwrap_or("");

            let aria_hidden = attrs
                .iter()
                .find(|(k, _)| k == "aria-hidden")
                .map(|(_, v)| v.as_str())
                .unwrap_or("");
            let is_aria_hidden = aria_hidden.trim().eq_ignore_ascii_case("true");

            let style_val = attrs
                .iter()
                .find(|(k, _)| k == "style")
                .map(|(_, v)| v.as_str())
                .unwrap_or("");

            // display:none (with or without spaces — whitespace-stripping handles both)
            let has_display_none = {
                let cleaned: String = style_val.chars().filter(|c| !c.is_whitespace()).collect();
                cleaned.to_lowercase().contains("display:none")
            };

            // hidden in style (matches Trafilatura's re:test(@id|@style, 'hidden'))
            let hidden_in_style = style_val.to_ascii_lowercase().contains("hidden");

            // Pattern 2 matches: class patterns, id patterns, style hidden
            let p2_class_match = OVERALL_DISCARD_P2_CLASS_RE.is_match(class_val);
            let p2_id_match = OVERALL_DISCARD_P2_ID_RE.is_match(id_val);

            // Structural elements guard for aria-hidden (SAFETY: prevents content loss)
            // Pattern 2's @aria-hidden='true' check uses this guard too.
            let structural_tag = matches!(tag.as_str(), "main" | "article" | "section" | "body");
            let p2_aria_hidden = !structural_tag && is_aria_hidden;

            // Pattern 2 removal decision (scope-unrestricted)
            let p2_removal = p2_class_match
                || p2_id_match
                || hidden_in_style
                || has_display_none
                || p2_aria_hidden;

            if p2_removal {
                return WalkerAction::Remove;
            }

            // === Gap 1: Scope restriction — only check Pattern 1 for allowed tags ===
            if !matches!(
                tag.as_str(),
                "div" | "item" | "list" | "p" | "section" | "span"
            ) {
                return WalkerAction::Continue;
            }

            // === Pattern 1: Scope-restricted discard (Trafilatura OVERALL_DISCARD_XPATH[0]) ===
            let role_val = attrs
                .iter()
                .find(|(k, _)| k == "role")
                .map(|(_, v)| v.as_str())
                .unwrap_or("");

            let has_lp_content = attrs
                .iter()
                .any(|(k, _)| k == "data-lp-replacement-content");
            let has_most_popular = attrs
                .iter()
                .any(|(k, v)| k == "data-component" && v.contains("MostPopularStories"));

            // aria-hidden for Pattern 1 (structural-guard protected, same as Pattern 2)
            let p1_aria_hidden = !structural_tag && is_aria_hidden;
            let attr_match =
                p1_aria_hidden || has_display_none || has_lp_content || has_most_popular;

            if OVERALL_DISCARD_SHARED_RE.is_match(class_val)
                || OVERALL_DISCARD_SHARED_RE.is_match(id_val)
                || OVERALL_DISCARD_ID_RE.is_match(id_val)
                || OVERALL_DISCARD_CLASS_RE.is_match(class_val)
                // Trafilatura's exact role check: contains(translate(@role, 'N', 'n'), 'nav')
                || role_val.to_ascii_lowercase().contains("nav")
                || attr_match
            {
                return WalkerAction::Remove;
            }

            WalkerAction::Continue
        }
        _ => WalkerAction::Continue,
    }
}
// MANUALLY_STRIPPED tags — unwrapped (replace element with children)
// ---------------------------------------------------------------------------

/// Tags from Trafilatura's `MANUALLY_STRIPPED` list (~22 tags).
///
/// Elements with these tags are replaced by their children (unwrap).
pub const TF_STRIPPED_TAGS: &[&str] = &[
    "abbr", "acronym", "address", "bdi", "bdo", "big", "cite", "data", "dfn", "font", "hgroup",
    "img", "ins", "mark", "meta", "nobr", "ruby", "small", "tbody", "template", "tfoot", "thead",
];

/// Unwrap elements whose tag is in the `MANUALLY_STRIPPED` list.
///
/// Replaces each matched element with its children. If the element has no
/// children, it is removed. Operates on `Vec<DomNode>` directly because
/// `walk_pre_mut` does not support "replace with children."
///
/// Uses manual iteration with index tracking to handle the splice operation.
pub fn tf_strip_unwrapped(node: &mut DomNode) {
    // Helper that operates on a Vec<DomNode> (used for recursion)
    fn strip_inner(nodes: &mut Vec<DomNode>) {
        let mut i = 0;
        while i < nodes.len() {
            match &mut nodes[i] {
                DomNode::Element { tag, children, .. }
                    if TF_STRIPPED_TAGS.contains(&tag.as_str()) =>
                {
                    let mut extracted = std::mem::take(children);
                    nodes.splice(i..=i, extracted.drain(..));
                    // Don't increment i — splice puts children (or nothing) at position i
                }
                DomNode::Element { children, .. } => {
                    strip_inner(children); // Recurse
                    i += 1;
                }
                _ => i += 1,
            }
        }
    }
    if let DomNode::Element { children, .. } = node {
        strip_inner(children);
    }
}

// ---------------------------------------------------------------------------
// CUT_EMPTY_ELEMS — remove empty elements
// ---------------------------------------------------------------------------

/// Tags from Trafilatura's `CUT_EMPTY_ELEMS` list (21 tags).
///
/// Empty or whitespace-only elements with these tags are removed.
pub const TF_CUT_EMPTY_TAGS: &[&str] = &[
    "p",
    "div",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "li",
    "span",
    "a",
    "blockquote",
    "pre",
    "cite",
    "q",
    "code",
    "dd",
    "dl",
    "dt",
    "th",
    "td",
];

/// Remove empty elements whose tag is in the `CUT_EMPTY_ELEMS` list.
///
/// An element is considered empty if:
/// - It has no children, OR
/// - All children are whitespace-only text nodes or void elements like `<br>`.
pub fn tf_remove_empty_cut(node: &mut DomNode) -> WalkerAction {
    match node {
        DomNode::Element { tag, children, .. } if TF_CUT_EMPTY_TAGS.contains(&tag.as_str()) => {
            if children.is_empty() {
                return WalkerAction::Remove;
            }
            // Check if all children are whitespace-only text or void elements
            let all_whitespace_or_void = children.iter().all(|child| match child {
                DomNode::Text(t) => t.trim().is_empty(),
                DomNode::Element { tag, .. } => {
                    matches!(tag.as_str(), "br" | "hr" | "img" | "wbr")
                }
                _ => false,
            });
            if all_whitespace_or_void {
                WalkerAction::Remove
            } else {
                WalkerAction::Continue
            }
        }
        _ => WalkerAction::Continue,
    }
}

// ---------------------------------------------------------------------------
// Utility: text collection helpers
// ---------------------------------------------------------------------------

/// Collect all text content from a subtree, excluding comments and doctypes.
fn collect_text(nodes: &[DomNode]) -> String {
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

/// Count total text length from all `<p>` elements in the subtree (any depth).
///
/// Assumes cleaned tags (script, style) have been removed by earlier pipeline steps.
/// Uses byte length (`String::len()`). O(N) traversal; called per matching container.
fn count_p_text(nodes: &[DomNode]) -> usize {
    nodes
        .iter()
        .map(|node| match node {
            DomNode::Element { tag, children, .. } if tag == "p" => collect_text(children).len(),
            DomNode::Element { children, .. } => count_p_text(children),
            _ => 0,
        })
        .sum()
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

// ---------------------------------------------------------------------------
// BODY_XPATH container isolation
// ---------------------------------------------------------------------------

/// Regex for Pattern 0: specific class/id/role selectors.
/// Maps to Trafilatura's BODY_XPATH Pattern 0 (specific class/id selectors).
static BODY_XPATH_PATTERN_0_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        r#"(?ix)^(?:
            post|entry|text|cell|story|postarea|art-postcontent|
            post[-_]text|post-body|post-?entry|post[-_]?content|postContent|post_inner_wrapper|
            article-?text|articleText|(?:entry|page|text|article|art)-content|article__content|
            article(?:-|__)?body|articleBody|ArticleContent|body-text|article__container|
            (?:entry|article|art)-content|article__content|article(?:-|__)?body|articleBody|body-text
        )$"#,
    )
    .expect("BODY_XPATH_PATTERN_0_RE: invalid regex")
});

/// Regex for Pattern 2: content class/id patterns.
/// Maps to Trafilatura's BODY_XPATH Pattern 2 (content class/id).
static BODY_XPATH_PATTERN_2_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        r#"(?i)^(?:content[-_]main|content(?:-|__)?body|contentBody|main-content|page-content)$"#,
    )
    .expect("BODY_XPATH_PATTERN_2_RE: invalid regex")
});

/// Check all 4 BODY_XPATH patterns in cascade order (Pattern 0 → 1 → 2 → 3).
///
/// Pattern 0: specific class/id/role selectors (checked first)
/// Pattern 1: bare article/main tags
/// Pattern 2: content class/id patterns
/// Pattern 3: starts-with main / role main
///
/// ReDoS guard: skips regex matching if class_val or id_val exceeds 200 chars.
fn matches_body_xpath_patterns(
    tag: &str,
    class_val: &str,
    id_val: &str,
    role_val: &str,
    itemprop_val: &str,
) -> bool {
    // ReDoS guard: skip regex matching for very long values
    if class_val.len() > 200 || id_val.len() > 200 {
        return false;
    }

    // Pattern 0: specific selectors
    if itemprop_val == "articleBody"
        || id_val == "articleContent"
        || matches!(
            class_val,
            "post" | "entry" | "text" | "cell" | "story" | "postarea" | "art-postcontent"
        )
        || role_val == "article"
        || BODY_XPATH_PATTERN_0_RE.is_match(class_val)
        || BODY_XPATH_PATTERN_0_RE.is_match(id_val)
    {
        return true;
    }

    // Pattern 1: bare article/main tag
    if matches!(tag, "article" | "main") {
        return true;
    }

    // Pattern 2: content class/id
    if class_val == "content"
        || id_val == "content"
        || BODY_XPATH_PATTERN_2_RE.is_match(class_val)
        || BODY_XPATH_PATTERN_2_RE.is_match(id_val)
        || class_val.contains("main-content")
        || class_val.contains("page-content")
    {
        return true;
    }

    // Pattern 3: starts-with main / role main
    if tag == "main"
        || class_val.starts_with("main")
        || id_val.starts_with("main")
        || role_val.starts_with("main")
    {
        return true;
    }

    false
}

/// Isolate the main content container using BODY_XPATH patterns.
///
/// Recursively walks the DOM tree depth-first. For each element with tag
/// `article`, `div`, `main`, or `section`, extracts `class`, `id`, `role`,
/// and `itemprop` attributes and probes the 4 BODY_XPATH patterns in order.
///
/// On match, discards all sibling nodes at the same level, keeping only the
/// matched container and its subtree. Uses deepest-match strategy (recurses
/// fully, matches on way up) so the innermost container wins.
///
/// If no match found, the tree is unchanged (no-op).
///
/// Pre-condition: DOM tree is fully parsed.
/// Post-condition: If a container was matched, only that container's subtree
///   (and its ancestor chain) survives. All siblings of the matched container
///   are discarded.
pub fn tf_isolate_content_container(node: &mut DomNode) {
    if let DomNode::Element { children, .. } = node {
        isolate_container_recursive(children);
    }
}

/// Recursive helper for `tf_isolate_content_container`.
/// Returns true if a container was matched at this level or in any descendant.
fn isolate_container_recursive(nodes: &mut Vec<DomNode>) -> bool {
    let mut i = 0;
    while i < nodes.len() {
        let is_match = match &mut nodes[i] {
            DomNode::Element {
                tag,
                attrs,
                children,
                ..
            } if matches!(tag.as_str(), "article" | "div" | "main" | "section") => {
                // FIRST recurse into children to find deepest match
                let child_matched = isolate_container_recursive(children);
                if child_matched {
                    // A deeper container was found and already isolated within children
                    // Now isolate THIS level too (keep only the element containing the match)
                    true
                } else {
                    // No deeper match found — check if THIS element matches
                    // Extract attributes in a single pass through attrs
                    let class_val = attrs
                        .iter()
                        .find(|(k, _)| k == "class")
                        .map(|(_, v)| v.as_str())
                        .unwrap_or("");
                    let id_val = attrs
                        .iter()
                        .find(|(k, _)| k == "id")
                        .map(|(_, v)| v.as_str())
                        .unwrap_or("");
                    let role_val = attrs
                        .iter()
                        .find(|(k, _)| k == "role")
                        .map(|(_, v)| v.as_str())
                        .unwrap_or("");
                    let itemprop_val = attrs
                        .iter()
                        .find(|(k, _)| k == "itemprop")
                        .map(|(_, v)| v.as_str())
                        .unwrap_or("");

                    // Check if this element matches any BODY_XPATH pattern
                    // AND has sufficient <p> text content
                    if matches_body_xpath_patterns(
                        tag.as_str(),
                        class_val,
                        id_val,
                        role_val,
                        itemprop_val,
                    ) {
                        let p_text_total = count_p_text(children);
                        p_text_total >= MIN_EXTRACTED_SIZE
                    } else {
                        false
                    }
                }
            }
            DomNode::Element { children, .. } => {
                // Non-container tag — recurse into children
                isolate_container_recursive(children)
            }
            _ => false, // Text, Comment, Doctype — skip
        };

        if is_match {
            // Found a container match at this level: isolate it by discarding siblings
            let matched = nodes.remove(i);
            nodes.clear();
            nodes.push(matched);
            return true;
        }

        i += 1;
    }
    false
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipelines::parse_html;
    use crate::pipelines::walk_pre_mut;
    use std::collections::HashMap;

    // ── tf_remove_cleaned ────────────────────────────────────────────────

    #[test]
    fn test_tf_remove_cleaned_removes_aside() {
        let mut root = parse_html("<aside>content</aside><p>keep</p>").unwrap();
        walk_pre_mut(&mut root, &|n| tf_remove_cleaned(n));
        assert!(!find_tag(&root, "aside"), "<aside> should be removed");
        assert!(find_tag(&root, "p"), "<p> should still exist");
    }

    #[test]
    fn test_tf_remove_cleaned_removes_figure() {
        let mut root = parse_html("<figure><img src='x.png'></figure><p>text</p>").unwrap();
        walk_pre_mut(&mut root, &|n| tf_remove_cleaned(n));
        assert!(!find_tag(&root, "figure"), "<figure> should be removed");
    }

    #[test]
    fn test_tf_remove_cleaned_keeps_unlisted() {
        let mut root = parse_html("<p>keep</p><div>keep</div>").unwrap();
        walk_pre_mut(&mut root, &|n| tf_remove_cleaned(n));
        assert!(find_tag(&root, "p"), "<p> should be kept");
        assert!(find_tag(&root, "div"), "<div> should be kept");
    }
    #[test]
    fn test_tf_remove_cleaned_preserves_head_with_rend() {
        let mut root = parse_html("<head rend=\"h1\">Title</head><p>text</p>").unwrap();
        walk_pre_mut(&mut root, &|n| tf_remove_cleaned(n));
        assert!(
            find_tag(&root, "head"),
            "<head rend=\"h1\"> should be preserved"
        );
        assert!(find_tag(&root, "p"), "<p> should survive");
    }

    #[test]
    fn test_tf_remove_cleaned_removes_bare_head() {
        let mut root = parse_html("<head>Title</head><p>text</p>").unwrap();
        walk_pre_mut(&mut root, &|n| tf_remove_cleaned(n));
        assert!(!find_tag(&root, "head"), "bare <head> should be removed");
        assert!(find_tag(&root, "p"), "<p> should survive");
    }

    #[test]
    fn test_tf_remove_cleaned_preserves_other_cleaned() {
        let mut root = parse_html("<aside>side</aside><figure>fig</figure><p>text</p>").unwrap();
        walk_pre_mut(&mut root, &|n| tf_remove_cleaned(n));
        assert!(!find_tag(&root, "aside"), "<aside> should still be removed");
        assert!(
            !find_tag(&root, "figure"),
            "<figure> should still be removed"
        );
    }

    // ── tf_remove_teaser ──────────────────────────────────────────────

    #[test]
    fn test_tf_remove_teaser_class_div() {
        let mut root = parse_html("<div class=\"teaser\">content</div><p>keep</p>").unwrap();
        walk_pre_mut(&mut root, &|n| tf_remove_teaser(n));
        assert!(
            !find_tag(&root, "div"),
            "<div class='teaser'> should be removed"
        );
        assert!(find_tag(&root, "p"), "<p> should still exist");
    }

    #[test]
    fn test_tf_remove_teaser_class_case_insensitive() {
        let mut root = parse_html("<div class=\"TeasEr\">content</div><p>keep</p>").unwrap();
        walk_pre_mut(&mut root, &|n| tf_remove_teaser(n));
        assert!(
            !find_tag(&root, "div"),
            "<div class='TeasEr'> should be removed (case insensitive)"
        );
        assert!(find_tag(&root, "p"));
    }

    #[test]
    fn test_tf_remove_teaser_class_contains() {
        let mut root =
            parse_html("<div class=\"post-teaser-content\">content</div><p>keep</p>").unwrap();
        walk_pre_mut(&mut root, &|n| tf_remove_teaser(n));
        assert!(
            !find_tag(&root, "div"),
            "<div class='post-teaser-content'> should be removed (contains)"
        );
        assert!(find_tag(&root, "p"));
    }

    #[test]
    fn test_tf_remove_teaser_keeps_normal() {
        let mut root = parse_html("<div class=\"content\">keep me</div>").unwrap();
        walk_pre_mut(&mut root, &|n| tf_remove_teaser(n));
        assert!(find_tag(&root, "div"), "normal <div> should be kept");
    }

    #[test]
    fn test_tf_remove_teaser_paragraph_class() {
        let mut root =
            parse_html("<p class=\"teaser\">teaser text</p><p>real content</p>").unwrap();
        walk_pre_mut(&mut root, &|n| tf_remove_teaser(n));
        // teaser <p> should be removed; real content <p> should remain
        assert!(find_tag(&root, "p"), "the non-teaser <p> should remain");
    }

    #[test]
    fn test_tf_remove_teaser_section_id() {
        let mut root =
            parse_html("<section id=\"teaser-block\">teaser text</section><p>content</p>").unwrap();
        walk_pre_mut(&mut root, &|n| tf_remove_teaser(n));
        assert!(
            !find_tag(&root, "section"),
            "<section id='teaser-block'> should be removed"
        );
        assert!(find_tag(&root, "p"));
    }

    #[test]
    fn test_tf_remove_teaser_no_match_wrong_tag() {
        // <article> is NOT in the allowed tag list
        let mut root = parse_html("<article class=\"teaser\">content</article>").unwrap();
        walk_pre_mut(&mut root, &|n| tf_remove_teaser(n));
        assert!(
            find_tag(&root, "article"),
            "<article> should be kept (not in allowed tags)"
        );
    }

    // ── tf_strip_unwrapped ──────────────────────────────────────────────

    #[test]
    fn test_tf_strip_unwrapped_abbr_becomes_text() {
        let mut nodes = parse_html("<abbr title='World'>W</abbr>").unwrap();
        tf_strip_unwrapped(&mut nodes);
        // The abbr is unwrapped, leaving just text "W"
        assert!(!find_tag(&nodes, "abbr"), "<abbr> should be unwrapped");
    }

    #[test]
    fn test_tf_strip_unwrapped_address_promotes_children() {
        let mut nodes = parse_html("<address><p>content</p></address>").unwrap();
        tf_strip_unwrapped(&mut nodes);
        assert!(
            !find_tag(&nodes, "address"),
            "<address> should be unwrapped"
        );
        assert!(find_tag(&nodes, "p"), "<p> should be promoted");
    }

    // ── tf_remove_empty_cut ─────────────────────────────────────────────

    #[test]
    fn test_tf_remove_empty_cut_empty_div() {
        let mut root = parse_html("<div></div>").unwrap();
        walk_pre_mut(&mut root, &|n| tf_remove_empty_cut(n));
        assert!(!find_tag(&root, "div"), "empty <div> should be removed");
    }

    #[test]
    fn test_tf_remove_empty_cut_whitespace_p() {
        let mut root = parse_html("<p>  </p>").unwrap();
        walk_pre_mut(&mut root, &|n| tf_remove_empty_cut(n));
        assert!(
            !find_tag(&root, "p"),
            "whitespace-only <p> should be removed"
        );
    }

    #[test]
    fn test_tf_remove_empty_cut_keeps_text() {
        let mut root = parse_html("<p>text</p>").unwrap();
        walk_pre_mut(&mut root, &|n| tf_remove_empty_cut(n));
        assert!(find_tag(&root, "p"), "<p> with text should be kept");
    }

    #[test]
    fn test_tf_remove_empty_cut_keeps_li_with_link() {
        let mut root = parse_html("<li><a>link</a></li>").unwrap();
        walk_pre_mut(&mut root, &|n| tf_remove_empty_cut(n));
        assert!(find_tag(&root, "li"), "<li> with <a> should be kept");
    }

    #[test]
    fn test_tf_remove_empty_cut_void_br_children() {
        let mut root = parse_html("<p><br></p>").unwrap();
        walk_pre_mut(&mut root, &|n| tf_remove_empty_cut(n));
        assert!(
            !find_tag(&root, "p"),
            "<p> with only <br> should be removed"
        );
    }

    // ── Helpers ─────────────────────────────────────────────────────────

    fn find_tag(node: &DomNode, tag: &str) -> bool {
        match node {
            DomNode::Element {
                tag: t, children, ..
            } if t == tag => return true,
            DomNode::Element { children, .. } => children.iter().any(|c| find_tag(c, tag)),
            _ => false,
        }
    }

    fn inject_max_score(node: &mut DomNode, score: f64) {
        match node {
            DomNode::Element {
                metadata, children, ..
            } => {
                metadata.insert("md_rd_subtree_max_score".to_string(), score.to_string());
                for child in children.iter_mut() {
                    inject_max_score(child, score);
                }
            }
            _ => {}
        }
    }

    /// Inject md_rd_subtree_max_score only on nodes with a specific tag.
    /// All other nodes get a low score (0.0).
    fn inject_score_for_tag(node: &mut DomNode, target_tag: &str, high_score: f64) {
        set_score_recursive(node, 0.0, target_tag, high_score);
    }

    fn set_score_recursive(node: &mut DomNode, low_score: f64, target_tag: &str, high_score: f64) {
        match node {
            DomNode::Element {
                tag,
                metadata,
                children,
                ..
            } => {
                let score = if tag == target_tag {
                    high_score
                } else {
                    low_score
                };
                metadata.insert("md_rd_subtree_max_score".to_string(), score.to_string());
                for child in children.iter_mut() {
                    set_score_recursive(child, low_score, target_tag, high_score);
                }
            }
            _ => {}
        }
    }

    // ── BODY_XPATH container isolation ──────────────────────────────────

    #[test]
    fn test_isolate_container_class_post() {
        let p_text: String = "A".repeat(250);
        let mut nodes = parse_html(&format!(
            "<div class=\"post\"><p>{}</p></div><nav>junk</nav>",
            p_text
        ))
        .unwrap();
        tf_isolate_content_container(&mut nodes);
        assert!(find_tag(&nodes, "div"), "<div> should be kept");
        assert!(find_tag(&nodes, "p"), "<p> content should be kept");
        assert!(!find_tag(&nodes, "nav"), "<nav> sibling should be removed");
    }

    #[test]
    fn test_isolate_container_class_entry() {
        let p_text: String = "A".repeat(250);
        let mut nodes = parse_html(&format!(
            "<div class=\"entry\"><p>{}</p></div><nav>junk</nav>",
            p_text
        ))
        .unwrap();
        tf_isolate_content_container(&mut nodes);
        assert!(find_tag(&nodes, "div"), "<div> should be kept");
        assert!(!find_tag(&nodes, "nav"), "<nav> sibling should be removed");
    }

    #[test]
    fn test_isolate_container_id_content() {
        let p_text: String = "A".repeat(250);
        let mut nodes = parse_html(&format!(
            "<div id=\"content\"><p>{}</p></div><aside>junk</aside>",
            p_text
        ))
        .unwrap();
        tf_isolate_content_container(&mut nodes);
        assert!(find_tag(&nodes, "div"), "<div> should be kept");
        assert!(
            !find_tag(&nodes, "aside"),
            "<aside> sibling should be removed"
        );
    }

    #[test]
    fn test_isolate_container_article_tag() {
        let p_text: String = "A".repeat(250);
        let mut nodes = parse_html(&format!(
            "<article><p>{}</p></article><footer>junk</footer>",
            p_text
        ))
        .unwrap();
        tf_isolate_content_container(&mut nodes);
        assert!(find_tag(&nodes, "article"), "<article> should be kept");
        assert!(
            !find_tag(&nodes, "footer"),
            "<footer> sibling should be removed"
        );
    }

    #[test]
    fn test_isolate_container_main_tag() {
        let p_text: String = "A".repeat(250);
        let mut nodes = parse_html(&format!(
            "<main><p>{}</p></main><aside>junk</aside>",
            p_text
        ))
        .unwrap();
        tf_isolate_content_container(&mut nodes);
        assert!(find_tag(&nodes, "main"), "<main> should be kept");
        assert!(
            !find_tag(&nodes, "aside"),
            "<aside> sibling should be removed"
        );
    }

    #[test]
    fn test_isolate_container_re_class() {
        let p_text: String = "A".repeat(250);
        let mut nodes = parse_html(&format!(
            "<div class=\"post-content\"><p>{}</p></div><nav>junk</nav>",
            p_text
        ))
        .unwrap();
        tf_isolate_content_container(&mut nodes);
        assert!(find_tag(&nodes, "div"), "<div> should be kept");
        assert!(!find_tag(&nodes, "nav"), "<nav> should be removed");
    }

    #[test]
    fn test_isolate_container_re_id() {
        let p_text: String = "A".repeat(250);
        let mut nodes = parse_html(&format!(
            "<section id=\"article-body\"><p>{}</p></section><aside>junk</aside>",
            p_text
        ))
        .unwrap();
        tf_isolate_content_container(&mut nodes);
        assert!(find_tag(&nodes, "section"), "<section> should be kept");
        assert!(!find_tag(&nodes, "aside"), "<aside> should be removed");
    }

    #[test]
    fn test_isolate_container_starts_with_main() {
        let p_text: String = "A".repeat(250);
        let mut nodes = parse_html(&format!(
            "<div class=\"main-content\"><p>{}</p></div><nav>junk</nav>",
            p_text
        ))
        .unwrap();
        tf_isolate_content_container(&mut nodes);
        assert!(find_tag(&nodes, "div"), "<div> should be kept");
        assert!(!find_tag(&nodes, "nav"), "<nav> should be removed");
    }

    #[test]
    fn test_isolate_container_role_main() {
        let p_text: String = "A".repeat(250);
        let mut nodes = parse_html(&format!(
            "<div role=\"main\"><p>{}</p></div><footer>junk</footer>",
            p_text
        ))
        .unwrap();
        tf_isolate_content_container(&mut nodes);
        assert!(find_tag(&nodes, "div"), "<div> should be kept");
        assert!(!find_tag(&nodes, "footer"), "<footer> should be removed");
    }

    #[test]
    fn test_isolate_container_first_match_wins() {
        let p_text: String = "A".repeat(250);
        let mut nodes = parse_html(&format!(
            "<div class=\"post\"><p>{}</p></div><article><p>{}</p></article>",
            p_text, p_text
        ))
        .unwrap();
        tf_isolate_content_container(&mut nodes);
        // Pattern 0 match (<div class=\"post\">) should win over Pattern 1 (<article>)
        assert!(
            find_tag(&nodes, "div"),
            "<div> should be kept (Pattern 0 wins)"
        );
        assert!(!find_tag(&nodes, "article"), "<article> should be removed");
    }

    #[test]
    fn test_isolate_container_no_match_noop() {
        let mut nodes = parse_html("<div><p>A</p><span>B</span></div>").unwrap();
        let mut nodes = vec![parse_html("<div><p>A</p><span>B</span></div>").unwrap()];
        let original = nodes.clone();
        tf_isolate_content_container(&mut nodes[0]);
        // Tree should be unchanged (no container matched)
        assert_eq!(nodes.len(), original.len(), "tree should be unchanged");
    }

    #[test]
    fn test_isolate_container_sibling_discard() {
        let p_text: String = "A".repeat(250);
        let mut nodes = parse_html(&format!(
            "<div class=\"post\"><p>{}</p></div><nav>junk</nav><footer>x</footer>",
            p_text
        ))
        .unwrap();
        tf_isolate_content_container(&mut nodes);
        assert!(find_tag(&nodes, "div"), "<div> should be kept");
        assert!(find_tag(&nodes, "p"), "<p> content should be kept");
        assert!(!find_tag(&nodes, "nav"), "<nav> should be removed");
        assert!(!find_tag(&nodes, "footer"), "<footer> should be removed");
    }

    #[test]
    fn test_isolate_container_nested() {
        let p_text: String = "A".repeat(250);
        let mut nodes = parse_html(&format!(
            "<main><div class=\"post\"><p>{}</p></div><aside>junk</aside></main>",
            p_text
        ))
        .unwrap();
        tf_isolate_content_container(&mut nodes);
        // The outermost ancestor <main> should be kept
        assert!(find_tag(&nodes, "main"), "<main> should be kept");
        // <div> inside <main> should be kept
        assert!(find_tag(&nodes, "div"), "<div> should be kept");
        // <aside> sibling of <div> should be removed
        assert!(!find_tag(&nodes, "aside"), "<aside> should be removed");
    }

    #[test]
    fn test_isolate_container_tag_scope() {
        let mut nodes =
            parse_html("<span class=\"post\">text</span><p id=\"content\">text</p>").unwrap();
        tf_isolate_content_container(&mut nodes);
        // Neither <span> nor <p> are in the allowed tags — tree unchanged
        assert!(find_tag(&nodes, "span"), "<span> should survive");
        assert!(find_tag(&nodes, "p"), "<p> should survive");
    }

    #[test]
    fn test_isolate_container_itemprop_articleBody() {
        let p_text: String = "A".repeat(250);
        let mut nodes = parse_html(&format!(
            "<div itemprop=\"articleBody\"><p>{}</p></div><nav>junk</nav>",
            p_text
        ))
        .unwrap();
        tf_isolate_content_container(&mut nodes);
        assert!(find_tag(&nodes, "div"), "<div> should be kept");
        assert!(!find_tag(&nodes, "nav"), "<nav> should be removed");
    }

    #[test]
    fn test_isolate_container_empty_input() {
        let mut nodes = DomNode::Element {
            tag: "html".into(),
            attrs: vec![],
            children: vec![],
            scores: HashMap::new(),
            metadata: HashMap::new(),
        };
        tf_isolate_content_container(&mut nodes);
        assert!(
            matches!(&nodes, DomNode::Element { children, .. } if children.is_empty()),
            "empty input should stay empty"
        );
    }

    #[test]
    fn test_isolate_container_deeply_nested() {
        let p_text: String = "A".repeat(250);
        let mut nodes = parse_html(
            &format!("<div class=\"post\"><section><article><p>{}</p></article></section></div><nav>junk</nav>", p_text),
        )
        .unwrap();
        tf_isolate_content_container(&mut nodes);
        // The deepest container strategy should find the match at the outermost container
        assert!(
            find_tag(&nodes, "div"),
            "<div> outermost container should be kept"
        );
        assert!(
            find_tag(&nodes, "article"),
            "<article> inside should be kept"
        );
        assert!(!find_tag(&nodes, "nav"), "<nav> sibling should be removed");
    }

    #[test]
    fn test_body_xpath_regex_compiles() {
        // Verify regex statics don't panic at access time
        let _ = &*BODY_XPATH_PATTERN_0_RE;
        let _ = &*BODY_XPATH_PATTERN_2_RE;
        // Verify they don't match empty string
        assert!(!BODY_XPATH_PATTERN_0_RE.is_match(""));
        assert!(!BODY_XPATH_PATTERN_2_RE.is_match(""));
    }

    #[test]
    fn test_isolate_container_role_article() {
        let p_text: String = "A".repeat(250);
        let mut nodes = parse_html(&format!(
            "<div role=\"article\"><p>{}</p></div><nav>junk</nav>",
            p_text
        ))
        .unwrap();
        tf_isolate_content_container(&mut nodes);
        assert!(find_tag(&nodes, "div"), "<div> should be kept");
        assert!(!find_tag(&nodes, "nav"), "<nav> should be removed");
    }
    // ── Test helper for building containers with controlled p-text ─────────

    /// Build a container element with `<p>` elements containing `n_chars` of text each.
    fn make_container_with_p_text(
        tag: &str,
        class_val: &str,
        p_count: usize,
        n_chars: usize,
    ) -> DomNode {
        let p_text: String = "x".repeat(n_chars);
        let children: Vec<DomNode> = (0..p_count)
            .map(|_| DomNode::Element {
                tag: "p".into(),
                attrs: vec![],
                children: vec![DomNode::Text(p_text.clone())],
                scores: HashMap::new(),
                metadata: HashMap::new(),
            })
            .collect();
        DomNode::Element {
            tag: tag.into(),
            attrs: vec![("class".into(), class_val.into())],
            children,
            scores: HashMap::new(),
            metadata: HashMap::new(),
        }
    }

    // ── Content-length check tests ─────────────────────────────────────

    #[test]
    fn test_isolate_container_rejects_short_content() {
        let mut nodes = DomNode::Element {
            tag: "html".into(),
            attrs: vec![],
            children: vec![
                make_container_with_p_text("div", "post", 1, 10),
                DomNode::Text("other".into()),
            ],
            scores: HashMap::new(),
            metadata: HashMap::new(),
        };
        tf_isolate_content_container(&mut nodes);
        if let DomNode::Element { children, .. } = &nodes {
            assert_eq!(
                children.len(),
                2,
                "short container should not match, tree unchanged"
            );
        };
    }

    #[test]
    fn test_isolate_container_accepts_sufficient_content() {
        let mut nodes = DomNode::Element {
            tag: "html".into(),
            attrs: vec![],
            children: vec![
                make_container_with_p_text("div", "post", 1, 250),
                DomNode::Text("junk".into()),
            ],
            scores: HashMap::new(),
            metadata: HashMap::new(),
        };
        tf_isolate_content_container(&mut nodes);
        if let DomNode::Element { children, .. } = &nodes {
            assert_eq!(
                children.len(),
                1,
                "container with >=250 chars should be accepted"
            );
        };
        assert!(find_tag(&nodes, "div"), "<div> should be kept");
    }

    #[test]
    fn test_isolate_container_fallthrough_to_next_pattern() {
        // First container (Pattern 0 class "post") too short
        // Second container (Pattern 1 tag "article") has enough text
        let mut nodes = DomNode::Element {
            tag: "html".into(),
            attrs: vec![],
            children: vec![
                make_container_with_p_text("div", "post", 1, 10),
                DomNode::Element {
                    tag: "article".into(),
                    attrs: vec![],
                    children: vec![DomNode::Element {
                        tag: "p".into(),
                        attrs: vec![],
                        children: vec![DomNode::Text("x".repeat(250))],
                        scores: HashMap::new(),
                        metadata: HashMap::new(),
                    }],
                    scores: HashMap::new(),
                    metadata: HashMap::new(),
                },
            ],
            scores: HashMap::new(),
            metadata: HashMap::new(),
        };
        tf_isolate_content_container(&mut nodes);
        if let DomNode::Element { children, .. } = &nodes {
            assert_eq!(
                children.len(),
                1,
                "article should be selected after div rejected"
            );
        };
        assert!(find_tag(&nodes, "article"), "<article> should be kept");
        assert!(!find_tag(&nodes, "div"), "<div> should be removed");
    }

    #[test]
    fn test_isolate_container_count_p_text_no_p_elements() {
        let container = DomNode::Element {
            tag: "div".into(),
            attrs: vec![("class".into(), "post".into())],
            children: vec![DomNode::Text("text without p tags".into())],
            scores: HashMap::new(),
            metadata: HashMap::new(),
        };
        let mut nodes = DomNode::Element {
            tag: "html".into(),
            attrs: vec![],
            children: vec![],
            scores: HashMap::new(),
            metadata: HashMap::new(),
        };
        tf_isolate_content_container(&mut nodes);
        if let DomNode::Element { children, .. } = &nodes {
            assert_eq!(
                children.len(),
                1,
                "no <p> elements -> no match -> unchanged"
            );
        };
    }

    #[test]
    fn test_isolate_container_non_p_text_not_counted() {
        let long_text: String = "x".repeat(500);
        let container = DomNode::Element {
            tag: "div".into(),
            attrs: vec![("class".into(), "post".into())],
            children: vec![DomNode::Element {
                tag: "div".into(),
                attrs: vec![],
                children: vec![DomNode::Text(long_text)],
                scores: HashMap::new(),
                metadata: HashMap::new(),
            }],
            scores: HashMap::new(),
            metadata: HashMap::new(),
        };
        let mut nodes = DomNode::Element {
            tag: "html".into(),
            attrs: vec![],
            children: vec![],
            scores: HashMap::new(),
            metadata: HashMap::new(),
        };
        tf_isolate_content_container(&mut nodes);
        if let DomNode::Element { children, .. } = &nodes {
            assert_eq!(
                children.len(),
                1,
                "500 chars in <div> but no <p> -> no match"
            );
        };
    }

    #[test]
    fn test_isolate_container_exact_threshold_250_accepted() {
        let mut nodes = DomNode::Element {
            tag: "html".into(),
            attrs: vec![],
            children: vec![
                make_container_with_p_text("div", "post", 1, 250),
                DomNode::Text("junk".into()),
            ],
            scores: HashMap::new(),
            metadata: HashMap::new(),
        };
        tf_isolate_content_container(&mut nodes);
        if let DomNode::Element { children, .. } = &nodes {
            assert_eq!(children.len(), 1, "exactly 250 chars should be accepted");
        };
        assert!(find_tag(&nodes, "div"));
    }

    #[test]
    fn test_isolate_container_249_rejected() {
        let mut nodes = DomNode::Element {
            tag: "html".into(),
            attrs: vec![],
            children: vec![
                make_container_with_p_text("div", "post", 1, 249),
                DomNode::Text("junk".into()),
            ],
            scores: HashMap::new(),
            metadata: HashMap::new(),
        };
        tf_isolate_content_container(&mut nodes);
        if let DomNode::Element { children, .. } = &nodes {
            assert_eq!(
                children.len(),
                2,
                "249 chars should be rejected, tree unchanged"
            );
        };
    }

    #[test]
    fn test_isolate_container_whitespace_only_p_not_counted() {
        let container = DomNode::Element {
            tag: "div".into(),
            attrs: vec![("class".into(), "post".into())],
            children: vec![DomNode::Element {
                tag: "p".into(),
                attrs: vec![],
                children: vec![DomNode::Text("   ".into())],
                scores: HashMap::new(),
                metadata: HashMap::new(),
            }],
            scores: HashMap::new(),
            metadata: HashMap::new(),
        };
        let mut nodes = DomNode::Element {
            tag: "html".into(),
            attrs: vec![],
            children: vec![],
            scores: HashMap::new(),
            metadata: HashMap::new(),
        };
        tf_isolate_content_container(&mut nodes);
        if let DomNode::Element { children, .. } = &nodes {
            assert_eq!(children.len(), 1, "whitespace-only <p> -> no match");
        };
    }

    #[test]
    fn test_isolate_container_empty_container_rejected() {
        let container = DomNode::Element {
            tag: "div".into(),
            attrs: vec![("class".into(), "post".into())],
            children: vec![],
            scores: HashMap::new(),
            metadata: HashMap::new(),
        };
        let mut nodes = DomNode::Element {
            tag: "html".into(),
            attrs: vec![],
            children: vec![],
            scores: HashMap::new(),
            metadata: HashMap::new(),
        };
        tf_isolate_content_container(&mut nodes);
        if let DomNode::Element { children, .. } = &nodes {
            assert_eq!(children.len(), 1, "empty container -> no match");
        };
    }

    #[test]
    fn test_isolate_container_sibling_both_match_short_first() {
        // Two sibling containers with same pattern class "post"
        // First is short, second has enough text
        let mut nodes = DomNode::Element {
            tag: "html".into(),
            attrs: vec![],
            children: vec![
                make_container_with_p_text("div", "post", 1, 10),
                make_container_with_p_text("div", "post", 1, 250),
            ],
            scores: HashMap::new(),
            metadata: HashMap::new(),
        };
        tf_isolate_content_container(&mut nodes);
        if let DomNode::Element { children, .. } = &nodes {
            assert_eq!(children.len(), 1, "second container should be selected");
        };
        assert!(find_tag(&nodes, "div"));
    }

    #[test]
    fn test_isolate_container_integration_sidebar_vs_article() {
        // Realistic scenario: sidebar nav div with class "content" but no real p text
        // Article body with class "main-content" having enough p text
        let sidebar = DomNode::Element {
            tag: "div".into(),
            attrs: vec![("class".into(), "content".into())],
            children: vec![DomNode::Element {
                tag: "ul".into(),
                attrs: vec![],
                children: vec![DomNode::Element {
                    tag: "li".into(),
                    attrs: vec![],
                    children: vec![DomNode::Text("nav link".into())],
                    scores: HashMap::new(),
                    metadata: HashMap::new(),
                }],
                scores: HashMap::new(),
                metadata: HashMap::new(),
            }],
            scores: HashMap::new(),
            metadata: HashMap::new(),
        };
        let article_body = DomNode::Element {
            tag: "div".into(),
            attrs: vec![("class".into(), "main-content".into())],
            children: vec![DomNode::Element {
                tag: "p".into(),
                attrs: vec![],
                children: vec![DomNode::Text("x".repeat(250))],
                scores: HashMap::new(),
                metadata: HashMap::new(),
            }],
            scores: HashMap::new(),
            metadata: HashMap::new(),
        };
        let mut nodes = DomNode::Element {
            tag: "html".into(),
            attrs: vec![],
            children: vec![],
            scores: HashMap::new(),
            metadata: HashMap::new(),
        };
        tf_isolate_content_container(&mut nodes);
        if let DomNode::Element { attrs, .. } = &nodes {
            assert!(
                attrs
                    .iter()
                    .any(|(k, v)| k == "class" && v == "main-content"),
                "surviving div should have main-content class"
            );
        } else {
            panic!("expected Element node");
        }
    }

    // ── tf_remove_unlikely_candidates (has_likely_content guard removal) ──

    #[test]
    fn test_tf_remove_unlikely_candidates_removes_despite_likely_content() {
        // Core behavioral change: elements with likely-content children
        // (e.g., <p>) are now unconditionally removed when they match OVERALL_DISCARD patterns.
        // Before this change, this would have been KEPT by the has_likely_content guard.
        let mut root = parse_html("<div class=\"sidebar\"><p>content text here</p></div>").unwrap();
        walk_pre_mut(&mut root, &|n| tf_remove_unlikely_candidates(n));
        assert!(
            !find_tag(&root, "div"),
            "<div class='sidebar'> should be removed despite <p> child"
        );
        assert!(
            !find_tag(&root, "p"),
            "<p> child should also be removed with parent"
        );
    }

    #[test]
    fn test_tf_remove_unlikely_candidates_removes_display_none_with_content() {
        // attr_match path: display:none elements with <p> children should also be
        // unconditionally removed now.
        let mut root =
            parse_html("<div style=\"display:none\"><p>hidden content</p></div>").unwrap();
        walk_pre_mut(&mut root, &|n| tf_remove_unlikely_candidates(n));
        assert!(
            !find_tag(&root, "div"),
            "<div style='display:none'> should be removed despite <p> child"
        );
    }

    #[test]
    fn test_tf_remove_unlikely_candidates_keeps_non_matching() {
        // Elements that do NOT match UNLIKELY_CANDIDATES_RE should still be kept.
        let mut root =
            parse_html("<div class=\"content\"><p>actual article content</p></div>").unwrap();
        walk_pre_mut(&mut root, &|n| tf_remove_unlikely_candidates(n));
        assert!(
            find_tag(&root, "div"),
            "<div class='content'> should be kept (no match)"
        );
        assert!(find_tag(&root, "p"), "<p> should be kept (parent kept)");
    }

    // ── Gap 1: Scope restriction tests ────────────────────────────────────

    #[test]
    fn test_scope_restriction_keeps_a_tag() {
        // <a> is NOT in the allowed scope (div|item|list|p|section|span)
        let mut root = parse_html("<a class=\"sidebar\">link text</a>").unwrap();
        walk_pre_mut(&mut root, &|n| tf_remove_unlikely_candidates(n));
        assert!(
            find_tag(&root, "a"),
            "<a class='sidebar'> should be KEPT (not in scope)"
        );
    }

    #[test]
    fn test_scope_restriction_removes_div() {
        // <div> IS in the allowed scope
        let mut root = parse_html("<div class=\"sidebar\">side content</div>").unwrap();
        walk_pre_mut(&mut root, &|n| tf_remove_unlikely_candidates(n));
        assert!(
            !find_tag(&root, "div"),
            "<div class='sidebar'> should be REMOVED (in scope)"
        );
    }

    #[test]
    fn test_scope_restriction_nested_parent_kept_child_removed() {
        // Parent <a> not in scope, child <div> in scope
        let mut root =
            parse_html("<a class=\"sidebar\"><div class=\"sidebar\">text</div></a>").unwrap();
        walk_pre_mut(&mut root, &|n| tf_remove_unlikely_candidates(n));
        assert!(
            find_tag(&root, "a"),
            "<a> parent should be KEPT (not in scope)"
        );
        assert!(
            !find_tag(&root, "div"),
            "<div> child should be REMOVED (in scope)"
        );
    }

    #[test]
    fn test_scope_restriction_keeps_li() {
        // <li> is NOT in the allowed scope
        let mut root = parse_html("<li class=\"sidebar\">list item</li>").unwrap();
        walk_pre_mut(&mut root, &|n| tf_remove_unlikely_candidates(n));
        assert!(
            find_tag(&root, "li"),
            "<li class='sidebar'> should be KEPT (not in scope)"
        );
    }

    // ── Gap 2: Separate pattern tests ─────────────────────────────────────

    #[test]
    fn test_separate_patterns_id_premium() {
        // ID-only pattern matches "premium"
        let mut root = parse_html("<div id=\"premium-content\">premium</div>").unwrap();
        walk_pre_mut(&mut root, &|n| tf_remove_unlikely_candidates(n));
        assert!(
            !find_tag(&root, "div"),
            "<div id='premium-content'> should be REMOVED (id pattern)"
        );
    }

    #[test]
    fn test_separate_patterns_class_footer() {
        // Class-only pattern matches "footer"
        let mut root = parse_html("<div class=\"footer\">footer</div>").unwrap();
        walk_pre_mut(&mut root, &|n| tf_remove_unlikely_candidates(n));
        assert!(
            !find_tag(&root, "div"),
            "<div class='footer'> should be REMOVED (class pattern)"
        );
    }

    #[test]
    fn test_separate_patterns_class_share_contains() {
        // Class-only pattern matches "share-" (substring match)
        let mut root = parse_html("<div class=\"share-icons\">share</div>").unwrap();
        walk_pre_mut(&mut root, &|n| tf_remove_unlikely_candidates(n));
        assert!(
            !find_tag(&root, "div"),
            "<div class='share-icons'> should be REMOVED (class pattern share-)"
        );
    }

    #[test]
    fn test_separate_patterns_id_share_only() {
        // ID-only pattern matches "share"
        let mut root = parse_html("<div id=\"share-buttons\">share</div>").unwrap();
        walk_pre_mut(&mut root, &|n| tf_remove_unlikely_candidates(n));
        assert!(
            !find_tag(&root, "div"),
            "<div id='share-buttons'> should be REMOVED (id pattern share)"
        );
    }

    #[test]
    fn test_separate_patterns_shared_sidebar() {
        // Shared pattern matches "sidebar" (ACLU regression scenario)
        let mut root = parse_html(
            "<div class=\"panel-two-col-sidebar-right-mix\"><p>article content here</p></div>",
        )
        .unwrap();
        walk_pre_mut(&mut root, &|n| tf_remove_unlikely_candidates(n));
        assert!(
            !find_tag(&root, "div"),
            "<div class='panel-two-col-sidebar-right-mix'> should be REMOVED (shared sidebar)"
        );
    }

    #[test]
    fn test_role_nav_check() {
        // Trafilatura's exact role check: contains "nav"
        let mut root = parse_html("<div role=\"navigation\">nav</div>").unwrap();
        walk_pre_mut(&mut root, &|n| tf_remove_unlikely_candidates(n));
        assert!(
            !find_tag(&root, "div"),
            "<div role='navigation'> should be REMOVED (role contains nav)"
        );
    }

    #[test]
    fn test_role_nav_check_non_matching() {
        // Non-matching role should NOT trigger removal
        let mut root = parse_html("<div role=\"main\"><p>content</p></div>").unwrap();
        walk_pre_mut(&mut root, &|n| tf_remove_unlikely_candidates(n));
        assert!(
            find_tag(&root, "div"),
            "<div role='main'> should be KEPT (no nav match)"
        );
    }

    // ── Pattern 2: scope-unrestricted discard ──────────────────────

    #[test]
    fn test_pattern2_noprint_class_removed() {
        let mut root =
            parse_html("<section class=\"top-article noprint\">nav stuff</section><p>content</p>")
                .unwrap();
        walk_pre_mut(&mut root, &|n| tf_remove_unlikely_candidates(n));
        assert!(
            !find_tag(&root, "section"),
            "section with noprint class should be removed"
        );
    }

    #[test]
    fn test_pattern2_scope_unrestricted_catches_any_tag() {
        // Pattern 2 catches <figure> (not in Pattern 1 scope) with noprint
        let mut root = parse_html("<figure class=\"noprint\">fig</figure><p>content</p>").unwrap();
        walk_pre_mut(&mut root, &|n| tf_remove_unlikely_candidates(n));
        assert!(
            !find_tag(&root, "figure"),
            "figure with noprint should be removed (unrestricted)"
        );
    }

    #[test]
    fn test_pattern2_hide_class_removed() {
        let mut root = parse_html("<div class=\"hide-ads\">ads</div><p>content</p>").unwrap();
        walk_pre_mut(&mut root, &|n| tf_remove_unlikely_candidates(n));
        assert!(
            !find_tag(&root, "div"),
            "div with hide- class should be removed"
        );
    }

    #[test]
    fn test_pattern2_notloaded_removed() {
        let mut root = parse_html("<div class=\"notloaded\">lazy</div><p>content</p>").unwrap();
        walk_pre_mut(&mut root, &|n| tf_remove_unlikely_candidates(n));
        assert!(
            !find_tag(&root, "div"),
            "div with notloaded class should be removed"
        );
    }

    #[test]
    fn test_pattern2_akismet_id_removed() {
        let mut root = parse_html("<div id=\"akismet\">spam</div><p>content</p>").unwrap();
        walk_pre_mut(&mut root, &|n| tf_remove_unlikely_candidates(n));
        assert!(
            !find_tag(&root, "div"),
            "div with akismet id should be removed"
        );
    }

    #[test]
    fn test_pattern2_reply_prefix_removed() {
        let mut root =
            parse_html("<div class=\"reply-comment-123\">reply form</div><p>content</p>").unwrap();
        walk_pre_mut(&mut root, &|n| tf_remove_unlikely_candidates(n));
        assert!(
            !find_tag(&root, "div"),
            "div with reply- class should be removed"
        );
    }

    #[test]
    fn test_pattern2_class_pattern_does_not_match_id() {
        // REGRESSION: SLOP-001 — class-only patterns (noprint, hide-, reply-)
        // must NOT match against id values.
        let mut root =
            parse_html("<div id=\"noprint\">should be kept</div><p>content</p>").unwrap();
        walk_pre_mut(&mut root, &|n| tf_remove_unlikely_candidates(n));
        assert!(
            find_tag(&root, "div"),
            "div with id='noprint' should be KEPT (class-only pattern)"
        );
    }

    #[test]
    fn test_pattern2_hidden_id_removed() {
        let mut root =
            parse_html("<div id=\"hidden-content\">hidden div</div><p>content</p>").unwrap();
        walk_pre_mut(&mut root, &|n| tf_remove_unlikely_candidates(n));
        assert!(
            !find_tag(&root, "div"),
            "div with 'hidden' in id should be removed"
        );
    }

    #[test]
    fn test_pattern2_hidden_in_style_removed() {
        let mut root =
            parse_html("<div style=\"visibility:hidden\">hidden</div><p>content</p>").unwrap();
        walk_pre_mut(&mut root, &|n| tf_remove_unlikely_candidates(n));
        assert!(
            !find_tag(&root, "div"),
            "div with 'hidden' in style should be removed"
        );
    }

    #[test]
    fn test_pattern2_comments_title_removed() {
        let mut root =
            parse_html("<div class=\"comments-title\">comments</div><p>content</p>").unwrap();
        walk_pre_mut(&mut root, &|n| tf_remove_unlikely_candidates(n));
        assert!(
            !find_tag(&root, "div"),
            "div with comments-title class should be removed"
        );
    }

    #[test]
    fn test_pattern2_suggest_links_removed() {
        let mut root =
            parse_html("<div class=\"suggest-links\">suggest</div><p>content</p>").unwrap();
        walk_pre_mut(&mut root, &|n| tf_remove_unlikely_candidates(n));
        assert!(
            !find_tag(&root, "div"),
            "div with suggest-links class should be removed"
        );
    }

    #[test]
    fn test_pattern2_preserves_body_html() {
        let mut root = parse_html("<html lang=\"en\"><body><p>content</p></body></html>").unwrap();
        walk_pre_mut(&mut root, &|n| tf_remove_unlikely_candidates(n));
        assert!(find_tag(&root, "html"), "<html> should never be removed");
        assert!(find_tag(&root, "body"), "<body> should never be removed");
    }

    #[test]
    fn test_pattern2_aria_hidden_structural_preserved() {
        let mut root = parse_html("<main aria-hidden=\"true\"><p>main content</p></main>").unwrap();
        walk_pre_mut(&mut root, &|n| tf_remove_unlikely_candidates(n));
        assert!(
            find_tag(&root, "main"),
            "<main aria-hidden='true'> should be preserved (structural guard)"
        );
    }

    #[test]
    fn test_pattern2_aria_hidden_nonstructural_removed() {
        let mut root =
            parse_html("<div aria-hidden=\"true\">hidden div</div><p>content</p>").unwrap();
        walk_pre_mut(&mut root, &|n| tf_remove_unlikely_candidates(n));
        assert!(
            !find_tag(&root, "div"),
            "<div aria-hidden='true'> should be removed"
        );
    }

    #[test]
    fn test_pattern2_preserves_pattern1_sidebar() {
        // Pattern 1 still works: sidebar in class
        let mut root =
            parse_html("<div class=\"panel-two-col-sidebar-right-mix\"><p>content</p></div>")
                .unwrap();
        walk_pre_mut(&mut root, &|n| tf_remove_unlikely_candidates(n));
        assert!(
            !find_tag(&root, "div"),
            "Pattern 1 sidebar removal should still work"
        );
    }
}
