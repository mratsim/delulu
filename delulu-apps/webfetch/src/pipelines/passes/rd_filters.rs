use crate::pipelines::passes::rd_utils::{get_inner_text, meta_get_f64};
use crate::pipelines::{DomNode, WalkerAction};
use once_cell::sync::Lazy;
use regex::Regex;

// ── Structural constants ──

/// Elements that are considered "structural" for emptiness checking.
const STRUCTURAL_TAGS: &[&str] = &[
    "div",
    "section",
    "header",
    "footer",
    "nav",
    "article",
    "aside",
    "main",
    "hr",
    "figure",
    "figcaption",
    "ul",
    "ol",
    "li",
    "dl",
    "dt",
    "dd",
    "form",
    "fieldset",
];

/// Elements that should *never* be removed even when empty.
const PROTECTED_TAGS: &[&str] = &["td", "th", "pre", "textarea", "code"];

// ── Density heuristic thresholds ──

/// Threshold for high link density — Heuristic 1 (port of JS `config.CHR_THRESHOLD * 10`).
const HIGH_LINK_DENSITY_THRESHOLD: f64 = 0.333;
/// Threshold for high heading density with zero embeds — Heuristic 2.
const HIGH_HEADING_DENSITY_THRESHOLD: f64 = 0.9;
/// Image-to-paragraph ratio above which the element is considered media-heavy — Heuristic 3.
const IMG_PARA_RATIO_THRESHOLD: f64 = 1.0;
/// Base link-density threshold for low-weight elements — Heuristic 4 (no ad-class).
const LOW_WEIGHT_BASE_THRESHOLD: f64 = 0.2;
/// Base link-density threshold for high-weight elements — Heuristic 5 (no ad-class).
const HIGH_WEIGHT_BASE_THRESHOLD: f64 = 0.5;
/// Multiplier applied to base thresholds when the element has an ad-class.
const AD_CLASS_MULTIPLIER: f64 = 0.5;
/// Comma count below which heuristics 4–8, li_count, and ad-word checks are evaluated.
const COMMA_COUNT_GATE: usize = 10;
/// Weight boundary separating low-weight (Heuristic 4) and high-weight (Heuristic 5) branches.
const HIGH_WEIGHT_BOUNDARY: i32 = 25;
const LI_COUNT_SUBTRACT: usize = 100;

// ---------------------------------------------------------------------------
// Unlikely-candidate regex (shared across invocations)
// ---------------------------------------------------------------------------

pub(crate) static UNLIKELY_CANDIDATES_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        "(?i)-ad-|author|banner|breadcrumbs|button|byline|combx|comment|community|\
         consent|cookie|cover-wrap|disqus|embed|extra|footer|gdpr|header|\
         hide-|index-title|kxtag|kxinvisible|legends|menu|message-container|\
         modal-content|most-popular|navbar|navbox|\\bnext-|newsletter|notloaded|\
         noprint|obfuscated|outbrain|overlay|paid-?content|pagination|pager|\
         permission|popup|premium|related|remark|replies|reply-|rss|\
         \\bshare\\b|shoutbox|sidebar|skyscraper|social|sociable|sponsor|\
         subnav|supplemental|syndication|taboola|tag-list|criteo|\
         ad-break|agegate|article-infos|blurred|bmdh|dpsp-content|\
         expand|jp-|mol-factbox|nfoline|options|-icon|-stories|slide|\
         timestamp|viewport|viral|widget|xg1|yin|zlylin|yom-remote",
    )
    .expect("invalid unlikely-candidate regex")
});

/// Roles that are considered "unlikely" for content (mirrors JS Readability).
pub(crate) static UNLIKELY_ROLES: &[&str] = &[
    "navigation",
    "nav",
    "banner",
    "contentinfo",
    "complementary",
    "search",
    "广告",
    "advertisement",
];

/// Content-positive class/id patterns — mirrors JS Readability's `okMaybeItsACandidate`.
/// Elements whose class or id matches these patterns are considered likely content.
/// Used by `has_likely_content` to override unlikely-candidate removal.
pub(crate) static CONTENT_CANDIDATE_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?i)\b(?:and|article|body|column|content|main|shadow|MathJax)\b")
        .expect("invalid content-candidate regex")
});

/// Share/matched element patterns — mirrors JS Readability's `SHARE_ELEMENT_RE`.
/// Removes elements whose class/id matches these patterns (share buttons,
/// print-friendly widgets, invisible helpers, etc.).
static SHARE_ELEMENT_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        r"(?i)^(?:(?:clear(?:fix|both)?)|(?:visible-?(?:webkit|print))|(?:hidden-?(?:print|phone|tablet))|(?:invisible-?(?:print|phone|tablet))|(?:js-)|(?:printfriendly|print-?button|print_?this))$"
    )
    .expect("invalid share-element regex")
});

// ---------------------------------------------------------------------------
// Analytics/tracking regexes (shared across invocations)
// ---------------------------------------------------------------------------
// Analytics/tracking regexes (shared across invocations)
// ---------------------------------------------------------------------------

/// Match `src` attributes pointing to known analytics/tracking domains.
static ANALYTICS_SRC_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        "(?i)google-analytics\\.com|googletagmanager\\.com|\
         facebook\\.(net|com)|cdn\\.segment\\.(io|com)|cdn\\.amplitude\\.com|\
         cdn\\.mxpnl\\.com|static\\.hotjar\\.com|cdn\\.matomo\\.cloud|piwik|\
         clarity\\.ms|analytics\\.tiktok\\.com|snap\\.licdn\\.com|bat\\.bing\\.com|\
         pixel\\.quantserve\\.com|secure\\.quantserve\\.com|d\\.adroll\\.com|\
         ads\\.tiktok\\.com|cdn\\.optimizely\\.com|dpm\\.demdex\\.net|\
         stats\\.g\\.doubleclick\\.net|www\\.googleadservices\\.com|googleads\\.g\\.doubleclick\\.net"
    )
    .expect("invalid analytics-source regex")
});

/// Match script text content containing inline analytics API calls.
static ANALYTICS_INLINE_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        "(?i)\\bgtag\\(|ga\\(['\"]create['\"]|fbq\\(|analytics\\.(track|page)\\(|\
         _paq\\.push\\(|clarity\\s*\\(['\"&]|ttq\\.track\\(|_linkedin_partner_id|\
         uetq\\b|_gaq\\.push\\(|dataLayer\\.push\\(|StatCounter|sentry",
    )
    .expect("invalid analytics-inline regex")
});
/// Match `src` attributes for tracking pixel detection.
static ANALYTICS_PIXEL_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        "(?i)google-analytics\\.com|googletagmanager\\.com|facebook\\.(net|com)/tr|\
         bat\\.bing\\.com|pixel\\.quantserve\\.com|stats\\.g\\.doubleclick\\.net|\
         secure\\.quantserve\\.com|d\\.adroll\\.com|ads\\.tiktok\\.com|\
         www\\.googleadservices\\.com|googleads\\.g\\.doubleclick\\.net",
    )
    .expect("invalid analytics-pixel regex")
});

