use super::*;
use crate::pipelines::dom_convert::convert_tree;

fn find_tag(node: &DomNode, tag: &str) -> bool {
    match node {
        DomNode::Element { tag: t, .. } if t == tag => true,
        DomNode::Element { children, .. } => children.iter().any(|c| find_tag(c, tag)),
        _ => false,
    }
}

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
            DomNode::Comment(_) => true,
            DomNode::Element { children, .. } => children.iter().any(find_comment),
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

// ── DomNode::text_len and text_content ────────────────────────────

#[test]
fn test_text_len_empty_text() {
    let node = DomNode::Text(String::new());
    assert_eq!(node.text_len(), 0);
}

#[test]
fn test_text_len_single_char() {
    let node = DomNode::Text("a".to_string());
    assert_eq!(node.text_len(), 1);
}

#[test]
fn test_text_len_simple_element() {
    let node = DomNode::Element {
        tag: "p".to_string(),
        attrs: vec![],
        children: vec![DomNode::Text("hello".to_string())],
        scores: HashMap::new(),
        metadata: HashMap::new(),
    };
    assert_eq!(node.text_len(), 5);
}

#[test]
fn test_text_len_deeply_nested() {
    let node = DomNode::Element {
        tag: "div".to_string(),
        attrs: vec![],
        children: vec![
            DomNode::Text("Hello ".to_string()),
            DomNode::Element {
                tag: "b".to_string(),
                attrs: vec![],
                children: vec![DomNode::Text("World".to_string())],
                scores: HashMap::new(),
                metadata: HashMap::new(),
            },
        ],
        scores: HashMap::new(),
        metadata: HashMap::new(),
    };
    assert_eq!(node.text_len(), 11);
}

#[test]
fn test_text_len_comment_returns_zero() {
    let node = DomNode::Comment("ignored".to_string());
    assert_eq!(node.text_len(), 0);
}

#[test]
fn test_text_len_doctype_returns_zero() {
    let node = DomNode::Doctype("html".to_string());
    assert_eq!(node.text_len(), 0);
}

#[test]
fn test_text_len_mixed_tree() {
    let node = DomNode::Element {
        tag: "div".to_string(),
        attrs: vec![],
        children: vec![
            DomNode::Comment("comment".to_string()),
            DomNode::Text("visible".to_string()),
            DomNode::Doctype("html".to_string()),
            DomNode::Element {
                tag: "span".to_string(),
                attrs: vec![],
                children: vec![DomNode::Text("text".to_string())],
                scores: HashMap::new(),
                metadata: HashMap::new(),
            },
        ],
        scores: HashMap::new(),
        metadata: HashMap::new(),
    };
    assert_eq!(node.text_len(), 11);
}

#[test]
fn test_text_len_matches_text_content_len() {
    let node = DomNode::Element {
        tag: "article".to_string(),
        attrs: vec![],
        children: vec![
            DomNode::Text("First ".to_string()),
            DomNode::Element {
                tag: "p".to_string(),
                attrs: vec![],
                children: vec![
                    DomNode::Text("Second ".to_string()),
                    DomNode::Element {
                        tag: "b".to_string(),
                        attrs: vec![],
                        children: vec![DomNode::Text("Third".to_string())],
                        scores: HashMap::new(),
                        metadata: HashMap::new(),
                    },
                ],
                scores: HashMap::new(),
                metadata: HashMap::new(),
            },
        ],
        scores: HashMap::new(),
        metadata: HashMap::new(),
    };
    assert_eq!(node.text_len(), node.text_content().len());
}

#[test]
fn test_text_content_empty_text() {
    let node = DomNode::Text(String::new());
    assert_eq!(node.text_content(), "");
}

#[test]
fn test_text_content_single_text() {
    let node = DomNode::Text("hello".to_string());
    assert_eq!(node.text_content(), "hello");
}

#[test]
fn test_text_content_simple_element() {
    let node = DomNode::Element {
        tag: "p".to_string(),
        attrs: vec![],
        children: vec![DomNode::Text("Hello World".to_string())],
        scores: HashMap::new(),
        metadata: HashMap::new(),
    };
    assert_eq!(node.text_content(), "Hello World");
}

#[test]
fn test_text_content_nested() {
    let node = DomNode::Element {
        tag: "div".to_string(),
        attrs: vec![],
        children: vec![
            DomNode::Text("Hello ".to_string()),
            DomNode::Element {
                tag: "b".to_string(),
                attrs: vec![],
                children: vec![DomNode::Text("World".to_string())],
                scores: HashMap::new(),
                metadata: HashMap::new(),
            },
        ],
        scores: HashMap::new(),
        metadata: HashMap::new(),
    };
    assert_eq!(node.text_content(), "Hello World");
}

#[test]
fn test_text_content_comment_returns_empty() {
    let node = DomNode::Comment("ignored".to_string());
    assert_eq!(node.text_content(), "");
}

#[test]
fn test_text_content_doctype_returns_empty() {
    let node = DomNode::Doctype("html".to_string());
    assert_eq!(node.text_content(), "");
}

