use super::*;
use std::sync::{Arc, Mutex};

/// Helper struct that captures tracing output into a shared buffer.
struct CaptureWriter(Arc<Mutex<Vec<u8>>>);

impl std::io::Write for CaptureWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// Run a closure with a tracing subscriber that captures output into a buffer.
/// Returns the captured buffer so callers can assert on its contents.
fn with_captured_tracing<F: FnOnce()>(f: F) -> Arc<Mutex<Vec<u8>>> {
    let buf: Arc<Mutex<Vec<u8>>> = Arc::new(Mutex::new(Vec::new()));
    let buf_clone = buf.clone();
    let subscriber = tracing_subscriber::fmt()
        .with_writer(move || CaptureWriter(buf_clone.clone()))
        .finish();
    let _guard = tracing::subscriber::set_default(subscriber);
    f();
    buf
}
// ── pass_keep_qualifying_siblings ─────────────────────────────────────

#[test]
#[test]
fn test_qualifying_sibling_candidate_relative_floor() {
    // Best child score = 50, sibling score = 12.
    // Old floor (global_max * 0.2): if global_max > 60, floor > 12 → removed.
    // New floor (candidate_score * 0.2): floor = 50 * 0.2 = 10, 12 >= 10 → kept.
    let mut root = DomNode::Element {
        tag: "div".into(),
        attrs: vec![],
        children: vec![DomNode::Element {
            tag: "parent".into(),
            attrs: vec![],
            children: vec![
                DomNode::Element {
                    tag: "p".into(),
                    attrs: vec![],
                    children: vec![DomNode::Text(
                        "best child with some text for scoring".into(),
                    )],
                    scores: [("mozilla_readability".into(), 50.0)].into(),
                    metadata: [("md_rd_subtree_acc_score".into(), "50".into())].into(),
                },
                DomNode::Element {
                    tag: "p".into(),
                    attrs: vec![],
                    children: vec![DomNode::Text("sibling with some text for scoring".into())],
                    scores: [("mozilla_readability".into(), 12.0)].into(),
                    metadata: [("md_rd_subtree_acc_score".into(), "12".into())].into(),
                },
            ],
            scores: Default::default(),
            metadata: [("md_rd_subtree_acc_score".into(), "60".into())].into(),
        }],
        scores: Default::default(),
        metadata: [("md_rd_subtree_max_score".into(), "100".into())].into(),
    };
    pass_keep_qualifying_siblings(&mut root);
    // After fix: sibling should survive (floor = 50 * 0.2 = 10, sibling score = 12 >= 10)
    let root_children = match &root {
        DomNode::Element { children, .. } => children,
        _ => panic!("root should be Element"),
    };
    if let DomNode::Element { children, .. } = &root_children[0] {
        assert_eq!(
            children.len(),
            2,
            "both children should survive with candidate-relative floor"
        );
    } else {
        panic!("parent should be Element");
    }
}

#[test]
fn test_qualifying_sibling_candidate_relative_floor_multi_branch() {
    // Multi-branch: parent1 best=50, sibling=12. parent2 other=100.
    // global_max=100. Old floor=20 → sibling(12) removed.
    // New floor=max(50*0.2,10)=10 → sibling(12) kept.
    let mut root = DomNode::Element {
        tag: "div".into(),
        attrs: vec![],
        children: vec![
            DomNode::Element {
                tag: "parent1".into(),
                attrs: vec![],
                children: vec![
                    DomNode::Element {
                        tag: "p".into(),
                        attrs: vec![],
                        children: vec![DomNode::Text(
                            "best child with enough text for scoring".into(),
                        )],
                        scores: [("mozilla_readability".into(), 50.0)].into(),
                        metadata: [("md_rd_subtree_acc_score".into(), "50".into())].into(),
                    },
                    DomNode::Element {
                        tag: "p".into(),
                        attrs: vec![],
                        children: vec![DomNode::Text("sibling text content".into())],
                        scores: [("mozilla_readability".into(), 12.0)].into(),
                        metadata: [("md_rd_subtree_acc_score".into(), "12".into())].into(),
                    },
                ],
                scores: Default::default(),
                metadata: [("md_rd_subtree_acc_score".into(), "60".into())].into(),
            },
            DomNode::Element {
                tag: "parent2".into(),
                attrs: vec![],
                children: vec![DomNode::Element {
                    tag: "p".into(),
                    attrs: vec![],
                    children: vec![DomNode::Text(
                        "other branch with lots of content for scoring here".into(),
                    )],
                    scores: [("mozilla_readability".into(), 100.0)].into(),
                    metadata: [("md_rd_subtree_acc_score".into(), "100".into())].into(),
                }],
                scores: Default::default(),
                metadata: [("md_rd_subtree_acc_score".into(), "100".into())].into(),
            },
        ],
        scores: Default::default(),
        metadata: [("md_rd_subtree_max_score".into(), "100".into())].into(),
    };
    pass_keep_qualifying_siblings(&mut root);
    // parent1 should keep both children (sibling score 12 >= floor 10)
    let root_children = match &root {
        DomNode::Element { children, .. } => children,
        _ => panic!("root should be Element"),
    };
    if let DomNode::Element { children, .. } = &root_children[0] {
        assert_eq!(
            children.len(),
            2,
            "parent1 should keep both children with candidate-relative floor"
        );
    } else {
        panic!("parent1 should be Element");
    }
    // parent2 keeps its one child
    if let DomNode::Element { children, .. } = &root_children[1] {
        assert_eq!(children.len(), 1, "parent2 should keep its child");
    } else {
        panic!("parent2 should be Element");
    }
}

#[test]
fn test_qualifying_sibling_missing_score() {
    // Sibling with missing md_rd_subtree_acc_score should survive (f64::MAX fallback).
    let mut root = DomNode::Element {
        tag: "div".into(),
        attrs: vec![],
        children: vec![DomNode::Element {
            tag: "parent".into(),
            attrs: vec![],
            children: vec![
                DomNode::Element {
                    tag: "p".into(),
                    attrs: vec![],
                    children: vec![DomNode::Text("best child text for scoring".into())],
                    scores: [("mozilla_readability".into(), 50.0)].into(),
                    metadata: [("md_rd_subtree_acc_score".into(), "50".into())].into(),
                },
                DomNode::Element {
                    tag: "span".into(),
                    attrs: vec![],
                    children: vec![DomNode::Text("sibling content".into())],
                    scores: Default::default(),
                    metadata: [("md_rd_subtree_acc_score".into(), "15".into())].into(),
                },
            ],
            scores: Default::default(),
            metadata: [("md_rd_subtree_acc_score".into(), "60".into())].into(),
        }],
        scores: Default::default(),
        metadata: [("md_rd_subtree_max_score".into(), "100".into())].into(),
    };
    pass_keep_qualifying_siblings(&mut root);
    // The span sibling has no score — should be preserved via f64::MAX fallback
    let root_children = match &root {
        DomNode::Element { children, .. } => children,
        _ => panic!("root should be Element"),
    };
    if let DomNode::Element { children, .. } = &root_children[0] {
        assert_eq!(
            children.len(),
            2,
            "sibling with score 15 should survive (floor 50*0.2=10)"
        );
    } else {
        panic!("parent should be Element");
    }
}

#[test]
fn test_qualifying_sibling_below_floor_removed_relative() {
    // Sibling with score below candidate-relative floor should be removed.
    let mut root = DomNode::Element {
        tag: "div".into(),
        attrs: vec![],
        children: vec![DomNode::Element {
            tag: "parent".into(),
            attrs: vec![],
            children: vec![
                DomNode::Element {
                    tag: "p".into(),
                    attrs: vec![],
                    children: vec![DomNode::Text("best child text for scoring".into())],
                    scores: [("mozilla_readability".into(), 100.0)].into(),
                    metadata: [("md_rd_subtree_acc_score".into(), "100".into())].into(),
                },
                DomNode::Element {
                    tag: "p".into(),
                    attrs: vec![],
                    children: vec![DomNode::Text("low sibling".into())],
                    scores: [("mozilla_readability".into(), 5.0)].into(),
                    metadata: [("md_rd_subtree_acc_score".into(), "5".into())].into(),
                },
            ],
            scores: Default::default(),
            metadata: [("md_rd_subtree_acc_score".into(), "100".into())].into(),
        }],
        scores: Default::default(),
        metadata: [("md_rd_subtree_max_score".into(), "100".into())].into(),
    };
    pass_keep_qualifying_siblings(&mut root);
    // Sibling score 5 < floor 20 (100*0.2) → should be removed
    let root_children = match &root {
        DomNode::Element { children, .. } => children,
        _ => panic!("root should be Element"),
    };
    if let DomNode::Element { children, .. } = &root_children[0] {
        assert_eq!(children.len(), 1, "low-scoring sibling should be removed");
    } else {
        panic!("parent should be Element");
    }
}
#[test]
fn test_qualifying_sibling_score_floor_kept() {
    // Sibling with score >= sibling_floor should be kept.
    // global_max = 100.0, sibling_floor = (100.0 * 0.2).max(10.0) = 20.0
    // best child (article) score = 100.0, sibling (section) score = 25.0 >= 20.0 → kept
    // Structure: root > parent > [article (best), section (sibling)]
    let mut root = DomNode::Element {
        tag: "div".into(),
        attrs: vec![],
        children: vec![DomNode::Element {
            tag: "section".into(),
            attrs: vec![],
            children: vec![
                DomNode::Element {
                    tag: "article".into(),
                    attrs: vec![],
                    children: vec![DomNode::Text("content".into())],
                    scores: Default::default(),
                    metadata: [("md_rd_subtree_acc_score".into(), "100.0".into())].into(),
                },
                DomNode::Element {
                    tag: "section".into(),
                    attrs: vec![],
                    children: vec![],
                    scores: Default::default(),
                    metadata: [("md_rd_subtree_acc_score".into(), "25.0".into())].into(),
                },
            ],
            scores: Default::default(),
            metadata: [("md_rd_subtree_acc_score".into(), "50.0".into())].into(),
        }],
        scores: Default::default(),
        metadata: [("md_rd_subtree_max_score".into(), "100.0".into())].into(),
    };
    pass_keep_qualifying_siblings(&mut root);
    if let DomNode::Element { children, .. } = &root {
        assert_eq!(children.len(), 1, "root should have 1 child (parent)");
        if let DomNode::Element {
            tag,
            children: inner,
            ..
        } = &children[0]
        {
            assert_eq!(tag, "section", "parent should remain");
            assert_eq!(
                inner.len(),
                2,
                "both best child and floor-qualified sibling should be kept"
            );
            let tags: Vec<&str> = inner
                .iter()
                .filter_map(|c| match c {
                    DomNode::Element { tag, .. } => Some(tag.as_str()),
                    _ => None,
                })
                .collect();
            assert!(
                tags.contains(&"article"),
                "article (best child) should be kept"
            );
            assert!(
                tags.contains(&"section"),
                "section (floor-qualified) should be kept"
            );
        } else {
            panic!("root child should be Element");
        }
    } else {
        panic!("root should remain Element");
    }
}