/// Match video embed source URLs (youtube, vimeo).
static VIDEO_SRC_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        r"(?:youtube(?:-nocookie)?\.com|youtu\.be|player\.vimeo\.com|vimeo\.com|dailymotion\.com|dai\.ly)"
    )
    .expect("invalid video regex")
});

// ---------------------------------------------------------------------------
// 1.  remove_style_elements
// ---------------------------------------------------------------------------

/// Remove all `<style>` elements from the DOM tree.
///
/// Pre: DOM tree is fully parsed.
/// Post: No `<style>` elements remain.
pub fn remove_style_elements(node: &mut DomNode) -> WalkerAction {
    match node {
        DomNode::Element { tag, .. } if tag == "style" => WalkerAction::Remove,
        _ => WalkerAction::Continue,
    }
}

/// Remove all `<script>` elements from the DOM.
/// Runs before scoring to prevent script text from inflating parent node scores.
pub fn remove_script_elements(node: &mut DomNode) -> WalkerAction {
    match node {
        DomNode::Element { tag, .. } if tag == "script" => WalkerAction::Remove,
        _ => WalkerAction::Continue,
    }
}

// ---------------------------------------------------------------------------
// 2.  rd_strip_analytics
// ---------------------------------------------------------------------------

/// Remove analytics/tracking scripts, noscript tracking iframes, and tracking pixels.
///
/// Targets:
/// - `<script>` tags with `src` matching known analytics domains (Google Analytics,
///   Google Tag Manager, Facebook Pixel, Segment, Amplitude, Mixpanel, Hotjar,
///   Matomo, Clarity, TikTok, LinkedIn, Bing, etc.)
/// - `<script>` tags whose text content contains inline analytics API calls
///   (gtag, ga, fbq, analytics.track, _paq.push, etc.)
/// - `<noscript>` elements containing tracking iframes or tracking images
/// - `<img>` tracking pixels (must match tracking domain AND have 1x1 dimensions
///   or pixel-related URL path)
///
/// Pre: DOM tree is fully parsed.
/// Post: Analytics/tracking elements are removed. All other elements pass through unchanged.
pub fn rd_strip_analytics(node: &mut DomNode) -> WalkerAction {
    match node {
        DomNode::Element {
            tag,
            attrs,
            children,
            ..
        } => {
            match tag.as_str() {
                "script" => {
                    // Check `src` attribute against known tracking domains
                    if let Some(src) = attrs
                        .iter()
                        .find(|(k, _)| k == "src")
                        .map(|(_, v)| v.as_str())
                        && ANALYTICS_SRC_RE.is_match(src)
                    {
                        return WalkerAction::Remove;
                    }
                    // Check inline text content for analytics API calls
                    let text_content: String = children
                        .iter()
                        .filter_map(|c| {
                            if let DomNode::Text(t) = c {
                                Some(t.as_str())
                            } else {
                                None
                            }
                        })
                        .collect();
                    if ANALYTICS_INLINE_RE.is_match(&text_content) {
                        return WalkerAction::Remove;
                    }
                    WalkerAction::Continue
                }
                "noscript" => {
                    // Check children text content for tracking URLs
                    // (scraper parses noscript children as text nodes, not elements)
                    let text_content: String = children
                        .iter()
                        .filter_map(|c| {
                            if let DomNode::Text(t) = c {
                                Some(t.as_str())
                            } else {
                                None
                            }
                        })
                        .collect();
                    if ANALYTICS_SRC_RE.is_match(&text_content)
                        || ANALYTICS_PIXEL_RE.is_match(&text_content)
                    {
                        return WalkerAction::Remove;
                    }
                    WalkerAction::Continue
                }
                "img" => {
                    // Check src against tracking pixel patterns
                    if let Some(src) = attrs
                        .iter()
                        .find(|(k, _)| k == "src")
                        .map(|(_, v)| v.as_str())
                    {
                        let is_tracking_domain = ANALYTICS_PIXEL_RE.is_match(src);
                        let is_pixel_path = src.contains("/pixel")
                            || src.contains("/collect")
                            || src.contains("pageview");
                        // Dimension check: width=1 AND height=1
                        let width_is_one = attrs.iter().any(|(k, v)| k == "width" && v == "1");
                        let height_is_one = attrs.iter().any(|(k, v)| k == "height" && v == "1");
                        let is_one_by_one = width_is_one && height_is_one;
                        if is_tracking_domain && (is_one_by_one || is_pixel_path) {
                            return WalkerAction::Remove;
                        }
                    }
                    WalkerAction::Continue
                }
                _ => WalkerAction::Continue,
            }
        }
        _ => WalkerAction::Continue,
    }
}

// ---------------------------------------------------------------------------
// 4.  strip_unlikely_candidates
// ---------------------------------------------------------------------------

/// Remove elements whose `class` or `id` attribute matches the unlikely-
/// candidate pattern.
///
/// Pre: DOM tree is fully parsed.
/// Post: Elements with unlikely-candidate class/id patterns are removed.
pub fn strip_unlikely_candidates(node: &mut DomNode) -> WalkerAction {
    match node {
        DomNode::Element {
            tag,
            attrs,
            children,
            metadata,
            ..
        } => {
            // Never strip <html>, <body>, <head>, <base>.
            if matches!(tag.as_str(), "html" | "body" | "head" | "base") {
                return WalkerAction::Continue;
            }

            // Data tables: skip children to protect table content from being stripped.
            // mark_data_tables_by_structure() runs before this pass, so is_data_table
            // metadata is already set on qualifying <table> elements.
            if metadata.get("is_data_table").is_some_and(|v| v == "true") {
                return WalkerAction::SkipChildren;
            }
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

            // === Attribute-level discard checks ===
            let aria_hidden = attrs
                .iter()
                .find(|(k, _)| k == "aria-hidden")
                .map(|(_, v)| v.as_str())
                .unwrap_or("");
            let is_aria_hidden = aria_hidden.eq_ignore_ascii_case("true");

            let style_val = attrs
                .iter()
                .find(|(k, _)| k == "style")
                .map(|(_, v)| v.as_str())
                .unwrap_or("");
            let has_display_none = {
                let cleaned: String = style_val.chars().filter(|c| !c.is_whitespace()).collect();
                cleaned.to_lowercase().contains("display:none")
            };

            let has_lp_content = attrs
                .iter()
                .any(|(k, _)| k == "data-lp-replacement-content");

            let has_most_popular = attrs
                .iter()
                .any(|(k, v)| k == "data-component" && v.contains("MostPopularStories"));

            // Structural elements: never remove via aria-hidden
            let structural_tag = matches!(tag.as_str(), "main" | "article" | "section" | "body");
            let attr_match = (!structural_tag && is_aria_hidden)
                || has_display_none
                || has_lp_content
                || has_most_popular;

            // JS Readability line 1124: node.tagName !== "A"
            // <a> elements are never stripped even if their class/id matches the unlikely regex.
            if tag == "a" {
                return WalkerAction::Continue;
            }

            if UNLIKELY_CANDIDATES_RE.is_match(class_val)
                || UNLIKELY_CANDIDATES_RE.is_match(id_val)
                || UNLIKELY_ROLES.contains(&role_val)
                || attr_match
            {
                if has_likely_content(children) {
                    return WalkerAction::Continue;
                }
                // Before removal, check if this element itself is a "maybe candidate"
                // Note: MathJax added as workaround — JS Readability relies on retry workflow
                // to recover from over-stripping, but Rust's retry doesn't fire because
                // residual markdown output exceeds the 500-char threshold.
                if CONTENT_CANDIDATE_RE.is_match(class_val) || CONTENT_CANDIDATE_RE.is_match(id_val)
                {
                    return WalkerAction::Continue; // okMaybeItsACandidate — keep it
                }
                return WalkerAction::Remove;
            }

            WalkerAction::Continue
        }
        _ => WalkerAction::Continue,
    }
}

