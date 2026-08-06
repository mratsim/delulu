use crate::pipelines::DomNode;
use crate::pipelines::walkers::{WalkerAction, WalkerFilter, walk_post_mut};
use std::collections::HashMap;

// ---------------------------------------------------------------------------
// Heading conversion (h1-h6 → head)
// ---------------------------------------------------------------------------

/// Convert heading tags (h1-h6) to `<head>` with a `rend` attribute.
///
/// - `h1` → `<head rend="h1">`
/// - `h3` → `<head rend="h3">`
///   Reference: Trafilatura `htmlprocessing.py:316-320` `convert_headings()`
pub fn tf_convert_headings(node: &mut DomNode) -> WalkerAction {
    match node {
        DomNode::Element { tag, attrs, .. }
            if matches!(tag.as_str(), "h1" | "h2" | "h3" | "h4" | "h5" | "h6") =>
        {
            let orig_tag = tag.clone();
            tag.clear();
            tag.push_str("head");
            // Add rend attribute with original tag name
            if !attrs.iter().any(|(k, _)| k == "rend") {
                attrs.push(("rend".to_string(), orig_tag));
            }
            WalkerAction::Continue
        }
        _ => WalkerAction::Continue,
    }
}

// ---------------------------------------------------------------------------
// List conversion (ul/ol → list, li → item)
// ---------------------------------------------------------------------------

/// Convert list tags to Trafilatura's XML schema.
///
/// - `ul`/`ol` → `list`
/// - `li` → `item`
///   Reference: Trafilatura `htmlprocessing.py:271-284` `convert_lists()`
pub fn tf_convert_lists(node: &mut DomNode) -> WalkerAction {
    match node {
        DomNode::Element { tag, .. } if matches!(tag.as_str(), "ul" | "ol") => {
            tag.clear();
            tag.push_str("list");
            WalkerAction::Continue
        }
        DomNode::Element { tag, .. } if tag == "li" => {
            tag.clear();
            tag.push_str("item");
            WalkerAction::Continue
        }
        _ => WalkerAction::Continue,
    }
}

// ---------------------------------------------------------------------------
// Quote/code conversion (blockquote, pre, q)
// ---------------------------------------------------------------------------

/// Convert quotation tags and normalize code blocks.
///
/// - `blockquote`/`q` → `quote`
/// - `pre` → stays `<pre>` (block code). The canonical `pre>code.language-x`
///   shape is normalized in place: the `<code>` child is unwrapped (its
///   children spliced into the pre) and the language is resolved from the
///   pre's own class or the nested code's class, appended to the pre's class
///   as a single `language-<lang>` token (see [`normalize_pre_block`]).
///   Keeping `<pre>` as the block-code element makes block-ness structural —
///   every backend lowers it without re-deciding inline vs. block (markdown:
///   fenced block; HTML: `<pre>`).
/// - `code` → stays `<code>` (inline)
///
///   Reference: Trafilatura `htmlprocessing.py:287-303` `convert_quotes()`
///
/// Deliberate deviation from python: python's `convert_quotes()` renames
/// `pre` → `code`, losing block-ness in the XML schema — which is why
/// python's own markdown output jams code blocks onto one line. We keep
/// `<pre>` so the backends stay dumb.
pub fn tf_convert_quotes(node: &mut DomNode) -> WalkerAction {
    match node {
        DomNode::Element { tag, .. } if matches!(tag.as_str(), "blockquote" | "q") => {
            tag.clear();
            tag.push_str("quote");
            WalkerAction::Continue
        }
        DomNode::Element {
            tag,
            attrs,
            children,
            ..
        } if tag == "pre" => {
            normalize_pre_block(attrs, children);
            WalkerAction::Continue
        }
        _ => WalkerAction::Continue,
    }
}

/// First whitespace-delimited `language-*` token in `class`, if any.
///
/// `class="language-python highlight"` yields `python`, not `python highlight`
/// (a multi-token class would produce an invalid fence info string).
fn code_language_from_class(class: &str) -> Option<String> {
    class
        .split_whitespace()
        .find_map(|token| token.strip_prefix("language-"))
        .map(str::to_string)
}