#[test]
fn test_qualifying_sibling_below_floor_removed() {
    // Sibling with score < sibling_floor should be removed.
    // global_max = 100.0, sibling_floor = 20.0
    // sibling (span) score = 5.0 < 20.0 → removed
    // Structure: root > parent > [article (best), span (sibling)]
    let mut root = DomNode::Element {
        tag: "div".into(),
        attrs: vec![],
        children: vec![DomNode::Element {
            tag: "section".into(),
            attrs: vec![],
            children: vec![
                DomNode::Element {
                    tag: "article".into(),
                    attrs: vec![],
                    children: vec![DomNode::Text("content".into())],
                    scores: Default::default(),
                    metadata: [("md_rd_subtree_acc_score".into(), "100.0".into())].into(),
                },
                DomNode::Element {
                    tag: "span".into(),
                    attrs: vec![],
                    children: vec![],
                    scores: Default::default(),
                    metadata: [("md_rd_subtree_acc_score".into(), "5.0".into())].into(),
                },
            ],
            scores: Default::default(),
            metadata: [("md_rd_subtree_acc_score".into(), "50.0".into())].into(),
        }],
        scores: Default::default(),
        metadata: [("md_rd_subtree_max_score".into(), "100.0".into())].into(),
    };
    pass_keep_qualifying_siblings(&mut root);
    if let DomNode::Element { children, .. } = &root {
        assert_eq!(children.len(), 1, "root should have 1 child (parent)");
        if let DomNode::Element {
            tag,
            children: inner,
            ..
        } = &children[0]
        {
            assert_eq!(tag, "section", "parent should remain");
            assert_eq!(inner.len(), 1, "only best child should remain");
            if let DomNode::Element { tag: ct, .. } = &inner[0] {
                assert_eq!(ct, "article", "article (best child) should be kept");
            } else {
                panic!("child should be Element");
            }
        } else {
            panic!("root child should be Element");
        }
    } else {
        panic!("root should remain Element");
    }
}

#[test]
fn test_qualifying_sibling_same_class_bonus() {
    // Same-class bonus (+20%) keeps an otherwise low-scored sibling.
    // candidate_score = 100.0, same-class bonus = 100.0 * 0.2 = 20.0
    // sibling score = 5.0 + 20.0 bonus = 25.0 >= 20.0 floor → kept
    // Structure: root > parent > [article (best, class=content), div (sibling, class=content)]
    let mut root = DomNode::Element {
        tag: "div".into(),
        attrs: vec![],
        children: vec![DomNode::Element {
            tag: "section".into(),
            attrs: vec![],
            children: vec![
                DomNode::Element {
                    tag: "article".into(),
                    attrs: vec![("class".into(), "content".into())],
                    children: vec![DomNode::Text("content".into())],
                    scores: Default::default(),
                    metadata: [("md_rd_subtree_acc_score".into(), "100.0".into())].into(),
                },
                DomNode::Element {
                    tag: "div".into(),
                    attrs: vec![("class".into(), "content".into())],
                    children: vec![],
                    scores: Default::default(),
                    metadata: [("md_rd_subtree_acc_score".into(), "5.0".into())].into(),
                },
            ],
            scores: Default::default(),
            metadata: [("md_rd_subtree_acc_score".into(), "50.0".into())].into(),
        }],
        scores: Default::default(),
        metadata: [("md_rd_subtree_max_score".into(), "100.0".into())].into(),
    };
    pass_keep_qualifying_siblings(&mut root);
    if let DomNode::Element { children, .. } = &root {
        assert_eq!(children.len(), 1, "root should have 1 child (parent)");
        if let DomNode::Element {
            tag,
            children: inner,
            ..
        } = &children[0]
        {
            assert_eq!(tag, "section", "parent should remain");
            assert_eq!(
                inner.len(),
                2,
                "both best child and same-class sibling should be kept"
            );
            let tags: Vec<&str> = inner
                .iter()
                .filter_map(|c| match c {
                    DomNode::Element { tag, .. } => Some(tag.as_str()),
                    _ => None,
                })
                .collect();
            assert!(
                tags.contains(&"article"),
                "article (best child) should be kept"
            );
            assert!(
                tags.contains(&"div"),
                "div (same-class sibling) should be kept via bonus"
            );
        } else {
            panic!("root child should be Element");
        }
    } else {
        panic!("root should remain Element");
    }
}

#[test]
fn test_qualifying_sibling_p_long_text() {
    // P-sibling long-text heuristic (node_length > 80 AND link_density < 0.25).
    let long_text = "This is a long paragraph that exceeds eighty characters in total length so that it triggers the p-sibling heuristic for keeping low-scored p elements that have meaningful content.";
    let mut root = DomNode::Element {
        tag: "div".into(),
        attrs: vec![],
        children: vec![DomNode::Element {
            tag: "section".into(),
            attrs: vec![],
            children: vec![
                DomNode::Element {
                    tag: "article".into(),
                    attrs: vec![],
                    children: vec![DomNode::Text("content".into())],
                    scores: Default::default(),
                    metadata: [("md_rd_subtree_acc_score".into(), "100.0".into())].into(),
                },
                DomNode::Element {
                    tag: "p".into(),
                    attrs: vec![],
                    children: vec![DomNode::Text(long_text.into())],
                    scores: Default::default(),
                    metadata: [
                        ("md_rd_subtree_acc_score".into(), "5.0".into()),
                        ("link_density".into(), "0.1".into()),
                    ]
                    .into(),
                },
            ],
            scores: Default::default(),
            metadata: [("md_rd_subtree_acc_score".into(), "50.0".into())].into(),
        }],
        scores: Default::default(),
        metadata: [("md_rd_subtree_max_score".into(), "100.0".into())].into(),
    };
    pass_keep_qualifying_siblings(&mut root);
    if let DomNode::Element { children, .. } = &root {
        assert_eq!(children.len(), 1, "root should have 1 child (parent)");
        if let DomNode::Element {
            tag,
            children: inner,
            ..
        } = &children[0]
        {
            assert_eq!(tag, "section", "parent should remain");
            assert_eq!(
                inner.len(),
                2,
                "both best child and long-text p-sibling should be kept"
            );
            let tags: Vec<&str> = inner
                .iter()
                .filter_map(|c| match c {
                    DomNode::Element { tag, .. } => Some(tag.as_str()),
                    _ => None,
                })
                .collect();
            assert!(
                tags.contains(&"article"),
                "article (best child) should be kept"
            );
            assert!(
                tags.contains(&"p"),
                "p (long-text sibling) should be kept via heuristic"
            );
        } else {
            panic!("root child should be Element");
        }
    } else {
        panic!("root should remain Element");
    }
}

#[test]
fn test_qualifying_sibling_p_short_sentence() {
    // P-sibling short-sentence heuristic (length > 0, ≤ 80, link_density == 0.0, contains ". " or ends with '.').
    let mut root = DomNode::Element {
        tag: "div".into(),
        attrs: vec![],
        children: vec![DomNode::Element {
            tag: "section".into(),
            attrs: vec![],
            children: vec![
                DomNode::Element {
                    tag: "article".into(),
                    attrs: vec![],
                    children: vec![DomNode::Text("content".into())],
                    scores: Default::default(),
                    metadata: [("md_rd_subtree_acc_score".into(), "100.0".into())].into(),
                },
                DomNode::Element {
                    tag: "p".into(),
                    attrs: vec![],
                    children: vec![DomNode::Text("Short sentence.".into())],
                    scores: Default::default(),
                    metadata: [
                        ("md_rd_subtree_acc_score".into(), "5.0".into()),
                        ("link_density".into(), "0.0".into()),
                    ]
                    .into(),
                },
            ],
            scores: Default::default(),
            metadata: [("md_rd_subtree_acc_score".into(), "50.0".into())].into(),
        }],
        scores: Default::default(),
        metadata: [("md_rd_subtree_max_score".into(), "100.0".into())].into(),
    };
    pass_keep_qualifying_siblings(&mut root);
    if let DomNode::Element { children, .. } = &root {
        assert_eq!(children.len(), 1, "root should have 1 child (parent)");
        if let DomNode::Element {
            tag,
            children: inner,
            ..
        } = &children[0]
        {
            assert_eq!(tag, "section", "parent should remain");
            assert_eq!(
                inner.len(),
                2,
                "both best child and short-sentence p-sibling should be kept"
            );
            let tags: Vec<&str> = inner
                .iter()
                .filter_map(|c| match c {
                    DomNode::Element { tag, .. } => Some(tag.as_str()),
                    _ => None,
                })
                .collect();
            assert!(
                tags.contains(&"article"),
                "article (best child) should be kept"
            );
            assert!(
                tags.contains(&"p"),
                "p (short-sentence sibling) should be kept via heuristic"
            );
        } else {
            panic!("root child should be Element");
        }
    } else {
        panic!("root should remain Element");
    }
}