pub(crate) fn has_likely_content(children: &[DomNode]) -> bool {
    children.iter().any(|child| match child {
        DomNode::Element {
            tag,
            children,
            attrs,
            ..
        } => {
            // Check if tag is a semantically likely-content tag
            if matches!(
                tag.as_str(),
                "article" | "section" | "p" | "pre" | "code" | "blockquote"
            ) {
                return true;
            }
            // Check class/id for content-positive patterns (okMaybeItsACandidate)
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
            if CONTENT_CANDIDATE_RE.is_match(class_val) || CONTENT_CANDIDATE_RE.is_match(id_val) {
                return true;
            }
            // Recurse into children
            has_likely_content(children)
        }
        _ => false,
    })
}

// ---------------------------------------------------------------------------
// 5.  remove_empty_structural_elements
// ---------------------------------------------------------------------------
/// Remove structural elements that have no text content and no children.
///
/// Protected tags (`<td>`, `<th>`, `<pre>`) are never removed.
///
/// Pre: DOM tree is fully parsed.
/// Post: Empty structural elements are removed.
pub fn remove_empty_structural_elements(node: &mut DomNode) -> WalkerAction {
    match node {
        DomNode::Element { tag, children, .. } => {
            if PROTECTED_TAGS.contains(&tag.as_str()) {
                return WalkerAction::Continue;
            }
            if STRUCTURAL_TAGS.contains(&tag.as_str()) && children.is_empty() {
                return WalkerAction::Remove;
            }
            WalkerAction::Continue
        }
        _ => WalkerAction::Continue,
    }
}

// ---------------------------------------------------------------------------
// 10. rd_filter_by_score — REMOVED (superseded by rd_transforms::rd_extract_candidate_subtree)
// ---------------------------------------------------------------------------

// 11.  remove_garbage_interactive_elements
// ---------------------------------------------------------------------------

/// Remove interactive/non-content elements: form, fieldset, object, embed,
/// footer, link, aside, iframe, input, textarea, select, button, noscript,
/// canvas, audio, video, source, track, applet, marquee.
///
/// Preserves:
/// - Elements whose ancestor table has metadata["is_data_table"] == "true"
///
/// Pre: Analysis passes have run (needs "is_data_table" metadata).
/// Post: Garbage interactive elements are removed.
pub fn remove_garbage_interactive_elements(node: &mut DomNode) -> WalkerAction {
    // Tags to remove unconditionally (if not protected)
    const GARBAGE_TAGS: &[&str] = &[
        "object", "embed", "footer", "link", "aside", "iframe", "input", "textarea", "select",
        "button", "noscript", "canvas", "audio", "video", "source", "track", "applet", "marquee",
    ];

    match node {
        DomNode::Element { tag, attrs, .. } => {
            if !GARBAGE_TAGS.contains(&tag.as_str()) {
                return WalkerAction::Continue;
            }

            // Check if this is a video embed (preserve it)
            let is_video = attrs.iter().any(|(_, v)| VIDEO_SRC_RE.is_match(v));
            if is_video {
                return WalkerAction::Continue;
            }

            WalkerAction::Remove
        }
        _ => WalkerAction::Continue,
    }
}
// ---------------------------------------------------------------------------
// 23.  is_probably_visible
// ---------------------------------------------------------------------------

/// Remove elements that are likely invisible to the user.
///
/// Checks:
/// - `hidden` attribute on the element
/// - `style="display:none"` (whitespace-agnostic, case-insensitive)
/// - `style="visibility:hidden"` (whitespace-agnostic, case-insensitive)
/// - `aria-hidden="true"`
///
/// Never removes structural elements (`html`, `body`, `head`, `base`).
///
/// Pre: DOM tree is fully parsed.
/// Post: Invisible elements are removed from the tree.
pub fn is_probably_visible(node: &mut DomNode) -> WalkerAction {
    match node {
        DomNode::Doctype(_) | DomNode::Comment(_) => WalkerAction::Remove,
        DomNode::Element { tag, attrs, .. } => {
            // Never remove structural elements
            if matches!(tag.as_str(), "html" | "body" | "head" | "base") {
                return WalkerAction::Continue;
            }

            // Check for `hidden` attribute
            let has_hidden = attrs.iter().any(|(k, _)| k == "hidden");

            // Check style attribute for display:none and visibility:hidden
            let style_val = attrs
                .iter()
                .find(|(k, _)| k == "style")
                .map(|(_, v)| v.as_str())
                .unwrap_or("");
            let cleaned_style: String = style_val.chars().filter(|c| !c.is_whitespace()).collect();
            let style_lower = cleaned_style.to_lowercase();
            let has_display_none = style_lower.contains("display:none");
            let has_visibility_hidden = style_lower.contains("visibility:hidden");

            // Check aria-hidden attribute
            let aria_hidden = attrs
                .iter()
                .find(|(k, _)| k == "aria-hidden")
                .map(|(_, v)| v.as_str())
                .unwrap_or("");
            let is_aria_hidden = aria_hidden.trim().eq_ignore_ascii_case("true");

            if has_hidden || has_display_none || has_visibility_hidden || is_aria_hidden {
                return WalkerAction::Remove;
            }

            WalkerAction::Continue
        }
        _ => WalkerAction::Continue,
    }
}

// ---------------------------------------------------------------------------
// 12.  clean_negative_headers
// ---------------------------------------------------------------------------

/// Remove `<h1>` and `<h2>` elements whose class/ID weight is negative.
///
/// These are likely sidebar/menu/related-content headers that happened to
/// survive the score filter. Uses `get_class_weight` from the shared utils.
///
/// Pre: None (class/id matching is self-contained).
/// Post: H1/H2 elements with negative class weight are removed.
pub fn clean_negative_headers(node: &mut DomNode) -> WalkerAction {
    match node {
        DomNode::Element { tag, attrs, .. } => {
            let is_header = matches!(tag.as_str(), "h1" | "h2");
            if !is_header {
                return WalkerAction::Continue;
            }

            let weight = crate::pipelines::passes::rd_utils::get_class_weight(attrs);
            if weight < 0 {
                return WalkerAction::Remove;
            }

            WalkerAction::Continue
        }
        _ => WalkerAction::Continue,
    }
}

// ---------------------------------------------------------------------------
// 15.  filter_low_density_elements
// ---------------------------------------------------------------------------