#[test]
fn test_text_content_mixed_tree() {
    let node = DomNode::Element {
        tag: "div".to_string(),
        attrs: vec![],
        children: vec![
            DomNode::Comment("skip".to_string()),
            DomNode::Text("A".to_string()),
            DomNode::Doctype("html".to_string()),
            DomNode::Element {
                tag: "span".to_string(),
                attrs: vec![],
                children: vec![DomNode::Text("B".to_string())],
                scores: HashMap::new(),
                metadata: HashMap::new(),
            },
        ],
        scores: HashMap::new(),
        metadata: HashMap::new(),
    };
    assert_eq!(node.text_content(), "AB");
}

#[test]
fn test_text_content_matches_get_inner_text() {
    use crate::pipelines::DomNode;
    let t = DomNode::Text("hello".to_string());
    assert_eq!(t.text_content(), "hello");
    let e = DomNode::Element {
        tag: "p".to_string(),
        attrs: vec![],
        children: vec![
            DomNode::Text("Hello ".to_string()),
            DomNode::Element {
                tag: "b".to_string(),
                attrs: vec![],
                children: vec![DomNode::Text("World".to_string())],
                scores: HashMap::new(),
                metadata: HashMap::new(),
            },
        ],
        scores: HashMap::new(),
        metadata: HashMap::new(),
    };
    assert_eq!(e.text_content(), "Hello World");
    let mixed = DomNode::Element {
        tag: "div".to_string(),
        attrs: vec![],
        children: vec![
            DomNode::Comment("ignored".to_string()),
            DomNode::Text("text".to_string()),
            DomNode::Doctype("html".to_string()),
        ],
        scores: HashMap::new(),
        metadata: HashMap::new(),
    };
    assert_eq!(mixed.text_content(), "text");
    let c = DomNode::Comment("ignored".to_string());
    assert_eq!(c.text_content(), "");
    let d = DomNode::Doctype("html".to_string());
    assert_eq!(d.text_content(), "");
    let empty = DomNode::Text(String::new());
    assert_eq!(empty.text_content(), "");
    let empty_elem = DomNode::Element {
        tag: "div".to_string(),
        attrs: vec![],
        children: vec![],
        scores: HashMap::new(),
        metadata: HashMap::new(),
    };
    assert_eq!(empty_elem.text_content(), "");
    let with_script = DomNode::Element {
        tag: "body".to_string(),
        attrs: vec![],
        children: vec![
            DomNode::Text("before".to_string()),
            DomNode::Element {
                tag: "script".to_string(),
                attrs: vec![],
                children: vec![DomNode::Text("code".to_string())],
                scores: HashMap::new(),
                metadata: HashMap::new(),
            },
            DomNode::Text("after".to_string()),
        ],
        scores: HashMap::new(),
        metadata: HashMap::new(),
    };
    assert_eq!(with_script.text_content(), "beforecodeafter");
}

// ── DomNode::visible_text_len ────────────────────────────────────

#[test]
fn test_visible_text_len_excludes_script() {
    let node = DomNode::Element {
        tag: "div".to_string(),
        attrs: vec![],
        children: vec![
            DomNode::Text("visible ".to_string()),
            DomNode::Element {
                tag: "script".to_string(),
                attrs: vec![],
                children: vec![DomNode::Text("hidden".to_string())],
                scores: HashMap::new(),
                metadata: HashMap::new(),
            },
        ],
        scores: HashMap::new(),
        metadata: HashMap::new(),
    };
    assert_eq!(node.visible_text_len(), 8); // "visible " = 8
}

#[test]
fn test_visible_text_len_excludes_style() {
    let node = DomNode::Element {
        tag: "div".to_string(),
        attrs: vec![],
        children: vec![
            DomNode::Text("content ".to_string()),
            DomNode::Element {
                tag: "style".to_string(),
                attrs: vec![],
                children: vec![DomNode::Text("css".to_string())],
                scores: HashMap::new(),
                metadata: HashMap::new(),
            },
        ],
        scores: HashMap::new(),
        metadata: HashMap::new(),
    };
    assert_eq!(node.visible_text_len(), 8); // "content " = 8
}

#[test]
fn test_visible_text_len_all_text() {
    let node = DomNode::Element {
        tag: "p".to_string(),
        attrs: vec![],
        children: vec![DomNode::Text("hello".to_string())],
        scores: HashMap::new(),
        metadata: HashMap::new(),
    };
    assert_eq!(node.visible_text_len(), 5);
}

#[test]
fn test_visible_text_len_comment_doctype_zero() {
    assert_eq!(DomNode::Comment("x".to_string()).visible_text_len(), 0);
    assert_eq!(DomNode::Doctype("html".to_string()).visible_text_len(), 0);
}

#[test]
fn test_visible_text_len_deeply_nested() {
    let node = DomNode::Element {
        tag: "div".to_string(),
        attrs: vec![],
        children: vec![DomNode::Element {
            tag: "article".to_string(),
            attrs: vec![],
            children: vec![
                DomNode::Text("A".to_string()),
                DomNode::Element {
                    tag: "script".to_string(),
                    attrs: vec![],
                    children: vec![DomNode::Text("SKIP".to_string())],
                    scores: HashMap::new(),
                    metadata: HashMap::new(),
                },
                DomNode::Text("B".to_string()),
            ],
            scores: HashMap::new(),
            metadata: HashMap::new(),
        }],
        scores: HashMap::new(),
        metadata: HashMap::new(),
    };
    assert_eq!(node.visible_text_len(), 2); // "A" + "B" = 2
}