#[test]
fn test_qualifying_sibling_p_high_link_density() {
    // P-sibling with high link_density (>= 0.25) should be removed.
    let long_text = "This is a long paragraph that exceeds eighty characters in total length so that it would trigger the p-sibling heuristic but has high link density.";
    let mut root = DomNode::Element {
        tag: "div".into(),
        attrs: vec![],
        children: vec![DomNode::Element {
            tag: "section".into(),
            attrs: vec![],
            children: vec![
                DomNode::Element {
                    tag: "article".into(),
                    attrs: vec![],
                    children: vec![DomNode::Text("content".into())],
                    scores: Default::default(),
                    metadata: [("md_rd_subtree_acc_score".into(), "100.0".into())].into(),
                },
                DomNode::Element {
                    tag: "p".into(),
                    attrs: vec![],
                    children: vec![DomNode::Text(long_text.into())],
                    scores: Default::default(),
                    metadata: [
                        ("md_rd_subtree_acc_score".into(), "5.0".into()),
                        ("link_density".into(), "0.5".into()),
                    ]
                    .into(),
                },
            ],
            scores: Default::default(),
            metadata: [("md_rd_subtree_acc_score".into(), "50.0".into())].into(),
        }],
        scores: Default::default(),
        metadata: [("md_rd_subtree_max_score".into(), "100.0".into())].into(),
    };
    pass_keep_qualifying_siblings(&mut root);
    if let DomNode::Element { children, .. } = &root {
        assert_eq!(children.len(), 1, "root should have 1 child (parent)");
        if let DomNode::Element {
            tag,
            children: inner,
            ..
        } = &children[0]
        {
            assert_eq!(tag, "section", "parent should remain");
            assert_eq!(
                inner.len(),
                1,
                "only best child should remain, high-LD p removed"
            );
            if let DomNode::Element { tag: ct, .. } = &inner[0] {
                assert_eq!(ct, "article", "article (best child) should be kept");
            } else {
                panic!("child should be Element");
            }
        } else {
            panic!("root child should be Element");
        }
    } else {
        panic!("root should remain Element");
    }
}

#[test]
fn test_qualifying_sibling_body_html_excluded() {
    // Body/html children excluded from best child selection.
    // body has score 200.0 but is excluded; article with 100.0 is selected as best.
    // Structure: root > parent > [body (score=200), article (score=100)]
    let mut root = DomNode::Element {
        tag: "div".into(),
        attrs: vec![],
        children: vec![DomNode::Element {
            tag: "section".into(),
            attrs: vec![],
            children: vec![
                DomNode::Element {
                    tag: "body".into(),
                    attrs: vec![],
                    children: vec![],
                    scores: Default::default(),
                    metadata: [("md_rd_subtree_acc_score".into(), "200.0".into())].into(),
                },
                DomNode::Element {
                    tag: "article".into(),
                    attrs: vec![],
                    children: vec![DomNode::Text("content".into())],
                    scores: Default::default(),
                    metadata: [("md_rd_subtree_acc_score".into(), "100.0".into())].into(),
                },
            ],
            scores: Default::default(),
            metadata: [("md_rd_subtree_acc_score".into(), "50.0".into())].into(),
        }],
        scores: Default::default(),
        metadata: [("md_rd_subtree_max_score".into(), "200.0".into())].into(),
    };
    pass_keep_qualifying_siblings(&mut root);
    if let DomNode::Element { children, .. } = &root {
        assert_eq!(children.len(), 1, "root should have 1 child (parent)");
        if let DomNode::Element {
            tag,
            children: inner,
            ..
        } = &children[0]
        {
            assert_eq!(tag, "section", "parent should remain");
            // body is excluded from best child selection, so article is selected.
            // body has score 200.0 which is >= sibling_floor (200.0*0.2=40.0), so body is kept as sibling.
            assert_eq!(
                inner.len(),
                2,
                "both article (best) and body (qualifying sibling) should be kept"
            );
            let tags: Vec<&str> = inner
                .iter()
                .filter_map(|c| match c {
                    DomNode::Element { tag, .. } => Some(tag.as_str()),
                    _ => None,
                })
                .collect();
            assert!(
                tags.contains(&"article"),
                "article should be selected as best child"
            );
            assert!(
                tags.contains(&"body"),
                "body should be kept as qualifying sibling"
            );
        } else {
            panic!("root child should be Element");
        }
    } else {
        panic!("root should remain Element");
    }
}

#[test]
fn test_qualifying_sibling_no_qualifying() {
    // No qualifying siblings → only best child kept.
    // sibling (span) score = 3.0 < 20.0 floor, no class bonus, not <p> → removed.
    // Structure: root > parent > [article (best), span (sibling)]
    let mut root = DomNode::Element {
        tag: "div".into(),
        attrs: vec![],
        children: vec![DomNode::Element {
            tag: "section".into(),
            attrs: vec![],
            children: vec![
                DomNode::Element {
                    tag: "article".into(),
                    attrs: vec![],
                    children: vec![DomNode::Text("content".into())],
                    scores: Default::default(),
                    metadata: [("md_rd_subtree_acc_score".into(), "100.0".into())].into(),
                },
                DomNode::Element {
                    tag: "span".into(),
                    attrs: vec![],
                    children: vec![],
                    scores: Default::default(),
                    metadata: [("md_rd_subtree_acc_score".into(), "3.0".into())].into(),
                },
            ],
            scores: Default::default(),
            metadata: [("md_rd_subtree_acc_score".into(), "50.0".into())].into(),
        }],
        scores: Default::default(),
        metadata: [("md_rd_subtree_max_score".into(), "100.0".into())].into(),
    };
    pass_keep_qualifying_siblings(&mut root);
    if let DomNode::Element { children, .. } = &root {
        assert_eq!(children.len(), 1, "root should have 1 child (parent)");
        if let DomNode::Element {
            tag,
            children: inner,
            ..
        } = &children[0]
        {
            assert_eq!(tag, "section", "parent should remain");
            assert_eq!(inner.len(), 1, "only best child should remain");
            if let DomNode::Element { tag: ct, .. } = &inner[0] {
                assert_eq!(ct, "article", "article (best child) should be kept");
            } else {
                panic!("child should be Element");
            }
        } else {
            panic!("root child should be Element");
        }
    } else {
        panic!("root should remain Element");
    }
}

#[test]
fn test_qualifying_sibling_non_element_preserved() {
    // Non-Element siblings (Text nodes) should be preserved (not removed by the pass).
    // Structure: root > parent > [article (best), Text("some text")]
    let mut root = DomNode::Element {
        tag: "div".into(),
        attrs: vec![],
        children: vec![DomNode::Element {
            tag: "section".into(),
            attrs: vec![],
            children: vec![
                DomNode::Element {
                    tag: "article".into(),
                    attrs: vec![],
                    children: vec![DomNode::Text("content".into())],
                    scores: Default::default(),
                    metadata: [("md_rd_subtree_acc_score".into(), "100.0".into())].into(),
                },
                DomNode::Text("some text".into()),
            ],
            scores: Default::default(),
            metadata: [("md_rd_subtree_acc_score".into(), "50.0".into())].into(),
        }],
        scores: Default::default(),
        metadata: [("md_rd_subtree_max_score".into(), "100.0".into())].into(),
    };
    pass_keep_qualifying_siblings(&mut root);
    if let DomNode::Element { children, .. } = &root {
        assert_eq!(children.len(), 1, "root should have 1 child (parent)");
        if let DomNode::Element {
            tag,
            children: inner,
            ..
        } = &children[0]
        {
            assert_eq!(tag, "section", "parent should remain");
            assert_eq!(
                inner.len(),
                2,
                "best child and text node should be preserved"
            );
            let has_text = inner
                .iter()
                .any(|c| matches!(c, DomNode::Text(t) if t == "some text"));
            assert!(has_text, "text node should be preserved");
            let has_article = inner
                .iter()
                .any(|c| matches!(c, DomNode::Element { tag, .. } if tag == "article"));
            assert!(has_article, "article should be kept");
        } else {
            panic!("root child should be Element");
        }
    } else {
        panic!("root should remain Element");
    }
}

// ── should_keep_sibling unit tests ───────────────────────────────────────

#[test]
fn test_should_keep_sibling_score_floor() {
    // Sibling with score >= sibling_floor → true.
    let sibling = DomNode::Element {
        tag: "div".into(),
        attrs: vec![],
        children: vec![],
        scores: Default::default(),
        metadata: [("md_rd_subtree_acc_score".into(), "30.0".into())].into(),
    };
    assert!(should_keep_sibling(&sibling, 100.0, "", 20.0));
}

#[test]
fn test_should_keep_sibling_below_floor() {
    // Sibling with score < sibling_floor, no class bonus, not <p> → false.
    let sibling = DomNode::Element {
        tag: "span".into(),
        attrs: vec![],
        children: vec![],
        scores: Default::default(),
        metadata: [("md_rd_subtree_acc_score".into(), "5.0".into())].into(),
    };
    assert!(!should_keep_sibling(&sibling, 100.0, "", 20.0));
}

#[test]
fn test_should_keep_sibling_class_bonus() {
    // Same-class bonus (+20%) lifts effective score above floor.
    let sibling = DomNode::Element {
        tag: "div".into(),
        attrs: vec![("class".into(), "content".into())],
        children: vec![],
        scores: Default::default(),
        metadata: [("md_rd_subtree_acc_score".into(), "5.0".into())].into(),
    };
    assert!(should_keep_sibling(&sibling, 100.0, "content", 20.0));
}

