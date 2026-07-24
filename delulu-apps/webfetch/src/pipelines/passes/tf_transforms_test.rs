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

fn get_attr<'a>(node: &'a DomNode, key: &str) -> Option<&'a str> {
    match node {
        DomNode::Element { attrs, .. } => attrs
            .iter()
            .find_map(|(k, v)| if k == key { Some(v.as_str()) } else { None }),
        _ => None,
    }
}

/// Find a <head> element with a matching `rend` attribute.
/// This skips the HTML document head (which has no rend attribute).
fn find_head_with_rend<'a>(node: &'a DomNode, expected_rend: &str) -> Option<&'a DomNode> {
    match node {
        DomNode::Element {
            tag: t,
            attrs,
            children,
            ..
        } if t == "head" => {
            if attrs.iter().any(|(k, v)| k == "rend" && v == expected_rend) {
                return Some(node);
            }
            for child in children {
                if let Some(found) = find_head_with_rend(child, expected_rend) {
                    return Some(found);
                }
            }
            None
        }
        DomNode::Element { children, .. } => {
            for child in children {
                if let Some(found) = find_head_with_rend(child, expected_rend) {
                    return Some(found);
                }
            }
            None
        }
        _ => None,
    }
}
fn find_node_matching<'a>(node: &'a DomNode, tag: &str) -> Option<&'a DomNode> {
    match node {
        DomNode::Element {
            tag: t, children, ..
        } if t == tag => Some(node),
        DomNode::Element { children, .. } => {
            for child in children {
                if let Some(found) = find_node_matching(child, tag) {
                    return Some(found);
                }
            }
            None
        }
        _ => None,
    }
}

// ── tf_convert_headings ─────────────────────────────────────────────

#[test]
fn test_tf_convert_headings_h1() {
    let mut root = parse_html("<h1>Title</h1>").unwrap();
    walk_pre_mut_test(&mut root, &|n| tf_convert_headings(n));
    // Search for a <head> element with rend=h1 (skip the HTML document head)
    let head = find_head_with_rend(&root, "h1").expect("should find <head rend=\"h1\">");
    assert_eq!(get_attr(head, "rend"), Some("h1"), "rend should be h1");
}

#[test]
fn test_tf_convert_headings_h3() {
    let mut root = parse_html("<h3>Sub</h3>").unwrap();
    walk_pre_mut_test(&mut root, &|n| tf_convert_headings(n));
    let head = find_head_with_rend(&root, "h3").expect("should find <head rend=\"h3\">");
    assert_eq!(get_attr(head, "rend"), Some("h3"), "rend should be h3");
}

#[test]
fn test_tf_convert_headings_keeps_non_heading() {
    let mut root = parse_html("<p>text</p>").unwrap();
    walk_pre_mut_test(&mut root, &|n| tf_convert_headings(n));
    assert!(find_tag(&root, "p"), "<p> should remain unchanged");
}

// ── tf_convert_lists ────────────────────────────────────────────────

#[test]
fn test_tf_convert_lists_ul_ol() {
    let mut root = parse_html("<ul><li>A</li><li>B</li></ul>").unwrap();
    walk_pre_mut_test(&mut root, &|n| tf_convert_lists(n));
    assert!(find_tag(&root, "list"), "ul should become list");
    assert!(find_tag(&root, "item"), "li should become item");
    assert!(!find_tag(&root, "ul"), "ul should not remain");
    assert!(!find_tag(&root, "li"), "li should not remain");
}

// ── tf_convert_quotes ───────────────────────────────────────────────

#[test]
fn test_tf_convert_quotes_blockquote() {
    let mut root = parse_html("<blockquote><p>cite</p></blockquote>").unwrap();
    walk_pre_mut_test(&mut root, &|n| tf_convert_quotes(n));
    assert!(find_tag(&root, "quote"), "blockquote should become quote");
    assert!(
        !find_tag(&root, "blockquote"),
        "blockquote should not remain"
    );
}

#[test]
fn test_tf_convert_quotes_q() {
    let mut root = parse_html("<q>short</q>").unwrap();
    walk_pre_mut_test(&mut root, &|n| tf_convert_quotes(n));
    assert!(find_tag(&root, "quote"), "q should become quote");
}

#[test]
fn test_tf_convert_quotes_pre() {
    let mut root = parse_html("<pre>code here</pre>").unwrap();
    walk_pre_mut_test(&mut root, &|n| tf_convert_quotes(n));
    assert!(find_tag(&root, "code"), "pre should become code");
    assert!(!find_tag(&root, "pre"), "pre should not remain");
}

// ── tf_convert_formatting ───────────────────────────────────────────

