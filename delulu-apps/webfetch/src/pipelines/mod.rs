use std::collections::HashMap;

use ego_tree::NodeRef;
use scraper::Node as ScraperNode;

use crate::core::types::WebfetchError;

pub use passes::{dl_arxiv, dl_doc};
pub mod error;
pub mod mozilla_readability;
pub mod passes;
pub mod trafilatura;
pub mod walkers;

pub use self::walkers::{PassFn, WalkerAction, walk_post_mut, walk_pre_mut};
pub use error::PipelineError;
// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

// TODO: Add fuzzing guard for large DOM trees

/// Maximum recursion depth for tree walking.
const MAX_DEPTH: usize = 1000;

#[cfg(test)]
use std::sync::atomic::AtomicUsize;

#[cfg(test)]
pub(crate) static TEXT_STATS_TRAVERSAL_COUNT: AtomicUsize = AtomicUsize::new(0);

#[cfg(test)]
pub(crate) static LINK_DENSITY_STATS_TRAVERSAL_COUNT: AtomicUsize = AtomicUsize::new(0);

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
// DomNode methods
// ---------------------------------------------------------------------------

impl DomNode {
    /// Total byte length of all descendant Text nodes. Zero allocation.
    /// Panic-if: Never panics (infallible).
    pub fn text_len(&self) -> usize {
        self.text_len_inner(MAX_DEPTH)
    }

    fn text_len_inner(&self, depth: usize) -> usize {
        if depth == 0 {
            return 0;
        }
        match self {
            DomNode::Text(t) => t.len(),
            DomNode::Element { children, .. } => {
                children.iter().map(|c| c.text_len_inner(depth - 1)).sum()
            }
            DomNode::Comment(_) | DomNode::Doctype(_) => 0,
        }
    }

    /// Concatenated text of all descendant Text nodes (raw, no normalization).
    /// Equivalent to Python lxml's `HtmlElement.text_content()`.
    /// Panic-if: Never panics (infallible).
    pub fn text_content(&self) -> String {
        let mut buf = String::new();
        self.collect_text_inner(&mut buf, MAX_DEPTH);
        buf
    }

    fn collect_text_inner(&self, buf: &mut String, depth: usize) {
        if depth == 0 {
            return;
        }
        match self {
            DomNode::Text(t) => buf.push_str(t),
            DomNode::Element { children, .. } => {
                for child in children {
                    child.collect_text_inner(buf, depth - 1);
                }
            }
            DomNode::Comment(_) | DomNode::Doctype(_) => {}
        }
    }

    /// Total byte length of visible text content (skips <script>, <style>).
    /// Zero allocation. Matches `get_visible_text(node).len()`.
    /// Panic-if: Never panics (infallible).
    pub fn visible_text_len(&self) -> usize {
        self.visible_text_len_inner(MAX_DEPTH)
    }

    fn visible_text_len_inner(&self, depth: usize) -> usize {
        if depth == 0 {
            return 0;
        }
        match self {
            DomNode::Text(t) => t.len(),
            DomNode::Element { tag, children, .. }
                if matches!(tag.as_str(), "script" | "style") => 0,
            DomNode::Element { children, .. } => {
                children.iter().map(|c| c.visible_text_len_inner(depth - 1)).sum()
            }
            DomNode::Comment(_) | DomNode::Doctype(_) => 0,
        }
    }

    /// Total byte length of text inside descendant <a> elements.
    /// Zero allocation. Matches `count_link_text(children)` on a single node.
    /// Panic-if: Never panics (infallible).
    pub fn link_text_len(&self) -> usize {
        self.link_text_len_inner(MAX_DEPTH)
    }

    fn link_text_len_inner(&self, depth: usize) -> usize {
        if depth == 0 {
            return 0;
        }
        match self {
            DomNode::Text(_) => 0,
            DomNode::Element { tag, children, .. } if tag == "a" => {
                children.iter().map(|c| c.text_len_inner(depth - 1)).sum()
            }
            DomNode::Element { children, .. } => {
                children.iter().map(|c| c.link_text_len_inner(depth - 1)).sum()
            }
            DomNode::Comment(_) | DomNode::Doctype(_) => 0,
        }
    }