/// Normalize a `<pre>` block in place: unwrap a nested `<code>` child and
/// resolve the language onto the pre's own class.
///
/// After this, backends read the language from a single canonical place — the
/// pre's `class` attribute — and the pre holds plain text (or inline markup)
/// directly. The code child's children are spliced into the pre at its
/// position; the pre's other children are untouched.
fn normalize_pre_block(attrs: &mut Vec<(String, String)>, children: &mut Vec<DomNode>) {
    // Resolve the language: the pre's own class takes precedence, then a
    // nested <code> child's class (the canonical pre>code.language-x shape).
    let mut language = attrs.iter().find_map(|(k, v)| {
        if k == "class" {
            code_language_from_class(v)
        } else {
            None
        }
    });
    let mut spliced: Vec<DomNode> = Vec::with_capacity(children.len());
    for child in children.drain(..) {
        if let DomNode::Element {
            tag,
            attrs: code_attrs,
            children: code_children,
            ..
        } = &child
            && tag == "code"
        {
            if language.is_none() {
                language = code_attrs.iter().find_map(|(k, v)| {
                    if k == "class" {
                        code_language_from_class(v)
                    } else {
                        None
                    }
                });
            }
            spliced.extend(code_children.clone());
        } else {
            spliced.push(child);
        }
    }
    *children = spliced;
    // Hoist the language onto the pre's own class (append, dedup).
    if let Some(lang) = language {
        if let Some((_, class)) = attrs.iter_mut().find(|(k, _)| k == "class") {
            if !class
                .split_whitespace()
                .any(|t| t == format!("language-{lang}"))
            {
                class.push(' ');
                class.push_str(&format!("language-{lang}"));
            }
        } else {
            attrs.push(("class".to_string(), format!("language-{lang}")));
        }
    }
}

// ---------------------------------------------------------------------------
// Formatting conversion (b/strong → hi, em/i → hi, del → del)
// ---------------------------------------------------------------------------

/// Convert formatting tags to Trafilatura's XML schema.
///
/// - `b`/`strong` → `<hi rend="#b">`
/// - `em`/`i` → `<hi rend="#i">`
/// - `del`/`s`/`strike` → `<del rend="overstrike">`
///   Reference: Trafilatura `htmlprocessing.py:26-38` `REND_TAG_MAPPING` + `convert_tags()`
pub fn tf_convert_formatting(node: &mut DomNode) -> WalkerAction {
    match node {
        DomNode::Element { tag, attrs, .. } if matches!(tag.as_str(), "b" | "strong") => {
            tag.clear();
            tag.push_str("hi");
            if !attrs.iter().any(|(k, _)| k == "rend") {
                attrs.push(("rend".to_string(), "#b".to_string()));
            }
            WalkerAction::Continue
        }
        DomNode::Element { tag, attrs, .. } if matches!(tag.as_str(), "em" | "i") => {
            tag.clear();
            tag.push_str("hi");
            if !attrs.iter().any(|(k, _)| k == "rend") {
                attrs.push(("rend".to_string(), "#i".to_string()));
            }
            WalkerAction::Continue
        }
        DomNode::Element { tag, attrs, .. } if matches!(tag.as_str(), "del" | "s" | "strike") => {
            tag.clear();
            tag.push_str("del");
            if !attrs.iter().any(|(k, _)| k == "rend") {
                attrs.push(("rend".to_string(), "overstrike".to_string()));
            }
            WalkerAction::Continue
        }
        _ => WalkerAction::Continue,
    }
}

// ---------------------------------------------------------------------------
// Break conversion (br/hr → lb)
// ---------------------------------------------------------------------------

/// Convert line break and horizontal rule tags to `<lb>`.
///
/// - `br` → `lb`
/// - `hr` → `lb`
///   Reference: Trafilatura `htmlprocessing.py:323-325` `convert_line_breaks()`
pub fn tf_convert_breaks(node: &mut DomNode) -> WalkerAction {
    match node {
        DomNode::Element { tag, .. } if matches!(tag.as_str(), "br" | "hr") => {
            tag.clear();
            tag.push_str("lb");
            WalkerAction::Continue
        }
        _ => WalkerAction::Continue,
    }
}

// ---------------------------------------------------------------------------
// Link/details conversion (a → ref, details → div, summary → head)
// ---------------------------------------------------------------------------

