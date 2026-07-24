//! Document (xberg) HTML → Markdown pipeline.
//!
//! Specialized passes for cleaning generic document HTML (from xberg PDF/text
//! conversion) before markdown lowering. Strips scripts, styles, and empty
//! elements while preserving void elements like `<img>`, `<br>`, `<hr>`, `<wbr>`.

use crate::pipelines::{DomNode, WalkerAction, walk_post_mut};

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Apply all document-specific cleaning passes.
///
/// This is the single entry point for the xberg document pipeline. It:
/// - Removes all `<script>` and `<style>` elements
/// - Removes empty elements (elements with no children and no text content)
/// - Preserves void elements: `<img>`, `<br>`, `<hr>`, `<wbr>`
pub fn filter_doc(node: &mut DomNode) {
    // Post-order walk: children processed before parent, so empty
    // cascades (parent emptied by child removal) are handled in one pass.
    let mut filter = |node: &mut DomNode| {
        if let DomNode::Element { tag, children, .. } = node {
            let tag_lower = tag.to_lowercase();

            // Remove script and style elements entirely
            if tag_lower == "script" || tag_lower == "style" {
                return WalkerAction::Remove;
            }

            // Remove empty elements (no children, no text descendants)
            // but preserve void/inline-void elements
            if children.is_empty() && !is_void_element(&tag_lower) {
                return WalkerAction::Remove;
            }
        }
        WalkerAction::Continue
    };
    walk_post_mut(node, &mut [&mut filter], None);
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Returns `true` for HTML void elements that should be preserved even when empty.
fn is_void_element(tag: &str) -> bool {
    matches!(
        tag,
        "img"
            | "br"
            | "hr"
            | "wbr"
            | "input"
            | "meta"
            | "link"
            | "area"
            | "base"
            | "col"
            | "embed"
            | "source"
            | "track"
    )
}

#[cfg(test)]
#[path = "../../../tests/unit/pipelines/passes/dl_doc_test.rs"]
mod tests;
