use crate::pipelines::DomNode;
use crate::pipelines::walkers::{WalkerAction, WalkerFilter, walk_post_mut};

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

/// Convert quotation and code tags.
///
/// - `blockquote` → `quote`
/// - `pre` → `code`
/// - `q` → `quote`
///   Reference: Trafilatura `htmlprocessing.py:287-303` `convert_quotes()`
pub fn tf_convert_quotes(node: &mut DomNode) -> WalkerAction {
    match node {
        DomNode::Element { tag, .. } if matches!(tag.as_str(), "blockquote" | "q") => {
            tag.clear();
            tag.push_str("quote");
            WalkerAction::Continue
        }
        DomNode::Element { tag, .. } if tag == "pre" => {
            tag.clear();
            tag.push_str("code");
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

/// Convert links and details elements.
///
/// - `a` → `ref`, move `href` → `target`
/// - `details` → `div`
/// - `summary` → `head`
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
        DomNode::Element { tag, .. } if tag == "summary" => {
            tag.clear();
            tag.push_str("head");
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
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[path = "../../../tests/unit/pipelines/passes/tf_transforms_test.rs"]
mod tests;
