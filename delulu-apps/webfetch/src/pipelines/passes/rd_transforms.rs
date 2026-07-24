use crate::pipelines::DomNode;
use crate::pipelines::walkers::{WalkerAction, WalkerFilter, walk_post_mut};
use regex::Regex;
use std::collections::HashMap;
use std::sync::LazyLock;

// ---------------------------------------------------------------------------
// 2.  convert_double_br_to_paragraph
// ---------------------------------------------------------------------------

/// Replace adjacent `<br><br>` pairs with paragraph breaks.
///
/// Any content between two `<br><br>` boundaries — or before the first or after
/// the last — is wrapped in a `<p>` element.
///
/// Pre: DOM tree is fully parsed.
/// Post: No adjacent `<br>` elements remain as siblings.
pub fn convert_double_br_to_paragraph(node: &mut DomNode) -> WalkerAction {
    if let DomNode::Element { children, .. } = node
        && has_adjacent_brs(children)
    {
        let mut buf: Vec<DomNode> = Vec::new();
        let mut new_children: Vec<DomNode> = Vec::new();
        let old = std::mem::take(children);
        let mut iter = old.into_iter().peekable();
        while let Some(child) = iter.next() {
            if is_br(&child) && iter.peek().is_some_and(is_br) {
                iter.next(); // skip the second br
                if !buf.is_empty() {
                    new_children.push(make_p_element(std::mem::take(&mut buf)));
                }
            } else {
                buf.push(child);
            }
        }
        if !buf.is_empty() {
            new_children.push(make_p_element(std::mem::take(&mut buf)));
        }
        *children = new_children;
    }
    WalkerAction::Continue
}

/// Returns `true` when two adjacent `<br>` elements exist among siblings.
fn has_adjacent_brs(children: &[DomNode]) -> bool {
    children.windows(2).any(|w| is_br(&w[0]) && is_br(&w[1]))
}

/// Returns `true` when the node is a `<br>` element.
fn is_br(node: &DomNode) -> bool {
    matches!(node, DomNode::Element { tag, .. } if tag == "br")
}

/// Build a `<p>` element wrapping the given children.
fn make_p_element(children: Vec<DomNode>) -> DomNode {
    DomNode::Element {
        tag: "p".into(),
        attrs: vec![],
        children,
        scores: Default::default(),
        metadata: Default::default(),
    }
}

// ---------------------------------------------------------------------------
// 3.  convert_font_to_span
// ---------------------------------------------------------------------------

/// Replace all `<font>` elements with `<span>`, preserving their attributes.
///
/// Pre: DOM tree is fully parsed.
/// Post: No `<font>` elements remain.
pub fn convert_font_to_span(node: &mut DomNode) -> WalkerAction {
    if let DomNode::Element { tag, .. } = node
        && tag == "font"
    {
        *tag = "span".to_string();
    }
    WalkerAction::Continue
}

// ---------------------------------------------------------------------------
// 6.  convert_div_containing_phrasing_to_paragraph
// ---------------------------------------------------------------------------

/// Convert a `<div>` whose children are exclusively phrasing content into a
/// `<p>`.
///
/// Pre: DOM tree is fully parsed.
/// Post: Divs that only contain phrasing content are now `<p>` elements.
pub fn convert_div_containing_phrasing_to_paragraph(node: &mut DomNode) -> WalkerAction {
    if let DomNode::Element { tag, children, .. } = node
        && tag == "div"
        && !children.is_empty()
        && children
            .iter()
            .all(crate::pipelines::passes::rd_utils::is_phrasing)
    {
        *tag = "p".to_string();
    }
    WalkerAction::Continue
}

// ---------------------------------------------------------------------------
// 13.  fix_lazy_loaded_images
// ---------------------------------------------------------------------------