/// Convert links and details elements.
///
/// - `a` → `ref`, move `href` → `target`
/// - `details` → `div`
/// - `summary` → `head` with `rend="h3"` (when the summary has no rend of its own)
///   Deliberate deviation from python: python's `convert_details()`
///   (`htmlprocessing.py:334-338`) sets no rend and renders summary-heads as
///   plain newline blocks. We set `rend="h3"` so gen_md renders them as `###`
///   markdown headings (LLM readability, matching FAQ heading levels).
///   Reference: Trafilatura `htmlprocessing.py:334-338` `convert_details()` + `htmlprocessing.py:364-373` `convert_link()`
pub fn tf_convert_refs_and_details(node: &mut DomNode) -> WalkerAction {
    match node {
        DomNode::Element { tag, attrs, .. } if tag == "a" => {
            tag.clear();
            tag.push_str("ref");
            // Rename href -> target, dropping any pre-existing target
            // attribute (e.g. target="_blank") so the converted <ref>
            // carries exactly ONE target: the URL. Keeping both makes
            // attr("target") return whichever comes first in an arbitrary
            // attr order, producing markdown links like [text](_blank).
            let href_val = attrs
                .iter()
                .find(|(k, _)| k == "href")
                .map(|(_, v)| v.clone());
            attrs.retain(|(k, _)| k != "href" && k != "target");
            if let Some(href_val) = href_val {
                attrs.push(("target".to_string(), href_val));
            }
            WalkerAction::Continue
        }
        DomNode::Element { tag, .. } if tag == "details" => {
            tag.clear();
            tag.push_str("div");
            WalkerAction::Continue
        }
        DomNode::Element { tag, attrs, .. } if tag == "summary" => {
            tag.clear();
            tag.push_str("head");
            // Deliberate deviation from python: python's convert_details
            // (htmlprocessing.py:334-338) sets no rend on the summary-head and
            // renders it as a plain newline block. We set rend="h3" so gen_md
            // renders summary-derived heads as `###` markdown headings (LLM
            // readability, matching the FAQ heading level). Only set when the
            // summary has no rend of its own.
            if !attrs.iter().any(|(k, _)| k == "rend") {
                attrs.push(("rend".to_string(), "h3".to_string()));
            }
            WalkerAction::Continue
        }
        _ => WalkerAction::Continue,
    }
}

// ---------------------------------------------------------------------------
// tf_canonicalize_strip_non_content — tf-specific strip with correct list
// ---------------------------------------------------------------------------

