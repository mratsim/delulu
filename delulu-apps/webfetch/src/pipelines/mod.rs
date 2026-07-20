use std::collections::HashMap;

use ego_tree::NodeRef;
use scraper::Node as ScraperNode;

use crate::core::types::WebbfetchError;

pub mod error;
pub mod mozilla_readability;
pub mod passes;
pub mod trafilatura;
pub mod walkers;

pub use self::walkers::{PassFn, WalkerAction, walk_pre_mut};
pub use error::PipelineError;
// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

// TODO: Add fuzzing guard for large DOM trees

/// Maximum recursion depth for tree walking.
const MAX_DEPTH: usize = 1000;

// ---------------------------------------------------------------------------
// DomNode
// ---------------------------------------------------------------------------

/// A node in the DOM intermediate representation.
///
/// This is the node type that all pipeline passes operate on. It can be
/// constructed from a [`scraper::Html`] tree via `parse_html()` or
/// `convert_tree()`.
#[derive(Debug, Clone)]
pub enum DomNode {
    Element {
        tag: String,
        attrs: Vec<(String, String)>,
        children: Vec<DomNode>,
        scores: HashMap<String, f64>,
        metadata: HashMap<String, String>,
    },
    Text(String),
    Comment(String),
    Doctype(String),
}

// ---------------------------------------------------------------------------
// Conversion from scraper::Html
// ---------------------------------------------------------------------------

// Conversion from scraper::Html is done via convert_tree() / parse_html().
// From/TryFrom cannot be used here due to orphan rules (both
// scraper::Html and Result/Vec are foreign types).

fn convert_tree(html: &scraper::Html) -> Result<DomNode, WebbfetchError> {
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

fn convert_node(
    node: NodeRef<'_, ScraperNode>,
    result: &mut Vec<DomNode>,
    depth: usize,
    total: &mut usize,
) -> Result<(), WebbfetchError> {
    *total += 1;
    // TODO: fuzz/hardening — total is incremented but never enforced. Add a
    // MAX_NODES check that returns Err when exceeded (DoS guard for deeply
    // nested / massive HTML documents).
    // See https://github.com/mratsim/delulu/pull/7
    if depth > MAX_DEPTH {
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
            let attrs: Vec<(String, String)> = element
                .attrs()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect();

            let mut children = Vec::new();
            for (i, child) in node.children().enumerate() {
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

// ---------------------------------------------------------------------------
// parse_html
// ---------------------------------------------------------------------------

/// Parse an HTML string into a single root [`DomNode`] tree.
///
/// Strips Document-level noise and returns the `<html>` element as the single root.
/// For empty input, returns a default empty `DomNode::Element { tag: "html", ... }`.
pub fn parse_html(html: &str) -> Result<DomNode, WebbfetchError> {
    // Guard: empty HTML should return a default empty html element.
    if html.trim().is_empty() {
        return Ok(DomNode::Element {
            tag: "html".to_string(),
            attrs: Vec::new(),
            children: Vec::new(),
            scores: HashMap::new(),
            metadata: HashMap::new(),
        });
    }
    let doc = scraper::Html::parse_document(html);
    convert_tree(&doc)
}

#[cfg(test)]
#[path = "mod_test.rs"]
mod tests;
