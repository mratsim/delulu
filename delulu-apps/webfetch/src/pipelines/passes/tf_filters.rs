use once_cell::sync::Lazy;
use regex::Regex;
use std::collections::HashMap;

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

/// Extract content from `<script type="text/template">` elements before they are removed.
///
/// Some websites (e.g., Google Blogger) embed article HTML inside `<script type='text/template'>`
/// elements. Since `"script"` is in `TF_CLEANED_TAGS`, these elements would be removed entirely
/// by `tf_remove_cleaned`, losing the article content.
///
/// This function runs BEFORE `tf_remove_cleaned` and replaces each matching `<script>` element
/// with a `<div>` containing the script's text content as a text node. The content is then
/// preserved and processed by subsequent pipeline passes.
///
/// Matches `<script>` elements whose `type` attribute contains "template" (case-insensitive),
/// e.g., `type="text/template"`, `type="text/template-picture"`, etc.
///
/// Uses manual recursive traversal (similar to `tf_strip_unwrapped`) because elements need to
/// be replaced with new ones.
/// Note: No direct Python trafilatura equivalent — Rust-specific.
pub fn tf_extract_script_templates(node: &mut DomNode) {
    fn extract_inner(nodes: &mut Vec<DomNode>) {
        let mut i = 0;
        while i < nodes.len() {
            match &mut nodes[i] {
                DomNode::Element {
                    tag,
                    attrs,
                    children,
                    ..
                } if tag == "script" => {
                    // Check if type attribute contains "template" (case-insensitive)
                    let is_template = attrs.iter().any(|(k, v)| {
                        k.eq_ignore_ascii_case("type")
                            && v.to_ascii_lowercase().contains("template")
                    });
                    if is_template {
                        // Extract text content from the script element's children
                        let text_content: String =
                            children.iter().map(DomNode::text_content).collect();
                        // Replace the <script> with a <div> containing the text
                        let new_div = DomNode::Element {
                            tag: "div".to_string(),
                            attrs: vec![],
                            children: vec![DomNode::Text(text_content)],
                            scores: HashMap::new(),
                            metadata: HashMap::new(),
                        };
                        nodes[i] = new_div;
                        // Don't recurse into the new div (it has only a text child).
                        // Increment i to continue with next sibling.
                        i += 1;
                    } else {
                        // Regular <script> (JavaScript) — recurse into children then continue
                        extract_inner(children);
                        i += 1;
                    }
                }
                DomNode::Element { children, .. } => {
                    extract_inner(children);
                    i += 1;
                }
                _ => i += 1,
            }
        }
    }
    if let DomNode::Element { children, .. } = node {
        extract_inner(children);
    }
}