// ── DomNode::script_len ─────────────────────────────────
// Counts text inside <script> subtrees only; text outside contributes 0.

#[test]
fn test_script_len_counts_text_inside_script_only() {
    // <script>abc</script> -> script_len==3, visible_text_len==0
    let node = DomNode::Element {
        tag: "script".to_string(),
        attrs: vec![],
        children: vec![DomNode::Text("abc".to_string())],
        scores: HashMap::new(),
        metadata: HashMap::new(),
    };
    assert_eq!(node.script_len(), 3);
    assert_eq!(node.visible_text_len(), 0);
}

#[test]
fn test_script_len_does_not_count_text_outside_script() {
    // <script>abc</script><p>hello</p> -> script_len==3, visible_text_len==5
    let node = DomNode::Element {
        tag: "div".to_string(),
        attrs: vec![],
        children: vec![
            DomNode::Element {
                tag: "script".to_string(),
                attrs: vec![],
                children: vec![DomNode::Text("abc".to_string())],
                scores: HashMap::new(),
                metadata: HashMap::new(),
            },
            DomNode::Element {
                tag: "p".to_string(),
                attrs: vec![],
                children: vec![DomNode::Text("hello".to_string())],
                scores: HashMap::new(),
                metadata: HashMap::new(),
            },
        ],
        scores: HashMap::new(),
        metadata: HashMap::new(),
    };
    assert_eq!(node.script_len(), 3);
    assert_eq!(node.visible_text_len(), 5);
}

#[test]
fn test_script_len_no_script_is_zero() {
    let node = DomNode::Element {
        tag: "p".to_string(),
        attrs: vec![],
        children: vec![DomNode::Text("hello".to_string())],
        scores: HashMap::new(),
        metadata: HashMap::new(),
    };
    assert_eq!(node.script_len(), 0);
    assert_eq!(DomNode::Comment("x".to_string()).script_len(), 0);
    assert_eq!(DomNode::Doctype("html".to_string()).script_len(), 0);
}

#[test]
fn test_script_len_nested_script_sums_subtree() {
    // A <script> containing nested text (and even a nested element with text)
    // sums its whole subtree via text_len_inner.
    let node = DomNode::Element {
        tag: "script".to_string(),
        attrs: vec![],
        children: vec![
            DomNode::Text("ab".to_string()),
            DomNode::Element {
                tag: "div".to_string(),
                attrs: vec![],
                children: vec![DomNode::Text("cd".to_string())],
                scores: HashMap::new(),
                metadata: HashMap::new(),
            },
        ],
        scores: HashMap::new(),
        metadata: HashMap::new(),
    };
    assert_eq!(node.script_len(), 4);
}

#[test]
fn test_script_len_parse_html_pipeline_present() {
    // Measured on a script-bearing body via parse_html (pre-pipeline
    // scripts are present), script_len is non-zero while visible is small.
    let body = r#"<html><head><title>Enable JavaScript</title></head><body><script>var escaped = '\u003ciframe\u003e'.repeat(50);</script><p>hi</p></body></html>"#;
    let dom = parse_html(body).unwrap();
    assert!(
        dom.script_len() > 0,
        "script_len must be non-zero pre-pipeline"
    );
    assert!(
        dom.script_len() > dom.visible_text_len(),
        "script-dominant on this body"
    );
}

// ── DomNode::link_text_len ────────────────────────────────────────

#[test]
fn test_link_text_len_counts_only_a() {
    let node = DomNode::Element {
        tag: "div".to_string(),
        attrs: vec![],
        children: vec![
            DomNode::Text("before ".to_string()),
            DomNode::Element {
                tag: "a".to_string(),
                attrs: vec![],
                children: vec![DomNode::Text("link".to_string())],
                scores: HashMap::new(),
                metadata: HashMap::new(),
            },
            DomNode::Text(" after".to_string()),
        ],
        scores: HashMap::new(),
        metadata: HashMap::new(),
    };
    assert_eq!(node.link_text_len(), 4); // only "link"
}

#[test]
fn test_link_text_len_nested_a() {
    // Malformed HTML: <a> inside <a>
    let node = DomNode::Element {
        tag: "div".to_string(),
        attrs: vec![],
        children: vec![DomNode::Element {
            tag: "a".to_string(),
            attrs: vec![],
            children: vec![
                DomNode::Text("outer ".to_string()),
                DomNode::Element {
                    tag: "a".to_string(),
                    attrs: vec![],
                    children: vec![DomNode::Text("inner".to_string())],
                    scores: HashMap::new(),
                    metadata: HashMap::new(),
                },
            ],
            scores: HashMap::new(),
            metadata: HashMap::new(),
        }],
        scores: HashMap::new(),
        metadata: HashMap::new(),
    };
    // outer <a> counts all text: "outer " + "inner" = 11
    // No non-<a> ancestors to recurse into, so total = 11
    assert_eq!(node.link_text_len(), 11);
}

