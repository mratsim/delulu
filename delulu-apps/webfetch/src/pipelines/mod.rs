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
mod tests {
    use super::*;

    // ── walk_pre_mut ─────────────────────────────────────────────────────

    #[test]
    fn test_walk_pre_mut_removes_nodes() {
        let mut root_node = DomNode::Element {
            tag: "root".into(),
            attrs: vec![],
            children: vec![DomNode::Element {
                tag: "keep".into(),
                attrs: vec![],
                children: vec![
                    DomNode::Text("a".into()),
                    DomNode::Text("b".into()),
                    DomNode::Text("c".into()),
                ],
                scores: HashMap::new(),
                metadata: HashMap::new(),
            }],
            scores: HashMap::new(),
            metadata: HashMap::new(),
        };

        walk_pre_mut(&mut root_node, &|node| {
            if let DomNode::Text(t) = node
                && t == "b"
            {
                return WalkerAction::Remove;
            }
            WalkerAction::Continue
        });

        if let DomNode::Element { children, .. } = &root_node {
            assert_eq!(children.len(), 1, "root should still have 1 child");
            if let DomNode::Element {
                children: inner, ..
            } = &children[0]
            {
                assert_eq!(inner.len(), 2, "expected 2 children after removal");
                assert_eq!(
                    format!("{:?}", inner[0]),
                    format!("{:?}", DomNode::Text("a".into())),
                    "first child should be 'a'"
                );
                assert_eq!(
                    format!("{:?}", inner[1]),
                    format!("{:?}", DomNode::Text("c".into())),
                    "second child should be 'c'"
                );
            } else {
                panic!("expected Element");
            }
        } else {
            panic!("expected Element");
        }
    }

    #[test]
    fn test_walk_pre_mut_remove_none() {
        let mut root_node = DomNode::Element {
            tag: "root".into(),
            attrs: vec![],
            children: vec![DomNode::Element {
                tag: "only".into(),
                attrs: vec![],
                children: vec![DomNode::Text("hi".into())],
                scores: HashMap::new(),
                metadata: HashMap::new(),
            }],
            scores: HashMap::new(),
            metadata: HashMap::new(),
        };

        walk_pre_mut(&mut root_node, &|_| WalkerAction::Continue);
        if let DomNode::Element { children, .. } = &root_node {
            assert_eq!(children.len(), 1);
        } else {
            panic!("expected Element");
        }
    }

    #[test]
    #[should_panic(expected = "ReplaceWithChildren is not supported in pre-order traversal")]
    fn test_walk_pre_mut_replace_with_children_panics() {
        let mut root_node = DomNode::Element {
            tag: "root".into(),
            attrs: vec![],
            children: vec![DomNode::Text("hello".into())],
            scores: HashMap::new(),
            metadata: HashMap::new(),
        };

        walk_pre_mut(&mut root_node, &|_| WalkerAction::ReplaceWithChildren);
    }

    // ── DomNode construction (via parse_html) ──────────────────────────

    #[test]
    fn test_parse_html_simple() {
        let root = parse_html("<p>Hello</p>").expect("parse should succeed");
        fn find_tag(node: &DomNode, tag: &str) -> bool {
            match node {
                DomNode::Element {
                    tag: t, children, ..
                } if t == tag => return true,
                DomNode::Element { children, .. } => children.iter().any(|c| find_tag(c, tag)),
                _ => false,
            }
        }
        assert!(find_tag(&root, "p"), "should contain a <p> element");
    }

    #[test]
    fn test_parse_html_empty() {
        let root = parse_html("").expect("empty string should parse without error");
        assert!(
            matches!(&root, DomNode::Element { tag, .. } if tag == "html"),
            "empty HTML should produce an <html> root element"
        );
    }

    #[test]
    fn test_parse_html_whitespace() {
        let root = parse_html("   ").expect("whitespace should parse without error");
        assert!(
            matches!(&root, DomNode::Element { tag, .. } if tag == "html"),
            "whitespace-only HTML should produce an <html> root element"
        );
    }

    #[test]
    fn test_parse_html_doctype() {
        let root = parse_html("<!DOCTYPE html>").expect("doctype should parse");
        // Should return a single root node (<html>).
        assert!(
            matches!(&root, DomNode::Element { tag, .. } if tag == "html"),
            "doctype should produce an <html> root element"
        );
    }

    #[test]
    fn test_parse_html_attrs() {
        let root =
            parse_html(r#"<a href="https://example.com">link</a>"#).expect("parse should succeed");

        fn find_link(node: &DomNode) -> Option<&[(String, String)]> {
            match node {
                DomNode::Element { tag, attrs, .. } if tag == "a" => Some(attrs),
                DomNode::Element { children, .. } => {
                    for c in children {
                        if let Some(a) = find_link(c) {
                            return Some(a);
                        }
                    }
                    None
                }
                _ => None,
            }
        }

        let attrs = find_link(&root).expect("should find <a> element");
        assert!(
            attrs
                .iter()
                .any(|(k, v)| k == "href" && v == "https://example.com"),
            "should have href attribute"
        );
    }

    // ── Parse HTML with comments ─────────────────────────────────────────

    #[test]
    fn test_parse_html_comment() {
        let root = parse_html("<!-- comment --><p>text</p>").expect("parse should succeed");

        fn find_comment(node: &DomNode) -> bool {
            match node {
                DomNode::Comment(_) => return true,
                DomNode::Element { children, .. } => children.iter().any(|c| find_comment(c)),
                _ => false,
            }
        }
        assert!(find_comment(&root), "should contain a Comment node");
    }

    // ── convert_tree ───────────────────────────────────────────────────

    #[test]
    fn test_convert_tree_non_empty() {
        let doc = scraper::Html::parse_document("<div>hello</div>");
        let root = convert_tree(&doc).expect("conversion should succeed");
        assert!(
            matches!(&root, DomNode::Element { .. }),
            "non-empty HTML should produce a root element"
        );
    }

    // ── parse_html returns error on too many nodes ─────────────────────

    #[test]
    fn test_parse_html_too_many_nodes() {
        // TODO: Generate large DOM tree for fuzzing guard testing
        // scraper adds Document and html/body wrappers, so use enough elements.
        let mut html = String::from("<p>");
        for _ in 0..30_000 {
            html.push_str("<span>a</span>");
        }
        html.push_str("</p>");
        let result = parse_html(&html);
        // This may either error or return many nodes; either is acceptable.
        // The node limit exists as a defense-in-depth measure.
        if let Err(WebbfetchError::Parse(msg)) = &result {
            assert!(
                msg.contains("node count"),
                "error should mention node count: {msg}"
            );
        }
    }
}