/// Remove low-content tables, ULs, and DIVs based on density heuristics.
///
/// Implements Mozilla Readability's `_cleanConditionally` multi-heuristic.
///
/// Uses `walk_pre_mut` with SkipSubtree/Remove instead of manual recursion.
///
/// Algorithm:
/// 1. Find the global max score (md_rd_subtree_max_score) across ALL nodes
/// 2. Call `walk_pre_mut` with a closure that:
///    a. Returns SkipSubtree for top-scored nodes (subtree_max >= global_max)
///    b. Returns SkipSubtree for data tables, code, pre (keep entire subtree)
///    c. Returns Keep for figure (let children evaluate normally)
///    d. Returns Remove for table|ul|div that fail density heuristics
///    e. Returns Keep for everything else
///
/// Pre: Analysis passes have run (needs "link_density", "heading_density",
///      "embed_count", "img_para_ratio" metadata on each node).
///      ScorerMozillaReadability has run (needs "mozilla_readability" scores).
/// Post: Low-density elements are removed. Top-scored nodes are never removed.
/// Pre-scan: find tables inside `<figure>` and mark them as data tables.
/// Analysis passes can't see parent context, so this is done at filtering level.
// ---------------------------------------------------------------------------
// Helper functions for density filter passes
// ---------------------------------------------------------------------------

/// Returns `true` when the node should be descended into (i.e., it is NOT a data table).
/// Used as `should_descend` guard in `walk_post_acc_mut` to skip data table subtrees.
pub(crate) fn is_data_table(node: &DomNode) -> bool {
    if let DomNode::Element { metadata, .. } = node
        && metadata.get("is_data_table").map(|s| s.as_str()) == Some("true")
    {
        return false;
    }
    true
}

/// Bottom-up walk that counts commas in text content and stores `_comma_count` in metadata.
/// Returns the total comma count for the subtree.
fn compute_comma_counts(node: &mut DomNode) -> usize {
    match node {
        DomNode::Element {
            children, metadata, ..
        } => {
            let mut total = 0usize;
            for child in children.iter_mut() {
                total += compute_comma_counts(child);
            }
            metadata.insert("_comma_count".to_string(), total.to_string());
            total
        }
        DomNode::Text(t) => t.chars().filter(|&c| c == ',').count(),
        _ => 0,
    }
}

// ---------------------------------------------------------------------------
// 9 density filter passes
// ---------------------------------------------------------------------------

/// H1: Remove elements with high link density (no comma gate).
fn remove_high_link_density(node: &mut DomNode) {
    let global_max = match node {
        DomNode::Element { metadata, .. } => {
            meta_get_f64(metadata, "md_rd_subtree_max_score").unwrap_or(0.0)
        }
        _ => 0.0,
    };
    if !global_max.is_finite() || global_max == 0.0 {
        return;
    }
    if let DomNode::Element { children, .. } = node {
        use crate::pipelines::walkers::{WalkerAction, walk_post_acc_mut};
        walk_post_acc_mut::<(bool,)>(children, Some(is_data_table), &mut |n: &mut DomNode,
                                                                          child_counts: &[(
            bool,
        )]| {
            // (has_data_table_descendant)
            // Check if any child subtree contains a data table (post-order accumulator)
            let child_has_dt = child_counts.iter().any(|c| c.0);
            if child_has_dt {
                return (WalkerAction::Continue, (false,));
            }
            if let DomNode::Element {
                tag,
                metadata,
                children: ch,
                ..
            } = n
            {
                // is_top_scored guard
                if let Some(score) = meta_get_f64(metadata, "md_rd_subtree_max_score")
                    && (score - global_max).abs() < 1e-4
                {
                    return (WalkerAction::Continue, (false,));
                }
                // Data table guard
                if metadata.get("is_data_table").is_some_and(|v| v == "true") {
                    return (WalkerAction::Continue, (false,));
                }
                // Tag filter
                if !matches!(tag.as_str(), "table" | "ul" | "div" | "form" | "fieldset") {
                    return (WalkerAction::Continue, (false,));
                }
                // Compute link_density using recursive get_inner_text on children
                let all_text: String = ch.iter().map(get_inner_text).collect();
                let child_text_len = all_text.len();
                // Count link text from <a> children using recursive get_inner_text
                let link_text_len: usize = ch
                    .iter()
                    .filter_map(|c| {
                        if let DomNode::Element { tag: t, .. } = c {
                            if t == "a" {
                                Some(get_inner_text(c).len())
                            } else {
                                None
                            }
                        } else {
                            None
                        }
                    })
                    .sum();
                let link_density = if child_text_len > 0 {
                    link_text_len as f64 / child_text_len as f64
                } else {
                    0.0
                };
                metadata.insert("link_density".into(), format!("{:.6}", link_density));
                // H1: high link density (no comma gate)
                if check_high_link_density(metadata) {
                    return (WalkerAction::Remove, (false,));
                }
            }
            (WalkerAction::Continue, (false,))
        });
    }
}

/// H2: Remove heading-heavy elements (no comma gate).
fn remove_heading_heavy(node: &mut DomNode) {
    let global_max = match node {
        DomNode::Element { metadata, .. } => {
            meta_get_f64(metadata, "md_rd_subtree_max_score").unwrap_or(0.0)
        }
        _ => 0.0,
    };
    if !global_max.is_finite() || global_max == 0.0 {
        return;
    }
    if let DomNode::Element { children, .. } = node {
        use crate::pipelines::walkers::{WalkerAction, walk_post_acc_mut};
        walk_post_acc_mut::<(usize, usize, usize, bool)>(
            children,
            Some(is_data_table),
            &mut |n: &mut DomNode, child_counts: &[(usize, usize, usize, bool)]| {
                // (headings, embeds, total, has_data_table_descendant)
                // Check if any child subtree contains a data table (post-order accumulator)
                let child_has_dt = child_counts.iter().any(|c| c.3);
                if child_has_dt {
                    return (WalkerAction::Continue, (0, 0, 0, false));
                }
                match n {
                    DomNode::Element { tag, metadata, .. } => {
                        // Sum children: (headings, embeds, total)
                        let mut h = 0usize;
                        let mut e = 0usize;
                        let mut t = 0usize;
                        for &(ch_h, ch_e, ch_t, _) in child_counts {
                            h += ch_h;
                            e += ch_e;
                            t += ch_t;
                        }
                        t += 1; // self
                        match tag.as_str() {
                            "h1" | "h2" | "h3" | "h4" | "h5" | "h6" | "th" => h += 1,
                            "img" | "embed" | "object" | "iframe" => e += 1,
                            _ => {}
                        }
                        // is_top_scored guard
                        if let Some(score) = meta_get_f64(metadata, "md_rd_subtree_max_score")
                            && (score - global_max).abs() < 1e-4
                        {
                            return (WalkerAction::Continue, (h, e, t, false));
                        }
                        // Data table guard
                        if metadata.get("is_data_table").is_some_and(|v| v == "true") {
                            return (WalkerAction::Continue, (h, e, t, false));
                        }
                        // Tag filter
                        if !matches!(tag.as_str(), "table" | "ul" | "div" | "form" | "fieldset") {
                            return (WalkerAction::Continue, (h, e, t, false));
                        }
                        // H2: high heading density (no comma gate)
                        if check_high_heading_density(h, t, e) {
                            return (WalkerAction::Remove, (h, e, t, false));
                        }
                        (WalkerAction::Continue, (h, e, t, false))
                    }
                    _ => (WalkerAction::Continue, (0, 0, 0, false)),
                }
            },
        );
    }
}

