use super::*;
use crate::pipelines::parse_html;
use crate::pipelines::walk_pre_mut;

fn find_tag(node: &DomNode, tag: &str) -> bool {
    match node {
        DomNode::Element {
            tag: t, children, ..
        } if t == tag => true,
        DomNode::Element { children, .. } => children.iter().any(|c| find_tag(c, tag)),
        _ => false,
    }
}

fn find_node_matching<'a>(node: &'a DomNode, tag: &str) -> Option<&'a DomNode> {
    match node {
        DomNode::Element {
            tag: t, children, ..
        } if t == tag => Some(node),
        DomNode::Element { children, .. } => {
            children.iter().find_map(|c| find_node_matching(c, tag))
        }
        _ => None,
    }
}

fn get_attr<'a>(node: &'a DomNode, key: &str) -> Option<&'a str> {
    match node {
        DomNode::Element { attrs, .. } => attrs
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str()),
        _ => None,
    }
}

#[test]
fn test_normalize_code_blocks_pre_stays_pre() {
    // Block code is structurally `<pre>`: normalize_code_blocks does not
    // rename pre -> code (that rename lost block-ness and forced backend hacks).
    let mut root = parse_html("<pre>code here</pre>").unwrap();
    walk_pre_mut(&mut root, &|n| normalize_code_blocks(n));
    assert!(find_tag(&root, "pre"), "pre stays pre (block code)");
    assert!(!find_tag(&root, "code"), "no spurious code element");
}

#[test]
fn test_normalize_code_blocks_with_code_child_unwraps_and_hoists_language() {
    // Canonical pre>code.language-x shape: the code child is unwrapped and the
    // language is hoisted onto the pre's own class.
    let mut root = parse_html(
        r#"<pre class="not-prose"><code class="language-rust">fn main() {}</code></pre>"#,
    )
    .unwrap();
    walk_pre_mut(&mut root, &|n| normalize_code_blocks(n));
    assert!(find_tag(&root, "pre"), "pre stays pre");
    assert!(!find_tag(&root, "code"), "code child unwrapped");
    let pre = find_node_matching(&root, "pre").expect("pre exists");
    assert_eq!(
        get_attr(pre, "class"),
        Some("not-prose language-rust"),
        "language hoisted onto pre class"
    );
    assert!(
        pre.text_content().contains("fn main() {}"),
        "code text spliced into pre"
    );
}

#[test]
fn test_normalize_code_blocks_pre_own_class_language_wins() {
    // The pre's own language class takes precedence over a nested code's.
    let mut root = parse_html(
        r#"<pre class="language-python"><code class="language-rust">print('x')</code></pre>"#,
    )
    .unwrap();
    walk_pre_mut(&mut root, &|n| normalize_code_blocks(n));
    let pre = find_node_matching(&root, "pre").expect("pre exists");
    assert_eq!(get_attr(pre, "class"), Some("language-python"));
}

#[test]
fn test_normalize_code_blocks_inline_code_untouched() {
    // Inline <code> is left as-is.
    let mut root = parse_html("<p>run <code>cmd</code> now</p>").unwrap();
    walk_pre_mut(&mut root, &|n| normalize_code_blocks(n));
    assert!(find_tag(&root, "code"), "inline code stays code");
}
