use super::*;
use std::cell::RefCell;
use std::rc::Rc;

// ── walk_post_mut ────────────────────────────────────────────────

#[test]
fn test_walk_post_mut_visits_children_before_parent() {
    let mut tree = DomNode::Element {
        tag: "parent".into(),
        attrs: vec![],
        children: vec![DomNode::Element {
            tag: "child".into(),
            attrs: vec![],
            children: vec![DomNode::Text("grandchild".into())],
            scores: std::collections::HashMap::new(),
            metadata: std::collections::HashMap::new(),
        }],
        scores: std::collections::HashMap::new(),
        metadata: std::collections::HashMap::new(),
    };

    let visited = Rc::new(RefCell::new(Vec::new()));
    let v = visited.clone();
    let mut visit_filter = move |n: &mut DomNode| -> WalkerAction {
        match n {
            DomNode::Element { tag, .. } => v.borrow_mut().push(format!("el:{}", tag)),
            DomNode::Text(t) => v.borrow_mut().push(format!("text:{}", t)),
            _ => v.borrow_mut().push("other".into()),
        }
        WalkerAction::Continue
    };
    let mut filters: Vec<&mut WalkerFilter> = vec![&mut visit_filter];
    walk_post_mut(&mut tree, &mut filters, None);

    // Post-order: grandchild -> child (parent is entry point, not visited by filters)
    assert_eq!(
        *visited.borrow(),
        vec!["text:grandchild", "el:child"],
        "walk_post_mut must visit children before parent"
    );
}

#[test]
fn test_walk_post_mut_empty_nodes() {
    // A Text node has no children — walk is a no-op
    let mut node = DomNode::Text("hello".into());
    let mut filters: Vec<&mut WalkerFilter> = vec![];
    walk_post_mut(&mut node, &mut filters, None);
}

#[test]
fn test_walk_post_mut_empty_filters() {
    let mut parent = DomNode::Element {
        tag: "div".into(),
        attrs: vec![],
        children: vec![DomNode::Text("hello".into())],
        scores: std::collections::HashMap::new(),
        metadata: std::collections::HashMap::new(),
    };
    let mut filters: Vec<&mut WalkerFilter> = vec![];
    walk_post_mut(&mut parent, &mut filters, None);
    if let DomNode::Element { children, .. } = &parent {
        assert_eq!(children.len(), 1);
    } else {
        panic!("expected Element");
    }
}

#[test]
fn test_walk_post_mut_remove_removes_node() {
    // Tree [a, b, c] where b returns Remove
    let mut parent = DomNode::Element {
        tag: "parent".into(),
        attrs: vec![],
        children: vec![
            DomNode::Text("a".into()),
            DomNode::Text("b".into()),
            DomNode::Text("c".into()),
        ],
        scores: std::collections::HashMap::new(),
        metadata: std::collections::HashMap::new(),
    };
    let mut remove_b = |n: &mut DomNode| -> WalkerAction {
        match n {
            DomNode::Text(t) if t == "b" => WalkerAction::Remove,
            _ => WalkerAction::Continue,
        }
    };
    let mut filters: Vec<&mut WalkerFilter> = vec![&mut remove_b];
    walk_post_mut(&mut parent, &mut filters, None);
    if let DomNode::Element { children, .. } = &parent {
        assert_eq!(children.len(), 2, "should have [a, c]");
        if let DomNode::Text(t) = &children[0] {
            assert_eq!(t, "a");
        } else {
            panic!("expected Text node");
        }
        if let DomNode::Text(t) = &children[1] {
            assert_eq!(t, "c");
        } else {
            panic!("expected Text node");
        }
    } else {
        panic!("expected Element");
    }
}

#[test]
fn test_walk_post_mut_remove_element_node() {
    // A child Element matching the filter is removed from the parent's children.
    let mut node = DomNode::Element {
        tag: "parent".into(),
        attrs: vec![],
        children: vec![DomNode::Element {
            tag: "div".into(),
            attrs: vec![],
            children: vec![DomNode::Text("child".into())],
            scores: std::collections::HashMap::new(),
            metadata: std::collections::HashMap::new(),
        }],
        scores: std::collections::HashMap::new(),
        metadata: std::collections::HashMap::new(),
    };
    let mut remove_div = |n: &mut DomNode| match n {
        DomNode::Element { tag, .. } if tag == "div" => WalkerAction::Remove,
        _ => WalkerAction::Continue,
    };
    let mut filters: Vec<&mut WalkerFilter> = vec![&mut remove_div];
    walk_post_mut(&mut node, &mut filters, None);
    // The child div element is removed by the filter; parent retains no children
    if let DomNode::Element { children, .. } = &node {
        assert_eq!(children.len(), 0, "child div should be removed");
    } else {
        panic!("expected Element");
    }
}