#[test]
fn test_should_keep_sibling_class_bonus_different_class() {
    // Different class → no bonus.
    let sibling = DomNode::Element {
        tag: "div".into(),
        attrs: vec![("class".into(), "other".into())],
        children: vec![],
        scores: Default::default(),
        metadata: [("md_rd_subtree_acc_score".into(), "5.0".into())].into(),
    };
    assert!(!should_keep_sibling(&sibling, 100.0, "content", 20.0));
}

#[test]
fn test_should_keep_sibling_p_long_text() {
    // <p> with long text (>80 chars) and low link_density (<0.25) → true.
    let long_text = "This is a long paragraph that exceeds eighty characters in total length so that it triggers the p-sibling heuristic for keeping low-scored p elements that have meaningful content.";
    let sibling = DomNode::Element {
        tag: "p".into(),
        attrs: vec![],
        children: vec![DomNode::Text(long_text.into())],
        scores: Default::default(),
        metadata: [
            ("md_rd_subtree_acc_score".into(), "5.0".into()),
            ("link_density".into(), "0.1".into()),
        ]
        .into(),
    };
    assert!(should_keep_sibling(&sibling, 100.0, "", 20.0));
}

#[test]
fn test_should_keep_sibling_p_short_sentence() {
    // <p> with short sentence (<=80 chars), link_density == 0.0, ends with '.' → true.
    let sibling = DomNode::Element {
        tag: "p".into(),
        attrs: vec![],
        children: vec![DomNode::Text("Short sentence.".into())],
        scores: Default::default(),
        metadata: [
            ("md_rd_subtree_acc_score".into(), "5.0".into()),
            ("link_density".into(), "0.0".into()),
        ]
        .into(),
    };
    assert!(should_keep_sibling(&sibling, 100.0, "", 20.0));
}

#[test]
fn test_should_keep_sibling_p_short_sentence_with_space() {
    // <p> with short sentence containing ". " → true.
    let sibling = DomNode::Element {
        tag: "p".into(),
        attrs: vec![],
        children: vec![DomNode::Text("Hi. There".into())],
        scores: Default::default(),
        metadata: [
            ("md_rd_subtree_acc_score".into(), "5.0".into()),
            ("link_density".into(), "0.0".into()),
        ]
        .into(),
    };
    assert!(should_keep_sibling(&sibling, 100.0, "", 20.0));
}

#[test]
fn test_should_keep_sibling_p_high_link_density() {
    // <p> with long text but high link_density (>= 0.25) → false.
    let long_text =
        "This is a long paragraph that exceeds eighty characters but has high link density.";
    let sibling = DomNode::Element {
        tag: "p".into(),
        attrs: vec![],
        children: vec![DomNode::Text(long_text.into())],
        scores: Default::default(),
        metadata: [
            ("md_rd_subtree_acc_score".into(), "5.0".into()),
            ("link_density".into(), "0.5".into()),
        ]
        .into(),
    };
    assert!(!should_keep_sibling(&sibling, 100.0, "", 20.0));
}

#[test]
fn test_should_keep_sibling_non_element() {
    // Non-Element node (Text) → false.
    let sibling = DomNode::Text("hello".into());
    assert!(!should_keep_sibling(&sibling, 100.0, "", 20.0));
}

#[test]
fn test_should_keep_sibling_non_p_low_score() {
    // Non-<p> element with low score and no class bonus → false.
    let sibling = DomNode::Element {
        tag: "span".into(),
        attrs: vec![],
        children: vec![],
        scores: Default::default(),
        metadata: [("md_rd_subtree_acc_score".into(), "5.0".into())].into(),
    };
    assert!(!should_keep_sibling(&sibling, 100.0, "", 20.0));
}

#[test]
fn test_should_keep_sibling_p_no_link_density() {
    // <p> without link_density metadata defaults to 0.0, long text → true.
    let long_text = "This is a long paragraph that exceeds eighty characters in total length without any link density metadata so the default kicks in and it should be kept.";
    let sibling = DomNode::Element {
        tag: "p".into(),
        attrs: vec![],
        children: vec![DomNode::Text(long_text.into())],
        scores: Default::default(),
        // NOTE: no link_density metadata
        metadata: [("md_rd_subtree_acc_score".into(), "5.0".into())].into(),
    };
    assert!(should_keep_sibling(&sibling, 100.0, "", 20.0));
}
// ── pass_prune_no_candidate ─────────────────────────────────────────────

#[test]
fn test_prune_zero_score() {
    // Scored tag (p) with md_rd_subtree_acc_score = 0.0 should be removed.
    let mut parent = DomNode::Element {
        tag: "div".into(),
        attrs: vec![],
        children: vec![DomNode::Element {
            tag: "p".into(),
            attrs: vec![],
            children: vec![],
            scores: Default::default(),
            metadata: [("md_rd_subtree_acc_score".into(), "0.0".into())].into(),
        }],
        scores: Default::default(),
        metadata: Default::default(),
    };
    pass_prune_no_candidate(&mut parent);
    if let DomNode::Element { children, .. } = &parent {
        assert!(
            children.is_empty(),
            "zero-score scored tag should be removed"
        );
    } else {
        panic!("parent should remain Element");
    }
}

#[test]
#[should_panic(expected = "pass_prune_no_candidate: node missing md_rd_subtree_acc_score")]
fn test_prune_missing_score() {
    // Element with missing md_rd_subtree_acc_score should panic
    // (pipeline ordering bug: scoring must run before extraction).
    let mut parent = DomNode::Element {
        tag: "div".into(),
        attrs: vec![],
        children: vec![DomNode::Element {
            tag: "article".into(),
            attrs: vec![],
            children: vec![],
            scores: Default::default(),
            metadata: Default::default(), // no md_rd_subtree_acc_score
        }],
        scores: Default::default(),
        metadata: Default::default(),
    };
    pass_prune_no_candidate(&mut parent);
}

#[test]
#[should_panic(expected = "pass_prune_no_candidate: unparsable md_rd_subtree_acc_score")]
fn test_prune_nan_score() {
    // Element with NaN md_rd_subtree_acc_score should panic
    // (scoring bug: meta_parse_f64 should have rejected this).
    let mut parent = DomNode::Element {
        tag: "div".into(),
        attrs: vec![],
        children: vec![DomNode::Element {
            tag: "article".into(),
            attrs: vec![],
            children: vec![],
            scores: Default::default(),
            metadata: [("md_rd_subtree_acc_score".into(), "NaN".into())].into(),
        }],
        scores: Default::default(),
        metadata: Default::default(),
    };
    pass_prune_no_candidate(&mut parent);
}

#[test]
fn test_prune_positive_score() {
    // Element with positive md_rd_subtree_acc_score should be kept.
    let mut parent = DomNode::Element {
        tag: "div".into(),
        attrs: vec![],
        children: vec![DomNode::Element {
            tag: "article".into(),
            attrs: vec![],
            children: vec![],
            scores: Default::default(),
            metadata: [("md_rd_subtree_acc_score".into(), "42.5".into())].into(),
        }],
        scores: Default::default(),
        metadata: Default::default(),
    };
    pass_prune_no_candidate(&mut parent);
    if let DomNode::Element { children, .. } = &parent {
        assert_eq!(children.len(), 1, "positive-score element should be kept");
        if let DomNode::Element { tag, .. } = &children[0] {
            assert_eq!(tag, "article", "article should remain");
        } else {
            panic!("child should be Element");
        }
    } else {
        panic!("parent should remain Element");
    }
}

#[test]
fn test_prune_non_element() {
    // Non-Element nodes (Text, Comment) should pass through unchanged.
    let mut parent = DomNode::Element {
        tag: "div".into(),
        attrs: vec![],
        children: vec![
            DomNode::Text("hello".into()),
            DomNode::Comment("comment".into()),
        ],
        scores: Default::default(),
        metadata: Default::default(),
    };
    pass_prune_no_candidate(&mut parent);
    if let DomNode::Element { children, .. } = &parent {
        assert_eq!(children.len(), 2, "non-Element nodes should be preserved");
    } else {
        panic!("parent should remain Element");
    }
}

#[test]
fn test_prune_mixed_siblings() {
    // Mixed siblings: some with zero score (scored tags), some with positive score.
    let mut parent = DomNode::Element {
        tag: "div".into(),
        attrs: vec![],
        children: vec![
            DomNode::Element {
                tag: "p".into(), // scored tag — removed on zero score
                attrs: vec![],
                children: vec![],
                scores: Default::default(),
                metadata: [("md_rd_subtree_acc_score".into(), "0.0".into())].into(),
            },
            DomNode::Element {
                tag: "positive".into(),
                attrs: vec![],
                children: vec![],
                scores: Default::default(),
                metadata: [("md_rd_subtree_acc_score".into(), "100.0".into())].into(),
            },
            DomNode::Element {
                tag: "p".into(), // scored tag — removed on zero score
                attrs: vec![],
                children: vec![],
                scores: Default::default(),
                metadata: [("md_rd_subtree_acc_score".into(), "0.0".into())].into(),
            },
            DomNode::Text("text node".into()),
        ],
        scores: Default::default(),
        metadata: Default::default(),
    };
    pass_prune_no_candidate(&mut parent);
    if let DomNode::Element { children, .. } = &parent {
        assert_eq!(
            children.len(),
            2,
            "should keep positive-score element + text node"
        );
        // Verify the positive-score element survived
        let has_positive = children
            .iter()
            .any(|c| matches!(c, DomNode::Element { tag, .. } if tag == "positive"));
        assert!(has_positive, "positive-score element should be kept");
        // Verify text node survived
        let has_text = children
            .iter()
            .any(|c| matches!(c, DomNode::Text(t) if t == "text node"));
        assert!(has_text, "text node should be preserved");
    } else {
        panic!("parent should remain Element");
    }
}