/// H3, H6, H8: Remove media-heavy elements. H3 is unconditional; H6/H8 are comma-gated.
fn remove_media_heavy(node: &mut DomNode) {
    let global_max = match node {
        DomNode::Element { metadata, .. } => {
            meta_get_f64(metadata, "md_rd_subtree_max_score").unwrap_or(0.0)
        }
        _ => 0.0,
    };
    if !global_max.is_finite() || global_max == 0.0 {
        return;
    }
    if let DomNode::Element { children, .. } = node {
        use crate::pipelines::walkers::{WalkerAction, walk_post_acc_mut};
        walk_post_acc_mut::<(usize, usize, usize, bool)>(
            children,
            Some(is_data_table),
            &mut |n: &mut DomNode, child_counts: &[(usize, usize, usize, bool)]| {
                // (headings, embeds, total, has_data_table_descendant)
                // Check if any child subtree contains a data table (post-order accumulator)
                let child_has_dt = child_counts.iter().any(|c| c.3);
                if child_has_dt {
                    return (WalkerAction::Continue, (0, 0, 0, false));
                }
                match n {
                    DomNode::Element { tag, metadata, .. } => {
                        let mut imgs = 0usize;
                        let mut paras = 0usize;
                        let mut embeds = 0usize;
                        for &(i, p, e, _) in child_counts {
                            imgs += i;
                            paras += p;
                            embeds += e;
                        }
                        match tag.as_str() {
                            "img" => imgs += 1,
                            "p" => paras += 1,
                            "embed" | "object" | "iframe" => embeds += 1,
                            _ => {}
                        }
                        if let Some(score) = meta_get_f64(metadata, "md_rd_subtree_max_score")
                            && (score - global_max).abs() < 1e-4
                        {
                            return (WalkerAction::Continue, (imgs, paras, embeds, false));
                        }
                        if metadata.get("is_data_table").is_some_and(|v| v == "true") {
                            return (WalkerAction::Continue, (imgs, paras, embeds, false));
                        }
                        if !matches!(tag.as_str(), "table" | "ul" | "div" | "form" | "fieldset") {
                            return (WalkerAction::Continue, (imgs, paras, embeds, false));
                        }
                        let skip = tag == "figure";
                        // H3: img_para_ratio (no comma gate)
                        if check_img_para_ratio(imgs, paras, skip) {
                            return (WalkerAction::Remove, (imgs, paras, embeds, false));
                        }
                        // H6, H8: gated
                        let comma_count = metadata
                            .get("_comma_count")
                            .and_then(|s| s.parse::<usize>().ok())
                            .unwrap_or(0);
                        if comma_count < COMMA_COUNT_GATE {
                            if check_media_heavy(imgs, paras, embeds, skip) {
                                return (WalkerAction::Remove, (imgs, paras, embeds, false));
                            }
                            if check_gallery(imgs, paras, skip) {
                                return (WalkerAction::Remove, (imgs, paras, embeds, false));
                            }
                        }
                        (WalkerAction::Continue, (imgs, paras, embeds, false))
                    }
                    _ => (WalkerAction::Continue, (0, 0, 0, false)),
                }
            },
        );
    }
}

/// H7: Remove form-heavy elements (comma-gated).
fn remove_form_heavy(node: &mut DomNode) {
    let global_max = match node {
        DomNode::Element { metadata, .. } => {
            meta_get_f64(metadata, "md_rd_subtree_max_score").unwrap_or(0.0)
        }
        _ => 0.0,
    };
    if !global_max.is_finite() || global_max == 0.0 {
        return;
    }
    if let DomNode::Element { children, .. } = node {
        use crate::pipelines::walkers::{WalkerAction, walk_post_acc_mut};
        walk_post_acc_mut::<(usize, usize, bool)>(
            children,
            Some(is_data_table),
            &mut |n: &mut DomNode, child_counts: &[(usize, usize, bool)]| {
                // (inputs, paras, has_data_table_descendant)
                // Check if any child subtree contains a data table (post-order accumulator)
                let child_has_dt = child_counts.iter().any(|c| c.2);
                if child_has_dt {
                    return (WalkerAction::Continue, (0, 0, false));
                }
                match n {
                    DomNode::Element { tag, metadata, .. } => {
                        let mut inputs = 0usize;
                        let mut paras = 0usize;
                        for &(i, p, _) in child_counts {
                            inputs += i;
                            paras += p;
                        }
                        match tag.as_str() {
                            "input" | "textarea" | "select" => inputs += 1,
                            "p" => paras += 1,
                            _ => {}
                        }
                        if let Some(score) = meta_get_f64(metadata, "md_rd_subtree_max_score")
                            && (score - global_max).abs() < 1e-4
                        {
                            return (WalkerAction::Continue, (inputs, paras, false));
                        }
                        if metadata.get("is_data_table").is_some_and(|v| v == "true") {
                            return (WalkerAction::Continue, (inputs, paras, false));
                        }
                        if !matches!(tag.as_str(), "table" | "ul" | "div" | "form" | "fieldset") {
                            return (WalkerAction::Continue, (inputs, paras, false));
                        }
                        let comma_count = metadata
                            .get("_comma_count")
                            .and_then(|s| s.parse::<usize>().ok())
                            .unwrap_or(0);
                        if comma_count < COMMA_COUNT_GATE && check_form_heavy(inputs, paras) {
                            return (WalkerAction::Remove, (inputs, paras, false));
                        }
                        (WalkerAction::Continue, (inputs, paras, false))
                    }
                    _ => (WalkerAction::Continue, (0, 0, false)),
                }
            },
        );
    }
}

/// HA: Remove list-heavy elements (comma-gated).
fn remove_list_heavy(node: &mut DomNode) {
    let global_max = match node {
        DomNode::Element { metadata, .. } => {
            meta_get_f64(metadata, "md_rd_subtree_max_score").unwrap_or(0.0)
        }
        _ => 0.0,
    };
    if !global_max.is_finite() || global_max == 0.0 {
        return;
    }
    if let DomNode::Element { children, .. } = node {
        use crate::pipelines::walkers::{WalkerAction, walk_post_acc_mut};
        walk_post_acc_mut::<(usize, usize, bool)>(
            children,
            Some(is_data_table),
            &mut |n: &mut DomNode, child_counts: &[(usize, usize, bool)]| {
                // (inputs, paras, has_data_table_descendant)
                // Check if any child subtree contains a data table (post-order accumulator)
                let child_has_dt = child_counts.iter().any(|c| c.2);
                if child_has_dt {
                    return (WalkerAction::Continue, (0, 0, false));
                }
                match n {
                    DomNode::Element { tag, metadata, .. } => {
                        let mut lis = 0usize;
                        let mut paras = 0usize;
                        for &(l, p, _) in child_counts {
                            lis += l;
                            paras += p;
                        }
                        match tag.as_str() {
                            "li" => lis += 1,
                            "p" => paras += 1,
                            _ => {}
                        }
                        if let Some(score) = meta_get_f64(metadata, "md_rd_subtree_max_score")
                            && (score - global_max).abs() < 1e-4
                        {
                            return (WalkerAction::Continue, (lis, paras, false));
                        }
                        if metadata.get("is_data_table").is_some_and(|v| v == "true") {
                            return (WalkerAction::Continue, (lis, paras, false));
                        }
                        if !matches!(tag.as_str(), "table" | "ul" | "div" | "form" | "fieldset") {
                            return (WalkerAction::Continue, (lis, paras, false));
                        }
                        let comma_count = metadata
                            .get("_comma_count")
                            .and_then(|s| s.parse::<usize>().ok())
                            .unwrap_or(0);
                        if comma_count < COMMA_COUNT_GATE && check_list_heavy(lis, paras) {
                            return (WalkerAction::Remove, (lis, paras, false));
                        }
                        (WalkerAction::Continue, (lis, paras, false))
                    }
                    _ => (WalkerAction::Continue, (0, 0, false)),
                }
            },
        );
    }
}

