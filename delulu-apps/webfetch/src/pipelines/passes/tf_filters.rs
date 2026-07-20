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
#[path = "tf_filters_test.rs"]
mod tests;
