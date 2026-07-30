use crate::pipelines::DomNode;
use crate::pipelines::walkers::{WalkerAction, WalkerFilter, walk_post_mut, walk_pre_mut};
use super::tf_filters::{count_p_text, collect_text, MIN_EXTRACTED_SIZE};

// ---------------------------------------------------------------------------
// Heading conversion (h1-h6 → head)
// ---------------------------------------------------------------------------

/// Convert heading tags (h1-h6) to `<head>` with a `rend` attribute.
///
/// - `h1` → `<head rend="h1">`
/// - `h3` → `<head rend="h3">`
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
pub fn tf_convert_refs_and_details(node: &mut DomNode) -> WalkerAction {
    match node {
        DomNode::Element { tag, attrs, .. } if tag == "a" => {
            tag.clear();
            tag.push_str("ref");
            // Rename href to target
            if let Some(href_pos) = attrs.iter().position(|(k, _)| k == "href") {
                let href_val = attrs[href_pos].1.clone();
                attrs[href_pos].0 = "target".to_string();
                // Keep the value as-is
                attrs[href_pos].1 = href_val;
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
pub fn tf_canonicalize_strip_non_content(node: &mut DomNode) {
    // NOTE: head is intentionally EXCLUDED from this list.
    // The rd version (rd_strip_non_content) includes head.
    const STRIPPED_TAGS: &[&str] = &[
        "script", "style", "form", "iframe", "nav", "footer", "aside", "noscript", "meta", "link",
        "svg", "canvas", "template", "object", "embed",
    ];
    debug_assert!(!STRIPPED_TAGS.is_empty(), "strip list must not be empty");
    let mut filter = |n: &mut DomNode| -> WalkerAction {
        if let DomNode::Element { tag, metadata, .. } = n
            && STRIPPED_TAGS.contains(&tag.as_str())
            && !(tag == "form"
                && metadata.get("tf_protected").map(|v| v.as_str()) == Some("true"))
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
        let is_protected_form = matches!(n, DomNode::Element { tag, metadata, .. }
            if tag == "form"
                && metadata.get("tf_protected").map(|v| v.as_str()) == Some("true"));

        match n {
            DomNode::Element { tag, .. } => {
                if is_protected_form {
                    WalkerAction::ReplaceWithChildren
                } else if is_data_table {
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
// ---------------------------------------------------------------------------
// Div-to-paragraph conversion — ptest + div fallback (Python main_extractor.py:765-768)
// ---------------------------------------------------------------------------
// Div-to-paragraph conversion — ptest + div fallback (Python main_extractor.py:765-768)
// ---------------------------------------------------------------------------
//
/// Convert `<div>` elements to `<p>` when `<p>` text in the tree is sparse.
///
/// Python equivalent (main_extractor.py:765-768):
/// ```python
/// ptest = subtree.xpath("//p//text()")
/// factor = 1 if options.focus == "precision" else 3
/// if not ptest or len("".join(ptest)) < options.min_extracted_size * factor:
///     potential_tags.add("div")
/// ```
///
/// Then in `handle_other_elements` (line 285-309):
/// - If `"div"` is in `potential_tags` and the div has meaningful text,
///   its tag is changed from `"div"` to `"p"`.
///
/// This pass:
/// 1. Counts `<p>` text in the tree
/// 2. If p_text < MIN_EXTRACTED_SIZE * 3 (750 chars for balanced mode),
///    converts `<div>` elements with meaningful text into `<p>` elements
/// 3. Only converts divs that are NOT data tables (is_data_table != "true")
/// 4. Only converts divs with > 50 chars of text content
/// 5. Skips divs that contain block-level elements (p, div, table, ul, ol, dl,
///    blockquote, pre, figure, h1-h6) to avoid creating nested <p> structures
///
/// Must run AFTER container isolation but BEFORE `tf_canonicalize_unwrap_containers`
/// so that the converted `<p>` elements survive the unwrap pass.
pub fn tf_convert_divs_to_paragraphs(node: &mut DomNode) {
    // Count <p> text in the tree
    if let DomNode::Element { children, .. } = node {
        let p_text_len = count_p_text(children);
        let threshold = MIN_EXTRACTED_SIZE * 3; // 750 chars for balanced mode

        if p_text_len < threshold {
            // Walk the tree and convert <div> elements to <p>
            walk_pre_mut(node, &|n: &mut DomNode| {
                if let DomNode::Element { tag, attrs, children, metadata, .. } = n {
                    if tag == "div" {
                        // Skip data tables
                        let is_data_table = metadata.get("is_data_table")
                            .map(|v| v == "true")
                            .unwrap_or(false);
                        if is_data_table {
                            return WalkerAction::Continue;
                        }

                        // Only convert leaf divs: skip divs that contain block-level elements
                        // (p, div, table, ul, ol, dl, blockquote, pre, figure, h1-h6)
                        let has_block_child = children.iter().any(|child| {
                            matches!(child, DomNode::Element { tag: ct, .. } if matches!(
                                ct.as_str(),
                                "p" | "div" | "table" | "ul" | "ol" | "dl"
                                    | "blockquote" | "pre" | "figure"
                                    | "h1" | "h2" | "h3" | "h4" | "h5" | "h6"
                            ))
                        });
                        if has_block_child {
                            return WalkerAction::Continue;
                        }

                        // Check if div has meaningful text content (> 50 chars)
                        // Use flattened text from all descendants since leaf divs
                        // only contain inline content (text, spans, anchors, etc.)
                        let text = collect_text(children);
                        let trimmed = text.trim();
                        if trimmed.len() > 50 {
                            // Convert div to p (Python: processed_element.tag = "p")
                            tag.clear();
                            tag.push_str("p");
                            // Clear attributes (Python: processed_element.attrib.clear())
                            attrs.clear();
                        }
                    }
                }
                WalkerAction::Continue
            });
        }
    }
}
// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[path = "../../../tests/unit/pipelines/passes/tf_transforms_test.rs"]
mod tests;