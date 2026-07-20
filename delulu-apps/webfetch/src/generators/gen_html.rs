use crate::pipelines::DomNode;

/// Reconstruct HTML from a DomNode tree (for webfetch_raw).
pub fn dom_nodes_to_html(node: &DomNode) -> String {
    node_to_html(node)
}

const VOID_ELEMENTS: &[&str] = &[
    "area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "param", "source",
    "track", "wbr",
];

fn escape_html_text(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn escape_html_attr(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn node_to_html(node: &DomNode) -> String {
    match node {
        DomNode::Text(text) => escape_html_text(text),
        DomNode::Comment(text) => format!("<!--{}-->", text),
        DomNode::Doctype(text) => format!("<!DOCTYPE {}>", text),
        DomNode::Element {
            tag,
            attrs,
            children,
            ..
        } => {
            let attrs_str: String = attrs
                .iter()
                .map(|(k, v)| format!(" {}=\"{}\"", k, escape_html_attr(v)))
                .collect();
            let children_html: String = children.iter().map(node_to_html).collect();
            if VOID_ELEMENTS.contains(&tag.as_str()) {
                format!("<{}{}>", tag, attrs_str)
            } else {
                format!("<{}{}>{}</{}>", tag, attrs_str, children_html, tag)
            }
        }
    }
}
