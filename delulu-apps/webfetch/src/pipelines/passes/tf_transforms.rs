use crate::pipelines::DomNode;
use crate::pipelines::passes::code_blocks::push_language_class;
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
// Quote conversion (blockquote, q)
// ---------------------------------------------------------------------------

/// Convert quotation tags.
///
/// - `blockquote`/`q` → `quote`
///   Reference: Trafilatura `htmlprocessing.py:287-303` `convert_quotes()`
///
/// Code blocks are handled by the separate [`normalize_code_blocks`] pass —
/// this pass only deals with quotations.
pub fn tf_convert_quotes(node: &mut DomNode) -> WalkerAction {
    match node {
        DomNode::Element { tag, .. } if matches!(tag.as_str(), "blockquote" | "q") => {
            tag.clear();
            tag.push_str("quote");
            WalkerAction::Continue
        }
        _ => WalkerAction::Continue,
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

/// ⚠️ CUSTOM PASS — not present in reference trafilatura (v2.1.0).
/// Improves fixture: `blog/particula.tech/sglang-vs-vllm-inference-engine-comparison`
/// (and any accordion-style `<details>`/`<summary>` content). The `a → ref` link
/// conversion is a 1:1 port of trafilatura's `convert_link()`, but the
/// `details`/`summary` *preservation* below deliberately overrides reference
/// trafilatura's `convert_details()` (`htmlprocessing.py:334-338`), which flattens
/// `details → div` and `summary → head`. We keep them so the accordion pass and
/// the generators can emit native collapsible markup.
/// TODO: create a custom webfetch pipeline so that we can have the proper canonical
/// trafilatura behavior in the trafilatura pipeline (this logic belongs in a webfetch-specific
/// pipeline, not baked into the TF pipeline).
/// Convert links and details elements.
///
/// - `a` → `ref`, move `href` → `target`
/// - `details`/`summary` are LEFT AS-IS so the generators can emit them:
///   gen_md renders `<details><summary>` as a raw-HTML block (GFM renders
///   collapsible FAQ items) and gen_html emits the real tags (browsers get
///   the native accordion). This is a deliberate deviation from python
///   trafilatura, whose `convert_details()` (`htmlprocessing.py:334-338`)
///   flattens `details → div` and `summary → head` — which is exactly why
///   `tf_convert_accordion_to_details` would be pointless: it converts a
///   div-accordion INTO `<details><summary>` only to have this pass flatten
///   it right back. The accordion structure IS the end state.
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
        // details/summary: preserved as-is (see doc comment).
        DomNode::Element { tag, .. } if tag == "details" || tag == "summary" => {
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

/// ⚠️ CUSTOM PASS — not present in reference trafilatura (v2.1.0).
/// Improves fixture: `blog/particula.tech/sglang-vs-vllm-inference-engine-comparison`
/// (the `<div>` TL;DR label must be demoted to a `<p>`, not unwrapped into loose
/// text that jams onto the following paragraph).
/// TODO: create a custom webfetch pipeline so that we can have the proper canonical
/// trafilatura behavior in the trafilatura pipeline (this logic belongs in a webfetch-specific
/// pipeline, not baked into the TF pipeline).
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

    // Tags that make a container a block-level layout wrapper. A `<div>`
    // whose children are all inline (text/spans/links) is a *paragraph*, not
    // layout — see the `tag == "div"` arm below.
    const BLOCK_TAGS: &[&str] = &[
        "p",
        "div",
        "section",
        "article",
        "header",
        "footer",
        "main",
        "body",
        "html",
        "ul",
        "ol",
        "list",
        "item",
        "table",
        "tr",
        "td",
        "th",
        "row",
        "cell",
        "pre",
        "blockquote",
        "quote",
        "h1",
        "h2",
        "h3",
        "h4",
        "h5",
        "h6",
        "head",
        "details",
        "figure",
        "dl",
        "form",
        "nav",
        "aside",
        "hr",
    ];

    fn is_inline_content(n: &DomNode) -> bool {
        match n {
            DomNode::Element { tag, .. } => !BLOCK_TAGS.contains(&tag.as_str()),
            // Text/comments are inline content.
            _ => true,
        }
    }

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
            DomNode::Element { tag, children, .. } => {
                if is_data_table {
                    WalkerAction::Continue
                } else if tag == "table" && has_is_data_table_key {
                    // Explicitly marked as layout (is_data_table set to non-true) — unwrap
                    WalkerAction::ReplaceWithChildren
                } else if tag == "div"
                    && children.iter().all(|c| is_inline_content(c))
                    && children.iter().any(|c| !c.text_content().trim().is_empty())
                {
                    // A div whose children are all inline is a paragraph,
                    // not a layout container. Unwrapping it would demote the
                    // text to loose nodes that jam onto the next block
                    // (a "TL;DR" label div before the summary paragraph
                    // rendered as "TL;DRSGLang's..."). Rename to <p> to keep
                    // the paragraph boundary. The non-empty guard evaluates each
                    // child's RENDERED text so an inline wrapper holding only
                    // whitespace (e.g. <span> </span>) is not treated as a real
                    // paragraph.
                    tag.clear();
                    tag.push_str("p");
                    WalkerAction::Continue
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
    /// Returns whether this subtree contains a `<table>` (either the node
    /// itself is a table, or one of its children reported one). A `<figure>`
    /// whose subtree contains a table is renamed to `<div>` during the unwind,
    /// so nested figures convert inner-first. Visits every node once — O(n).
    fn convert(node: &mut DomNode) -> bool {
        match node {
            DomNode::Element { tag, children, .. } => {
                let mut contains_table = tag == "table";
                for child in children.iter_mut() {
                    if convert(child) {
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
    convert(node);
}

/// Page-level layout/layout-chrome tags that an accordion-style `<details>`
/// conversion must NEVER apply to. Conversion is open to any other container
/// tag (div, li, item, dd, dt, section, article, ...) because real FAQ
/// wrappers frequently use semantic content elements such as `<section>` /
/// `<article>`; the pass's strict detection (first ELEMENT child is a
/// `<button aria-expanded>` with >=1 following sibling ELEMENT) already makes
/// page-level false positives very unlikely. Only unambiguous page chrome is
/// excluded here.
const PAGE_LEVEL_TAGS: [&str; 8] = [
    "body", "main", "html", "header", "footer", "nav", "aside", "form",
];

fn is_accordion_container(tag: &str) -> bool {
    !PAGE_LEVEL_TAGS.contains(&tag)
}

/// ⚠️ CUSTOM PASS — not present in reference trafilatura (v2.1.0).
/// Improves fixture: `blog/particula.tech/sglang-vs-vllm-inference-engine-comparison`
/// (div-based FAQ accordions with `<button aria-expanded>` headers).
/// TODO: create a custom webfetch pipeline so that we can have the proper canonical
/// trafilatura behavior in the trafilatura pipeline (this logic belongs in a webfetch-specific
/// pipeline, not baked into the TF pipeline).
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
/// - The container must not be a page-level layout tag (`body`, `main`, `header`,
///   `nav`, `aside`, ...). Item containers (div, li, section, article, ...)
///   convert to `<details>`. Page-level wrappers are never converted.
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
        // Skip page-level layout wrappers (body/main/header/nav/aside/...) so
        // they are never turned into <details>; real FAQ item containers (div,
        // li, section, article, ...) convert normally.
        DomNode::Element { tag, children, .. } if is_accordion_container(tag) => {
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
// Code header label → pre language (chrome removal)
// ---------------------------------------------------------------------------

/// ⚠️ CUSTOM PASS — not present in reference trafilatura (v2.1.0).
/// Improves fixture: `blog/particula.tech/sglang-vs-vllm-inference-engine-comparison`
/// (chrome header-bar language pills hoisted onto the `<pre>` fence info).
/// TODO: create a custom webfetch pipeline so that we can have the proper canonical
/// trafilatura behavior in the trafilatura pipeline (this logic belongs in a webfetch-specific
/// pipeline, not baked into the TF pipeline).
/// Recognize code-block header labels (e.g. a "BASH" pill in a code header
/// bar) that sit as a sibling immediately before a `<pre>` and hoist them
/// onto the pre's class as `language-<name>`.
///
/// Sites like particula.tech render code blocks as a chrome header bar
/// (macOS traffic lights + a language pill like "BASH" + a copy button)
/// followed by a bare `<pre>` with NO language class and no nested `<code>`:
///
/// ```html
/// <div class="…header bar…"><span>BASH</span><button>…copy…</button></div>
/// <pre>pip install sglang[all]</pre>
/// ```
///
/// Without this pass the markdown output carries a stray "BASH" paragraph
/// and a fence with an empty info string. This pass runs AFTER
/// [`tf_canonicalize_unwrap_containers`], which converts the header-bar div
/// (whose children are all inline) into a `<p>`/`<span>` that RETAINS the
/// header bar's `class` attribute. The pass treats any `<p>`/`<span>`/`<div>`
/// that sits immediately before a `<pre>` and whose text is a single known
/// language name (see [`code_label_language`]) as a code-header label: the
/// language is appended to the pre's class and the label element is deleted
/// (its content is fully represented by the fence info string). The single
/// known-language token is the discriminator.
///
/// Known residual limitation (undecidable from DOM alone): a genuine content
/// element that is a single known language word placed immediately before a
/// `<pre>` — such as a bare `<p>Go</p>` or a `<div>Python</div>` section
/// header — is indistinguishable from a chrome label and is therefore
/// treated as chrome: it is hoisted onto the pre and removed. This is the
/// accepted tradeoff that allows Tailwind/utility-class language pills (e.g.
/// particula.tech's `<span class="text-ink-label">BASH</span>`, whose class
/// carries no chrome token) to be recognized. A `<pre>` that already carries
/// a `language-*` token never gets its label deleted either.
///
/// Pre: DOM tree is fully parsed; `unwrap_containers` has run (the label is
///      a direct sibling of the pre).
/// Post: The pre carries `language-<name>`; the chrome label element is
///      removed.
/// Note: No direct Python trafilatura equivalent — Rust-specific.
pub fn tf_convert_code_header_label(node: &mut DomNode) {
    let mut filter = |n: &mut DomNode| -> WalkerAction {
        if let DomNode::Element { children, .. } = n {
            let mut i = 0;
            while i < children.len() {
                let is_pre = matches!(&children[i], DomNode::Element { tag, .. } if tag == "pre");
                if is_pre && let Some(label_idx) = prev_label_sibling(children, i) {
                    // If the pre already carries a language token, the label
                    // is legitimate content — never delete it / never overwrite.
                    let pre_has_language = matches!(&children[i], DomNode::Element { attrs, .. } if attrs
                        .iter()
                        .filter(|(k, _)| k == "class")
                        .any(|(_, v)| v.split_whitespace().any(|t| t.starts_with("language-"))));
                    let label_text = children[label_idx].text_content();
                    if !pre_has_language && let Some(lang) = code_label_language(label_text.trim())
                    {
                        if let DomNode::Element { attrs, .. } = &mut children[i] {
                            push_language_class(attrs, &lang);
                        }
                        children.remove(label_idx);
                        // The pre shifted down one slot; re-scan it.
                        i -= 1;
                        continue;
                    }
                }
                i += 1;
            }
        }
        WalkerAction::Continue
    };
    // walk_post_mut never runs filters on the ROOT node itself — apply the
    // filter to the root explicitly so a <pre> whose label sits directly
    // under <html> (or any root) is still handled.
    let _ = filter(node);
    let mut filters: Vec<&mut WalkerFilter> = vec![&mut filter];
    walk_post_mut(node, &mut filters, None);
}

/// Index of the nearest preceding sibling of `children[i]` that is a
/// code-header label element: a `<p>`, `<span>`, or `<div>` whose text is a
/// single known language name, skipping whitespace-only text nodes and
/// comments (pretty-printed HTML). The single known-language token (see
/// [`code_label_language`]) is the only discriminator: any `p`/`span`/`div`
/// immediately before a `<pre>` is a label candidate regardless of its
/// `class`, so Tailwind/utility-class pills (e.g. `class="text-ink-label"`)
/// are recognized. Returns `None` when the nearest preceding sibling is any
/// other element (e.g. a `head`, `pre`, `li`, `summary`), or a non-whitespace
/// text node.
fn prev_label_sibling(children: &[DomNode], i: usize) -> Option<usize> {
    let mut j = i;
    while j > 0 {
        j -= 1;
        match &children[j] {
            DomNode::Text(t) if t.trim().is_empty() => continue,
            DomNode::Comment(_) | DomNode::Doctype(_) => continue,
            DomNode::Element {
                tag, children: _, ..
            } if matches!(tag.as_str(), "p" | "span" | "div") => {
                return Some(j);
            }
            _ => return None,
        }
    }
    None
}

/// Map a single-token code header label ("BASH", "Python", …) to a fence
/// info string, or `None` if it is not a known language name.
fn code_label_language(label: &str) -> Option<String> {
    if label.split_whitespace().count() != 1 {
        return None;
    }
    let lower = label.to_lowercase();
    let lang = match lower.as_str() {
        "bash" | "sh" | "shell" | "zsh" => "bash",
        "python" | "py" => "python",
        "rust" | "rs" => "rust",
        "javascript" | "js" => "javascript",
        "typescript" | "ts" => "typescript",
        "json" => "json",
        "yaml" | "yml" => "yaml",
        "toml" => "toml",
        "xml" => "xml",
        "html" => "html",
        "css" | "scss" | "sass" => "css",
        "sql" => "sql",
        "go" | "golang" => "go",
        "java" => "java",
        "c" => "c",
        "c++" | "cpp" | "cxx" => "cpp",
        "c#" | "csharp" | "cs" => "csharp",
        "ruby" | "rb" => "ruby",
        "php" => "php",
        "swift" => "swift",
        "kotlin" | "kt" => "kotlin",
        "scala" => "scala",
        "perl" => "perl",
        "lua" => "lua",
        "r" => "r",
        "matlab" => "matlab",
        "powershell" | "ps1" => "powershell",
        "bat" | "cmd" => "bat",
        "dockerfile" => "dockerfile",
        "makefile" => "makefile",
        "terraform" | "hcl" => "hcl",
        "graphql" => "graphql",
        "diff" => "diff",
        "markdown" | "md" => "markdown",
        "text" | "txt" | "plain" | "plaintext" => "text",
        _ => return None,
    };
    Some(lang.to_string())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[path = "../../../tests/unit/pipelines/passes/tf_transforms_test.rs"]
mod tests;