#[test]
fn test_walk_post_mut_remove_siblings_still_visited() {
    // Tree: parent with children [a(child_a), b(child_b), c(child_c)]
    // b is removed in post-order. Children of b are visited before removal.
    // Children of c should still be visited (right-side siblings shift into position).
    let mut parent = DomNode::Element {
        tag: "parent".into(),
        attrs: vec![],
        children: vec![
            DomNode::Element {
                tag: "a".into(),
                attrs: vec![],
                children: vec![DomNode::Text("child_a".into())],
                scores: std::collections::HashMap::new(),
                metadata: std::collections::HashMap::new(),
            },
            DomNode::Element {
                tag: "b".into(),
                attrs: vec![],
                children: vec![DomNode::Text("child_b".into())],
                scores: std::collections::HashMap::new(),
                metadata: std::collections::HashMap::new(),
            },
            DomNode::Element {
                tag: "c".into(),
                attrs: vec![],
                children: vec![DomNode::Text("child_c".into())],
                scores: std::collections::HashMap::new(),
                metadata: std::collections::HashMap::new(),
            },
        ],
        scores: std::collections::HashMap::new(),
        metadata: std::collections::HashMap::new(),
    };

    let visited = Rc::new(RefCell::new(Vec::new()));
    let v = visited.clone();
    let mut remove_b = move |n: &mut DomNode| -> WalkerAction {
        match n {
            DomNode::Element { tag, .. } if tag == "b" => WalkerAction::Remove,
            DomNode::Element { tag, .. } => {
                v.borrow_mut().push(format!("el:{}", tag));
                WalkerAction::Continue
            }
            DomNode::Text(t) => {
                v.borrow_mut().push(format!("text:{}", t));
                WalkerAction::Continue
            }
            _ => WalkerAction::Continue,
        }
    };
    let mut filters: Vec<&mut WalkerFilter> = vec![&mut remove_b];
    walk_post_mut(&mut parent, &mut filters, None);

    if let DomNode::Element { children, .. } = &parent {
        assert_eq!(children.len(), 2, "should have [a, c]");
        // child_b IS visited (children processed before parent removal in post-order).
        // c and child_c are visited (right-side siblings shift into position after removal).
        assert!(
            visited.borrow().contains(&"text:child_c".to_string()),
            "child_c should be visited"
        );
        assert!(
            visited.borrow().contains(&"text:child_a".to_string()),
            "child_a should be visited"
        );
    } else {
        panic!("expected Element");
    }
}

#[test]
fn test_walk_post_mut_remove_last_sibling() {
    let mut parent = DomNode::Element {
        tag: "parent".into(),
        attrs: vec![],
        children: vec![DomNode::Text("a".into()), DomNode::Text("b".into())],
        scores: std::collections::HashMap::new(),
        metadata: std::collections::HashMap::new(),
    };
    let mut remove_b = |n: &mut DomNode| -> WalkerAction {
        match n {
            DomNode::Text(t) if t == "b" => WalkerAction::Remove,
            _ => WalkerAction::Continue,
        }
    };
    let mut filters: Vec<&mut WalkerFilter> = vec![&mut remove_b];
    walk_post_mut(&mut parent, &mut filters, None);
    if let DomNode::Element { children, .. } = &parent {
        assert_eq!(children.len(), 1);
        if let DomNode::Text(t) = &children[0] {
            assert_eq!(t, "a");
        } else {
            panic!("expected Text node");
        }
    } else {
        panic!("expected Element");
    }
}

#[test]
#[should_panic(expected = "SkipChildren has no effect in post-order")]
fn test_walk_post_mut_skip_children_panics() {
    let mut node = DomNode::Element {
        tag: "div".into(),
        attrs: vec![],
        children: vec![DomNode::Text("child".into())],
        scores: std::collections::HashMap::new(),
        metadata: std::collections::HashMap::new(),
    };
    let mut skip_filter = |_: &mut DomNode| -> WalkerAction { WalkerAction::SkipChildren };
    let mut filters: Vec<&mut WalkerFilter> = vec![&mut skip_filter];
    walk_post_mut(&mut node, &mut filters, None);
}