/// Remove non-content elements from the DOM tree using the tf-specific strip list.
///
/// The tf strip list intentionally EXCLUDES `<head>` (unlike the rd version)
/// so that converted headings (`<head rend="h1">`) survive.
///
/// Stripped tags: script, style, form, iframe, nav, footer, aside, noscript,
/// meta, link, svg, canvas, template, object, embed.
///
/// Pre: DOM tree is fully parsed.
/// Post: All non-content elements in the strip list are removed.
///   Reference: Trafilatura `htmlprocessing.py:47-79` `tree_cleaning()` (partial)
pub fn tf_canonicalize_strip_non_content(node: &mut DomNode) {
    // NOTE: head is intentionally EXCLUDED from this list.
    // The rd version (rd_strip_non_content) includes head.
    const STRIPPED_TAGS: &[&str] = &[
        "script", "style", "form", "iframe", "nav", "footer", "aside", "noscript", "meta", "link",
        "svg", "canvas", "template", "object", "embed",
    ];
    debug_assert!(!STRIPPED_TAGS.is_empty(), "strip list must not be empty");
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
// tf_canonicalize_unwrap_containers — unwraps 8 tags (div, span, section, article, header, main, body, html); rd_unwrap_structural_wrappers only unwraps 3 (html, head, body)
// ---------------------------------------------------------------------------

/// Unwrap layout container elements by replacing each container node with its
/// child nodes, simplifying the DOM tree for downstream passes.
///
/// Container tags unwrapped: div, span, section, article, header, main, body,
/// html. Note: li, td, th are preserved (needed for list/table rendering).
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
/// Post: Layout containers are unwrapped. Data tables are preserved intact.
/// Note: No direct Python trafilatura equivalent — Rust-specific.
pub fn tf_canonicalize_unwrap_containers(node: &mut DomNode) {
    const CONTAINER_TAGS: &[&str] = &[
        "div", "span", "section", "article", "header", "main", "body", "html",
    ];
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
}

// ---------------------------------------------------------------------------
// Pre-cleaning conversions (run BEFORE tf_remove_cleaned in the pipeline)
// ---------------------------------------------------------------------------

/// Convert `<figure>` elements containing a descendant `<table>` to `<div>`.
///
/// `<figure>` is in `TF_CLEANED_TAGS` (removed entirely by `tf_remove_cleaned`),
/// which destroys figure-wrapped data tables. Python trafilatura prevents this
/// in `htmlprocessing.py:53-56` (issue #301) — when tables are enabled, every
/// `figure[descendant::table]` is renamed to `div` BEFORE tree cleaning:
///
/// ```python
/// # prevent this issue: https://github.com/adbar/trafilatura/issues/301
/// for elem in tree.xpath(".//figure[descendant::table]"):
///     elem.tag = "div"
/// ```
///
/// `elem.tag = "div"` keeps the element's attributes — we do the same.
/// Figures WITHOUT a descendant table stay `<figure>` and are still removed
/// by `tf_remove_cleaned`.
///
/// Implemented as a single bottom-up traversal: each node reports whether its
/// subtree contains a `<table>` and a `<figure>` whose subtree does is renamed
/// on the way back up. Total work is O(n) — one visit per node — instead of
/// the quadratic "walk every figure's subtree" approach. Nested figures both
/// convert (inner first, then outer).
///
/// Pre: DOM tree is fully parsed. Runs BEFORE `tf_remove_cleaned`.
/// Post: Every `<figure>` with a descendant `<table>` is renamed to `<div>`
///       (attributes preserved).
/// Note: Port of Python `htmlprocessing.py:53-56`.
pub fn tf_convert_figure_with_table(node: &mut DomNode) {
    convert_figure_with_table(node);
}

/// Bottom-up recursive helper for [`tf_convert_figure_with_table`].
///
/// Returns whether this subtree contains a `<table>` (either the node itself
/// is a table, or one of its children reported one). A `<figure>` whose
/// subtree contains a table is renamed to `<div>` during the unwind, so
/// nested figures convert inner-first. Visits every node exactly once — O(n).
fn convert_figure_with_table(node: &mut DomNode) -> bool {
    match node {
        DomNode::Element { tag, children, .. } => {
            let mut contains_table = tag == "table";
            for child in children.iter_mut() {
                if convert_figure_with_table(child) {
                    contains_table = true;
                }
            }
            if tag == "figure" && contains_table {
                tag.clear();
                tag.push_str("div");
            }
            contains_table
        }
        _ => false,
    }
}

/// Convert div-based FAQ accordions to semantic `<details><summary>`.
///
/// Many sites (e.g. particula.tech) build FAQ items as:
///
/// ```html
/// <div class="rounded-dropdown ...">
///   <button class="..." aria-expanded="false">
///     <span>Question text</span>
///     <span aria-hidden="true"><svg>...</svg></span>
///   </button>
///   <div class="grid ...">…answer content…</div>
/// </div>
/// ```
///
/// `<button>` is in `TF_CLEANED_TAGS`, so `tf_remove_cleaned` would delete the
/// question text entirely and the answers would jam together. This pass runs
/// BEFORE `tf_remove_cleaned` and restructures the pattern into semantic
/// `<details><summary>`; the existing `tf_convert_refs_and_details` pass then
/// converts `details → div` and `summary → head rend="h3"`.
///
/// Detection is strict to avoid misfiring (see [`first_element_child_idx`] and
/// [`is_accordion_button`]):
/// - The container's first ELEMENT child must be `<button>` with an
///   `aria-expanded` attribute (any value). Leading whitespace text nodes and
///   comments (pretty-printed HTML) are skipped — the button does not have to
///   be the literal first DOM node.
/// - There must be ≥1 following sibling ELEMENT (the content panel).
///   A lone button (e.g. a real "Subscribe" control) is left alone.
/// - Native `<details>`/`<summary>` are untouched by this pass.
///
/// The `<summary>` keeps the button's visible text only (see
/// [`collect_visible_text`]): `<svg>`, `<path>`, `<rect>` and elements
/// carrying `aria-hidden="true"` are dropped recursively, as are all `aria-*`
/// attributes and classes. Buttons whose visible text is empty (icon-only
/// toggles) are left alone — converting them would produce empty `### `
/// headings. Remaining sibling children stay in place (they become the
/// details body; later canonicalization unwraps nested divs).
///
/// The pass runs as a post-order walk (`walk_post_mut`), so each container is
/// examined once; detection is O(children) per container and
/// [`collect_visible_text`] visits each matched button's subtree once (matched
/// buttons are disjoint) — total work is linear in the tree.
///
/// Pre: DOM tree is fully parsed. Runs BEFORE `tf_remove_cleaned`.
/// Post: Accordion containers become `<details>` with a `<summary>` element
///       (at the button's position) carrying the question text.
/// Note: No direct Python trafilatura equivalent — Rust-specific.
pub fn tf_convert_accordion_to_details(node: &mut DomNode) -> WalkerAction {
    match node {
        DomNode::Element { tag, children, .. } => {
            // The button must be the first ELEMENT child (whitespace text and
            // comments are skipped) and carry `aria-expanded`.
            let Some(button_idx) = first_element_child_idx(children) else {
                return WalkerAction::Continue;
            };
            if !is_accordion_button(&children[button_idx]) {
                return WalkerAction::Continue;
            }
            // There must be ≥1 following sibling ELEMENT (the content panel).
            if !children[button_idx + 1..]
                .iter()
                .any(|c| matches!(c, DomNode::Element { .. }))
            {
                return WalkerAction::Continue;
            }
            // Replace the button with a <summary> holding its visible text;
            // skip icon-only toggles (an empty summary renders as a bare `### `).
            let mut question_text = String::new();
            collect_visible_text(&children[button_idx], &mut question_text);
            let question_text = question_text.trim().to_string();
            if question_text.is_empty() {
                return WalkerAction::Continue;
            }
            children[button_idx] = DomNode::Element {
                tag: "summary".to_string(),
                attrs: vec![],
                children: vec![DomNode::Text(question_text)],
                scores: HashMap::new(),
                metadata: HashMap::new(),
            };
            // Rename the container to <details>.
            tag.clear();
            tag.push_str("details");
            WalkerAction::Continue
        }
        _ => WalkerAction::Continue,
    }
}

/// Index of the first ELEMENT child of `children`, skipping text, comment and
/// doctype nodes (pretty-printed HTML interleaves whitespace text and
/// comments between elements).
fn first_element_child_idx(children: &[DomNode]) -> Option<usize> {
    children
        .iter()
        .position(|c| matches!(c, DomNode::Element { .. }))
}

/// Whether `node` is a `<button>` carrying an `aria-expanded` attribute
/// (any value) — the question header of a div-based accordion.
fn is_accordion_button(node: &DomNode) -> bool {
    matches!(node, DomNode::Element { tag, attrs, .. }
        if tag == "button" && attrs.iter().any(|(k, _)| k == "aria-expanded"))
}

/// Collect the visible text content of a node, skipping icon markup
/// (`<svg>`/`<path>`/`<rect>`) and any element carrying `aria-hidden="true"`,
/// recursively. Used to build `<summary>` text that excludes toggle/chevron
/// icons.
fn collect_visible_text(node: &DomNode, buf: &mut String) {
    match node {
        DomNode::Text(t) => buf.push_str(t),
        DomNode::Element {
            tag,
            attrs,
            children,
            ..
        } => {
            let is_hidden = tag == "svg"
                || tag == "path"
                || tag == "rect"
                || attrs
                    .iter()
                    .any(|(k, v)| k == "aria-hidden" && v.trim().eq_ignore_ascii_case("true"));
            if !is_hidden {
                for child in children {
                    collect_visible_text(child, buf);
                }
            }
        }
        DomNode::Comment(_) | DomNode::Doctype(_) => {}
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[path = "../../../tests/unit/pipelines/passes/tf_transforms_test.rs"]
mod tests;