#[test]
fn test_prune_zero_score_non_scored_tag() {
    // Non-scored tag (span) with score 0.0 should be preserved (Bug A fix).
    let mut parent = DomNode::Element {
        tag: "div".into(),
        attrs: vec![],
        children: vec![DomNode::Element {
            tag: "span".into(),
            attrs: vec![],
            children: vec![DomNode::Text("content".into())],
            scores: Default::default(),
            metadata: [("md_rd_subtree_acc_score".into(), "0.0".into())].into(),
        }],
        scores: Default::default(),
        metadata: Default::default(),
    };
    pass_prune_no_candidate(&mut parent);
    if let DomNode::Element { children, .. } = &parent {
        assert_eq!(children.len(), 1, "non-scored tag should be preserved");
        if let DomNode::Element { tag, .. } = &children[0] {
            assert_eq!(tag, "span", "span should remain");
        } else {
            panic!("child should be Element");
        }
    } else {
        panic!("parent should remain Element");
    }
}

#[test]
fn test_prune_zero_score_anchor() {
    // Anchor element with score 0.0 should be preserved (Bug A fix).
    let mut parent = DomNode::Element {
        tag: "div".into(),
        attrs: vec![],
        children: vec![DomNode::Element {
            tag: "a".into(),
            attrs: vec![],
            children: vec![DomNode::Text("link".into())],
            scores: Default::default(),
            metadata: [("md_rd_subtree_acc_score".into(), "0.0".into())].into(),
        }],
        scores: Default::default(),
        metadata: Default::default(),
    };
    pass_prune_no_candidate(&mut parent);
    if let DomNode::Element { children, .. } = &parent {
        assert_eq!(children.len(), 1, "anchor should be preserved");
        if let DomNode::Element { tag, .. } = &children[0] {
            assert_eq!(tag, "a", "anchor should remain");
        } else {
            panic!("child should be Element");
        }
    } else {
        panic!("parent should remain Element");
    }
}

#[test]
fn test_prune_zero_score_div() {
    // Div element with score 0.0 should be preserved (Bug A fix — most common structural tag).
    let mut parent = DomNode::Element {
        tag: "div".into(),
        attrs: vec![],
        children: vec![DomNode::Element {
            tag: "div".into(),
            attrs: [("class".into(), "content".into())].into(),
            children: vec![DomNode::Text("content".into())],
            scores: Default::default(),
            metadata: [("md_rd_subtree_acc_score".into(), "0.0".into())].into(),
        }],
        scores: Default::default(),
        metadata: Default::default(),
    };
    pass_prune_no_candidate(&mut parent);
    if let DomNode::Element { children, .. } = &parent {
        assert_eq!(children.len(), 1, "div should be preserved");
        if let DomNode::Element { tag, .. } = &children[0] {
            assert_eq!(tag, "div", "div should remain");
        } else {
            panic!("child should be Element");
        }
    } else {
        panic!("parent should remain Element");
    }
}

#[test]
fn test_prune_data_table_skip() {
    // Elements inside a data table should survive pass_prune_no_candidate.
    // The table has is_data_table=true, so SkipChildren protects its children.
    let mut root = DomNode::Element {
        tag: "div".into(),
        attrs: vec![],
        children: vec![DomNode::Element {
            tag: "table".into(),
            attrs: vec![],
            children: vec![DomNode::Element {
                tag: "td".into(),
                attrs: vec![],
                children: vec![DomNode::Text("data".into())],
                scores: Default::default(),
                metadata: [("md_rd_subtree_acc_score".into(), "0.0".into())].into(),
            }],
            scores: Default::default(),
            metadata: [("is_data_table".into(), "true".into())].into(),
        }],
        scores: Default::default(),
        metadata: Default::default(),
    };
    pass_prune_no_candidate(&mut root);

    // The <td> with score 0.0 should survive inside the data table
    fn find_tag(node: &DomNode, tag: &str) -> bool {
        match node {
            DomNode::Element { tag: t, .. } if t == tag => true,
            DomNode::Element { children, .. } => children.iter().any(|c| find_tag(c, tag)),
            _ => false,
        }
    }
    assert!(
        find_tag(&root, "td"),
        "<td> inside data table should survive pass_prune_no_candidate"
    );
}
// ── pass_splice_cutoff ──────────────────────────────────────────

#[test]
fn test_splice_cutoff_low_score_spliced() {
    // Parent (child of root) has score=10, best child score=100.
    // 10 < 100/3.0 ≈ 33.33 → ReplaceWithChildren.
    let mut root = DomNode::Element {
        tag: "div".into(),
        attrs: vec![],
        children: vec![DomNode::Element {
            tag: "article".into(),
            attrs: vec![],
            children: vec![DomNode::Element {
                tag: "p".into(),
                attrs: vec![],
                children: vec![DomNode::Text("content".into())],
                scores: Default::default(),
                metadata: [("md_rd_subtree_acc_score".into(), "100.0".into())].into(),
            }],
            scores: Default::default(),
            metadata: [("md_rd_subtree_acc_score".into(), "10.0".into())].into(),
        }],
        scores: Default::default(),
        metadata: [("md_rd_subtree_acc_score".into(), "0.0".into())].into(),
    };
    pass_splice_cutoff(&mut root);
    if let DomNode::Element { children, .. } = &root {
        // article wrapper should be spliced away, leaving p
        assert_eq!(children.len(), 1, "article should be spliced, leaving p");
        if let DomNode::Element {
            tag,
            children: inner,
            ..
        } = &children[0]
        {
            assert_eq!(tag, "p", "p should remain after article is spliced");
            assert_eq!(inner.len(), 1, "p should keep its text child");
            assert!(matches!(&inner[0], DomNode::Text(t) if t == "content"));
        } else {
            panic!("children[0] should be Element");
        }
    } else {
        panic!("root should remain Element");
    }
}

#[test]
fn test_splice_cutoff_high_score_not_spliced() {
    // Parent (child of root) has score=40, best child score=100.
    // 40 >= 100/3.0 ≈ 33.33 → not spliced.
    let mut root = DomNode::Element {
        tag: "div".into(),
        attrs: vec![],
        children: vec![DomNode::Element {
            tag: "article".into(),
            attrs: vec![],
            children: vec![DomNode::Element {
                tag: "p".into(),
                attrs: vec![],
                children: vec![DomNode::Text("content".into())],
                scores: Default::default(),
                metadata: [("md_rd_subtree_acc_score".into(), "100.0".into())].into(),
            }],
            scores: Default::default(),
            metadata: [("md_rd_subtree_acc_score".into(), "40.0".into())].into(),
        }],
        scores: Default::default(),
        metadata: [("md_rd_subtree_acc_score".into(), "0.0".into())].into(),
    };
    pass_splice_cutoff(&mut root);
    if let DomNode::Element { children, .. } = &root {
        // article wrapper should remain (score 40 >= 100/3.0)
        assert_eq!(children.len(), 1, "article should NOT be spliced");
        if let DomNode::Element {
            tag,
            children: inner,
            ..
        } = &children[0]
        {
            assert_eq!(tag, "article", "article should remain");
            assert_eq!(inner.len(), 1, "article should keep its p child");
            if let DomNode::Element { tag: pt, .. } = &inner[0] {
                assert_eq!(pt, "p", "p should remain inside article");
            } else {
                panic!("inner[0] should be Element");
            }
        } else {
            panic!("children[0] should be Element");
        }
    } else {
        panic!("root should remain Element");
    }
}

#[test]
fn test_splice_cutoff_body_never_spliced() {
    // Body element with low score → is_body_or_html returns true → not spliced.
    let mut root = DomNode::Element {
        tag: "div".into(),
        attrs: vec![],
        children: vec![DomNode::Element {
            tag: "body".into(),
            attrs: vec![],
            children: vec![DomNode::Element {
                tag: "p".into(),
                attrs: vec![],
                children: vec![DomNode::Text("content".into())],
                scores: Default::default(),
                metadata: [("md_rd_subtree_acc_score".into(), "100.0".into())].into(),
            }],
            scores: Default::default(),
            metadata: [("md_rd_subtree_acc_score".into(), "10.0".into())].into(),
        }],
        scores: Default::default(),
        metadata: [("md_rd_subtree_acc_score".into(), "0.0".into())].into(),
    };
    pass_splice_cutoff(&mut root);
    if let DomNode::Element { children, .. } = &root {
        // body should NOT be spliced despite low score
        assert_eq!(children.len(), 1, "body should NOT be spliced");
        if let DomNode::Element {
            tag,
            children: inner,
            ..
        } = &children[0]
        {
            assert_eq!(tag, "body", "body should remain");
            // p is still inside body
            assert_eq!(inner.len(), 1, "body should keep its p child");
        } else {
            panic!("children[0] should be Element");
        }
    } else {
        panic!("root should remain Element");
    }
}

#[test]
fn test_splice_cutoff_html_never_spliced() {
    // Html element with low score → is_body_or_html returns true → not spliced.
    let mut root = DomNode::Element {
        tag: "div".into(),
        attrs: vec![],
        children: vec![DomNode::Element {
            tag: "html".into(),
            attrs: vec![],
            children: vec![DomNode::Element {
                tag: "p".into(),
                attrs: vec![],
                children: vec![DomNode::Text("content".into())],
                scores: Default::default(),
                metadata: [("md_rd_subtree_acc_score".into(), "100.0".into())].into(),
            }],
            scores: Default::default(),
            metadata: [("md_rd_subtree_acc_score".into(), "10.0".into())].into(),
        }],
        scores: Default::default(),
        metadata: [("md_rd_subtree_acc_score".into(), "0.0".into())].into(),
    };
    pass_splice_cutoff(&mut root);
    if let DomNode::Element { children, .. } = &root {
        // html should NOT be spliced despite low score
        assert_eq!(children.len(), 1, "html should NOT be spliced");
        if let DomNode::Element { tag, .. } = &children[0] {
            assert_eq!(tag, "html", "html should remain");
        } else {
            panic!("children[0] should be Element");
        }
    } else {
        panic!("root should remain Element");
    }
}

