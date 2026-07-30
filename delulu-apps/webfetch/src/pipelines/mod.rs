use std::collections::HashMap;

use crate::core::types::WebfetchError;

pub use passes::{dl_arxiv, dl_doc};
pub mod dom_convert;
pub mod dom_nodes;
pub mod error;
pub mod mozilla_readability;
pub mod passes;
pub mod trafilatura;
pub mod walkers;

#[cfg(feature = "use-xpath")]
pub mod dom_xpath;
#[cfg(feature = "use-xpath")]
pub use dom_xpath::{XPath, XPathError};

pub use self::dom_nodes::DomNode;
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
    dom_convert::convert_tree(&doc)
}

#[cfg(test)]
#[path = "../../tests/unit/pipelines/mod_test.rs"]
mod tests;
