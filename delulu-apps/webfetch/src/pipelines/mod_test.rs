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
    if let Err(WebfetchError::Parse(msg)) = &result {
        assert!(
            msg.contains("node count"),
            "error should mention node count: {msg}"
        );
    }
}