#[test]
fn test_link_text_len_a_with_span() {
    let node = DomNode::Element {
        tag: "div".to_string(),
        attrs: vec![],
        children: vec![DomNode::Element {
            tag: "a".to_string(),
            attrs: vec![],
            children: vec![
                DomNode::Text("click ".to_string()),
                DomNode::Element {
                    tag: "span".to_string(),
                    attrs: vec![],
                    children: vec![DomNode::Text("here".to_string())],
                    scores: HashMap::new(),
                    metadata: HashMap::new(),
                },
            ],
            scores: HashMap::new(),
            metadata: HashMap::new(),
        }],
        scores: HashMap::new(),
        metadata: HashMap::new(),
    };
    assert_eq!(node.link_text_len(), 10); // "click " + "here" = 10
}

#[test]
fn test_link_text_len_mixed_content() {
    let node = DomNode::Element {
        tag: "div".to_string(),
        attrs: vec![],
        children: vec![
            DomNode::Text("ignore".to_string()),
            DomNode::Element {
                tag: "a".to_string(),
                attrs: vec![],
                children: vec![DomNode::Text("link1".to_string())],
                scores: HashMap::new(),
                metadata: HashMap::new(),
            },
            DomNode::Text(" skip ".to_string()),
            DomNode::Element {
                tag: "a".to_string(),
                attrs: vec![],
                children: vec![DomNode::Text("link2".to_string())],
                scores: HashMap::new(),
                metadata: HashMap::new(),
            },
            DomNode::Text("end".to_string()),
        ],
        scores: HashMap::new(),
        metadata: HashMap::new(),
    };
    assert_eq!(node.link_text_len(), 10); // "link1" + "link2" = 10
}

#[test]
fn test_link_text_len_flat_a_siblings() {
    let node = DomNode::Element {
        tag: "nav".to_string(),
        attrs: vec![],
        children: vec![
            DomNode::Element {
                tag: "a".to_string(),
                attrs: vec![],
                children: vec![DomNode::Text("Home".to_string())],
                scores: HashMap::new(),
                metadata: HashMap::new(),
            },
            DomNode::Element {
                tag: "a".to_string(),
                attrs: vec![],
                children: vec![DomNode::Text("About".to_string())],
                scores: HashMap::new(),
                metadata: HashMap::new(),
            },
            DomNode::Element {
                tag: "a".to_string(),
                attrs: vec![],
                children: vec![DomNode::Text("Contact".to_string())],
                scores: HashMap::new(),
                metadata: HashMap::new(),
            },
        ],
        scores: HashMap::new(),
        metadata: HashMap::new(),
    };
    assert_eq!(node.link_text_len(), 16); // "Home" + "About" + "Contact" = 16
}

#[test]
fn test_link_text_len_comment_doctype_zero() {
    assert_eq!(DomNode::Comment("x".to_string()).link_text_len(), 0);
    assert_eq!(DomNode::Doctype("html".to_string()).link_text_len(), 0);
}

#[test]
fn test_link_text_len_matches_count_link_text() {
    use crate::pipelines::DomNode;
    // Flat <a> siblings
    let flat = DomNode::Element {
        tag: "nav".to_string(),
        attrs: vec![],
        children: vec![
            DomNode::Element {
                tag: "a".to_string(),
                attrs: vec![],
                children: vec![DomNode::Text("One".to_string())],
                scores: HashMap::new(),
                metadata: HashMap::new(),
            },
            DomNode::Element {
                tag: "a".to_string(),
                attrs: vec![],
                children: vec![DomNode::Text("Two".to_string())],
                scores: HashMap::new(),
                metadata: HashMap::new(),
            },
        ],
        scores: HashMap::new(),
        metadata: HashMap::new(),
    };
    assert_eq!(flat.link_text_len(), 6);
    // Nested <a> (malformed HTML)
    let nested = DomNode::Element {
        tag: "div".to_string(),
        attrs: vec![],
        children: vec![DomNode::Element {
            tag: "a".to_string(),
            attrs: vec![],
            children: vec![
                DomNode::Text("outer ".to_string()),
                DomNode::Element {
                    tag: "a".to_string(),
                    attrs: vec![],
                    children: vec![DomNode::Text("inner".to_string())],
                    scores: HashMap::new(),
                    metadata: HashMap::new(),
                },
            ],
            scores: HashMap::new(),
            metadata: HashMap::new(),
        }],
        scores: HashMap::new(),
        metadata: HashMap::new(),
    };
    assert_eq!(nested.link_text_len(), 11);
    // <a> with <span> children
    let with_span = DomNode::Element {
        tag: "div".to_string(),
        attrs: vec![],
        children: vec![DomNode::Element {
            tag: "a".to_string(),
            attrs: vec![],
            children: vec![
                DomNode::Text("click ".to_string()),
                DomNode::Element {
                    tag: "span".to_string(),
                    attrs: vec![],
                    children: vec![DomNode::Text("here".to_string())],
                    scores: HashMap::new(),
                    metadata: HashMap::new(),
                },
            ],
            scores: HashMap::new(),
            metadata: HashMap::new(),
        }],
        scores: HashMap::new(),
        metadata: HashMap::new(),
    };
    assert_eq!(with_span.link_text_len(), 10);
    // Mixed content (text + <a> + text + <a>)
    let mixed = DomNode::Element {
        tag: "div".to_string(),
        attrs: vec![],
        children: vec![
            DomNode::Text("prefix ".to_string()),
            DomNode::Element {
                tag: "a".to_string(),
                attrs: vec![],
                children: vec![DomNode::Text("L1".to_string())],
                scores: HashMap::new(),
                metadata: HashMap::new(),
            },
            DomNode::Text(" mid ".to_string()),
            DomNode::Element {
                tag: "a".to_string(),
                attrs: vec![],
                children: vec![DomNode::Text("L2".to_string())],
                scores: HashMap::new(),
                metadata: HashMap::new(),
            },
            DomNode::Text(" suffix".to_string()),
        ],
        scores: HashMap::new(),
        metadata: HashMap::new(),
    };
    assert_eq!(mixed.link_text_len(), 4);
    // No <a> elements
    let no_links = DomNode::Element {
        tag: "p".to_string(),
        attrs: vec![],
        children: vec![DomNode::Text("plain".to_string())],
        scores: HashMap::new(),
        metadata: HashMap::new(),
    };
    assert_eq!(no_links.link_text_len(), 0);
}