#[test]
fn test_splice_cutoff_no_children_not_spliced() {
    // Element with no Element children → best_child_score=0.0 → cutoff not triggered.
    let mut root = DomNode::Element {
        tag: "div".into(),
        attrs: vec![],
        children: vec![DomNode::Element {
            tag: "span".into(),
            attrs: vec![],
            children: vec![],
            scores: Default::default(),
            metadata: [("md_rd_subtree_acc_score".into(), "10.0".into())].into(),
        }],
        scores: Default::default(),
        metadata: [("md_rd_subtree_acc_score".into(), "0.0".into())].into(),
    };
    pass_splice_cutoff(&mut root);
    if let DomNode::Element { children, .. } = &root {
        // span has no children → best_child_score=0.0 → no cutoff
        assert_eq!(
            children.len(),
            1,
            "span with no children should NOT be spliced"
        );
        if let DomNode::Element { tag, .. } = &children[0] {
            assert_eq!(tag, "span", "span should remain");
        } else {
            panic!("children[0] should be Element");
        }
    } else {
        panic!("root should remain Element");
    }
}

#[test]
fn test_splice_cutoff_all_children_zero_score() {
    // All children have score 0.0 → best_child_score=0.0 → first guard fails → not spliced.
    let mut root = DomNode::Element {
        tag: "div".into(),
        attrs: vec![],
        children: vec![DomNode::Element {
            tag: "span".into(),
            attrs: vec![],
            children: vec![DomNode::Text("content".into())],
            scores: Default::default(),
            metadata: [("md_rd_subtree_acc_score".into(), "0.0".into())].into(),
        }],
        scores: Default::default(),
        metadata: [("md_rd_subtree_acc_score".into(), "5.0".into())].into(),
    };
    pass_splice_cutoff(&mut root);
    if let DomNode::Element { children, .. } = &root {
        // span has score 0.0 → best_child_score=0.0 → no cutoff
        assert_eq!(children.len(), 1, "zero-score child should NOT be spliced");
        if let DomNode::Element { tag, .. } = &children[0] {
            assert_eq!(tag, "span", "span should remain");
        } else {
            panic!("children[0] should be Element");
        }
    } else {
        panic!("root should remain Element");
    }
}

#[test]
fn test_splice_cutoff_single_child_not_spliced() {
    // Single child path: parent (score=50) > child (score=100).
    // 50 >= 100/3.0 ≈ 33.33 → not spliced.
    let mut root = DomNode::Element {
        tag: "div".into(),
        attrs: vec![],
        children: vec![DomNode::Element {
            tag: "article".into(),
            attrs: vec![],
            children: vec![DomNode::Element {
                tag: "p".into(),
                attrs: vec![],
                children: vec![],
                scores: Default::default(),
                metadata: [("md_rd_subtree_acc_score".into(), "100.0".into())].into(),
            }],
            scores: Default::default(),
            metadata: [("md_rd_subtree_acc_score".into(), "50.0".into())].into(),
        }],
        scores: Default::default(),
        metadata: [("md_rd_subtree_acc_score".into(), "0.0".into())].into(),
    };
    pass_splice_cutoff(&mut root);
    if let DomNode::Element { children, .. } = &root {
        assert_eq!(
            children.len(),
            1,
            "article with 50 >= 100/3.0 should NOT be spliced"
        );
        if let DomNode::Element { tag, .. } = &children[0] {
            assert_eq!(tag, "article", "article should remain");
        } else {
            panic!("children[0] should be Element");
        }
    } else {
        panic!("root should remain Element");
    }
}

#[test]
fn test_splice_cutoff_chain_thin_wrappers() {
    // Chain: grandparent (score=10) > parent (score=10) > child (score=100) > text
    // Both grandparent and parent have scores < 100/3.0 → both spliced.
    // Only child (section with text) should remain as child of root.
    let mut root = DomNode::Element {
        tag: "div".into(),
        attrs: vec![],
        children: vec![DomNode::Element {
            tag: "article".into(),
            attrs: vec![],
            children: vec![DomNode::Element {
                tag: "section".into(),
                attrs: vec![],
                children: vec![DomNode::Element {
                    tag: "p".into(),
                    attrs: vec![],
                    children: vec![DomNode::Text("final content".into())],
                    scores: Default::default(),
                    metadata: [("md_rd_subtree_acc_score".into(), "100.0".into())].into(),
                }],
                scores: Default::default(),
                metadata: [("md_rd_subtree_acc_score".into(), "10.0".into())].into(),
            }],
            scores: Default::default(),
            metadata: [("md_rd_subtree_acc_score".into(), "10.0".into())].into(),
        }],
        scores: Default::default(),
        metadata: [("md_rd_subtree_acc_score".into(), "0.0".into())].into(),
    };
    pass_splice_cutoff(&mut root);
    if let DomNode::Element { children, .. } = &root {
        // article (score=10, child section score=100) → 10 < 100/3.0 → ReplaceWithChildren
        // article replaced by section.
        // section (score=10, child p score=100) → 10 < 100/3.0 → ReplaceWithChildren
        // section replaced by p.
        // Result: root > p > text
        assert_eq!(
            children.len(),
            1,
            "both wrappers should be spliced, leaving p"
        );
        if let DomNode::Element {
            tag,
            children: inner,
            ..
        } = &children[0]
        {
            assert_eq!(tag, "p", "p should remain after both wrappers are spliced");
            assert_eq!(inner.len(), 1, "p should keep its text child");
            assert!(matches!(&inner[0], DomNode::Text(t) if t == "final content"));
        } else {
            panic!("children[0] should be Element");
        }
    } else {
        panic!("root should remain Element");
    }
}

// ── pass_keep_alt_cluster ─────────────────────────────────────────

// ── pass_keep_alt_cluster ─────────────────────────────────────────

#[test]
fn test_alt_cluster_three_qualifying() {
    // 3+ qualifying children → alt cluster detected, non-qualifying removed.
    // Root > section (cluster candidate) > [article, div, p, span]
    let mut root = DomNode::Element {
        tag: "div".into(),
        attrs: vec![],
        children: vec![DomNode::Element {
            tag: "section".into(),
            attrs: vec![],
            children: vec![
                DomNode::Element {
                    tag: "article".into(),
                    attrs: vec![],
                    children: vec![],
                    scores: Default::default(),
                    metadata: [("md_rd_subtree_acc_score".into(), "100.0".into())].into(),
                },
                DomNode::Element {
                    tag: "div".into(),
                    attrs: vec![],
                    children: vec![],
                    scores: Default::default(),
                    metadata: [("md_rd_subtree_acc_score".into(), "90.0".into())].into(),
                },
                DomNode::Element {
                    tag: "p".into(),
                    attrs: vec![],
                    children: vec![],
                    scores: Default::default(),
                    metadata: [("md_rd_subtree_acc_score".into(), "85.0".into())].into(),
                },
                DomNode::Element {
                    tag: "span".into(),
                    attrs: vec![],
                    children: vec![],
                    scores: Default::default(),
                    metadata: [("md_rd_subtree_acc_score".into(), "10.0".into())].into(),
                },
            ],
            scores: Default::default(),
            metadata: [("md_rd_subtree_acc_score".into(), "50.0".into())].into(),
        }],
        scores: Default::default(),
        metadata: [("md_rd_subtree_acc_score".into(), "10.0".into())].into(),
    };
    // top_child_score = 100.0, alt_threshold = 100.0 * 0.75 - 1e-9 ≈ 75.0
    // article(100), div(90), p(85) qualify; span(10) does not
    pass_keep_alt_cluster(&mut root);
    if let DomNode::Element { children, .. } = &root {
        assert_eq!(
            children.len(),
            1,
            "root should still have 1 child (section)"
        );
        if let DomNode::Element {
            tag,
            children: inner,
            ..
        } = &children[0]
        {
            assert_eq!(tag, "section", "section should remain");
            assert_eq!(
                inner.len(),
                3,
                "non-qualifying span should be removed from section"
            );
            let tags: Vec<&str> = inner
                .iter()
                .filter_map(|c| match c {
                    DomNode::Element { tag, .. } => Some(tag.as_str()),
                    _ => None,
                })
                .collect();
            assert!(tags.contains(&"article"), "article should be kept");
            assert!(tags.contains(&"div"), "div should be kept");
            assert!(tags.contains(&"p"), "p should be kept");
            assert!(!tags.contains(&"span"), "span should be removed");
        } else {
            panic!("root child should be Element");
        }
    } else {
        panic!("root should remain Element");
    }
}

#[test]
fn test_alt_cluster_two_qualifying() {
    // 2 qualifying children → no alt cluster, all kept.
    let mut root = DomNode::Element {
        tag: "div".into(),
        attrs: vec![],
        children: vec![DomNode::Element {
            tag: "section".into(),
            attrs: vec![],
            children: vec![
                DomNode::Element {
                    tag: "article".into(),
                    attrs: vec![],
                    children: vec![],
                    scores: Default::default(),
                    metadata: [("md_rd_subtree_acc_score".into(), "100.0".into())].into(),
                },
                DomNode::Element {
                    tag: "div".into(),
                    attrs: vec![],
                    children: vec![],
                    scores: Default::default(),
                    metadata: [("md_rd_subtree_acc_score".into(), "80.0".into())].into(),
                },
                DomNode::Element {
                    tag: "span".into(),
                    attrs: vec![],
                    children: vec![],
                    scores: Default::default(),
                    metadata: [("md_rd_subtree_acc_score".into(), "10.0".into())].into(),
                },
            ],
            scores: Default::default(),
            metadata: [("md_rd_subtree_acc_score".into(), "40.0".into())].into(),
        }],
        scores: Default::default(),
        metadata: [("md_rd_subtree_acc_score".into(), "10.0".into())].into(),
    };
    // alt_threshold = 100.0 * 0.75 - 1e-9 ≈ 75.0
    // Only article(100) and div(80) qualify → 2 < 3 → no alt cluster
    pass_keep_alt_cluster(&mut root);
    if let DomNode::Element { children, .. } = &root {
        assert_eq!(
            children.len(),
            1,
            "root should still have 1 child (section)"
        );
        if let DomNode::Element {
            children: inner, ..
        } = &children[0]
        {
            assert_eq!(
                inner.len(),
                3,
                "all children should remain (no alt cluster)"
            );
        } else {
            panic!("root child should be Element");
        }
    } else {
        panic!("root should remain Element");
    }
}

