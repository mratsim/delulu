use std::collections::HashMap;

use ego_tree::NodeRef;
use scraper::Node as ScraperNode;

use crate::core::types::WebfetchError;
use crate::pipelines::DomNode;

use super::MAX_DEPTH;

// ---------------------------------------------------------------------------
// Conversion from scraper::Html
// ---------------------------------------------------------------------------

// Conversion from scraper::Html is done via convert_tree() / parse_html().
// From/TryFrom cannot be used here due to orphan rules (both
// scraper::Html and Result/Vec are foreign types).

pub(crate) fn convert_tree(html: &scraper::Html) -> Result<DomNode, WebfetchError> {
    let root = html.tree.root();
    let mut nodes = Vec::new();
    let mut total = 0usize;
    for child in root.children() {
        convert_node(child, &mut nodes, 0, &mut total)?;
    }
    // Find the <html> element among the root-level nodes.
    // If it exists, return it as the single root.
    // Otherwise, wrap all nodes in a synthetic <html> element.
    for node in &nodes {
        if matches!(node, DomNode::Element { tag, .. } if tag == "html") {
            return Ok(node.clone());
        }
    }
    // No <html> found — wrap everything in a synthetic <html>.
    Ok(DomNode::Element {
        tag: "html".to_string(),
        attrs: Vec::new(),
        children: nodes,
        scores: HashMap::new(),
        metadata: HashMap::new(),
    })
}

pub(crate) fn convert_node(
    node: NodeRef<'_, ScraperNode>,
    result: &mut Vec<DomNode>,
    depth: usize,
    total: &mut usize,
) -> Result<(), WebfetchError> {
    *total += 1;
    // TODO: fuzz/hardening — total is incremented but never enforced. Add a
    // MAX_NODES check that returns Err when exceeded (DoS guard for deeply
    // nested / massive HTML documents).
    // See https://github.com/mratsim/delulu/pull/7
    if depth > MAX_DEPTH {
        // TODO side-effect to push to main: tracing::* logging in lib
        tracing::warn!(
            "DOM recursion depth exceeded {} at tag depth {}, flattening further nesting",
            MAX_DEPTH,
            depth,
        );
        return Ok(());
    }

    match node.value() {
        ScraperNode::Document | ScraperNode::Fragment => {
            for child in node.children() {
                convert_node(child, result, depth, total)?;
            }
        }
        ScraperNode::Element(element) => {
            let tag = element.name().to_string();
            let mut attrs: Vec<(String, String)> = element
                .attrs()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect();
            // scraper stores attributes in a hash map (ahash) whose iteration
            // order is randomized per process — serializing that order made the
            // HTML/markdown output non-deterministic run-to-run. Sort by key so
            // every run produces identical output (keys are already lowercase).
            attrs.sort_by(|a, b| a.0.cmp(&b.0));

            let mut children = Vec::new();
            for child in node.children() {
                // TODO: Add fuzzing guard for large DOM trees
                // Pass-through for now
                convert_node(child, &mut children, depth + 1, total)?;
            }

            result.push(DomNode::Element {
                tag,
                attrs,
                children,
                scores: HashMap::new(),
                metadata: HashMap::new(),
            });
        }
        ScraperNode::Text(text) => {
            result.push(DomNode::Text(text.to_string()));
        }
        ScraperNode::Comment(comment) => {
            result.push(DomNode::Comment(comment.to_string()));
        }
        ScraperNode::Doctype(doctype) => {
            result.push(DomNode::Doctype(doctype.name().to_string()));
        }
        ScraperNode::ProcessingInstruction(_) => {
            // Processing instructions are skipped.
        }
    }

    Ok(())
}
