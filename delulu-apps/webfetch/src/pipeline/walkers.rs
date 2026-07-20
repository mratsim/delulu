use crate::pipeline::DomNode;

/// Action returned by a walker callback.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WalkerAction {
    /// Continue walking (recurse into children).
    Continue,
    /// Remove this node from the tree.
    Remove,
    /// Keep this node but do not recurse into its children.
    SkipChildren,
    /// Replace this node with its children (splice them into the parent Vec).
    ///
    /// Only meaningful in `walk_post_mut` where children have already been
    /// visited. In `walk_pre_mut`, this action panics.
    ReplaceWithChildren,
}

/// A filter callback for use with `walk_post_mut`.
pub type WalkerFilter = dyn FnMut(&mut DomNode) -> WalkerAction;

/// A pipeline pass: a function that mutates a DOM tree in-place.
pub type PassFn = fn(&mut DomNode);

/// Post-order tree walk with bottom-up filter application.
///
/// # Precondition
/// - `node` is a valid DOM tree root (acyclic).
/// - `should_descend` must be infallible (must not panic).
///
/// # Postcondition
/// - Filters have been applied bottom-up (children before parent).
/// - Nodes for which any filter returned `WalkerAction::Remove` are removed.
/// - `ReplaceWithChildren` nodes are spliced in.
///
/// # Panic-if
/// - Any filter returns `WalkerAction::SkipChildren` — children are already
///   processed in post-order, so skipping is meaningless (caller bug).
/// - `should_descend` panics.
/// - `MAX_DEPTH` is NOT enforced. Callers processing untrusted HTML should
///   assess stack safety independently.
///
/// # Parameters
///
/// - `node`: The DOM node to walk (children are extracted internally).
/// - `filters`: Callbacks that receive each child node and return a `WalkerAction`.
/// - `should_descend`: Optional predicate checked before recursing into a node's
///   children. When it returns `false`, the walker does NOT recurse into children.
///
/// # WalkerAction semantics in post-order
///
/// - `Continue`: Node is left in place.
/// - `Remove`: Node is removed after its children have been processed.
/// - `SkipChildren`: Panics. Children were already processed before this filter
///   was called, so this action has no valid meaning in post-order.
/// - `ReplaceWithChildren`: The current node is replaced by its children (spliced
///   into the parent `Vec` at the same position). On non-Element nodes,
///   `ReplaceWithChildren` is silently treated as `Continue`.
///
/// # should_descend vs SkipChildren
///
/// - `WalkerAction::SkipChildren` in post-order **panics** — children were already
///   processed before the filter runs, so skipping has no valid meaning.
/// - The `should_descend` guard is checked **before** recursing into children.
///   When it returns `false`, the walker does NOT recurse, and no panic occurs.
///   This is the ONLY valid way to prevent child visitation in post-order.
///
/// # Recursion depth
///
/// This function recurses with stack depth equal to DOM tree depth.
/// `MAX_DEPTH` in `mod.rs` is NOT enforced here.
/// Callers processing untrusted HTML should assess stack safety independently.
///
/// # Remove cost
///
/// `WalkerAction::Remove` is O(N - i) in siblings via `Vec::remove`.
/// For large sibling lists, consider in-place mutation.
#[allow(clippy::collapsible_if)]
pub fn walk_post_mut(
    node: &mut DomNode,
    filters: &mut [&mut WalkerFilter],
    should_descend: Option<fn(&DomNode) -> bool>,
) {
    if let DomNode::Element { children, .. } = node {
        let mut i = 0;
        while i < children.len() {
            // Post-order: recurse into children FIRST
            if should_descend.map_or(true, |pred| pred(&children[i])) {
                walk_post_mut(&mut children[i], filters, should_descend);
            } else {
                tracing::debug!("should_descend blocked descent into element");
            }

            // Then run filters on the current node
            let mut removed_current = false;
            for filter in filters.iter_mut() {
                match filter(&mut children[i]) {
                    WalkerAction::Remove => {
                        // O(n) shift — Vec::remove moves all subsequent siblings left.
                        // Tolerable for typical DOM sibling counts (< 100). If this becomes
                        // a hotspot (e.g., removing thousands of siblings at one level),
                        // switch to swap_remove + post-pass reordering or a retain-based approach.
                        children.remove(i);
                        removed_current = true;
                        // Break out of filter loop since node is gone
                        break;
                    }
                    WalkerAction::Continue => {}
                    WalkerAction::SkipChildren => {
                        panic!(
                            "SkipChildren has no effect in post-order — children already processed"
                        );
                    }
                    WalkerAction::ReplaceWithChildren => {
                        if let DomNode::Element {
                            children: grand_children,
                            ..
                        } = &mut children[i]
                        {
                            let mut extracted = std::mem::take(grand_children);
                            let n = extracted.len();
                            // O(n) splice — shifts subsequent siblings. Same trade-offs as Vec::remove above.
                            children.splice(i..=i, extracted.drain(..));
                            removed_current = true;
                            i += n; // Skip past children already processed in recursion step
                            break;
                        }
                        // On non-Element nodes, ReplaceWithChildren is silently treated as Continue
                    }
                }
            }

            // Only increment if we did NOT remove the current node
            // (removal shifts the next sibling into position i)
            if !removed_current {
                i += 1;
            }
        }
    }
}