#[test]
fn test_alt_cluster_body_html_excluded() {
    // body/html children excluded from qualifying count.
    let mut root = DomNode::Element {
        tag: "div".into(),
        attrs: vec![],
        children: vec![DomNode::Element {
            tag: "section".into(),
            attrs: vec![],
            children: vec![
                DomNode::Element {
                    tag: "body".into(),
                    attrs: vec![],
                    children: vec![],
                    scores: Default::default(),
                    metadata: [("md_rd_subtree_acc_score".into(), "100.0".into())].into(),
                },
                DomNode::Element {
                    tag: "html".into(),
                    attrs: vec![],
                    children: vec![],
                    scores: Default::default(),
                    metadata: [("md_rd_subtree_acc_score".into(), "95.0".into())].into(),
                },
                DomNode::Element {
                    tag: "div".into(),
                    attrs: vec![],
                    children: vec![],
                    scores: Default::default(),
                    metadata: [("md_rd_subtree_acc_score".into(), "90.0".into())].into(),
                },
            ],
            scores: Default::default(),
            metadata: [("md_rd_subtree_acc_score".into(), "40.0".into())].into(),
        }],
        scores: Default::default(),
        metadata: [("md_rd_subtree_acc_score".into(), "10.0".into())].into(),
    };
    // top non-body/html child score = 90.0, alt_threshold = 90.0 * 0.75 - 1e-9 ≈ 67.5
    // Only div qualifies (body/html excluded) → 1 < 3 → no alt cluster
    pass_keep_alt_cluster(&mut root);
    if let DomNode::Element { children, .. } = &root {
        assert_eq!(
            children.len(),
            1,
            "root should still have 1 child (section)"
        );
        if let DomNode::Element {
            children: inner, ..
        } = &children[0]
        {
            assert_eq!(
                inner.len(),
                3,
                "all children should remain (body/html excluded from count)"
            );
        } else {
            panic!("root child should be Element");
        }
    } else {
        panic!("root should remain Element");
    }
}

#[test]
fn test_alt_cluster_no_qualifying() {
    // No qualifying children → no alt cluster.
    let mut root = DomNode::Element {
        tag: "div".into(),
        attrs: vec![],
        children: vec![DomNode::Element {
            tag: "section".into(),
            attrs: vec![],
            children: vec![
                DomNode::Element {
                    tag: "article".into(),
                    attrs: vec![],
                    children: vec![],
                    scores: Default::default(),
                    metadata: [("md_rd_subtree_acc_score".into(), "10.0".into())].into(),
                },
                DomNode::Element {
                    tag: "span".into(),
                    attrs: vec![],
                    children: vec![],
                    scores: Default::default(),
                    metadata: [("md_rd_subtree_acc_score".into(), "5.0".into())].into(),
                },
            ],
            scores: Default::default(),
            metadata: [("md_rd_subtree_acc_score".into(), "40.0".into())].into(),
        }],
        scores: Default::default(),
        metadata: [("md_rd_subtree_acc_score".into(), "10.0".into())].into(),
    };
    // top_child_score = 10.0, alt_threshold = 10.0 * 0.75 - 1e-9 ≈ 7.5
    // Only article(10.0) qualifies → 1 < 3 → no alt cluster
    pass_keep_alt_cluster(&mut root);
    if let DomNode::Element { children, .. } = &root {
        assert_eq!(
            children.len(),
            1,
            "root should still have 1 child (section)"
        );
        if let DomNode::Element {
            children: inner, ..
        } = &children[0]
        {
            assert_eq!(
                inner.len(),
                2,
                "all children should remain (no alt cluster)"
            );
        } else {
            panic!("root child should be Element");
        }
    } else {
        panic!("root should remain Element");
    }
}

#[test]
fn test_alt_cluster_mixed_qualifying() {
    // Alt cluster with mixed qualifying/non-qualifying, plus non-Element children.
    let mut root = DomNode::Element {
        tag: "div".into(),
        attrs: vec![],
        children: vec![DomNode::Element {
            tag: "section".into(),
            attrs: vec![],
            children: vec![
                DomNode::Element {
                    tag: "article".into(),
                    attrs: vec![],
                    children: vec![],
                    scores: Default::default(),
                    metadata: [("md_rd_subtree_acc_score".into(), "100.0".into())].into(),
                },
                DomNode::Element {
                    tag: "div".into(),
                    attrs: vec![],
                    children: vec![],
                    scores: Default::default(),
                    metadata: [("md_rd_subtree_acc_score".into(), "90.0".into())].into(),
                },
                DomNode::Element {
                    tag: "p".into(),
                    attrs: vec![],
                    children: vec![],
                    scores: Default::default(),
                    metadata: [("md_rd_subtree_acc_score".into(), "85.0".into())].into(),
                },
                DomNode::Element {
                    tag: "span".into(),
                    attrs: vec![],
                    children: vec![],
                    scores: Default::default(),
                    metadata: [("md_rd_subtree_acc_score".into(), "10.0".into())].into(),
                },
                DomNode::Text("some text".into()),
            ],
            scores: Default::default(),
            metadata: [("md_rd_subtree_acc_score".into(), "50.0".into())].into(),
        }],
        scores: Default::default(),
        metadata: [("md_rd_subtree_acc_score".into(), "10.0".into())].into(),
    };
    // alt_threshold = 100.0 * 0.75 - 1e-9 ≈ 75.0
    // article(100), div(90), p(85) qualify; span(10) does not; text node preserved
    pass_keep_alt_cluster(&mut root);
    if let DomNode::Element { children, .. } = &root {
        assert_eq!(
            children.len(),
            1,
            "root should still have 1 child (section)"
        );
        if let DomNode::Element {
            tag,
            children: inner,
            ..
        } = &children[0]
        {
            assert_eq!(tag, "section", "section should remain");
            assert_eq!(
                inner.len(),
                4,
                "3 qualifying elements + 1 text node should remain"
            );
            let tags: Vec<&str> = inner
                .iter()
                .filter_map(|c| match c {
                    DomNode::Element { tag, .. } => Some(tag.as_str()),
                    _ => None,
                })
                .collect();
            assert!(tags.contains(&"article"), "article should be kept");
            assert!(tags.contains(&"div"), "div should be kept");
            assert!(tags.contains(&"p"), "p should be kept");
            assert!(!tags.contains(&"span"), "span should be removed");
            // Verify text node survived
            let has_text = inner
                .iter()
                .any(|c| matches!(c, DomNode::Text(t) if t == "some text"));
            assert!(has_text, "text node should be preserved");
        } else {
            panic!("root child should be Element");
        }
    } else {
        panic!("root should remain Element");
    }
}

// ── pass_promote_content_child ──────────────────────────────────────────

#[test]
fn test_promote_content_best_child_promoted() {
    // Multiple children, best non-body/html child promoted, others removed.
    // Structure: root > parent(section) > [article(100.0), div(50.0), span(10.0)]
    // walk_pre_mut visits 'parent' which has 3 children → best (article) promoted.
    let mut root = DomNode::Element {
        tag: "div".into(),
        attrs: vec![],
        children: vec![DomNode::Element {
            tag: "parent".into(),
            attrs: vec![],
            children: vec![
                DomNode::Element {
                    tag: "article".into(),
                    attrs: vec![],
                    children: vec![DomNode::Text("content".into())],
                    scores: Default::default(),
                    metadata: [("md_rd_subtree_acc_score".into(), "100.0".into())].into(),
                },
                DomNode::Element {
                    tag: "div".into(),
                    attrs: vec![],
                    children: vec![],
                    scores: Default::default(),
                    metadata: [("md_rd_subtree_acc_score".into(), "50.0".into())].into(),
                },
                DomNode::Element {
                    tag: "span".into(),
                    attrs: vec![],
                    children: vec![],
                    scores: Default::default(),
                    metadata: [("md_rd_subtree_acc_score".into(), "10.0".into())].into(),
                },
            ],
            scores: Default::default(),
            metadata: Default::default(),
        }],
        scores: Default::default(),
        metadata: Default::default(),
    };
    pass_promote_content_child(&mut root);
    if let DomNode::Element { children, .. } = &root {
        assert_eq!(children.len(), 1, "root should have 1 child (parent)");
        if let DomNode::Element {
            tag,
            children: inner,
            ..
        } = &children[0]
        {
            assert_eq!(tag, "parent", "parent should remain");
            assert_eq!(inner.len(), 1, "only best child should remain in parent");
            if let DomNode::Element { tag: ct, .. } = &inner[0] {
                assert_eq!(ct, "article", "article with highest score should be kept");
            } else {
                panic!("child should be Element");
            }
        } else {
            panic!("root child should be Element");
        }
    } else {
        panic!("root should remain Element");
    }
}