#[test]
fn test_walk_post_mut_multi_callback() {
    let mut parent = DomNode::Element {
        tag: "parent".into(),
        attrs: vec![],
        children: vec![DomNode::Text("a".into()), DomNode::Text("b".into())],
        scores: std::collections::HashMap::new(),
        metadata: std::collections::HashMap::new(),
    };

    // First filter marks visited nodes
    let visited1 = Rc::new(RefCell::new(Vec::new()));
    let v1 = visited1.clone();
    let mut filter1 = move |n: &mut DomNode| -> WalkerAction {
        if let DomNode::Text(t) = n {
            v1.borrow_mut().push(t.clone());
        }
        WalkerAction::Continue
    };

    // Second filter also marks visited nodes
    let visited2 = Rc::new(RefCell::new(Vec::new()));
    let v2 = visited2.clone();
    let mut filter2 = move |n: &mut DomNode| -> WalkerAction {
        if let DomNode::Text(t) = n {
            v2.borrow_mut().push(t.clone());
        }
        WalkerAction::Continue
    };

    let mut filters: Vec<&mut WalkerFilter> = vec![&mut filter1, &mut filter2];
    walk_post_mut(&mut parent, &mut filters, None);
    assert_eq!(*visited1.borrow(), vec!["a", "b"]);
    assert_eq!(*visited2.borrow(), vec!["a", "b"]);
}

#[test]
fn test_walk_post_mut_multi_callback_break_on_remove() {
    // Filter A removes node 'b', filter B should NOT see 'b'
    let mut parent = DomNode::Element {
        tag: "parent".into(),
        attrs: vec![],
        children: vec![DomNode::Text("a".into()), DomNode::Text("b".into())],
        scores: std::collections::HashMap::new(),
        metadata: std::collections::HashMap::new(),
    };

    let visited_b = Rc::new(RefCell::new(Vec::new()));
    let vb = visited_b.clone();
    let mut filter_a = |n: &mut DomNode| -> WalkerAction {
        match n {
            DomNode::Text(t) if t == "b" => WalkerAction::Remove,
            _ => WalkerAction::Continue,
        }
    };

    let mut filter_b = move |n: &mut DomNode| -> WalkerAction {
        if let DomNode::Text(t) = n {
            vb.borrow_mut().push(t.clone());
        }
        WalkerAction::Continue
    };

    let mut filters: Vec<&mut WalkerFilter> = vec![&mut filter_a, &mut filter_b];
    walk_post_mut(&mut parent, &mut filters, None);
    // filter_b should NOT see 'b' because filter_a removed it
    assert_eq!(
        *visited_b.borrow(),
        vec!["a"],
        "filter_b should not see removed node"
    );
}

#[test]
fn test_walk_post_mut_continue_leaves_in_place() {
    let mut node = DomNode::Element {
        tag: "div".into(),
        attrs: vec![],
        children: vec![DomNode::Text("inner".into())],
        scores: std::collections::HashMap::new(),
        metadata: std::collections::HashMap::new(),
    };
    let mut continue_filter = |_: &mut DomNode| -> WalkerAction { WalkerAction::Continue };
    let mut filters: Vec<&mut WalkerFilter> = vec![&mut continue_filter];
    walk_post_mut(&mut node, &mut filters, None);
    if let DomNode::Element { tag, children, .. } = &node {
        assert_eq!(tag, "div");
        assert_eq!(children.len(), 1);
    } else {
        panic!("expected Element");
    }
}

// ── should_descend ────────────────────────────────────────────────────

#[test]
fn test_walk_post_mut_should_descend_prevents_child_visitation() {
    // should_descend returning false must prevent descent into grandchildren.
    let mut tree = DomNode::Element {
        tag: "parent".into(),
        attrs: vec![],
        children: vec![DomNode::Element {
            tag: "child".into(),
            attrs: vec![],
            children: vec![DomNode::Text("grandchild".into())],
            scores: std::collections::HashMap::new(),
            metadata: std::collections::HashMap::new(),
        }],
        scores: std::collections::HashMap::new(),
        metadata: std::collections::HashMap::new(),
    };

    let visited = Rc::new(RefCell::new(Vec::new()));
    let v = visited.clone();
    let mut visit_filter = move |n: &mut DomNode| -> WalkerAction {
        match n {
            DomNode::Element { tag, .. } => v.borrow_mut().push(format!("el:{}", tag)),
            DomNode::Text(t) => v.borrow_mut().push(format!("text:{}", t)),
            _ => v.borrow_mut().push("other".into()),
        }
        WalkerAction::Continue
    };
    let mut filters: Vec<&mut WalkerFilter> = vec![&mut visit_filter];
    // should_descend returns false for ALL nodes — no grandchildren visited
    walk_post_mut(&mut tree, &mut filters, Some(|_| false));

    // The child is visited (filter runs on each child), but its own children
    // (grandchild) are blocked by should_descend.
    assert_eq!(
        *visited.borrow(),
        vec!["el:child"],
        "should_descend=false must prevent grandchild descent"
    );
}