/// Promote data attributes to real attributes for lazy-loaded images.
///
/// Copies `data-src` -> `src` and `data-srcset` -> `srcset` for `<img>`,
/// `<picture>`, and `<figure>` elements when `src` is empty or a small
/// base64 placeholder (<133 bytes after the base64 prefix).
///
/// Pre: DOM tree is fully parsed.
/// Post: Lazy-loaded images have valid `src` attributes.
pub fn fix_lazy_loaded_images(node: &mut DomNode) -> WalkerAction {
    match node {
        DomNode::Element { tag, attrs, .. } => {
            if !matches!(tag.as_str(), "img" | "picture" | "figure") {
                return WalkerAction::Continue;
            }

            // Phase 1: Check what we have
            let mut has_data_src = false;
            let mut has_data_srcset = false;
            let mut has_data_original = false;
            let mut has_data_fallback = false;
            let mut has_data_lazy_src = false;
            let mut has_data_lazy_srcset = false;
            let mut has_data_src_original = false;
            let mut has_data_srcset_original = false;
            let mut src_empty = true;

            for (key, value) in attrs.iter() {
                match key.as_str() {
                    "data-src" => has_data_src = true,
                    "data-srcset" => has_data_srcset = true,
                    "data-original" => has_data_original = true,
                    "data-fallback" => has_data_fallback = true,
                    "data-lazy-src" => has_data_lazy_src = true,
                    "data-lazy-srcset" => has_data_lazy_srcset = true,
                    "data-src-original" => has_data_src_original = true,
                    "data-srcset-original" => has_data_srcset_original = true,
                    // Treat base64 placeholder src (<133 bytes) as empty
                    "src"
                        if !(value.is_empty()
                            || (value.starts_with("data:image/") && value.len() < 150)
                            || (value.starts_with("data:image/") && value.len() < 133)) =>
                    {
                        src_empty = false;
                    }
                    _ => {}
                }
            }

            // Helper: promote a data attr to a real attr
            let promote_to_attr =
                |attrs: &mut Vec<(String, String)>, data_key: &str, target_key: &str| {
                    let val = attrs
                        .iter()
                        .find(|(k, _)| k == data_key)
                        .map(|(_, v)| v.clone());
                    if let Some(val) = val {
                        if let Some(pos) = attrs.iter().position(|(k, _)| k == target_key) {
                            attrs[pos].1 = val;
                        } else {
                            attrs.push((target_key.to_string(), val));
                        }
                    }
                };

            // Phase 2: Promote data attrs to src/srcset
            // Priority: data-src > data-original > data-lazy-src > data-src-original > data-fallback
            if src_empty {
                if has_data_src {
                    promote_to_attr(attrs, "data-src", "src");
                } else if has_data_original {
                    promote_to_attr(attrs, "data-original", "src");
                } else if has_data_lazy_src {
                    promote_to_attr(attrs, "data-lazy-src", "src");
                } else if has_data_src_original {
                    promote_to_attr(attrs, "data-src-original", "src");
                } else if has_data_fallback {
                    promote_to_attr(attrs, "data-fallback", "src");
                }
            }

            // srcset promotions
            if has_data_srcset {
                promote_to_attr(attrs, "data-srcset", "srcset");
            } else if has_data_lazy_srcset {
                promote_to_attr(attrs, "data-lazy-srcset", "srcset");
            } else if has_data_srcset_original {
                promote_to_attr(attrs, "data-srcset-original", "srcset");
            }

            WalkerAction::Continue
        }
        _ => WalkerAction::Continue,
    }
}

// ---------------------------------------------------------------------------
// 14.  replace_h1_with_h2
// ---------------------------------------------------------------------------

/// Replace all `<h1>` elements with `<h2>`.
///
/// The first `<h1>` in the document is typically the article title, which
/// is extracted separately into frontmatter. Remaining H1s should be H2s
/// for proper heading hierarchy in the output.
///
/// Pre: DOM tree is fully parsed and scored.
/// Post: No `<h1>` elements remain (all are `<h2>`).
pub fn replace_h1_with_h2(node: &mut DomNode) -> WalkerAction {
    if let DomNode::Element { tag, .. } = node
        && tag == "h1"
    {
        *tag = "h2".to_string();
    }
    WalkerAction::Continue
}

// ---------------------------------------------------------------------------
// 17.  remove_br_before_paragraph
// ---------------------------------------------------------------------------