// ── DomNode::text_stats ────────────────────────────────────────────

#[test]
fn test_text_stats_simple() {
    let node = DomNode::Element {
        tag: "div".to_string(),
        attrs: vec![],
        children: vec![
            DomNode::Text("before ".to_string()),
            DomNode::Element {
                tag: "p".to_string(),
                attrs: vec![],
                children: vec![DomNode::Text("paragraph".to_string())],
                scores: HashMap::new(),
                metadata: HashMap::new(),
            },
            DomNode::Text(" after".to_string()),
        ],
        scores: HashMap::new(),
        metadata: HashMap::new(),
    };
    let (p_text, total) = node.text_stats();
    assert_eq!(p_text, 9); // "paragraph"
    assert_eq!(total, 22); // "before paragraph after" = 7 + 9 + 6 = 22
}

#[test]
fn test_text_stats_no_p() {
    let node = DomNode::Element {
        tag: "div".to_string(),
        attrs: vec![],
        children: vec![DomNode::Text("just text".to_string())],
        scores: HashMap::new(),
        metadata: HashMap::new(),
    };
    let (p_text, total) = node.text_stats();
    assert_eq!(p_text, 0); // no <p> elements
    assert_eq!(total, 9); // "just text"
}

#[test]
fn test_text_stats_empty() {
    let node = DomNode::Element {
        tag: "div".to_string(),
        attrs: vec![],
        children: vec![],
        scores: HashMap::new(),
        metadata: HashMap::new(),
    };
    assert_eq!(node.text_stats(), (0, 0));
}

#[test]
fn test_text_stats_text_node_direct() {
    let node = DomNode::Text("hello".to_string());
    assert_eq!(node.text_stats(), (0, 5));
}

#[test]
fn test_text_stats_comment_doctype_zero() {
    assert_eq!(DomNode::Comment("x".to_string()).text_stats(), (0, 0));
    assert_eq!(DomNode::Doctype("html".to_string()).text_stats(), (0, 0));
}

#[test]
fn test_text_stats_nested_p() {
    let node = DomNode::Element {
        tag: "div".to_string(),
        attrs: vec![],
        children: vec![DomNode::Element {
            tag: "p".to_string(),
            attrs: vec![],
            children: vec![
                DomNode::Text("level1 ".to_string()),
                DomNode::Element {
                    tag: "span".to_string(),
                    attrs: vec![],
                    children: vec![DomNode::Text("nested".to_string())],
                    scores: HashMap::new(),
                    metadata: HashMap::new(),
                },
            ],
            scores: HashMap::new(),
            metadata: HashMap::new(),
        }],
        scores: HashMap::new(),
        metadata: HashMap::new(),
    };
    let (p_text, total) = node.text_stats();
    assert_eq!(p_text, 13); // "level1 nested"
    assert_eq!(total, 13);
}

#[test]
fn test_text_stats_p_empty() {
    let node = DomNode::Element {
        tag: "div".to_string(),
        attrs: vec![],
        children: vec![DomNode::Element {
            tag: "p".to_string(),
            attrs: vec![],
            children: vec![],
            scores: HashMap::new(),
            metadata: HashMap::new(),
        }],
        scores: HashMap::new(),
        metadata: HashMap::new(),
    };
    assert_eq!(node.text_stats(), (0, 0));
}

#[test]
fn test_text_stats_script_style_included() {
    // text_stats does NOT skip script/style — matches count_p_text/collect_text behavior.
    let node = DomNode::Element {
        tag: "div".to_string(),
        attrs: vec![],
        children: vec![
            DomNode::Text("visible ".to_string()),
            DomNode::Element {
                tag: "script".to_string(),
                attrs: vec![],
                children: vec![DomNode::Text("code".to_string())],
                scores: HashMap::new(),
                metadata: HashMap::new(),
            },
            DomNode::Text(" more".to_string()),
        ],
        scores: HashMap::new(),
        metadata: HashMap::new(),
    };
    let (p_text, total) = node.text_stats();
    assert_eq!(p_text, 0);
    assert_eq!(total, 17); // "visible code more" = 8 + 4 + 5 = 17
}