#[test]
fn test_walk_post_mut_should_descend_default_descends() {
    // Without should_descend (None), must descend into all children (existing behavior).
    let mut tree = DomNode::Element {
        tag: "parent".into(),
        attrs: vec![],
        children: vec![DomNode::Element {
            tag: "child".into(),
            attrs: vec![],
            children: vec![DomNode::Text("grandchild".into())],
            scores: std::collections::HashMap::new(),
            metadata: std::collections::HashMap::new(),
        }],
        scores: std::collections::HashMap::new(),
        metadata: std::collections::HashMap::new(),
    };

    let visited = Rc::new(RefCell::new(Vec::new()));
    let v = visited.clone();
    let mut visit_filter = move |n: &mut DomNode| -> WalkerAction {
        match n {
            DomNode::Element { tag, .. } => v.borrow_mut().push(format!("el:{}", tag)),
            DomNode::Text(t) => v.borrow_mut().push(format!("text:{}", t)),
            _ => v.borrow_mut().push("other".into()),
        }
        WalkerAction::Continue
    };
    let mut filters: Vec<&mut WalkerFilter> = vec![&mut visit_filter];
    // should_descend=None — default descend into all children
    walk_post_mut(&mut tree, &mut filters, None);

    // Post-order: grandchild -> child (parent is entry point, not visited by filters)
    assert_eq!(
        *visited.borrow(),
        vec!["text:grandchild", "el:child"],
        "should_descend=None must descend into all children"
    );
}

#[test]
fn test_walk_post_mut_should_descend_data_table_guard() {
    // Simulate a data table guard: should_descend returns false for tables
    // with is_data_table=true metadata.
    let mut table_metadata = std::collections::HashMap::new();
    table_metadata.insert("is_data_table".to_string(), "true".to_string());

    let mut tree = DomNode::Element {
        tag: "root".into(),
        attrs: vec![],
        children: vec![DomNode::Element {
            tag: "table".into(),
            attrs: vec![],
            children: vec![DomNode::Element {
                tag: "tr".into(),
                attrs: vec![],
                children: vec![DomNode::Element {
                    tag: "td".into(),
                    attrs: vec![],
                    children: vec![DomNode::Text("data".into())],
                    scores: std::collections::HashMap::new(),
                    metadata: std::collections::HashMap::new(),
                }],
                scores: std::collections::HashMap::new(),
                metadata: std::collections::HashMap::new(),
            }],
            scores: std::collections::HashMap::new(),
            metadata: table_metadata,
        }],
        scores: std::collections::HashMap::new(),
        metadata: std::collections::HashMap::new(),
    };

    let visited = Rc::new(RefCell::new(Vec::new()));
    let v = visited.clone();
    let mut visit_filter = move |n: &mut DomNode| -> WalkerAction {
        match n {
            DomNode::Element { tag, .. } => v.borrow_mut().push(format!("el:{}", tag)),
            DomNode::Text(t) => v.borrow_mut().push(format!("text:{}", t)),
            _ => v.borrow_mut().push("other".into()),
        }
        WalkerAction::Continue
    };
    let mut filters: Vec<&mut WalkerFilter> = vec![&mut visit_filter];

    // Guard: block descent into data tables
    fn is_data_table(node: &DomNode) -> bool {
        !matches!(node, DomNode::Element { metadata, .. } if metadata.get("is_data_table").map(|s| s.as_str()) == Some("true"))
    }

    walk_post_mut(&mut tree, &mut filters, Some(is_data_table));

    // The table element is visited (filter runs on each child), but its
    // descendants (tr, td, text) are blocked by should_descend.
    assert_eq!(
        *visited.borrow(),
        vec!["el:table"],
        "data table guard must prevent table child descent"
    );
}

// ── ReplaceWithChildren ────────────────────────────────────────────