/// Remove `<br>` elements that immediately precede a `<p>` element among siblings.
///
/// Pre: DOM tree is fully parsed.
/// Post: No `<br>` immediately before `<p>` elements.
pub fn remove_br_before_paragraph(node: &mut DomNode) -> WalkerAction {
    if let DomNode::Element { children, .. } = node {
        let mut i = 0;
        while i + 1 < children.len() {
            let is_br = matches!(&children[i], DomNode::Element { tag, .. } if tag == "br");
            let next_is_p = matches!(&children[i + 1], DomNode::Element { tag, .. } if tag == "p");
            if is_br && next_is_p {
                children.remove(i);
            } else {
                i += 1;
            }
        }
    }
    WalkerAction::Continue
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
// 18.  unwrap_single_cell_tables
// ---------------------------------------------------------------------------

/// Unwrap tables that contain a single cell (one tr with one td/th).
///
/// `<table><tr><td>content</td></tr></table>` -> `<p>content</p>` if phrasing content,
/// or `<div><div>content</div></div>` if block children.
///
/// Skips if the table has metadata["is_data_table"] == "true".
///
/// Pre: DOM tree is fully parsed and analyzed (needs "is_data_table" metadata).
/// Post: Single-cell layout tables are unwrapped to simpler elements.
#[allow(clippy::collapsible_if)]
pub fn unwrap_single_cell_tables(node: &mut DomNode) -> WalkerAction {
    // Helper: count cells and check for th in a list of row-like children
    fn examine_rows(rows: &[DomNode]) -> (usize, bool, Option<Vec<DomNode>>) {
        let mut total_cells = 0;
        let mut has_th = false;
        let mut cell_content: Option<Vec<DomNode>> = None;
        for row in rows {
            if let DomNode::Element {
                tag: rt,
                children: rc,
                ..
            } = row
            {
                if rt == "tr" {
                    for cell in rc {
                        if let DomNode::Element {
                            tag: ct,
                            children: cc,
                            ..
                        } = cell
                        {
                            if ct == "th" {
                                has_th = true;
                            }
                            if ct == "td" || ct == "th" {
                                total_cells += 1;
                                if total_cells == 1 {
                                    cell_content = Some(cc.clone());
                                }
                            }
                        }
                    }
                }
            }
        }
        (total_cells, has_th, cell_content)
    }

    // Use a separate scope to avoid borrow conflicts
    let replacement = match node {
        DomNode::Element { tag, children, .. } if tag == "table" => {
            // Check immediate children first (table > tr > td)
            let (total, has_th, content) = examine_rows(children);
            if has_th {
                None // Data table -- don't unwrap
            } else if total == 1 {
                content
            } else if total == 0 && children.len() == 1 {
                // Maybe wrapped in <tbody> (table > tbody > tr > td)
                if let DomNode::Element {
                    tag: ct,
                    children: gc,
                    ..
                } = &children[0]
                {
                    if ct == "tbody" {
                        let (nested_total, nested_has_th, nested_content) = examine_rows(gc);
                        if nested_has_th {
                            None
                        } else if nested_total == 1 {
                            nested_content
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                } else {
                    None
                }
            } else {
                None
            }
        }
        _ => None,
    };

    if let Some(content) = replacement {
        if let DomNode::Element { tag, children, .. } = node {
            if crate::pipelines::passes::rd_utils::all_phrasing(&content) {
                *tag = "p".to_string();
            } else {
                *tag = "div".to_string();
            }
            *children = content;
        }
    }

    WalkerAction::Continue
}

// ---------------------------------------------------------------------------
// 19.  collapse_single_child_elements
// ---------------------------------------------------------------------------

/// Collapse DIV/SECTION elements that contain only a single child element.
/// Also removes empty DIV/SECTION elements and transfers parent attributes
/// to child when collapsing.
///
/// Repeatedly unwraps wrappers until no more candidates exist (fixed-point),
/// up to MAX_COLLAPSE_ROUNDS iterations.
///
/// `<div><section><p>text</p></section></div>` -> `<p>text</p>`
/// `<div class=\"foo\"><p>text</p></div>` -> `<p class=\"foo\">text</p>`
/// `<div></div>` -> (removed)
///
/// Pre: DOM tree is fully processed.
/// Post: No single-child DIV/SECTION wrappers remain.
///       No empty DIV/SECTION elements remain.
#[allow(clippy::ptr_arg)]
pub fn collapse_single_child_elements(node: &mut DomNode) {
    const MAX_COLLAPSE_ROUNDS: usize = 10;
    for _round in 0..MAX_COLLAPSE_ROUNDS {
        let mut changed = false;
        if let DomNode::Element { children, .. } = node {
            for child in children.iter_mut() {
                apply_collapse_round(child, &mut changed);
            }
        }
        if !changed {
            break;
        }
    }
}

/// Returns true if a node has no meaningful content (only whitespace text
/// and/or empty child elements).
fn is_element_without_content(children: &[DomNode]) -> bool {
    if children.is_empty() {
        return true;
    }
    for child in children {
        match child {
            DomNode::Text(t) if !t.trim().is_empty() => return false,
            DomNode::Element { children: cc, .. } if !is_element_without_content(cc) => {
                return false;
            }
            _ => {}
        }
    }
    true
}

fn apply_collapse_round(node: &mut DomNode, changed: &mut bool) {
    if let DomNode::Element {
        tag,
        children,
        attrs,
        ..
    } = node
    {
        // Phase 1: Remove empty div/section children
        for child in children.iter_mut() {
            apply_collapse_round(child, changed);
        }

        // Phase 1: Remove empty div/section children
        let _old_len = children.len();
        children.retain(|child| {
            if let DomNode::Element {
                tag: ct,
                children: cc,
                ..
            } = child
                && matches!(ct.as_str(), "div" | "section")
                && is_element_without_content(cc)
            {
                *changed = true;
                return false;
            }
            true
        });

        // Phase 2: If this node itself is an empty div/section, mark for later removal
        // (empty divs/sections at root level are handled by the caller)

        // Phase 3: Collapse single-child wrappers with attribute transfer
        // Extract info BEFORE mutation to avoid borrow conflicts
        let collapse_info = if matches!(tag.as_str(), "div" | "section") && children.len() == 1 {
            if let DomNode::Element {
                tag: child_tag,
                children: child_children,
                attrs: child_attrs,
                ..
            } = &children[0]
            {
                Some((
                    child_tag.clone(),
                    child_children.clone(),
                    child_attrs.clone(),
                ))
            } else {
                None
            }
        } else {
            None
        };

        // Apply collapse (no borrow conflict since info is already collected)
        if let Some((new_tag, new_children, child_attrs)) = collapse_info {
            // Transfer parent attrs to child (child attrs take precedence)
            for (k, v) in child_attrs {
                if !attrs.iter().any(|(ak, _)| ak == &k) {
                    attrs.push((k, v));
                }
            }
            *tag = new_tag;
            *children = new_children;
            *changed = true;
        }
    }
}

// ---------------------------------------------------------------------------
// 20.  strip_heading_edit_suffixes
// ---------------------------------------------------------------------------

static EDIT_SUFFIX_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\s*\[edit\]$").expect("valid regex: strip trailing [edit]"));

/// Strip `[edit]` suffix (with optional leading whitespace) from heading text nodes.
/// Targets h1-h6 elements. Works on direct text children only.
pub fn strip_heading_edit_suffixes(node: &mut DomNode) -> WalkerAction {
    if let DomNode::Element { tag, children, .. } = node
        && matches!(tag.as_str(), "h1" | "h2" | "h3" | "h4" | "h5" | "h6")
    {
        for child in children.iter_mut() {
            if let DomNode::Text(t) = child {
                *t = EDIT_SUFFIX_RE.replace(t, "").to_string();
            }
        }
    }
    WalkerAction::Continue
}

// ---------------------------------------------------------------------------
// 21.  rd_strip_non_content
// ---------------------------------------------------------------------------

/// Remove non-content elements (script, style, nav, footer, aside, form,
/// noscript, iframe, svg, canvas, template, head) from the DOM tree.
///
/// Uses `walk_post_mut` with `WalkerAction::Remove` for each matching element.
/// `<head>` is explicitly included to prevent `<title>` text from appearing as
/// body content in output.
///
/// Pre: DOM tree is fully parsed.
/// Post: No non-content elements remain.
pub fn rd_strip_non_content(node: &mut DomNode) {
    const STRIPPED_TAGS: &[&str] = &[
        "script", "style", "nav", "footer", "aside", "form", "noscript", "iframe", "svg", "canvas",
        "template", "head",
    ];
    let mut filter = |n: &mut DomNode| -> WalkerAction {
        if let DomNode::Element { tag, .. } = n
            && STRIPPED_TAGS.contains(&tag.as_str())
        {
            return WalkerAction::Remove;
        }
        WalkerAction::Continue
    };
    let mut filters: Vec<&mut WalkerFilter> = vec![&mut filter];
    walk_post_mut(node, &mut filters, None);
}

// ---------------------------------------------------------------------------
// 22.  rd_unwrap_structural_wrappers
// ---------------------------------------------------------------------------

/// Unwrap structural wrapper elements (html, head, body) by replacing each
/// container node with its child nodes, simplifying the DOM tree for
/// downstream passes.
///
/// Structural wrappers unwrapped: html, head, body.
///
/// Layout `<table>` elements (with explicit `is_data_table` metadata set to a
/// non-true value, e.g. "false") are unwrapped by replacing the `<table>`
/// with its child elements.
///
/// Data tables (`is_data_table="true"`) are preserved. Uses `walk_post_mut`
/// with `WalkerAction::ReplaceWithChildren` for the unwrap operation.
///
/// Pre: DOM tree is fully parsed. Analysis passes (rd_analysis) have populated
///      `metadata["is_data_table"]` on relevant `<table>` elements.
/// Post: Structural wrappers (html/head/body) are unwrapped.
///       Data tables are preserved intact.
pub fn rd_unwrap_structural_wrappers(node: &mut DomNode) {
    const CONTAINER_TAGS: &[&str] = &["html", "head", "body"];

    let mut unwrap_filter = |n: &mut DomNode| -> WalkerAction {
        let is_data_table = matches!(n, DomNode::Element { tag, metadata, .. } if tag == "table"
        && metadata.iter().any(|(k, v)|
            k.eq_ignore_ascii_case("is_data_table")
                && v.eq_ignore_ascii_case("true")
        ));
        let has_is_data_table_key = matches!(n, DomNode::Element { tag, metadata, .. } if tag == "table"
        && metadata.iter().any(|(k, _)|
            k.eq_ignore_ascii_case("is_data_table")
        ));

        match n {
            DomNode::Element { tag, .. } => {
                if is_data_table {
                    WalkerAction::Continue
                } else if tag == "table" && has_is_data_table_key {
                    // Explicitly marked as layout (is_data_table set to non-true) — unwrap
                    WalkerAction::ReplaceWithChildren
                } else if CONTAINER_TAGS.contains(&tag.as_str()) {
                    WalkerAction::ReplaceWithChildren
                } else {
                    WalkerAction::Continue
                }
            }
            _ => WalkerAction::Continue,
        }
    };

    let mut filters: Vec<&mut WalkerFilter> = vec![&mut unwrap_filter];
    walk_post_mut(node, &mut filters, None);

    // Also unwrap the root node if it is a structural wrapper or layout table.
    // walk_post_mut only processes children, so the root itself is never visited.
    let is_container =
        matches!(node, DomNode::Element { tag, .. } if CONTAINER_TAGS.contains(&tag.as_str()));
    let is_layout_table = matches!(node, DomNode::Element { tag, metadata, .. } if tag == "table"
        && metadata.iter().any(|(k, _)| k.eq_ignore_ascii_case("is_data_table"))
        && !metadata.iter().any(|(k, v)| k.eq_ignore_ascii_case("is_data_table") && v.eq_ignore_ascii_case("true")));
    if (is_container || is_layout_table)
        && let DomNode::Element { children, .. } = node
    {
        if children.len() == 1 {
            // Single child: replace root with that child
            let child = children.remove(0);
            *node = child;
        } else {
            // Multiple children: wrap in a synthetic div to preserve tree structure
            let old_children = std::mem::take(children);
            *node = DomNode::Element {
                tag: "div".to_string(),
                attrs: Vec::new(),
                children: old_children,
                scores: HashMap::new(),
                metadata: HashMap::new(),
            };
        }
    }
}

// ---------------------------------------------------------------------------
// 23.  clean_styles
// ---------------------------------------------------------------------------

/// Remove `style` attributes and event handler attributes from all elements.
///
/// Removes:
/// - The `style` attribute
/// - Any attribute whose name starts with `on` (onclick, onload, onmouseover, etc.)
///
/// This is an in-place mutation. The element structure is preserved.
///
/// Pre: DOM tree is fully parsed.
/// Post: No `style` or event handler attributes remain on any element.
pub fn clean_styles(node: &mut DomNode) -> WalkerAction {
    if let DomNode::Element { attrs, .. } = node {
        attrs.retain(|(k, _)| k != "style" && !k.starts_with("on"));
    }
    WalkerAction::Continue
}

// ---------------------------------------------------------------------------
// 24.  clean_classes
// ---------------------------------------------------------------------------

/// Remove `class` attributes from all elements.
///
/// This is an in-place mutation. The element structure is preserved.
///
/// Pre: DOM tree is fully parsed.
/// Post: No `class` attributes remain on any element.
pub fn clean_classes(node: &mut DomNode) -> WalkerAction {
    if let DomNode::Element { attrs, .. } = node {
        attrs.retain(|(k, _)| k != "class");
    }
    WalkerAction::Continue
}

// ---------------------------------------------------------------------------
// 25.  wrap_readability_output
// ---------------------------------------------------------------------------

/// Wrap all root-level nodes in a `<div id="readability-page-1" class="page">`.
///
/// This matches JS Readability's `_postProcessContent` which always wraps
/// output in a container div with id="readability-page-1" and class="page".
///
/// Pre: DOM tree has been fully processed (all passes have run).
/// Post: All root nodes are children of a single wrapper div element.
pub fn wrap_readability_output(node: &mut DomNode) {
    if let DomNode::Element { children, .. } = node {
        if children.is_empty() {
            return;
        }
        let wrapper = DomNode::Element {
            tag: "div".into(),
            attrs: vec![
                ("id".into(), "readability-page-1".into()),
                ("class".into(), "page".into()),
            ],
            children: std::mem::take(children),
            scores: Default::default(),
            metadata: Default::default(),
        };
        children.push(wrapper);
    }
}

// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[path = "rd_transforms_test.rs"]
mod tests;
