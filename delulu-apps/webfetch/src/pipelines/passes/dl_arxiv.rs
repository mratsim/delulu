//! arXiv HTML → Markdown pipeline.
//!
//! Specialized passes for cleaning arXiv HTML5 paper pages (LaTeXML output)
//! before markdown lowering. Strips navigation chrome, arXiv headers/footers,
//! and keeps only the article content.

use super::code_blocks::normalize_code_blocks;
use crate::pipelines::{DomNode, WalkerAction, walk_pre_mut};

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Apply all arXiv-specific cleaning passes.
///
/// This is the single entry point for the arXiv pipeline. It strips arXiv
/// chrome (navbars, headers, footers, search UI) and isolates the article
/// content from `<div class="ltx_page_content">`.
pub fn filter_arxiv(node: &mut DomNode) {
    strip_arxiv_chrome(node);
    isolate_ltx_content(node);
    // Normalize code blocks (pre stays pre; language hoisted) so gen_md
    // renders canonical <pre> blocks as fenced code.
    walk_pre_mut(node, &|n| normalize_code_blocks(n));
}

// ---------------------------------------------------------------------------
// Passes
// ---------------------------------------------------------------------------

/// Remove arXiv chrome elements: navbars, headers, footers, logos, overlays.
fn strip_arxiv_chrome(root: &mut DomNode) {
    walk_pre_mut(root, &|node| {
        if let DomNode::Element { tag, attrs, .. } = node {
            let class = get_class(attrs);
            let id = get_id(attrs);

            // arXiv navigation and header chrome
            let is_chrome = match tag.as_str() {
                "nav" => true,
                "header" => class.is_some_and(|c| c.contains("modal-header")),
                "footer" => class.is_some_and(|c| c.contains("modal-footer")),
                "div" => {
                    class.is_some_and(|c| {
                        c.contains("html-header-logo")
                            || c.contains("html-header-nav")
                            || c.contains("ltx_page_header")
                            || c.contains("ltx_page_footer")
                    }) || id == Some("header")
                }
                _ => false,
            };

            if is_chrome {
                *node = DomNode::Text(String::new());
            }
        }
        WalkerAction::Continue
    });
}

/// Isolate the article content by keeping only `<div class="ltx_page_content">`
/// and its descendants. If no such div is found, keeps the whole tree.
fn isolate_ltx_content(root: &mut DomNode) {
    let mut content_div: Option<DomNode> = None;
    collect_content_div(root, &mut content_div);

    if let Some(content) = content_div {
        *root = content;
    }
}

/// Recursively search for `<div class="ltx_page_content">` and extract it.
fn collect_content_div(node: &mut DomNode, result: &mut Option<DomNode>) {
    if result.is_some() {
        return;
    }

    if let DomNode::Element {
        tag,
        attrs,
        children,
        ..
    } = node
    {
        if tag == "div"
            && let Some(class) = get_class(attrs)
            && class.split_whitespace().any(|c| c == "ltx_page_content")
        {
            if children.len() == 1 {
                *result = Some(children.remove(0));
            } else {
                *result = Some(DomNode::Element {
                    tag: "div".to_string(),
                    attrs: Vec::new(),
                    children: std::mem::take(children),
                    scores: std::collections::HashMap::new(),
                    metadata: std::collections::HashMap::new(),
                });
            }
            return;
        }
        for child in children.iter_mut() {
            collect_content_div(child, result);
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Get the value of the `class` attribute.
fn get_class(attrs: &[(String, String)]) -> Option<&str> {
    attrs
        .iter()
        .find(|(k, _)| k == "class")
        .map(|(_, v)| v.as_str())
}

/// Get the value of the `id` attribute.
fn get_id(attrs: &[(String, String)]) -> Option<&str> {
    attrs
        .iter()
        .find(|(k, _)| k == "id")
        .map(|(_, v)| v.as_str())
}

#[cfg(test)]
#[path = "../../../tests/unit/pipelines/passes/dl_arxiv_test.rs"]
mod tests;