/// Post-order walk with bottom-up accumulation and removal.
///
/// Each node's filter receives the node and its children's accumulated values
/// (already computed, since children are processed first). Returns `(WalkerAction, A)`
/// where `A` is the accumulated value for this node.
///
/// The walker returns the accumulated values for all top-level nodes.
///
/// # Type parameters
/// - `A`: Accumulator type. Must implement `Default` (for empty/leaf nodes).
///
/// # should_descend
///
/// Same semantics as [`walk_post_mut`]: when `should_descend` returns `false`,
/// children are skipped and the filter receives an empty `&[]`.
pub fn walk_post_acc_mut<A: Default>(
    nodes: &mut Vec<DomNode>,
    should_descend: Option<fn(&DomNode) -> bool>,
    filter: &mut dyn FnMut(&mut DomNode, &[A]) -> (WalkerAction, A),
) -> Vec<A> {
    let mut results = Vec::with_capacity(nodes.len());
    let mut i = 0;
    while i < nodes.len() {
        let child_results = if should_descend.map_or(true, |pred| pred(&nodes[i])) {
            if let DomNode::Element { children, .. } = &mut nodes[i] {
                walk_post_acc_mut(children, should_descend, filter)
            } else {
                Vec::new()
            }
        } else {
            tracing::debug!("walk_post_acc_mut: should_descend blocked descent");
            Vec::new()
        };

        let (action, acc) = filter(&mut nodes[i], &child_results);
        match action {
            WalkerAction::Remove => {
                // O(n) shift — same as walk_post_mut above.
                nodes.remove(i);
            }
            WalkerAction::Continue => {
                results.push(acc);
                i += 1;
            }
            WalkerAction::SkipChildren => {
                panic!("SkipChildren has no effect in post-order");
            }
            WalkerAction::ReplaceWithChildren => {
                if let DomNode::Element { children, .. } = &mut nodes[i] {
                    let mut extracted = std::mem::take(children);
                    let n = extracted.len();
                    // O(n) splice — same as walk_post_mut above.
                    nodes.splice(i..=i, extracted.drain(..));
                    i += n;
                } else {
                    results.push(acc);
                    i += 1;
                }
            }
        }
    }
    results
}

// ---------------------------------------------------------------------------
// walk_pre_mut (filtering pass, supports removal)
// ---------------------------------------------------------------------------

/// Pre-order traversal with removal support.
///
/// # Precondition
/// - `node` is a valid DOM tree root (acyclic).
/// - `f` must not panic (intentional pre-alpha behavior).
///
/// # Postcondition
/// - All nodes in the tree rooted at `node` have been visited in pre-order.
/// - Nodes for which `f` returned `WalkerAction::Remove` are removed from their parent's children list.
/// - Nodes for which `f` returned `WalkerAction::SkipChildren` are kept but children are not visited.
///
/// # Panic-if
/// - `f` panics (intentional — pre-alpha crash-loudly principle).
/// - `f` returns `WalkerAction::ReplaceWithChildren` (invalid in pre-order).
/// - The DOM tree has cycles (stack overflow — not caught).
/// - `MAX_DEPTH` is NOT enforced. Callers processing untrusted HTML should assess stack safety independently.
pub fn walk_pre_mut(node: &mut DomNode, f: &impl Fn(&mut DomNode) -> WalkerAction) {
    if let DomNode::Element { children, .. } = node {
        let mut i = 0;
        while i < children.len() {
            let action = f(&mut children[i]);
            match action {
                WalkerAction::Continue => {
                    walk_pre_mut(&mut children[i], f);
                    i += 1;
                }
                WalkerAction::SkipChildren => {
                    i += 1;
                }
                WalkerAction::Remove => {
                    // O(n) shift — Vec::remove moves all subsequent siblings left.
                    // Tolerable for typical DOM sibling counts (< 100).
                    children.remove(i);
                    // Do not increment i; next element shifts into position i.
                }
                WalkerAction::ReplaceWithChildren => {
                    panic!("ReplaceWithChildren is not supported in pre-order traversal");
                }
            }
        }
    }
    // Non-Element nodes (Text, Comment, Doctype) are no-ops — no children to walk.
}

#[cfg(test)]
mod tests {
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
            match node {
                DomNode::Element { metadata, .. }
                    if metadata.get("is_data_table").map(|s| s.as_str()) == Some("true") =>
                {
                    false
                }
                _ => true,
            }
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
        let mut replace_text =
            |_: &mut DomNode| -> WalkerAction { WalkerAction::ReplaceWithChildren };
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
}