/// HD: Remove embed-heavy elements (comma-gated).
fn remove_embed_heavy(node: &mut DomNode) {
    let global_max = match node {
        DomNode::Element { metadata, .. } => {
            meta_get_f64(metadata, "md_rd_subtree_max_score").unwrap_or(0.0)
        }
        _ => 0.0,
    };
    if !global_max.is_finite() || global_max == 0.0 {
        return;
    }
    if let DomNode::Element { children, .. } = node {
        use crate::pipelines::walkers::{WalkerAction, walk_post_acc_mut};
        walk_post_acc_mut::<(usize, bool)>(
            children,
            Some(is_data_table),
            &mut |n: &mut DomNode, child_counts: &[(usize, bool)]| {
                // (embeds, has_data_table_descendant)
                // Check if any child subtree contains a data table (post-order accumulator)
                let child_has_dt = child_counts.iter().any(|c| c.1);
                if child_has_dt {
                    return (WalkerAction::Continue, (0, false));
                }
                match n {
                    DomNode::Element { tag, metadata, .. } => {
                        let mut embeds = 0usize;
                        for &(e, _) in child_counts {
                            embeds += e;
                        }
                        match tag.as_str() {
                            "img" | "embed" | "object" | "iframe" => embeds += 1,
                            _ => {}
                        }
                        if let Some(score) = meta_get_f64(metadata, "md_rd_subtree_max_score")
                            && (score - global_max).abs() < 1e-4
                        {
                            return (WalkerAction::Continue, (embeds, false));
                        }
                        if metadata.get("is_data_table").is_some_and(|v| v == "true") {
                            return (WalkerAction::Continue, (embeds, false));
                        }
                        if !matches!(tag.as_str(), "table" | "ul" | "div" | "form" | "fieldset") {
                            return (WalkerAction::Continue, (embeds, false));
                        }
                        let comma_count = metadata
                            .get("_comma_count")
                            .and_then(|s| s.parse::<usize>().ok())
                            .unwrap_or(0);
                        if comma_count < COMMA_COUNT_GATE && check_embed_count(embeds) {
                            return (WalkerAction::Remove, (embeds, false));
                        }
                        (WalkerAction::Continue, (embeds, false))
                    }
                    _ => (WalkerAction::Continue, (0, false)),
                }
            },
        );
    }
}

/// HE: Remove elements with low text density (comma-gated).
fn remove_low_text_density(node: &mut DomNode) {
    let global_max = match node {
        DomNode::Element { metadata, .. } => {
            meta_get_f64(metadata, "md_rd_subtree_max_score").unwrap_or(0.0)
        }
        _ => 0.0,
    };
    if !global_max.is_finite() || global_max == 0.0 {
        return;
    }
    if let DomNode::Element { children, .. } = node {
        use crate::pipelines::walkers::{WalkerAction, walk_post_acc_mut};
        walk_post_acc_mut::<(usize, usize, bool)>(
            children,
            Some(is_data_table),
            &mut |n: &mut DomNode, child_counts: &[(usize, usize, bool)]| {
                // (inputs, paras, has_data_table_descendant)
                // Check if any child subtree contains a data table (post-order accumulator)
                let child_has_dt = child_counts.iter().any(|c| c.2);
                if child_has_dt {
                    return (WalkerAction::Continue, (0, 0, false));
                }
                match n {
                    DomNode::Element {
                        tag,
                        metadata,
                        children: ch,
                        ..
                    } => {
                        let mut text_chars = 0usize;
                        let mut serialized_chars = 0usize;
                        for &(tc, sc, _) in child_counts {
                            text_chars += tc;
                            serialized_chars += sc;
                        }
                        let direct_text: String = ch
                            .iter()
                            .filter_map(|c| {
                                if let DomNode::Text(t) = c {
                                    Some(t.as_str())
                                } else {
                                    None
                                }
                            })
                            .collect();
                        text_chars += direct_text.len();
                        serialized_chars += direct_text.len();
                        if let Some(score) = meta_get_f64(metadata, "md_rd_subtree_max_score")
                            && (score - global_max).abs() < 1e-4
                        {
                            return (
                                WalkerAction::Continue,
                                (text_chars, serialized_chars, false),
                            );
                        }
                        if metadata.get("is_data_table").is_some_and(|v| v == "true") {
                            return (
                                WalkerAction::Continue,
                                (text_chars, serialized_chars, false),
                            );
                        }
                        if !matches!(tag.as_str(), "table" | "ul" | "div" | "form" | "fieldset") {
                            return (
                                WalkerAction::Continue,
                                (text_chars, serialized_chars, false),
                            );
                        }
                        let comma_count = metadata
                            .get("_comma_count")
                            .and_then(|s| s.parse::<usize>().ok())
                            .unwrap_or(0);
                        if comma_count < COMMA_COUNT_GATE
                            && check_text_density(text_chars, serialized_chars)
                        {
                            return (WalkerAction::Remove, (text_chars, serialized_chars, false));
                        }
                        (
                            WalkerAction::Continue,
                            (text_chars, serialized_chars, false),
                        )
                    }
                    _ => (WalkerAction::Continue, (0, 0, false)),
                }
            },
        );
    }
}