#[test]
fn test_text_stats_matches_count_p_text_and_collect_text() {
    use crate::pipelines::DomNode;
    // Fragment 1: with <p>
    let with_p = DomNode::Element {
        tag: "div".to_string(),
        attrs: vec![],
        children: vec![
            DomNode::Text("lead ".to_string()),
            DomNode::Element {
                tag: "p".to_string(),
                attrs: vec![],
                children: vec![DomNode::Text("content".to_string())],
                scores: HashMap::new(),
                metadata: HashMap::new(),
            },
        ],
        scores: HashMap::new(),
        metadata: HashMap::new(),
    };
    assert_eq!(with_p.text_stats(), (7, 12), "with_p");
    // Fragment 2: without <p>
    let no_p = DomNode::Element {
        tag: "div".to_string(),
        attrs: vec![],
        children: vec![DomNode::Text("plain".to_string())],
        scores: HashMap::new(),
        metadata: HashMap::new(),
    };
    assert_eq!(no_p.text_stats(), (0, 5), "no_p");
    // Fragment 3: mixed nesting
    let mixed = DomNode::Element {
        tag: "div".to_string(),
        attrs: vec![],
        children: vec![DomNode::Element {
            tag: "p".to_string(),
            attrs: vec![],
            children: vec![
                DomNode::Text("A".to_string()),
                DomNode::Element {
                    tag: "span".to_string(),
                    attrs: vec![],
                    children: vec![DomNode::Text("B".to_string())],
                    scores: HashMap::new(),
                    metadata: HashMap::new(),
                },
            ],
            scores: HashMap::new(),
            metadata: HashMap::new(),
        }],
        scores: HashMap::new(),
        metadata: HashMap::new(),
    };
    assert_eq!(mixed.text_stats(), (2, 2), "mixed");
    // Fragment 4: script/style boundaries
    let with_script = DomNode::Element {
        tag: "body".to_string(),
        attrs: vec![],
        children: vec![
            DomNode::Text("a".to_string()),
            DomNode::Element {
                tag: "script".to_string(),
                attrs: vec![],
                children: vec![DomNode::Text("code".to_string())],
                scores: HashMap::new(),
                metadata: HashMap::new(),
            },
            DomNode::Text("b".to_string()),
        ],
        scores: HashMap::new(),
        metadata: HashMap::new(),
    };
    assert_eq!(with_script.text_stats(), (0, 6), "with_script");
    // Fragment 5: empty tree
    let empty = DomNode::Element {
        tag: "div".to_string(),
        attrs: vec![],
        children: vec![],
        scores: HashMap::new(),
        metadata: HashMap::new(),
    };
    assert_eq!(empty.text_stats(), (0, 0), "empty");
    // Fragment 6: Comment/Doctype-only
    let comment_only = DomNode::Element {
        tag: "div".to_string(),
        attrs: vec![],
        children: vec![
            DomNode::Comment("ignored".to_string()),
            DomNode::Doctype("html".to_string()),
        ],
        scores: HashMap::new(),
        metadata: HashMap::new(),
    };
    assert_eq!(comment_only.text_stats(), (0, 0), "comment_only");
    // Fragment 7: Text node directly
    let text_node = DomNode::Text("hi".to_string());
    assert_eq!(text_node.text_stats(), (0, 2), "text_node");
    // Fragment 8: multiple <p> siblings
    let multi_p = DomNode::Element {
        tag: "div".to_string(),
        attrs: vec![],
        children: vec![
            DomNode::Element {
                tag: "p".to_string(),
                attrs: vec![],
                children: vec![DomNode::Text("first".to_string())],
                scores: HashMap::new(),
                metadata: HashMap::new(),
            },
            DomNode::Element {
                tag: "p".to_string(),
                attrs: vec![],
                children: vec![DomNode::Text("second".to_string())],
                scores: HashMap::new(),
                metadata: HashMap::new(),
            },
        ],
        scores: HashMap::new(),
        metadata: HashMap::new(),
    };
    assert_eq!(multi_p.text_stats(), (11, 11), "multi_p");
}

// ── DomNode::link_density_stats ───────────────────────────────────

#[test]
fn test_link_density_stats_simple() {
    let node = DomNode::Element {
        tag: "div".to_string(),
        attrs: vec![],
        children: vec![
            DomNode::Text("text ".to_string()),
            DomNode::Element {
                tag: "a".to_string(),
                attrs: vec![],
                children: vec![DomNode::Text("link".to_string())],
                scores: HashMap::new(),
                metadata: HashMap::new(),
            },
        ],
        scores: HashMap::new(),
        metadata: HashMap::new(),
    };
    let (total, link) = node.link_density_stats();
    assert_eq!(total, 9); // "text link"
    assert_eq!(link, 4); // "link"
}

#[test]
fn test_link_density_stats_no_links() {
    let node = DomNode::Element {
        tag: "div".to_string(),
        attrs: vec![],
        children: vec![DomNode::Text("plain".to_string())],
        scores: HashMap::new(),
        metadata: HashMap::new(),
    };
    assert_eq!(node.link_density_stats(), (5, 0));
}