/// Remove elements whose tag is in the `MANUALLY_CLEANED` list.
///
/// Returns `WalkerAction::Remove` if the node's tag is in `TF_CLEANED_TAGS`,
/// `WalkerAction::Continue` otherwise.
/// Reference: Trafilatura `htmlprocessing.py:47-79` `tree_cleaning()`
pub fn tf_remove_cleaned(node: &mut DomNode) -> WalkerAction {
    match node {
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
/// Reference: Trafilatura `xpaths.py:156-163` `TEASER_DISCARD_XPATH`
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
            // Protect content containers: skip removal if id matches article content patterns
            let is_content = attrs.iter().any(|(k, v)| {
                (k == "id" || k == "class") && BODY_XPATH_PATTERN_0_RE.is_match(v.as_str())
            });
            if has_teaser && !is_content {
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
        "(?i)^shar|viral|social|syndication|newsletter|cookie|tags|\\bsidebar\\b|banner|bread-?crumb|author|button"
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
        "(?i)^(?:nav|post-nav|ZendeskForm)| ad |footer|Footer|byline|Byline|elated|share-|sociable|embedded|embed|subnav|tag-list|\\bbar\\b|avigation|navbar|navbox|rating|(?:^| )widget(?: |$)|attachment|timestamp|user-info|user-profile|-ad-|-icon|article-infos|nfoline|outbrain|taboola|criteo|options|expand|consent|modal-content|permission|next-|-stories|most-popular|mol-factbox|message-container|yin|zlylin|xg1|slide|viewport|overlay|paid-?content|obfuscated|blurred"
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
        "(?i)^hide-|^reply-|comments-title|nocomments|-reply-|(?:^| )message(?:[^- ]|$)|akismet|suggest-links|-hide-|hide-print| hidden| hide|noprint|notloaded").expect("invalid OVERALL_DISCARD_P2_CLASS_RE")
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
/// Reference: Trafilatura `htmlprocessing.py:92-109` `prune_unwanted_nodes(tree, OVERALL_DISCARD_XPATH)`
#[cfg(not(feature = "use-xpath"))]
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
/// Reference: Trafilatura `htmlprocessing.py:63` `strip_tags(tree, stripping_list)` (MANUALLY_STRIPPED)
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
/// Reference: Trafilatura `htmlprocessing.py:82-89` `prune_html()` (CUT_EMPTY_ELEMS)
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
// LINK DENSITY FILTER — port of Readability's remove_high_link_density
// ---------------------------------------------------------------------------

/// Remove elements with high link density.
///
/// For each element with tag "table", "ul", "div", "form", or "fieldset",
/// computes `link_text_len / total_text_len` across its descendants.
/// If the ratio exceeds 0.5, the element is removed.
///
/// This is a port of Readability's `remove_high_link_density` adapted for
/// the Trafilatura pipeline:
/// - Uses `walk_pre_mut` instead of `walk_post_acc_mut` (no data table analysis)
/// - No metadata scoring infrastructure
/// - Simpler threshold (0.5 vs Readability's 0.333 with comma gate)
///
/// Pre-condition: MANUALLY_CLEANED tags have been removed by `tf_remove_cleaned`.
/// Post-condition: Elements whose link density exceeds 0.5 are removed.
///
/// Reference: Trafilatura `htmlprocessing.py:183-206` `delete_by_link_density()`
pub fn tf_filter_by_link_density(node: &mut DomNode) -> WalkerAction {
    match node {
        DomNode::Element { tag, .. }
            if matches!(tag.as_str(), "table" | "ul" | "div" | "form" | "fieldset") =>
        {
            // Compute total and link text length in a single traversal
            let (total_text_len, link_text_len) = node.link_density_stats();

            if total_text_len == 0 {
                return WalkerAction::Continue;
            }

            let link_density = link_text_len as f64 / total_text_len as f64;

            if link_density > 0.5 {
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

// ---------------------------------------------------------------------------
// Pass-level helpers: paragraph recovery, JSON-LD extraction, text measurement
// ---------------------------------------------------------------------------

/// Collect all `<p>` element subtrees from a DOM tree,
/// skipping `<p>` elements inside boilerplate containers.
///
/// Pre: DOM tree is fully parsed.
/// Post: All `<p>` element subtrees outside boilerplate containers are collected.
///
/// Note: This function is recursive. Stack overflow may occur on DOM trees deeper than ~1000 nodes.
///
/// Reference: Trafilatura `recover_wild_text()` in `main_extractor.py:536-560` (partial — collects `<p>` elements)
pub(crate) fn collect_p_elements(node: &DomNode, result: &mut Vec<DomNode>) {
    match node {
        // Skip boilerplate containers entirely
        DomNode::Element { tag, .. }
            if matches!(tag.as_str(), "nav" | "footer" | "header" | "form") =>
        {
            // Don't descend into boilerplate containers
        }
        DomNode::Element { tag, children, .. } if tag == "p" => {
            result.push(node.clone());
        }
        DomNode::Element { children, .. } => {
            for child in children {
                collect_p_elements(child, result);
            }
        }
        _ => {}
    }
}

/// Recover `<p>` elements from a backup tree that aren't already in the current node.
/// This is a simplified version of Python's recover_wild_text().
///
/// Pre: `node` is the current DOM tree, `backup` is the original tree,
///      `existing_text` is the concatenated text of `node`.
/// Post: `<p>` elements from `backup` whose text is not in `existing_text`
///       are added to `node`.
///
/// Note: This function is recursive (via `collect_p_elements`). Stack overflow may occur on DOM trees deeper than ~1000 nodes.
///
/// Reference: Trafilatura `recover_wild_text()` in `main_extractor.py:536-560`
pub(crate) fn recover_wild_p_elements(node: &mut DomNode, backup: &DomNode, existing_text: &str) {
    // Collect all <p> element text from the backup tree
    let mut recovered: Vec<DomNode> = Vec::new();
    collect_p_elements(backup, &mut recovered);

    // Add recovered <p> elements that aren't already in the current tree
    if let DomNode::Element { children, .. } = node {
        for p_node in recovered {
            let p_text = p_node.text_content();
            if !p_text.trim().is_empty() && !existing_text.contains(&p_text) {
                children.push(p_node);
            }
        }
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
    if matches!(&current[last_idx], DomNode::Element { .. }) {
        let (p_text, total_text) = current[last_idx].text_stats();
        if p_text >= MIN_EXTRACTED_SIZE || total_text >= MIN_EXTRACTED_SIZE {
            return true;
        }
    }
    false
}

// ---------------------------------------------------------------------------
// BODY_XPATH container isolation
// ---------------------------------------------------------------------------

/// Regex for Pattern 0: specific class/id/role selectors.
/// Maps to Trafilatura's BODY_XPATH Pattern 0 (specific class/id selectors).
static BODY_XPATH_PATTERN_0_RE: once_cell::sync::Lazy<regex::Regex> = once_cell::sync::Lazy::new(
    || {
        regex::Regex::new(
        r#"(?ix)(?:
            post[-_]text|post-body|post-?entry|post[-_]?content|postContent|post_inner_wrapper|
            article-?text|articleText|article[-_]?content|article[-_]?maincontent|(?:entry|page|text|article|art)-content|article__content|
            article(?:-|__)?body|articleBody|ArticleContent|body-text|article__container|
            (?:entry|article|art)-content|article__content|article(?:-|__)?body|articleBody|body-text
        )"#,
    )
    .expect("BODY_XPATH_PATTERN_0_RE: invalid regex")
    },
);

/// Regex for Pattern 2: content class/id patterns.
/// Maps to Trafilatura's BODY_XPATH Pattern 2 (content class/id).
pub static BODY_XPATH_PATTERN_2_RE: once_cell::sync::Lazy<regex::Regex> =
    once_cell::sync::Lazy::new(|| {
        regex::Regex::new(
        r#"(?i)^(?:content[-_]main|content(?:-|__)?body|contentBody|main-content|page-content)"#,
    )
    .expect("BODY_XPATH_PATTERN_2_RE: invalid regex")
    });

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
/// Reference: Trafilatura `main_extractor.py:597-647` `_extract()` (BODY_XPATH iteration)
pub fn tf_isolate_content_container(node: &mut DomNode) {
    if let DomNode::Element { children, .. } = node {
        let mut best_path: Option<Vec<usize>> = None;
        const PATTERN_CHECKS: [fn(&str, &str, &str, &str, &str) -> bool; 5] = [
            |_tag, cv, iv, rv, ipv| {
                ipv == "articleBody"
                    || iv == "articleContent"
                    || matches!(cv, "post" | "entry")
                    || rv == "article"
                    || BODY_XPATH_PATTERN_0_RE.is_match(cv)
                    || BODY_XPATH_PATTERN_0_RE.is_match(iv)
                    || cv.contains("p-body-pageContent")
                    || iv.contains("p-body-pageContent")
            },
            |tag, _, _, _, _| matches!(tag, "article" | "main"),
            |_, cv, _iv, _, _| {
                matches!(
                    cv,
                    "postarea" | "art-postcontent" | "text" | "cell" | "story"
                )
            },
            |_, cv, iv, _, _| {
                iv == "content"
                    || cv == "content"
                    || BODY_XPATH_PATTERN_2_RE.is_match(cv)
                    || BODY_XPATH_PATTERN_2_RE.is_match(iv)
                    || cv.contains("main-content")
                    || cv.contains("page-content")
            },
            |tag, cv, iv, rv, _| {
                tag == "main"
                    || cv.starts_with("main")
                    || iv.starts_with("main")
                    || rv.starts_with("main")
            },
        ];
        for check in PATTERN_CHECKS {
            // Collect ALL matches for this pattern (all siblings, all depths)
            let mut all_paths: Vec<Vec<usize>> = Vec::new();
            find_all_matches(children, &check, &mut all_paths, &mut Vec::new());
            // Try each match in document order; accept the first with enough content
            for path in &all_paths {
                if container_has_content(children, path) {
                    best_path = Some(path.clone());
                    break;
                }
            }
            if best_path.is_some() {
                break;
            }
            // No match with enough content → try next pattern
        }
        if let Some(path) = best_path {
            apply_path(children, &path);
        }
    }
}

/// Find the FIRST element matching a BODY_XPATH pattern in document order.
/// Unlike the original implementation, this does NOT check the content threshold —
/// it returns the first match regardless of `<p>` text count.
/// The caller (`tf_isolate_content_container`) separately checks content.
#[allow(dead_code)]
fn find_first_match(
    nodes: &[DomNode],
    check: &fn(&str, &str, &str, &str, &str) -> bool,
    path: &mut Vec<usize>,
) -> bool {
    for (i, node) in nodes.iter().enumerate() {
        if let DomNode::Element {
            tag,
            attrs,
            children,
            ..
        } = node
            && matches!(tag.as_str(), "article" | "div" | "main" | "section")
        {
            let cv = attrs
                .iter()
                .find(|(k, _)| k == "class")
                .map(|(_, v)| v.as_str())
                .unwrap_or("");
            let iv = attrs
                .iter()
                .find(|(k, _)| k == "id")
                .map(|(_, v)| v.as_str())
                .unwrap_or("");
            let rv = attrs
                .iter()
                .find(|(k, _)| k == "role")
                .map(|(_, v)| v.as_str())
                .unwrap_or("");
            let ipv = attrs
                .iter()
                .find(|(k, _)| k == "itemprop")
                .map(|(_, v)| v.as_str())
                .unwrap_or("");
            if check(tag.as_str(), cv, iv, rv, ipv) {
                path.push(i);
                return true;
            }
            path.push(i);
            if find_first_match(children, check, path) {
                return true;
            }
            path.pop();
        } else if let DomNode::Element { children, .. } = node {
            path.push(i);
            if find_first_match(children, check, path) {
                return true;
            }
            path.pop();
        }
    }
    false
}

/// Collect ALL elements matching a BODY_XPATH pattern in document order.
/// Unlike `find_first_match` which returns the first match, this collects
/// ALL matches (all siblings at all depths) so the caller can try each one
/// for content threshold before falling through to the next pattern.
/// This fixes sibling fallthrough: when two siblings both match Pattern 0 but
/// the first has insufficient content, we should try the second sibling
/// before moving to Pattern 1.
fn find_all_matches(
    nodes: &[DomNode],
    check: &fn(&str, &str, &str, &str, &str) -> bool,
    results: &mut Vec<Vec<usize>>,
    current_path: &mut Vec<usize>,
) {
    for (i, node) in nodes.iter().enumerate() {
        current_path.push(i);
        if let DomNode::Element {
            tag,
            attrs,
            children,
            ..
        } = node
            && matches!(tag.as_str(), "article" | "div" | "main" | "section")
        {
            let cv = attrs
                .iter()
                .find(|(k, _)| k == "class")
                .map(|(_, v)| v.as_str())
                .unwrap_or("");
            let iv = attrs
                .iter()
                .find(|(k, _)| k == "id")
                .map(|(_, v)| v.as_str())
                .unwrap_or("");
            let rv = attrs
                .iter()
                .find(|(k, _)| k == "role")
                .map(|(_, v)| v.as_str())
                .unwrap_or("");
            let ipv = attrs
                .iter()
                .find(|(k, _)| k == "itemprop")
                .map(|(_, v)| v.as_str())
                .unwrap_or("");
            if check(tag.as_str(), cv, iv, rv, ipv) {
                results.push(current_path.clone());
            }
            find_all_matches(children, check, results, current_path);
        } else if let DomNode::Element { children, .. } = node {
            find_all_matches(children, check, results, current_path);
        }
        current_path.pop();
    }
}

fn apply_path(nodes: &mut Vec<DomNode>, path: &[usize]) {
    // Isolate the matched container by removing siblings at each level
    // from the root down to the parent of the matched container.
    // At the deepest level (path.len() == 1): remove siblings of the matched container.
    // At intermediate levels: remove siblings of the path container.
    if path.is_empty() {
        return;
    }
    let idx = path[0];
    if path.len() == 1 {
        let matched = nodes.remove(idx);
        nodes.clear();
        nodes.push(matched);
    } else if let DomNode::Element { children, .. } = &mut nodes[idx] {
        apply_path(children, &path[1..]);
        let matched = nodes.remove(idx);
        nodes.clear();
        nodes.push(matched);
    }
}

// ---------------------------------------------------------------------------
// BODY_XPATH FALLBACK — heuristic container isolation when patterns fail
// ---------------------------------------------------------------------------

/// Fallback content container isolation for when BODY_XPATH patterns don't match.
///
/// When `tf_isolate_content_container` finds no matching container (tree still has
/// multiple top-level children), this function uses a heuristic:
///
/// 1. Check if the root has multiple top-level children.
/// 2. If so, find the child with the most `<p>` text content (using `text_stats`).
/// 3. If that child has at least `MIN_EXTRACTED_SIZE` chars of `<p>` text, isolate
///    it by discarding all sibling nodes.
/// 4. If no child has enough `<p>` text, use a secondary fallback: select the child
///    with the most total text content (`text_len`). This handles
///    CMS layouts that use `<div>`-based content without `<p>` tags (e.g., Webflow,
///    headless CMS, table-based layouts).
///
/// This handles Webflow-generated class names, table-based layouts, and nonstandard
/// HTML where BODY_XPATH patterns (which match specific class/id patterns like
/// "post", "article", "content", "main") cannot identify the main container.
///
/// Must run AFTER `tf_isolate_content_container` in the pipeline.
///
/// Pre: `tf_isolate_content_container` has already run (may have been a no-op).
/// Post: If a suitable container was found via heuristic, only that container's
///       subtree survives. Otherwise, the tree is unchanged.
/// Note: No direct Python trafilatura equivalent — Rust-specific heuristic.
pub fn tf_fallback_content_container(node: &mut DomNode) {
    if let DomNode::Element { children, .. } = node {
        // Only apply if there are multiple top-level children
        // (container isolation was a no-op or only partially effective)
        if children.len() <= 1 {
            return;
        }

        // Find the child with the most <p> text content
        let mut best_idx = None;
        let mut best_p_len = 0usize;

        for (i, child) in children.iter().enumerate() {
            let p_len = child.text_stats().0;
            if p_len > best_p_len {
                best_p_len = p_len;
                best_idx = Some(i);
            }
        }

        // If the best child has enough <p> text, isolate it
        if let Some(idx) = best_idx
            && best_p_len >= MIN_EXTRACTED_SIZE
        {
            let matched = children.remove(idx);
            children.clear();
            children.push(matched);
        } else {
            // Secondary fallback: use total text content (text_len)
            // when no child has enough <p> text. This handles CMS layouts
            // that use <div>-based content without <p> tags (e.g., Webflow,
            // headless CMS, table-based layouts).
            let mut best_text_idx = None;
            let mut best_text_len = 0usize;

            for (i, child) in children.iter().enumerate() {
                let text_len = child.text_len();
                if text_len > best_text_len {
                    best_text_len = text_len;
                    best_text_idx = Some(i);
                }
            }

            if let Some(idx) = best_text_idx
                && best_text_len >= MIN_EXTRACTED_SIZE
            {
                let matched = children.remove(idx);
                children.clear();
                children.push(matched);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// TAG_CATALOG — whitelist of allowed output tags
// ---------------------------------------------------------------------------

/// Set of tags allowed in the final tf output after all conversions.
///
/// Maps to Trafilatura's `TAG_CATALOG` (settings.py:462):
/// ```python
/// TAG_CATALOG = frozenset(["blockquote", "code", "del", "head", "hi", "lb",
///                          "list", "p", "pre", "quote"])
/// ```
///
/// Elements whose tag is NOT in this catalog are removed, with the following
/// exceptions preserved:
/// - `<item>` (converted from `<li>` by `tf_convert_lists`)
/// - `<ref>` (converted from `<a>` by `tf_convert_refs_and_details`)
/// - `<graphic>` (image references)
/// - `<html>`, `<body>` (structural root tags)
///
/// Text nodes, comments, and doctypes are preserved (left untouched).
///
/// Must run as the LAST pass in the pipeline, after all tag conversions and
/// canonicalization passes have completed.
///
/// Pre: All tag conversions (headings, lists, quotes, formatting, breaks,
///      refs) and canonicalization passes have run.
/// Post: Elements with tags outside the allowed set are removed. Text nodes
///       and structural root elements are preserved.
/// Reference: Trafilatura `settings.py` `TAG_CATALOG`
pub fn tf_filter_tag_catalog(node: &mut DomNode) -> WalkerAction {
    match node {
        // Preserve non-element nodes (text, comments, doctypes)
        DomNode::Text(_) | DomNode::Comment(_) | DomNode::Doctype(_) => WalkerAction::Continue,
        // Check element tags
        DomNode::Element { tag, .. } => {
            // Tags in TAG_CATALOG — allowed
            if matches!(
                tag.as_str(),
                "blockquote"
                    | "code"
                    | "del"
                    | "head"
                    | "hi"
                    | "lb"
                    | "list"
                    | "p"
                    | "pre"
                    | "quote"
            ) {
                return WalkerAction::Continue;
            }
            // Additional preserved tags (converted or structural)
            if matches!(
                tag.as_str(),
                "item"
                    | "ref"
                    | "graphic"
                    | "html"
                    | "body"
                    | "table"
                    | "tr"
                    | "td"
                    | "th"
                    | "row"
                    | "cell"
            ) {
                return WalkerAction::Continue;
            }
            // All other element tags — replace with children to preserve text content
            // even inside custom/non-standard wrapper elements (e.g., <eop-in-viewport>)
            WalkerAction::ReplaceWithChildren
        }
    }
}

// ---------------------------------------------------------------------------
// DISCARD_IMAGE_ELEMENTS — remove elements whose id/class contains "caption"
// ---------------------------------------------------------------------------

/// Remove elements whose `id` or `class` attribute contains "caption".
///
/// Maps to Trafilatura's `DISCARD_IMAGE_ELEMENTS` (xpaths.py:179-186):
/// ```xpath
/// .//*[self::div or self::item or self::list or self::p or self::section or self::span][
///   contains(@id, 'caption') or contains(@class, 'caption')]
/// ```
///
/// This implementation matches `div`, `p`, `section`, `span`, `item`, and `list`
/// elements whose `class` or `id` attribute contains "caption" (case-insensitive).
/// These are typically image captions or figure captions that should be
/// discarded in text-only extraction.
///
/// The `item` and `list` tags are included because `tf_discard_image_elements`
/// runs at pipeline step 20, AFTER `tf_convert_lists` (step 10), so converted
/// `<li>` and `<ul>`/`<ol>` elements exist as `item` and `list` at this point.
///
/// Place this pass after canonicalization passes but before TAG_CATALOG
/// filtering in the pipeline.
///
/// Pre: DOM tree is fully parsed, all tag conversions have run.
/// Post: Elements with "caption" in their class or id are removed.
/// Reference: Trafilatura `xpaths.py:179-186` `DISCARD_IMAGE_ELEMENTS`
#[cfg(not(feature = "use-xpath"))]
pub fn tf_discard_image_elements(node: &mut DomNode) -> WalkerAction {
    match node {
        DomNode::Element { tag, attrs, .. }
            if matches!(
                tag.as_str(),
                "div" | "p" | "section" | "span" | "item" | "list"
            ) =>
        {
            let has_caption = attrs.iter().any(|(key, val)| {
                matches!(key.as_str(), "class" | "id")
                    && val.to_ascii_lowercase().contains("caption")
            });
            if has_caption {
                WalkerAction::Remove
            } else {
                WalkerAction::Continue
            }
        }
        _ => WalkerAction::Continue,
    }
}
// ---------------------------------------------------------------------------
// XPath-based discard passes (Phase 2) — gated behind use-xpath feature
// ---------------------------------------------------------------------------

#[cfg(feature = "use-xpath")]
use crate::pipelines::dom_xpath::XPath;

/// Remove teaser elements using XPath (use-xpath feature).
///
/// Pre: DOM tree is fully parsed, cleaned tags already removed.
/// Post: Elements matching TEASER_DISCARD_XPATH are removed.
/// Reference: Trafilatura `xpaths.py:156-163` `TEASER_DISCARD_XPATH`
#[cfg(feature = "use-xpath")]
pub fn tf_remove_teaser_xpath(node: &mut DomNode) -> WalkerAction {
    match node {
        DomNode::Element { tag, attrs, .. }
            if matches!(
                tag.as_str(),
                "div" | "item" | "list" | "p" | "section" | "span"
            ) =>
        {
            // Check `class` and `id` independently (matches Trafilatura's
            // `contains(translate(@id,'T','t'),'teaser') or contains(translate(@class,'T','t'),'teaser')`
            // and the manual twin `tf_remove_teaser`). A teaser in either attribute
            // is enough to discard.
            let has_teaser = attrs.iter().any(|(key, val)| {
                matches!(key.as_str(), "class" | "id")
                    && val.to_ascii_lowercase().contains("teaser")
            });
            // Protect content containers: skip removal if class or id matches article content patterns
            let is_content = attrs.iter().any(|(k, v)| {
                (k == "id" || k == "class") && BODY_XPATH_PATTERN_0_RE.is_match(v.as_str())
            });
            if has_teaser && !is_content {
                WalkerAction::Remove
            } else {
                WalkerAction::Continue
            }
        }
        _ => WalkerAction::Continue,
    }
}

/// Remove unlikely candidates using direct attribute checks (use-xpath feature).
///
/// Uses the same attribute-checking logic as the manual `tf_remove_unlikely_candidates`
/// instead of XPath eval, to avoid the descendant-traversal bug where parent containers
/// are removed when any descendant matches a discard pattern.
///
/// Pre: DOM tree is fully parsed, cleaned tags already removed.
/// Post: Elements matching OVERALL_DISCARD_XPATH patterns are removed.
/// Reference: Trafilatura `xpaths.py:118-148` `OVERALL_DISCARD_XPATH`
#[cfg(feature = "use-xpath")]
pub fn tf_remove_unlikely_candidates_xpath(node: &mut DomNode) -> WalkerAction {
    match node {
        DomNode::Element { tag, attrs, .. } => {
            // Never strip <html>, <body>, <head>, <base>.
            if matches!(tag.as_str(), "html" | "body" | "head" | "base") {
                return WalkerAction::Continue;
            }

            // === Pattern 2: Scope-unrestricted discard (checks ALL elements) ===
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

            let has_display_none = {
                let cleaned: String = style_val.chars().filter(|c| !c.is_whitespace()).collect();
                cleaned.to_lowercase().contains("display:none")
            };
            let hidden_in_style = style_val.to_ascii_lowercase().contains("hidden");

            let p2_class_match = OVERALL_DISCARD_P2_CLASS_RE.is_match(class_val);
            let p2_id_match = OVERALL_DISCARD_P2_ID_RE.is_match(id_val);

            let structural_tag = matches!(tag.as_str(), "main" | "article" | "section" | "body");
            let p2_aria_hidden = !structural_tag && is_aria_hidden;

            let p2_removal = p2_class_match
                || p2_id_match
                || hidden_in_style
                || has_display_none
                || p2_aria_hidden;

            if p2_removal {
                return WalkerAction::Remove;
            }

            // === Pattern 1: Scope-restricted discard (only div/item/list/p/section/span) ===
            if !matches!(
                tag.as_str(),
                "div" | "item" | "list" | "p" | "section" | "span"
            ) {
                return WalkerAction::Continue;
            }

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

            let p1_aria_hidden = !structural_tag && is_aria_hidden;
            let attr_match =
                p1_aria_hidden || has_display_none || has_lp_content || has_most_popular;

            if OVERALL_DISCARD_SHARED_RE.is_match(class_val)
                || OVERALL_DISCARD_SHARED_RE.is_match(id_val)
                || OVERALL_DISCARD_ID_RE.is_match(id_val)
                || OVERALL_DISCARD_CLASS_RE.is_match(class_val)
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

/// Discard image elements using direct attribute checks (use-xpath feature).
///
/// Uses the same attribute-checking logic as the manual `tf_discard_image_elements`
/// instead of XPath eval, to avoid the descendant-traversal bug.
///
/// Pre: DOM tree is fully parsed, all tag conversions have run.
/// Post: Elements with "caption" in their class or id are removed.
/// Reference: Trafilatura `xpaths.py:179-186` `DISCARD_IMAGE_ELEMENTS`
#[cfg(feature = "use-xpath")]
pub fn tf_discard_image_elements_xpath(node: &mut DomNode) -> WalkerAction {
    match node {
        DomNode::Element { tag, attrs, .. }
            if matches!(
                tag.as_str(),
                "div" | "p" | "section" | "span" | "item" | "list"
            ) =>
        {
            let has_caption = attrs.iter().any(|(key, val)| {
                matches!(key.as_str(), "class" | "id")
                    && val.to_ascii_lowercase().contains("caption")
            });
            if has_caption {
                WalkerAction::Remove
            } else {
                WalkerAction::Continue
            }
        }
        _ => WalkerAction::Continue,
    }
}

// ---------------------------------------------------------------------------
// BODY_XPATH container isolation (Phase 3) — gated behind use-xpath feature
// ---------------------------------------------------------------------------

/// XPath expression for BODY_XPATH Pattern 0 (specific class/id/role selectors).
/// Reference: Trafilatura `xpaths.py:14-26` `BODY_XPATH[0]`
#[cfg(feature = "use-xpath")]
static BODY_XPATH_0: once_cell::sync::Lazy<XPath> = once_cell::sync::Lazy::new(|| {
    XPath::compile(".//*[self::article or self::div or self::main or self::section][@class='post' or @class='entry' or @itemprop='articleBody' or @id='articleContent' or re:test(@id, '(?:entry|article|art)-content|article__content|article(?:-|__)?body|articleBody|body-text') or re:test(@class, 'post[-_]text|post-body|post-?entry|post[-_]?content|postContent|post_inner_wrapper|article-?text|articleText|(?:entry|page|text|article|art)-content|article__content|article(?:-|__)?body|articleBody|ArticleContent|body-text|article__container')][1]").expect("BODY_XPATH_0: hardcoded expression must compile")
});

/// XPath expression for BODY_XPATH Pattern 1 (first article/main element).
/// Reference: Trafilatura `xpaths.py:27` `BODY_XPATH[1]`
#[cfg(feature = "use-xpath")]
static BODY_XPATH_1: once_cell::sync::Lazy<XPath> = once_cell::sync::Lazy::new(|| {
    XPath::compile("(.//article)[1]").expect("BODY_XPATH_1: hardcoded expression must compile")
});

/// XPath expression for BODY_XPATH Pattern 2 (role/article class/id selectors).
/// Reference: Trafilatura `xpaths.py:28-40` `BODY_XPATH[2]`
#[cfg(feature = "use-xpath")]
static BODY_XPATH_2: once_cell::sync::Lazy<XPath> = once_cell::sync::Lazy::new(|| {
    XPath::compile(".//*[self::article or self::div or self::main or self::section][@role='article' or @id='article' or @id='story' or @class='postarea' or @class='art-postcontent' or @class='text' or @class='cell' or @class='story' or re:test(@id, '^primary|story-body') or contains(translate(@class, 'FULTEX','fultex'), 'fulltext') or re:test(@class, '^article |post-bodycopy|story-?content|(?:theme|blog|section|single)-content|single-post|main-column|wpb_text_column|story-body|field-body')][1]").expect("BODY_XPATH_2: hardcoded expression must compile")
});

/// XPath expression for BODY_XPATH Pattern 3 (content class/id selectors).
/// Reference: Trafilatura `xpaths.py:41-52` `BODY_XPATH[3]`
#[cfg(feature = "use-xpath")]
static BODY_XPATH_3: once_cell::sync::Lazy<XPath> = once_cell::sync::Lazy::new(|| {
    XPath::compile(".//*[self::article or self::div or self::main or self::section][@id='content' or @class='content' or re:test(@id, 'content-main|content-body|contentBody') or re:test(@class, 'content[-_]main|content(?:-|__)?body') or contains(translate(@id, 'CM','cm'), 'main-content') or contains(translate(@class, 'CM','cm'), 'main-content') or contains(translate(@class, 'CP','cp'), 'page-content')][1]").expect("BODY_XPATH_3: hardcoded expression must compile")
});

/// XPath expression for BODY_XPATH Pattern 4 (main element with union fallback).
/// Reference: Trafilatura `xpaths.py:53-58` `BODY_XPATH[4]`
#[cfg(feature = "use-xpath")]
static BODY_XPATH_4: once_cell::sync::Lazy<XPath> = once_cell::sync::Lazy::new(|| {
    XPath::compile("(.//*[self::article or self::div or self::section][starts-with(@class, 'main') or starts-with(@id, 'main') or starts-with(@role, 'main')])[1]|(.//main)[1]").expect("BODY_XPATH_4: hardcoded expression must compile")
});

/// Isolate content container using XPath BODY_XPATH patterns (use-xpath feature).
///
/// Iterates BODY_XPATH[0-4] in order, evaluates each against the root node,
/// and isolates the first matching container (keeping only its subtree).
///
/// Pre: DOM tree is fully parsed, all cleaning passes have run.
/// Post: If a container matched any BODY_XPATH pattern, only that container's
///       subtree survives. Otherwise, the tree is unchanged.
/// Reference: Trafilatura `main_extractor.py:597-647` `_extract()` (BODY_XPATH iteration)
#[cfg(feature = "use-xpath")]
/// Find the ancestor index path from `nodes` down to the node at `ptr`.
/// On success, `path` is filled with the child index at each level from
/// `nodes` down to the matched node (the same convention `apply_path` expects).
fn find_node_path(nodes: &[DomNode], ptr: *const DomNode, path: &mut Vec<usize>) -> bool {
    for (i, node) in nodes.iter().enumerate() {
        if std::ptr::eq(node, ptr) {
            path.push(i);
            return true;
        }
        if let DomNode::Element { children, .. } = node {
            path.push(i);
            if find_node_path(children, ptr, path) {
                return true;
            }
            path.pop();
        }
    }
    false
}

/// Isolate content container using XPath BODY_XPATH patterns (use-xpath feature).
///
/// Iterates BODY_XPATH[0-4] in order, evaluates each against the root node,
/// and isolates the first matching container (keeping only its subtree).
///
/// Pre: DOM tree is fully parsed, all cleaning passes have run.
/// Post: If a container matched any BODY_XPATH pattern, only that container's
///       subtree survives. Otherwise, the tree is unchanged.
/// Reference: Trafilatura `main_extractor.py:597-647` `_extract()` (BODY_XPATH iteration)
#[cfg(feature = "use-xpath")]
pub fn tf_isolate_content_container_xpath(node: &mut DomNode) {
    let patterns: &[&once_cell::sync::Lazy<XPath>] = &[
        &BODY_XPATH_0,
        &BODY_XPATH_1,
        &BODY_XPATH_2,
        &BODY_XPATH_3,
        &BODY_XPATH_4,
    ];
    // Collect container pointers first to avoid borrow conflicts
    let mut container_ptr: Option<*const DomNode> = None;
    for pattern in patterns {
        match pattern.eval(node) {
            Ok(matched) if !matched.is_empty() => {
                if let Some(container) = matched.first() {
                    container_ptr = Some(*container as *const DomNode);
                    break;
                }
            }
            Ok(_) => {}
            Err(e) => {
                tracing::warn!("BODY_XPATH eval error: {:?}", e);
                return;
            }
        }
    }
    // Now isolate the container (separate from eval borrow). The XPath
    // expressions use `.//*`, so the matched container may be a descendant at
    // ANY depth. Record the full ancestor index path from `node` down to the
    // matched node, then prune siblings at every level via `apply_path` — the
    // same mechanism the manual `tf_isolate_content_container` uses — instead
    // of only inspecting direct children.
    if let Some(ptr) = container_ptr {
        if let DomNode::Element { children, .. } = node {
            let mut path = Vec::new();
            if find_node_path(children, ptr, &mut path) {
                apply_path(children, &path);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[path = "../../../tests/unit/pipelines/passes/tf_filters_test.rs"]
mod tests;