#[test]
fn test_tf_convert_formatting_bold() {
    let mut root = parse_html("<b>bold</b>").unwrap();
    walk_pre_mut_test(&mut root, &|n| tf_convert_formatting(n));
    let hi = find_node_matching(&root, "hi").expect("should find <hi>");
    assert_eq!(get_attr(hi, "rend"), Some("#b"), "rend should be #b");
}

#[test]
fn test_tf_convert_formatting_italic() {
    let mut root = parse_html("<em>italic</em>").unwrap();
    walk_pre_mut_test(&mut root, &|n| tf_convert_formatting(n));
    let hi = find_node_matching(&root, "hi").expect("should find <hi>");
    assert_eq!(get_attr(hi, "rend"), Some("#i"), "rend should be #i");
}

#[test]
fn test_tf_convert_formatting_strikethrough() {
    let mut root = parse_html("<del>gone</del>").unwrap();
    walk_pre_mut_test(&mut root, &|n| tf_convert_formatting(n));
    let del = find_node_matching(&root, "del").expect("should find <del>");
    assert_eq!(
        get_attr(del, "rend"),
        Some("overstrike"),
        "rend should be overstrike"
    );
}

#[test]
fn test_tf_convert_formatting_s_tag() {
    let mut root = parse_html("<s>strike</s>").unwrap();
    walk_pre_mut_test(&mut root, &|n| tf_convert_formatting(n));
    let del = find_node_matching(&root, "del").expect("should find <del>");
    assert_eq!(
        get_attr(del, "rend"),
        Some("overstrike"),
        "s should become del with overstrike"
    );
}

// ── tf_convert_breaks ───────────────────────────────────────────────

#[test]
fn test_tf_convert_breaks_br() {
    let mut root = parse_html("<br>").unwrap();
    walk_pre_mut_test(&mut root, &|n| tf_convert_breaks(n));
    assert!(find_tag(&root, "lb"), "br should become lb");
    assert!(!find_tag(&root, "br"), "br should not remain");
}

// ── tf_convert_refs_and_details ─────────────────────────────────────