/// HC: Remove short-content elements (comma-gated).
fn remove_short_content(node: &mut DomNode) {
    let global_max = match node {
        DomNode::Element { metadata, .. } => {
            meta_get_f64(metadata, "md_rd_subtree_max_score").unwrap_or(0.0)
        }
        _ => 0.0,
    };
    if !global_max.is_finite() || global_max == 0.0 {
        return;
    }
    if let DomNode::Element { children, .. } = node {
        use crate::pipelines::walkers::{WalkerAction, walk_post_acc_mut};
        walk_post_acc_mut::<(usize, usize, bool)>(
            children,
            Some(is_data_table),
            &mut |n: &mut DomNode, child_counts: &[(usize, usize, bool)]| {
                // (inputs, paras, has_data_table_descendant)
                // Check if any child subtree contains a data table (post-order accumulator)
                let child_has_dt = child_counts.iter().any(|c| c.2);
                if child_has_dt {
                    return (WalkerAction::Continue, (0, 0, false));
                }
                match n {
                    DomNode::Element {
                        tag,
                        metadata,
                        children: ch,
                        ..
                    } => {
                        let mut imgs = 0usize;
                        let mut text_chars = 0usize;
                        for &(i, tc, _) in child_counts {
                            imgs += i;
                            text_chars += tc;
                        }
                        if tag == "img" {
                            imgs += 1;
                        }
                        let direct_text: String = ch
                            .iter()
                            .filter_map(|c| {
                                if let DomNode::Text(t) = c {
                                    Some(t.as_str())
                                } else {
                                    None
                                }
                            })
                            .collect();
                        text_chars += direct_text.len();
                        if let Some(score) = meta_get_f64(metadata, "md_rd_subtree_max_score")
                            && (score - global_max).abs() < 1e-4
                        {
                            return (WalkerAction::Continue, (imgs, text_chars, false));
                        }
                        if metadata.get("is_data_table").is_some_and(|v| v == "true") {
                            return (WalkerAction::Continue, (imgs, text_chars, false));
                        }
                        if !matches!(tag.as_str(), "table" | "ul" | "div" | "form" | "fieldset") {
                            return (WalkerAction::Continue, (imgs, text_chars, false));
                        }
                        // Compute link_density for HC
                        // link_density always 0.0 (no child link-text accumulator)
                        let link_density = 0.0;
                        metadata.insert("link_density".into(), format!("{:.6}", link_density));
                        let comma_count = metadata
                            .get("_comma_count")
                            .and_then(|s| s.parse::<usize>().ok())
                            .unwrap_or(0);
                        if comma_count < COMMA_COUNT_GATE
                            && check_short_content(imgs, text_chars, metadata)
                        {
                            return (WalkerAction::Remove, (imgs, text_chars, false));
                        }
                        (WalkerAction::Continue, (imgs, text_chars, false))
                    }
                    _ => (WalkerAction::Continue, (0, 0, false)),
                }
            },
        );
    }
}

/// H4, H5, HB: Remove ad-content elements (comma-gated).
fn remove_ad_content(node: &mut DomNode) {
    let global_max = match node {
        DomNode::Element { metadata, .. } => {
            meta_get_f64(metadata, "md_rd_subtree_max_score").unwrap_or(0.0)
        }
        _ => 0.0,
    };
    if !global_max.is_finite() || global_max == 0.0 {
        return;
    }
    if let DomNode::Element { children, .. } = node {
        use crate::pipelines::walkers::{WalkerAction, walk_post_acc_mut};
        walk_post_acc_mut::<(bool,)>(children, Some(is_data_table), &mut |n: &mut DomNode,
                                                                          child_counts: &[(
            bool,
        )]| {
            // (has_data_table_descendant)
            // Check if any child subtree contains a data table (post-order accumulator)
            let child_has_dt = child_counts.iter().any(|c| c.0);
            if child_has_dt {
                return (WalkerAction::Continue, (false,));
            }
            if let DomNode::Element {
                tag,
                attrs,
                metadata,
                children: _ch,
                ..
            } = n
            {
                if let Some(score) = meta_get_f64(metadata, "md_rd_subtree_max_score")
                    && (score - global_max).abs() < 1e-4
                {
                    return (WalkerAction::Continue, (false,));
                }
                if metadata.get("is_data_table").is_some_and(|v| v == "true") {
                    return (WalkerAction::Continue, (false,));
                }
                if !matches!(tag.as_str(), "table" | "ul" | "div" | "form" | "fieldset") {
                    return (WalkerAction::Continue, (false,));
                }
                // Compute link_density, weight, has_ad_class
                // link_density always 0.0 (no child link-text accumulator)
                let link_density = 0.0;
                metadata.insert("link_density".into(), format!("{:.6}", link_density));
                let weight = crate::pipelines::passes::rd_utils::get_class_weight(attrs);
                let has_ad_class = attrs.iter().any(|(name, val)| {
                    name == "class"
                        && (val.contains("ad-")
                            || val.contains("ads-")
                            || val.contains("advertisement"))
                });
                let comma_count = metadata
                    .get("_comma_count")
                    .and_then(|s| s.parse::<usize>().ok())
                    .unwrap_or(0);
                if comma_count < COMMA_COUNT_GATE {
                    if check_low_weight_link_density(metadata, weight, has_ad_class) {
                        return (WalkerAction::Remove, (false,));
                    }
                    if check_high_weight_link_density(metadata, weight, has_ad_class) {
                        return (WalkerAction::Remove, (false,));
                    }
                    if check_ad_words(n) {
                        return (WalkerAction::Remove, (false,));
                    }
                }
            }
            (WalkerAction::Continue, (false,))
        });
    }
}

// ---------------------------------------------------------------------------
// 15.  filter_low_density_elements
// ---------------------------------------------------------------------------

/// Remove low-content elements by running 9 independent density filter passes.
///
/// Pre: Scoring has run — every Element node has `md_rd_subtree_max_score`
///      in metadata as valid `f64` strings.
/// Post: Low-density elements are removed. Top-scored nodes are never removed.
pub fn filter_low_density_elements(node: &mut DomNode) {
    compute_comma_counts(node);
    remove_high_link_density(node);
    remove_heading_heavy(node);
    remove_media_heavy(node);
    remove_form_heavy(node);
    remove_list_heavy(node);
    remove_embed_heavy(node);
    remove_low_text_density(node);
    remove_short_content(node);
    remove_ad_content(node);
}

// ---------------------------------------------------------------------------
// ---------------------------------------------------------------------------
// Per-heuristic building blocks
// ---------------------------------------------------------------------------

/// H1: High link density — reads `link_density` metadata.
pub(crate) fn check_high_link_density(
    metadata: &std::collections::HashMap<String, String>,
) -> bool {
    let ld = metadata
        .get("link_density")
        .and_then(|s| crate::pipelines::passes::rd_utils::meta_parse_f64(s))
        .unwrap_or(0.0);
    ld > HIGH_LINK_DENSITY_THRESHOLD
}

/// H2: High heading density — uses headings / total.
pub(crate) fn check_high_heading_density(
    headings: usize,
    total: usize,
    embed_count: usize,
) -> bool {
    total > 0
        && (headings as f64 / total as f64) > HIGH_HEADING_DENSITY_THRESHOLD
        && embed_count == 0
}

/// H3: Image-to-paragraph ratio — uses imgs / paras.
/// When `skip` is true, returns false.
pub(crate) fn check_img_para_ratio(imgs: usize, paras: usize, skip: bool) -> bool {
    if skip {
        return false;
    }
    imgs > paras && imgs as f64 > IMG_PARA_RATIO_THRESHOLD
}

/// H4: Low-weight link density — reads `link_density` metadata, checks weight < boundary.
pub(crate) fn check_low_weight_link_density(
    metadata: &std::collections::HashMap<String, String>,
    weight: i32,
    has_ad_class: bool,
) -> bool {
    let ld = metadata
        .get("link_density")
        .and_then(|s| crate::pipelines::passes::rd_utils::meta_parse_f64(s))
        .unwrap_or(0.0);
    let threshold = if has_ad_class {
        LOW_WEIGHT_BASE_THRESHOLD * AD_CLASS_MULTIPLIER
    } else {
        LOW_WEIGHT_BASE_THRESHOLD
    };
    weight < HIGH_WEIGHT_BOUNDARY && ld > threshold
}