    // ---------------------------------------------------------------------------
    // text_stats — single-pass (p_text_len, total_text_len)
    // ---------------------------------------------------------------------------

    /// Returns `(p_text_len, total_text_len)` in a single traversal.
    /// `p_text_len` is text inside `<p>` elements; `total_text_len` is all text.
    /// Does NOT skip `<script>`/`<style>` — matches `count_p_text`/`collect_text` behavior.
    /// Panic-if: Never panics (infallible).
    pub fn text_stats(&self) -> (usize, usize) {
        #[cfg(test)]
        crate::pipelines::TEXT_STATS_TRAVERSAL_COUNT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        self.text_stats_inner(false, MAX_DEPTH)
    }

    fn text_stats_inner(&self, in_p: bool, depth: usize) -> (usize, usize) {
        if depth == 0 {
            return (0, 0);
        }
        match self {
            DomNode::Text(t) => {
                let len = t.len();
                if in_p { (len, len) } else { (0, len) }
            }
            DomNode::Element { tag, children, .. } => {
                let is_p = tag == "p";
                let mut p_total = 0usize;
                let mut total = 0usize;
                for child in children {
                    let (p, t) = child.text_stats_inner(in_p || is_p, depth - 1);
                    p_total += p;
                    total += t;
                }
                (p_total, total)
            }
            DomNode::Comment(_) | DomNode::Doctype(_) => (0, 0),
        }
    }

    // ---------------------------------------------------------------------------
    // link_density_stats — single-pass (total_text_len, link_text_len)
    // ---------------------------------------------------------------------------

    /// Returns `(total_text_len, link_text_len)` in a single traversal.
    /// `link_text_len` is text inside `<a>` descendants.
    /// Does NOT skip `<script>`/`<style>` — matches `get_inner_text`/`count_link_text` behavior.
    /// Panic-if: Never panics (infallible).
    pub fn link_density_stats(&self) -> (usize, usize) {
        #[cfg(test)]
        crate::pipelines::LINK_DENSITY_STATS_TRAVERSAL_COUNT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        self.link_density_stats_inner(false, MAX_DEPTH)
    }

    fn link_density_stats_inner(&self, in_link: bool, depth: usize) -> (usize, usize) {
        if depth == 0 {
            return (0, 0);
        }
        match self {
            DomNode::Text(t) => {
                let len = t.len();
                if in_link { (len, len) } else { (len, 0) }
            }
            DomNode::Element { tag, children, .. } => {
                let is_a = tag == "a";
                let mut total = 0usize;
                let mut link_total = 0usize;
                for child in children {
                    let (t, l) = child.link_density_stats_inner(in_link || is_a, depth - 1);
                    total += t;
                    link_total += l;
                }
                (total, link_total)
            }
            DomNode::Comment(_) | DomNode::Doctype(_) => (0, 0),
        }
    }
}

// ---------------------------------------------------------------------------
// Conversion from scraper::Html
// ---------------------------------------------------------------------------

// Conversion from scraper::Html is done via convert_tree() / parse_html().
// From/TryFrom cannot be used here due to orphan rules (both
// scraper::Html and Result/Vec are foreign types).

fn convert_tree(html: &scraper::Html) -> Result<DomNode, WebfetchError> {
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
) -> Result<(), WebfetchError> {
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

// ---------------------------------------------------------------------------
// parse_html
// ---------------------------------------------------------------------------

/// Parse an HTML string into a single root [`DomNode`] tree.
///
/// Strips Document-level noise and returns the `<html>` element as the single root.
/// For empty input, returns a default empty `DomNode::Element { tag: "html", ... }`.
pub fn parse_html(html: &str) -> Result<DomNode, WebfetchError> {
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
#[path = "../../tests/unit/pipelines/mod_test.rs"]
mod tests;