#[test]
fn test_link_density_stats_empty() {
    let node = DomNode::Element {
        tag: "div".to_string(),
        attrs: vec![],
        children: vec![],
        scores: HashMap::new(),
        metadata: HashMap::new(),
    };
    assert_eq!(node.link_density_stats(), (0, 0));
}

#[test]
fn test_link_density_stats_text_node_direct() {
    let node = DomNode::Text("hello".to_string());
    assert_eq!(node.link_density_stats(), (5, 0));
}

#[test]
fn test_link_density_stats_comment_doctype_zero() {
    assert_eq!(
        DomNode::Comment("x".to_string()).link_density_stats(),
        (0, 0)
    );
    assert_eq!(
        DomNode::Doctype("html".to_string()).link_density_stats(),
        (0, 0)
    );
}

#[test]
fn test_link_density_stats_nested_a() {
    // Malformed HTML: nested <a> elements
    let node = DomNode::Element {
        tag: "div".to_string(),
        attrs: vec![],
        children: vec![DomNode::Element {
            tag: "a".to_string(),
            attrs: vec![],
            children: vec![
                DomNode::Text("outer ".to_string()),
                DomNode::Element {
                    tag: "a".to_string(),
                    attrs: vec![],
                    children: vec![DomNode::Text("inner".to_string())],
                    scores: HashMap::new(),
                    metadata: HashMap::new(),
                },
            ],
            scores: HashMap::new(),
            metadata: HashMap::new(),
        }],
        scores: HashMap::new(),
        metadata: HashMap::new(),
    };
    // outer <a> counts all text: "outer " + "inner" = 11
    let (total, link) = node.link_density_stats();
    assert_eq!(total, 11);
    assert_eq!(link, 11);
}

#[test]
fn test_link_density_stats_a_with_span() {
    let node = DomNode::Element {
        tag: "div".to_string(),
        attrs: vec![],
        children: vec![DomNode::Element {
            tag: "a".to_string(),
            attrs: vec![],
            children: vec![
                DomNode::Text("click ".to_string()),
                DomNode::Element {
                    tag: "span".to_string(),
                    attrs: vec![],
                    children: vec![DomNode::Text("here".to_string())],
                    scores: HashMap::new(),
                    metadata: HashMap::new(),
                },
            ],
            scores: HashMap::new(),
            metadata: HashMap::new(),
        }],
        scores: HashMap::new(),
        metadata: HashMap::new(),
    };
    let (total, link) = node.link_density_stats();
    assert_eq!(total, 10); // "click here"
    assert_eq!(link, 10); // all inside <a>
}

#[test]
fn test_link_density_stats_mixed_content() {
    let node = DomNode::Element {
        tag: "div".to_string(),
        attrs: vec![],
        children: vec![
            DomNode::Text("ignore".to_string()),
            DomNode::Element {
                tag: "a".to_string(),
                attrs: vec![],
                children: vec![DomNode::Text("link1".to_string())],
                scores: HashMap::new(),
                metadata: HashMap::new(),
            },
            DomNode::Text(" skip ".to_string()),
            DomNode::Element {
                tag: "a".to_string(),
                attrs: vec![],
                children: vec![DomNode::Text("link2".to_string())],
                scores: HashMap::new(),
                metadata: HashMap::new(),
            },
            DomNode::Text("end".to_string()),
        ],
        scores: HashMap::new(),
        metadata: HashMap::new(),
    };
    let (total, link) = node.link_density_stats();
    assert_eq!(total, 25); // "ignorelink1 skip link2end" = 6 + 5 + 6 + 5 + 3 = 25
    assert_eq!(link, 10); // "link1" + "link2"
}

#[test]
fn test_link_density_stats_flat_a_siblings() {
    let node = DomNode::Element {
        tag: "nav".to_string(),
        attrs: vec![],
        children: vec![
            DomNode::Element {
                tag: "a".to_string(),
                attrs: vec![],
                children: vec![DomNode::Text("Home".to_string())],
                scores: HashMap::new(),
                metadata: HashMap::new(),
            },
            DomNode::Element {
                tag: "a".to_string(),
                attrs: vec![],
                children: vec![DomNode::Text("About".to_string())],
                scores: HashMap::new(),
                metadata: HashMap::new(),
            },
            DomNode::Element {
                tag: "a".to_string(),
                attrs: vec![],
                children: vec![DomNode::Text("Contact".to_string())],
                scores: HashMap::new(),
                metadata: HashMap::new(),
            },
        ],
        scores: HashMap::new(),
        metadata: HashMap::new(),
    };
    let (total, link) = node.link_density_stats();
    assert_eq!(total, 16); // "HomeAboutContact"
    assert_eq!(link, 16); // all inside <a>
}