/// H5: High-weight link density — reads `link_density` metadata, checks weight >= boundary.
pub(crate) fn check_high_weight_link_density(
    metadata: &std::collections::HashMap<String, String>,
    weight: i32,
    has_ad_class: bool,
) -> bool {
    let ld = metadata
        .get("link_density")
        .and_then(|s| crate::pipelines::passes::rd_utils::meta_parse_f64(s))
        .unwrap_or(0.0);
    let threshold = if has_ad_class {
        HIGH_WEIGHT_BASE_THRESHOLD * AD_CLASS_MULTIPLIER
    } else {
        HIGH_WEIGHT_BASE_THRESHOLD
    };
    weight >= HIGH_WEIGHT_BOUNDARY && ld > threshold
}

/// H6: Media-heavy low-content — uses imgs, embeds, paras.
/// When `skip` is true, returns false.
pub(crate) fn check_media_heavy(imgs: usize, paras: usize, embeds: usize, skip: bool) -> bool {
    if skip {
        return false;
    }
    imgs > paras && (imgs + embeds) > 3
}

/// H7: Form-heavy — uses inputs / paras (guarded division).
pub(crate) fn check_form_heavy(inputs: usize, paras: usize) -> bool {
    inputs > 0 && inputs > paras.checked_div(3).unwrap_or(0)
}

/// H8: Gallery with thin text — uses imgs, paras.
/// When `skip` is true, returns false.
pub(crate) fn check_gallery(imgs: usize, paras: usize, skip: bool) -> bool {
    if skip {
        return false;
    }
    imgs > paras && imgs > 3
}

/// A: List-heavy removal — uses lis, paras.
pub(crate) fn check_list_heavy(lis: usize, paras: usize) -> bool {
    let effective_li = lis.saturating_sub(LI_COUNT_SUBTRACT);
    lis > 0 && effective_li > paras
}

/// B: Ad-word / loading-word detection — calls collect_text_to_string on children.
pub(crate) fn check_ad_words(node: &DomNode) -> bool {
    let children = match node {
        DomNode::Element { children, .. } => children.as_slice(),
        _ => return false,
    };
    let inner_text = collect_text_to_string(children);
    if inner_text.is_empty() {
        return false;
    }
    AD_WORDS_RE.is_match(&inner_text) || LOADING_WORDS_RE.is_match(&inner_text)
}

/// C: Short-content amplification — uses link_density metadata + imgs.
pub(crate) fn check_short_content(
    imgs: usize,
    text_chars: usize,
    metadata: &std::collections::HashMap<String, String>,
) -> bool {
    if imgs == 0 {
        return false;
    }
    let ld = metadata
        .get("link_density")
        .and_then(|s| crate::pipelines::passes::rd_utils::meta_parse_f64(s))
        .unwrap_or(0.0);
    // text_len < 100: short content check
    // Use text_chars as a proxy for content length
    ld > HIGH_LINK_DENSITY_THRESHOLD && text_chars < 100
}

/// D: Embed count check — uses embeds.
pub(crate) fn check_embed_count(embeds: usize) -> bool {
    embeds > 0
}

/// E: Text density check — computes text_chars / serialized_chars ratio.
/// Does NOT read `text_density` metadata.
pub(crate) fn check_text_density(text_chars: usize, serialized_chars: usize) -> bool {
    if serialized_chars == 0 {
        return false;
    }
    (text_chars as f64 / serialized_chars as f64) == 0.0
}

/// Count `<img>` elements in the children slice.

/// Check if an element fails the density heuristics and should be removed.
///
/// Accepts individual field values directly (caller is responsible for computing them).
/// The `metadata` map is still needed for the `link_density` key, which is set
/// by the production walker and read by `check_high_link_density` and related
/// heuristic functions.
///
static AD_WORDS_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"\b(?:ads?|advertisement|sponsor(?:ed)?|banner|promo(?:tion)?|marketing|paid|publicité|publicidad|sponsorisé)\b")
        .expect("invalid ad-words regex")
});

/// Loading-word patterns — ported from Readability.js `REGEXPS.loadingWords`.
static LOADING_WORDS_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"\b(?:loading|spinner|spinning|skeleton|placeholder|shimmer|snippet)\b")
        .expect("invalid loading-words regex")
});

/// Collect text from immediate element children (depth 1).
/// Unlike `collect_text` from rd_analysis.rs which walks ALL descendants,
/// this only aggregates text from direct child Text nodes and the text
/// of direct child Element nodes.
/// This avoids false positives where ad words in distant descendants
/// (e.g., a footnote) trigger removal.
fn collect_text_to_string(nodes: &[DomNode]) -> String {
    let mut result = String::new();
    for child in nodes {
        match child {
            DomNode::Text(text) => result.push_str(text),
            DomNode::Element { children, .. } => {
                // Only collect direct children's text, not full subtree
                for grandchild in children {
                    if let DomNode::Text(text) = grandchild {
                        result.push_str(text);
                    }
                }
            }
            _ => {}
        }
    }
    result
}

// ---------------------------------------------------------------------------
// 16.  remove_empty_paragraphs
// ---------------------------------------------------------------------------

/// Remove `<p>` elements that have no text content and no non-void child elements.
///
/// Pre: DOM tree is fully parsed.
/// Post: Empty `<p>` elements are removed.
pub fn remove_empty_paragraphs(node: &mut DomNode) -> WalkerAction {
    match node {
        DomNode::Element { tag, children, .. } if tag == "p" => {
            if children.is_empty() {
                return WalkerAction::Remove;
            }
            // Check if all children are empty text, whitespace, or void elements
            let has_content = children.iter().any(|child| match child {
                DomNode::Text(t) => !t.trim().is_empty(),
                DomNode::Element { tag, .. } if tag != "br" => true,
                _ => false,
            });
            if !has_content {
                return WalkerAction::Remove;
            }
            WalkerAction::Continue
        }
        _ => WalkerAction::Continue,
    }
}

// ---------------------------------------------------------------------------
// 24.  clean_matched_nodes
// ---------------------------------------------------------------------------

/// Remove elements whose `class` or `id` matches the SHARE_ELEMENT_RE pattern.
///
/// Port of Mozilla Readability's `_cleanMatchedNodes`, which removes
/// clearfix, print-friendly, and other non-content helper elements from
/// the article content.
///
/// Pre: DOM tree is fully parsed and scored.
/// Post: Elements with share/print/helper class/id patterns are removed.
pub fn clean_matched_nodes(node: &mut DomNode) -> WalkerAction {
    match node {
        DomNode::Element { tag: _, attrs, .. } => {
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
            if SHARE_ELEMENT_RE.is_match(class_val) || SHARE_ELEMENT_RE.is_match(id_val) {
                return WalkerAction::Remove;
            }
            WalkerAction::Continue
        }
        _ => WalkerAction::Continue,
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[path = "rd_filters_test.rs"]
mod tests;