#[test]
fn test_walk_post_mut_replace_with_children_splices_elements() {
    // Tree: parent [a, b(b1, b2), c] where b returns ReplaceWithChildren.
    // After splice: parent [a, b1, b2, c]
    let mut parent = DomNode::Element {
        tag: "parent".into(),
        attrs: vec![],
        children: vec![
            DomNode::Text("a".into()),
            DomNode::Element {
                tag: "b".into(),
                attrs: vec![],
                children: vec![DomNode::Text("b1".into()), DomNode::Text("b2".into())],
                scores: std::collections::HashMap::new(),
                metadata: std::collections::HashMap::new(),
            },
            DomNode::Text("c".into()),
        ],
        scores: std::collections::HashMap::new(),
        metadata: std::collections::HashMap::new(),
    };
    let mut replace_b = |n: &mut DomNode| -> WalkerAction {
        match n {
            DomNode::Element { tag, .. } if tag == "b" => WalkerAction::ReplaceWithChildren,
            _ => WalkerAction::Continue,
        }
    };
    let mut filters: Vec<&mut WalkerFilter> = vec![&mut replace_b];
    walk_post_mut(&mut parent, &mut filters, None);
    if let DomNode::Element { children, .. } = &parent {
        assert_eq!(children.len(), 4, "should have [a, b1, b2, c]");
        // Verify order is preserved
        if let DomNode::Text(t) = &children[0] {
            assert_eq!(t, "a");
        } else {
            panic!("expected Text");
        }
        if let DomNode::Text(t) = &children[1] {
            assert_eq!(t, "b1");
        } else {
            panic!("expected Text");
        }
        if let DomNode::Text(t) = &children[2] {
            assert_eq!(t, "b2");
        } else {
            panic!("expected Text");
        }
        if let DomNode::Text(t) = &children[3] {
            assert_eq!(t, "c");
        } else {
            panic!("expected Text");
        }
    } else {
        panic!("expected Element");
    }
}

#[test]
fn test_walk_post_mut_replace_with_children_on_text_is_continue() {
    // ReplaceWithChildren on a Text node is silently treated as Continue.
    let mut parent = DomNode::Element {
        tag: "parent".into(),
        attrs: vec![],
        children: vec![DomNode::Text("hello".into())],
        scores: std::collections::HashMap::new(),
        metadata: std::collections::HashMap::new(),
    };
    let mut replace_text = |_: &mut DomNode| -> WalkerAction { WalkerAction::ReplaceWithChildren };
    let mut filters: Vec<&mut WalkerFilter> = vec![&mut replace_text];
    walk_post_mut(&mut parent, &mut filters, None);
    if let DomNode::Element { children, .. } = &parent {
        assert_eq!(children.len(), 1, "Text node should be preserved");
        if let DomNode::Text(t) = &children[0] {
            assert_eq!(t, "hello");
        } else {
            panic!("expected Text");
        }
    } else {
        panic!("expected Element");
    }
}

// ── Tail text preservation on element removal ─────────────────────────

#[test]
fn test_walk_pre_mut_remove_element_preserves_trailing_sibling_text() {
    // Regression guard for a CodeRabbit claim that removing an element via
    // children.remove(i) loses its TRAILING text (lxml `.tail`).
    //
    // In this DOM model trailing text is NOT stored as an element `.tail` field;
    // dom_convert.rs:92-93 pushes following text as a SEPARATE sibling
    // DomNode::Text child (dom_nodes.rs:23). So removing the <span> element
    // leaves the sibling Text node "tail" intact.
    //
    // HTML: <div><span>x</span>tail</div>
    let mut tree = crate::pipelines::parse_html("<div><span>x</span>tail</div>").expect("parse");

    // Remove the <span> element in pre-order.
    walk_pre_mut(&mut tree, &|n: &mut DomNode| match n {
        DomNode::Element { tag, .. } if tag == "span" => WalkerAction::Remove,
        _ => WalkerAction::Continue,
    });

    // Locate the <div> element and verify its children.
    let div = tree
        .descendants()
        .into_iter()
        .find(|n| matches!(n, DomNode::Element { tag, .. } if tag == "div"))
        .expect("div should remain");
    if let DomNode::Element { children, .. } = div {
        // After removing <span>, only the trailing sibling Text "tail" remains.
        assert_eq!(
            children.len(),
            1,
            "span removed, tail sibling should remain"
        );
        match &children[0] {
            DomNode::Text(t) => assert_eq!(t, "tail", "trailing text must survive removal"),
            other => panic!("expected trailing Text node, got {:?}", other),
        }
        // text_content of the div should still be "tail" ("x" was inside the removed span).
        assert_eq!(div.text_content(), "tail");
    } else {
        panic!("expected div Element");
    }
}