#[test]
fn test_promote_content_single_child_unchanged() {
    // Only one child → unchanged.
    // Structure: root > parent(section) > [article(100.0)]
    let mut root = DomNode::Element {
        tag: "div".into(),
        attrs: vec![],
        children: vec![DomNode::Element {
            tag: "parent".into(),
            attrs: vec![],
            children: vec![DomNode::Element {
                tag: "article".into(),
                attrs: vec![],
                children: vec![],
                scores: Default::default(),
                metadata: [("md_rd_subtree_acc_score".into(), "100.0".into())].into(),
            }],
            scores: Default::default(),
            metadata: Default::default(),
        }],
        scores: Default::default(),
        metadata: Default::default(),
    };
    pass_promote_content_child(&mut root);
    if let DomNode::Element { children, .. } = &root {
        assert_eq!(children.len(), 1, "root should have 1 child (parent)");
        if let DomNode::Element {
            tag,
            children: inner,
            ..
        } = &children[0]
        {
            assert_eq!(tag, "parent", "parent should remain");
            assert_eq!(
                inner.len(),
                1,
                "single child in parent should remain unchanged"
            );
        } else {
            panic!("root child should be Element");
        }
    } else {
        panic!("root should remain Element");
    }
}

#[test]
fn test_promote_content_body_html_only_cleared() {
    // Body/html as only Element children → children cleared.
    // Structure: root > parent(section) > [body(200.0), html(300.0)]
    let mut root = DomNode::Element {
        tag: "div".into(),
        attrs: vec![],
        children: vec![DomNode::Element {
            tag: "parent".into(),
            attrs: vec![],
            children: vec![
                DomNode::Element {
                    tag: "body".into(),
                    attrs: vec![],
                    children: vec![],
                    scores: Default::default(),
                    metadata: [("md_rd_subtree_acc_score".into(), "200.0".into())].into(),
                },
                DomNode::Element {
                    tag: "html".into(),
                    attrs: vec![],
                    children: vec![],
                    scores: Default::default(),
                    metadata: [("md_rd_subtree_acc_score".into(), "300.0".into())].into(),
                },
            ],
            scores: Default::default(),
            metadata: Default::default(),
        }],
        scores: Default::default(),
        metadata: Default::default(),
    };
    pass_promote_content_child(&mut root);
    if let DomNode::Element { children, .. } = &root {
        assert_eq!(children.len(), 1, "root should have 1 child (parent)");
        if let DomNode::Element {
            tag,
            children: inner,
            ..
        } = &children[0]
        {
            assert_eq!(tag, "parent", "parent should remain");
            assert!(
                inner.is_empty(),
                "body/html-only children should be cleared from parent"
            );
        } else {
            panic!("root child should be Element");
        }
    } else {
        panic!("root should remain Element");
    }
}

#[test]
fn test_promote_content_all_zero_score_cleared() {
    // All children score 0.0 → children cleared.
    // Structure: root > parent(section) > [article(0.0), div(0.0)]
    let mut root = DomNode::Element {
        tag: "div".into(),
        attrs: vec![],
        children: vec![DomNode::Element {
            tag: "parent".into(),
            attrs: vec![],
            children: vec![
                DomNode::Element {
                    tag: "article".into(),
                    attrs: vec![],
                    children: vec![],
                    scores: Default::default(),
                    metadata: [("md_rd_subtree_acc_score".into(), "0.0".into())].into(),
                },
                DomNode::Element {
                    tag: "div".into(),
                    attrs: vec![],
                    children: vec![],
                    scores: Default::default(),
                    metadata: [("md_rd_subtree_acc_score".into(), "0.0".into())].into(),
                },
            ],
            scores: Default::default(),
            metadata: Default::default(),
        }],
        scores: Default::default(),
        metadata: Default::default(),
    };
    pass_promote_content_child(&mut root);
    if let DomNode::Element { children, .. } = &root {
        assert_eq!(children.len(), 1, "root should have 1 child (parent)");
        if let DomNode::Element {
            tag,
            children: inner,
            ..
        } = &children[0]
        {
            assert_eq!(tag, "parent", "parent should remain");
            assert!(
                inner.is_empty(),
                "all-zero-score children should be cleared from parent"
            );
        } else {
            panic!("root child should be Element");
        }
    } else {
        panic!("root should remain Element");
    }
}

#[test]
fn test_promote_content_non_element_children_graceful() {
    // Non-Element children (Text, Comment) → handled gracefully (not promoted).
    // Structure: root > parent(section) > [Text, Comment]
    let mut root = DomNode::Element {
        tag: "div".into(),
        attrs: vec![],
        children: vec![DomNode::Element {
            tag: "parent".into(),
            attrs: vec![],
            children: vec![
                DomNode::Text("some text".into()),
                DomNode::Comment("a comment".into()),
            ],
            scores: Default::default(),
            metadata: Default::default(),
        }],
        scores: Default::default(),
        metadata: Default::default(),
    };
    pass_promote_content_child(&mut root);
    if let DomNode::Element { children, .. } = &root {
        assert_eq!(children.len(), 1, "root should have 1 child (parent)");
        if let DomNode::Element {
            tag,
            children: inner,
            ..
        } = &children[0]
        {
            assert_eq!(tag, "parent", "parent should remain");
            assert!(
                inner.is_empty(),
                "only non-Element children should be cleared from parent"
            );
        } else {
            panic!("root child should be Element");
        }
    } else {
        panic!("root should remain Element");
    }
}

#[test]
fn test_promote_content_best_child_is_last() {
    // Best child is the last child → others removed correctly.
    // Structure: root > parent(section) > [span(10.0), div(50.0), article(100.0)]
    let mut root = DomNode::Element {
        tag: "div".into(),
        attrs: vec![],
        children: vec![DomNode::Element {
            tag: "parent".into(),
            attrs: vec![],
            children: vec![
                DomNode::Element {
                    tag: "span".into(),
                    attrs: vec![],
                    children: vec![],
                    scores: Default::default(),
                    metadata: [("md_rd_subtree_acc_score".into(), "10.0".into())].into(),
                },
                DomNode::Element {
                    tag: "div".into(),
                    attrs: vec![],
                    children: vec![],
                    scores: Default::default(),
                    metadata: [("md_rd_subtree_acc_score".into(), "50.0".into())].into(),
                },
                DomNode::Element {
                    tag: "article".into(),
                    attrs: vec![],
                    children: vec![DomNode::Text("content".into())],
                    scores: Default::default(),
                    metadata: [("md_rd_subtree_acc_score".into(), "100.0".into())].into(),
                },
            ],
            scores: Default::default(),
            metadata: Default::default(),
        }],
        scores: Default::default(),
        metadata: Default::default(),
    };
    pass_promote_content_child(&mut root);
    if let DomNode::Element { children, .. } = &root {
        assert_eq!(children.len(), 1, "root should have 1 child (parent)");
        if let DomNode::Element {
            tag,
            children: inner,
            ..
        } = &children[0]
        {
            assert_eq!(tag, "parent", "parent should remain");
            assert_eq!(inner.len(), 1, "only best child should remain in parent");
            if let DomNode::Element { tag: ct, .. } = &inner[0] {
                assert_eq!(
                    ct, "article",
                    "article (last child) with highest score should be kept"
                );
            } else {
                panic!("child should be Element");
            }
        } else {
            panic!("root child should be Element");
        }
    } else {
        panic!("root should remain Element");
    }
}

#[test]
fn test_promote_content_mixed_body_html_and_content() {
    // Body/html children exist alongside content children.
    // Body/html are excluded from selection, so content child wins.
    // Structure: root > parent(section) > [body(200.0), html(300.0), article(100.0)]
    let mut root = DomNode::Element {
        tag: "div".into(),
        attrs: vec![],
        children: vec![DomNode::Element {
            tag: "parent".into(),
            attrs: vec![],
            children: vec![
                DomNode::Element {
                    tag: "body".into(),
                    attrs: vec![],
                    children: vec![],
                    scores: Default::default(),
                    metadata: [("md_rd_subtree_acc_score".into(), "200.0".into())].into(),
                },
                DomNode::Element {
                    tag: "html".into(),
                    attrs: vec![],
                    children: vec![],
                    scores: Default::default(),
                    metadata: [("md_rd_subtree_acc_score".into(), "300.0".into())].into(),
                },
                DomNode::Element {
                    tag: "article".into(),
                    attrs: vec![],
                    children: vec![DomNode::Text("content".into())],
                    scores: Default::default(),
                    metadata: [("md_rd_subtree_acc_score".into(), "100.0".into())].into(),
                },
            ],
            scores: Default::default(),
            metadata: Default::default(),
        }],
        scores: Default::default(),
        metadata: Default::default(),
    };
    pass_promote_content_child(&mut root);
    if let DomNode::Element { children, .. } = &root {
        assert_eq!(children.len(), 1, "root should have 1 child (parent)");
        if let DomNode::Element {
            tag,
            children: inner,
            ..
        } = &children[0]
        {
            assert_eq!(tag, "parent", "parent should remain");
            assert_eq!(
                inner.len(),
                1,
                "only best content child should remain in parent"
            );
            if let DomNode::Element { tag: ct, .. } = &inner[0] {
                assert_eq!(ct, "article", "article should be selected over body/html");
            } else {
                panic!("child should be Element");
            }
        } else {
            panic!("root child should be Element");
        }
    } else {
        panic!("root should remain Element");
    }
}

#[test]
fn test_promote_content_non_element_root() {
    // Non-Element root (Text node) should be silently skipped.
    let mut node = DomNode::Text("hello".into());
    pass_promote_content_child(&mut node);
    assert!(
        matches!(&node, DomNode::Text(t) if t == "hello"),
        "non-Element root should be unchanged"
    );
}

#[test]
fn test_promote_content_no_children() {
    // Element with no children → unchanged.
    let mut root = DomNode::Element {
        tag: "div".into(),
        attrs: vec![],
        children: vec![],
        scores: Default::default(),
        metadata: Default::default(),
    };
    pass_promote_content_child(&mut root);
    if let DomNode::Element { children, .. } = &root {
        assert!(children.is_empty(), "empty children should remain empty");
    } else {
        panic!("root should remain Element");
    }
}
