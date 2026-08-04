use std::collections::HashMap;

use super::MAX_DEPTH;

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
    /// Zero-allocation text length excluding non-visible tags (script, style, svg, canvas, template, noscript).
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
                if matches!(
                    tag.as_str(),
                    "script" | "style" | "svg" | "canvas" | "template" | "noscript"
                ) =>
            {
                0
            }
            DomNode::Element { children, .. } => children
                .iter()
                .map(|c| c.visible_text_len_inner(depth - 1))
                .sum(),
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
            DomNode::Element { children, .. } => children
                .iter()
                .map(|c| c.link_text_len_inner(depth - 1))
                .sum(),
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

    // ---------------------------------------------------------------------------
    // New methods (Phase 0a)
    // ---------------------------------------------------------------------------

    /// Get the value of an attribute by name.
    ///
    /// Returns `None` if the node is not an Element or the attribute doesn't exist.
    pub fn attr(&self, name: &str) -> Option<&str> {
        match self {
            DomNode::Element { attrs, .. } => attrs
                .iter()
                .find(|(k, _)| k == name)
                .map(|(_, v)| v.as_str()),
            _ => None,
        }
    }

    /// Create a new Element node with the given tag and empty children/attrs.
    pub fn new_element(tag: &str) -> Self {
        DomNode::Element {
            tag: tag.to_string(),
            attrs: Vec::new(),
            children: Vec::new(),
            scores: HashMap::new(),
            metadata: HashMap::new(),
        }
    }

    /// Create a new Text node.
    pub fn new_text(text: &str) -> Self {
        DomNode::Text(text.to_string())
    }

    /// Return all descendant nodes (excluding self) in document order.
    ///
    /// Uses pre-order traversal. Returns a `Vec` for simplicity.
    /// Excludes the current node — matches `descendant::` axis semantics.
    pub fn descendants(&self) -> Vec<&DomNode> {
        let mut result = Vec::new();
        self.collect_descendants(&mut result, 0);
        result
    }

    fn collect_descendants<'a>(&'a self, result: &mut Vec<&'a DomNode>, depth: usize) {
        if depth > MAX_DEPTH {
            return;
        }
        if let DomNode::Element { children, .. } = self {
            for child in children {
                result.push(child);
                child.collect_descendants(result, depth + 1);
            }
        }
    }

    /// Evaluate an XPath expression against this node (test-only convenience method).
    ///
    /// Compiles the expression and evaluates it in one call.
    /// Only available in test builds to prevent runtime dynamic compilation.
    ///
    /// Pre: `expr` is a valid XPath expression string.
    /// Post: Returns matching nodes in document order.
    #[cfg(test)]
    #[cfg(feature = "use-xpath")]
    pub fn xpath(
        &self,
        expr: &str,
    ) -> Result<Vec<&DomNode>, crate::pipelines::dom_xpath::XPathError> {
        use crate::pipelines::dom_xpath::XPath;
        let compiled = XPath::compile(expr)?;
        compiled.eval(self)
    }
}