#[test]
fn test_tf_convert_refs_and_details_link() {
    let mut root = parse_html(r#"<a href="https://x.com">X</a>"#).unwrap();
    walk_pre_mut_test(&mut root, &|n| tf_convert_refs_and_details(n));
    let ref_node = find_node_matching(&root, "ref").expect("should find <ref>");
    assert_eq!(
        get_attr(ref_node, "target"),
        Some("https://x.com"),
        "href should become target"
    );
    assert!(!find_tag(&root, "a"), "a should not remain");
}

#[test]
fn test_tf_convert_refs_and_details_details() {
    let mut root = parse_html("<details><summary>Info</summary><p>text</p></details>").unwrap();
    walk_pre_mut_test(&mut root, &|n| tf_convert_refs_and_details(n));
    assert!(find_tag(&root, "div"), "details should become div");
    assert!(find_tag(&root, "head"), "summary should become head");
    assert!(!find_tag(&root, "details"), "details should not remain");
    assert!(!find_tag(&root, "summary"), "summary should not remain");
}

// ── tf_canonicalize_strip_non_content ───────────────────────────────

#[test]
fn test_tf_strip_removes_script() {
    let mut root = parse_html("<div><script>alert(1)</script><p>text</p></div>").unwrap();
    tf_canonicalize_strip_non_content(&mut root);
    assert!(!find_tag(&root, "script"), "<script> should be removed");
    assert!(find_tag(&root, "p"), "<p> should survive");
}

#[test]
fn test_tf_strip_preserves_head() {
    // Use manual DomNode construction because parse_html treats <head>
    // as the HTML document head (HTML5 parser special-cases it).
    let mut nodes = DomNode::Element {
        tag: "div".into(),
        attrs: vec![],
        children: vec![
            DomNode::Element {
                tag: "head".into(),
                attrs: vec![],
                children: vec![DomNode::Text("title".into())],
                scores: std::collections::HashMap::new(),
                metadata: std::collections::HashMap::new(),
            },
            DomNode::Element {
                tag: "p".into(),
                attrs: vec![],
                children: vec![DomNode::Text("text".into())],
                scores: std::collections::HashMap::new(),
                metadata: std::collections::HashMap::new(),
            },
        ],
        scores: std::collections::HashMap::new(),
        metadata: std::collections::HashMap::new(),
    };
    tf_canonicalize_strip_non_content(&mut nodes);
    assert!(
        find_tag(&nodes, "head"),
        "<head> should be preserved (no rend)"
    );
    assert!(find_tag(&nodes, "p"), "<p> should survive");
}

#[test]
fn test_tf_strip_preserves_head_with_rend() {
    // Manual DomNode construction (parse_html drops <head> attributes).
    let mut nodes = DomNode::Element {
        tag: "div".into(),
        attrs: vec![],
        children: vec![
            DomNode::Element {
                tag: "head".into(),
                attrs: vec![("rend".to_string(), "h1".to_string())],
                children: vec![DomNode::Text("Title".into())],
                scores: std::collections::HashMap::new(),
                metadata: std::collections::HashMap::new(),
            },
            DomNode::Element {
                tag: "p".into(),
                attrs: vec![],
                children: vec![DomNode::Text("text".into())],
                scores: std::collections::HashMap::new(),
                metadata: std::collections::HashMap::new(),
            },
        ],
        scores: std::collections::HashMap::new(),
        metadata: std::collections::HashMap::new(),
    };
    tf_canonicalize_strip_non_content(&mut nodes);
    assert!(
        find_tag(&nodes, "head"),
        "<head rend=\"h1\"> should be preserved"
    );
    assert!(
        find_head_with_rend(&nodes, "h1").is_some(),
        "head with rend=h1 should exist"
    );
    assert!(find_tag(&nodes, "p"), "<p> should survive");
}

#[test]
fn test_tf_strip_removes_multiple_tags() {
    let mut root = parse_html("<div><script>a</script><style>.c{}</style><nav>menu</nav><footer>copy</footer><aside>side</aside><form>f</form><p>text</p></div>").unwrap();
    tf_canonicalize_strip_non_content(&mut root);
    assert!(!find_tag(&root, "script"), "<script> removed");
    assert!(!find_tag(&root, "style"), "<style> removed");
    assert!(!find_tag(&root, "nav"), "<nav> removed");
    assert!(!find_tag(&root, "footer"), "<footer> removed");
    assert!(!find_tag(&root, "aside"), "<aside> removed");
    assert!(!find_tag(&root, "form"), "<form> removed");
    assert!(find_tag(&root, "p"), "<p> should survive");
}

// ── tf_canonicalize_unwrap_containers ─────────────────────────────────

#[test]
fn test_tf_unwrap_single_container() {
    let mut root = parse_html("<div><p>text</p></div>").unwrap();
    tf_canonicalize_unwrap_containers(&mut root);
    assert!(!find_tag(&root, "div"), "<div> should be unwrapped");
    assert!(find_tag(&root, "p"), "<p> should survive");
}

#[test]
fn test_tf_unwrap_nested_containers() {
    let mut root =
        parse_html("<main><article><section><p>text</p></section></article></main>").unwrap();
    tf_canonicalize_unwrap_containers(&mut root);
    assert!(!find_tag(&root, "main"), "<main> should be unwrapped");
    assert!(!find_tag(&root, "article"), "<article> should be unwrapped");
    assert!(!find_tag(&root, "section"), "<section> should be unwrapped");
    assert!(find_tag(&root, "p"), "<p> should survive");
}

#[test]
fn test_tf_strip_then_remove_cleaned_preserves_head_rend() {
    // Simulates the pipeline ordering using manual DomNode construction.
    let mut nodes = DomNode::Element {
        tag: "div".into(),
        attrs: vec![],
        children: vec![
            DomNode::Element {
                tag: "head".into(),
                attrs: vec![("rend".to_string(), "h1".to_string())],
                children: vec![DomNode::Text("Title".into())],
                scores: std::collections::HashMap::new(),
                metadata: std::collections::HashMap::new(),
            },
            DomNode::Element {
                tag: "head".into(),
                attrs: vec![],
                children: vec![DomNode::Text("bare".into())],
                scores: std::collections::HashMap::new(),
                metadata: std::collections::HashMap::new(),
            },
            DomNode::Element {
                tag: "script".into(),
                attrs: vec![],
                children: vec![DomNode::Text("bad".into())],
                scores: std::collections::HashMap::new(),
                metadata: std::collections::HashMap::new(),
            },
            DomNode::Element {
                tag: "p".into(),
                attrs: vec![],
                children: vec![DomNode::Text("text".into())],
                scores: std::collections::HashMap::new(),
                metadata: std::collections::HashMap::new(),
            },
        ],
        scores: std::collections::HashMap::new(),
        metadata: std::collections::HashMap::new(),
    };
    // tf_canonicalize_strip_non_content preserves all heads, removes script
    tf_canonicalize_strip_non_content(&mut nodes);
    assert!(!find_tag(&nodes, "script"), "<script> should be stripped");
    assert!(
        find_tag(&nodes, "head"),
        "<head> elements should survive strip"
    );
    assert!(
        find_head_with_rend(&nodes, "h1").is_some(),
        "head with rend=h1 should exist"
    );
    assert!(find_tag(&nodes, "p"), "<p> should survive");
}
// ── walk_pre_mut wrapper for tests ──────────────────────────────────

fn walk_pre_mut_test<F>(node: &mut DomNode, f: &F)
where
    F: Fn(&mut DomNode) -> WalkerAction,
{
    walk_pre_mut(node, &|n| f(n));
}