#[test]
fn test_link_density_stats_matches_get_inner_text_and_count_link_text() {
    use crate::pipelines::DomNode;
    // Fragment 1: with <a>
    let with_a = DomNode::Element {
        tag: "div".to_string(),
        attrs: vec![],
        children: vec![
            DomNode::Text("text ".to_string()),
            DomNode::Element {
                tag: "a".to_string(),
                attrs: vec![],
                children: vec![DomNode::Text("link".to_string())],
                scores: HashMap::new(),
                metadata: HashMap::new(),
            },
        ],
        scores: HashMap::new(),
        metadata: HashMap::new(),
    };
    assert_eq!(with_a.link_density_stats(), (9, 4), "with_a");
    // Fragment 2: without <a>
    let no_a = DomNode::Element {
        tag: "div".to_string(),
        attrs: vec![],
        children: vec![DomNode::Text("plain".to_string())],
        scores: HashMap::new(),
        metadata: HashMap::new(),
    };
    assert_eq!(no_a.link_density_stats(), (5, 0), "no_a");
    // Fragment 3: nested <a> (malformed)
    let nested = DomNode::Element {
        tag: "div".to_string(),
        attrs: vec![],
        children: vec![DomNode::Element {
            tag: "a".to_string(),
            attrs: vec![],
            children: vec![
                DomNode::Text("outer ".to_string()),
                DomNode::Element {
                    tag: "a".to_string(),
                    attrs: vec![],
                    children: vec![DomNode::Text("inner".to_string())],
                    scores: HashMap::new(),
                    metadata: HashMap::new(),
                },
            ],
            scores: HashMap::new(),
            metadata: HashMap::new(),
        }],
        scores: HashMap::new(),
        metadata: HashMap::new(),
    };
    assert_eq!(nested.link_density_stats(), (11, 11), "nested");
    // Fragment 4: <a> with <span> children
    let with_span = DomNode::Element {
        tag: "div".to_string(),
        attrs: vec![],
        children: vec![DomNode::Element {
            tag: "a".to_string(),
            attrs: vec![],
            children: vec![
                DomNode::Text("click ".to_string()),
                DomNode::Element {
                    tag: "span".to_string(),
                    attrs: vec![],
                    children: vec![DomNode::Text("here".to_string())],
                    scores: HashMap::new(),
                    metadata: HashMap::new(),
                },
            ],
            scores: HashMap::new(),
            metadata: HashMap::new(),
        }],
        scores: HashMap::new(),
        metadata: HashMap::new(),
    };
    assert_eq!(with_span.link_density_stats(), (10, 10), "with_span");
    // Fragment 5: empty tree
    let empty = DomNode::Element {
        tag: "div".to_string(),
        attrs: vec![],
        children: vec![],
        scores: HashMap::new(),
        metadata: HashMap::new(),
    };
    assert_eq!(empty.link_density_stats(), (0, 0), "empty");
    // Fragment 6: Comment/Doctype-only
    let comment_only = DomNode::Element {
        tag: "div".to_string(),
        attrs: vec![],
        children: vec![
            DomNode::Comment("ignored".to_string()),
            DomNode::Doctype("html".to_string()),
        ],
        scores: HashMap::new(),
        metadata: HashMap::new(),
    };
    assert_eq!(comment_only.link_density_stats(), (0, 0), "comment_only");
    // Fragment 7: Text node directly
    let text_node = DomNode::Text("hi".to_string());
    assert_eq!(text_node.link_density_stats(), (2, 0), "text_node");
    // Fragment 8: flat <a> siblings
    let flat = DomNode::Element {
        tag: "nav".to_string(),
        attrs: vec![],
        children: vec![
            DomNode::Element {
                tag: "a".to_string(),
                attrs: vec![],
                children: vec![DomNode::Text("One".to_string())],
                scores: HashMap::new(),
                metadata: HashMap::new(),
            },
            DomNode::Element {
                tag: "a".to_string(),
                attrs: vec![],
                children: vec![DomNode::Text("Two".to_string())],
                scores: HashMap::new(),
                metadata: HashMap::new(),
            },
        ],
        scores: HashMap::new(),
        metadata: HashMap::new(),
    };
    assert_eq!(flat.link_density_stats(), (6, 6), "flat");
    // Fragment 9: script/style boundaries
    let with_script = DomNode::Element {
        tag: "body".to_string(),
        attrs: vec![],
        children: vec![
            DomNode::Text("a".to_string()),
            DomNode::Element {
                tag: "script".to_string(),
                attrs: vec![],
                children: vec![DomNode::Text("code".to_string())],
                scores: HashMap::new(),
                metadata: HashMap::new(),
            },
            DomNode::Text("b".to_string()),
        ],
        scores: HashMap::new(),
        metadata: HashMap::new(),
    };
    assert_eq!(with_script.link_density_stats(), (6, 0), "with_script");
    // Fragment 10: multiple <a> with interleaved text
    let multi_a = DomNode::Element {
        tag: "div".to_string(),
        attrs: vec![],
        children: vec![
            DomNode::Element {
                tag: "a".to_string(),
                attrs: vec![],
                children: vec![DomNode::Text("A".to_string())],
                scores: HashMap::new(),
                metadata: HashMap::new(),
            },
            DomNode::Text(" between ".to_string()),
            DomNode::Element {
                tag: "a".to_string(),
                attrs: vec![],
                children: vec![DomNode::Text("B".to_string())],
                scores: HashMap::new(),
                metadata: HashMap::new(),
            },
        ],
        scores: HashMap::new(),
        metadata: HashMap::new(),
    };
    assert_eq!(multi_a.link_density_stats(), (11, 2), "multi_a");
}
