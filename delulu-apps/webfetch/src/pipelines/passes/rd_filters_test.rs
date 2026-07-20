use super::*;
use crate::pipelines::passes::rd_analysis::rd_score_mozilla_readability;
use crate::pipelines::{parse_html, walk_pre_mut};

// ── 1. remove_style_elements ──────────────────────────────────────────

#[test]
fn test_remove_style_elements() {
    let html = "<style>.x{}</style><p>text</p>";
    let mut nodes = parse_html(html).expect("valid HTML");
    walk_pre_mut(&mut nodes, &|n| remove_style_elements(n));

    fn find_tag(node: &DomNode, tag: &str) -> bool {
        match node {
            DomNode::Element {
                tag: t, children, ..
            } if t == tag => true,
            DomNode::Element { children, .. } => children.iter().any(|c| find_tag(c, tag)),
            _ => false,
        }
    }
    assert!(find_tag(&nodes, "p"), "<p> should remain");
    assert!(!find_tag(&nodes, "style"), "<style> should be removed");
}

// ── remove_script_elements ───────────────────────────────────────────

#[test]
fn test_remove_script_elements_removes_script_tags() {
    let html = "<script>alert('xss')</script><p>text</p><script src=\"evil.js\"></script>";
    let mut nodes = parse_html(html).expect("valid HTML");
    walk_pre_mut(&mut nodes, &|n| remove_script_elements(n));

    fn find_tag(node: &DomNode, tag: &str) -> bool {
        match node {
            DomNode::Element {
                tag: t, children, ..
            } if t == tag => true,
            DomNode::Element { children, .. } => children.iter().any(|c| find_tag(c, tag)),
            _ => false,
        }
    }
    assert!(find_tag(&nodes, "p"), "<p> should remain");
    assert!(
        !find_tag(&nodes, "script"),
        "<script> tags should be removed"
    );
}

// ── 4. strip_unlikely_candidates ──────────────────────────────────────

#[test]
fn test_strip_unlikely_candidates() {
    let html = r#"<div class="sidebar">nav</div><article>content</article>"#;
    let mut nodes = parse_html(html).expect("valid HTML");
    walk_pre_mut(&mut nodes, &|n| strip_unlikely_candidates(n));

    fn find_tag(node: &DomNode, tag: &str) -> bool {
        match node {
            DomNode::Element { tag: t, .. } if t == tag => true,
            DomNode::Element { children, .. } => children.iter().any(|c| find_tag(c, tag)),
            _ => false,
        }
    }

    assert!(
        !find_tag(&nodes, "div"),
        "<div class=\"sidebar\"> should be stripped"
    );
    assert!(find_tag(&nodes, "article"), "<article> should remain");
}

#[test]
fn test_strip_unlikely_candidates_keeps_likely_nested() {
    // An element with an unlikely class but containing <article> should be kept.
    let html = r#"<div class="sidebar"><article>content</article></div>"#;
    let mut nodes = parse_html(html).expect("valid HTML");
    walk_pre_mut(&mut nodes, &|n| strip_unlikely_candidates(n));

    fn find_tag(node: &DomNode, tag: &str) -> bool {
        match node {
            DomNode::Element { tag: t, .. } if t == tag => true,
            DomNode::Element { children, .. } => children.iter().any(|c| find_tag(c, tag)),
            _ => false,
        }
    }

    assert!(
        find_tag(&nodes, "div"),
        "unlikely-candidate div containing <article> should be kept"
    );
    assert!(
        find_tag(&nodes, "article"),
        "content inside kept div should survive"
    );
}

#[test]
fn test_strip_unlikely_anchor_guard() {
    // <a> elements with unlikely-candidate class should survive (JS Readability line 1124).
    let html = r#"<a class="sidebar">link</a><div class="sidebar">nav</div>"#;
    let mut nodes = parse_html(html).expect("valid HTML");
    walk_pre_mut(&mut nodes, &|n| strip_unlikely_candidates(n));

    fn find_tag(node: &DomNode, tag: &str) -> bool {
        match node {
            DomNode::Element { tag: t, .. } if t == tag => true,
            DomNode::Element { children, .. } => children.iter().any(|c| find_tag(c, tag)),
            _ => false,
        }
    }

    assert!(
        find_tag(&nodes, "a"),
        "<a> with unlikely class should survive (JS guard)"
    );
    assert!(
        !find_tag(&nodes, "div"),
        "<div> with unlikely class should still be removed"
    );
}

#[test]
fn test_strip_unlikely_role_removed() {
    // Non-<a> elements with unlikely role should still be removed (INV-011 regression guard).
    let html = r#"<div role="navigation">nav</div>"#;
    let mut nodes = parse_html(html).expect("valid HTML");
    walk_pre_mut(&mut nodes, &|n| strip_unlikely_candidates(n));

    fn find_tag(node: &DomNode, tag: &str) -> bool {
        match node {
            DomNode::Element { tag: t, .. } if t == tag => true,
            DomNode::Element { children, .. } => children.iter().any(|c| find_tag(c, tag)),
            _ => false,
        }
    }

    assert!(
        !find_tag(&nodes, "div"),
        "<div role='navigation'> should be removed (non-<a> guard)"
    );
}

#[test]
fn test_strip_unlikely_data_table_guard() {
    // Elements inside a data table should survive strip_unlikely_candidates.
    // The table is wrapped in a parent so walk_pre_mut visits it as a child.
    let mut root = DomNode::Element {
        tag: "div".into(),
        attrs: vec![],
        children: vec![DomNode::Element {
            tag: "table".into(),
            attrs: vec![("class".into(), "infobox".into())],
            children: vec![DomNode::Element {
                tag: "tr".into(),
                attrs: vec![],
                children: vec![DomNode::Element {
                    tag: "td".into(),
                    attrs: vec![("class".into(), "sidebar".into())],
                    children: vec![DomNode::Text("content".into())],
                    scores: Default::default(),
                    metadata: Default::default(),
                }],
                scores: Default::default(),
                metadata: Default::default(),
            }],
            scores: Default::default(),
            metadata: [("is_data_table".into(), "true".into())].into(),
        }],
        scores: Default::default(),
        metadata: Default::default(),
    };
    walk_pre_mut(&mut root, &|n| strip_unlikely_candidates(n));

    // The <td class="sidebar"> should survive inside the data table
    fn find_tag(node: &DomNode, tag: &str) -> bool {
        match node {
            DomNode::Element { tag: t, .. } if t == tag => true,
            DomNode::Element { children, .. } => children.iter().any(|c| find_tag(c, tag)),
            _ => false,
        }
    }
    assert!(
        find_tag(&root, "td"),
        "<td> inside data table should survive strip_unlikely_candidates"
    );
}
// ── 5. remove_empty_structural_elements ──────────────────────────────

#[test]
fn test_remove_empty_structural_elements() {
    let html = "<div></div><p>text</p>";
    let mut nodes = parse_html(html).expect("valid HTML");
    walk_pre_mut(&mut nodes, &|n| remove_empty_structural_elements(n));

    fn find_tag(node: &DomNode, tag: &str) -> bool {
        match node {
            DomNode::Element { tag: t, .. } if t == tag => true,
            DomNode::Element { children, .. } => children.iter().any(|c| find_tag(c, tag)),
            _ => false,
        }
    }

    assert!(!find_tag(&nodes, "div"), "empty <div> should be removed");
    assert!(find_tag(&nodes, "p"), "<p> with text should remain");
}

#[test]
fn test_remove_empty_structural_protected_tags() {
    let html = "<table><td></td></table>";
    let mut nodes = parse_html(html).expect("valid HTML");
    walk_pre_mut(&mut nodes, &|n| remove_empty_structural_elements(n));

    fn find_tag(node: &DomNode, tag: &str) -> bool {
        match node {
            DomNode::Element { tag: t, .. } if t == tag => true,
            DomNode::Element { children, .. } => children.iter().any(|c| find_tag(c, tag)),
            _ => false,
        }
    }

    assert!(
        find_tag(&nodes, "td"),
        "empty <td> should be kept (protected)"
    );
}

// ── 10. rd_filter_by_score ─────────────────────────────────────────────

// ── 11. remove_garbage_interactive_elements ───────────────────────────

#[test]
fn test_remove_garbage_form() {
    let html = "<form><input name='q'></form><p>content</p>";
    let mut nodes = crate::pipelines::parse_html(html).expect("valid HTML");
    walk_pre_mut(&mut nodes, &|n| remove_garbage_interactive_elements(n));

    fn find_tag(node: &DomNode, tag: &str) -> bool {
        match node {
            DomNode::Element { tag: t, .. } if t == tag => true,
            DomNode::Element { children, .. } => children.iter().any(|c| find_tag(c, tag)),
            _ => false,
        }
    }

    assert!(
        find_tag(&nodes, "form"),
        "<form> should no longer be unconditionally removed (now handled by filter_low_density_elements)"
    );
    assert!(find_tag(&nodes, "p"), "<p> should remain");
}

#[test]
fn test_remove_garbage_embed() {
    let html = "<embed src='flash.swf'><p>text</p>";
    let mut nodes = crate::pipelines::parse_html(html).expect("valid HTML");
    walk_pre_mut(&mut nodes, &|n| remove_garbage_interactive_elements(n));

    fn find_tag(node: &DomNode, tag: &str) -> bool {
        match node {
            DomNode::Element { tag: t, .. } if t == tag => true,
            DomNode::Element { children, .. } => children.iter().any(|c| find_tag(c, tag)),
            _ => false,
        }
    }

    assert!(!find_tag(&nodes, "embed"), "<embed> should be removed");
}

#[test]
fn test_remove_garbage_preserves_youtube() {
    // YouTube embed should be preserved
    let html = r#"<iframe src="https://www.youtube.com/embed/dQw4w9WgXcQ"></iframe>"#;
    let mut nodes = crate::pipelines::parse_html(html).expect("valid HTML");
    walk_pre_mut(&mut nodes, &|n| remove_garbage_interactive_elements(n));

    fn find_tag(node: &DomNode, tag: &str) -> bool {
        match node {
            DomNode::Element { tag: t, .. } if t == tag => true,
            DomNode::Element { children, .. } => children.iter().any(|c| find_tag(c, tag)),
            _ => false,
        }
    }

    assert!(
        find_tag(&nodes, "iframe"),
        "YouTube iframe should be preserved"
    );
}

// ── 12. clean_negative_headers ──────────────────────────────────────

#[test]
fn test_clean_negative_headers_removes_negative() {
    // H1 with negative class weight (sidebar) should be removed
    let html = r#"<h1 class="sidebar">nav</h1><h2>content</h2>"#;
    let mut nodes = crate::pipelines::parse_html(html).expect("valid HTML");
    walk_pre_mut(&mut nodes, &|n| clean_negative_headers(n));

    fn find_tag(node: &DomNode, tag: &str) -> bool {
        match node {
            DomNode::Element { tag: t, .. } if t == tag => true,
            DomNode::Element { children, .. } => children.iter().any(|c| find_tag(c, tag)),
            _ => false,
        }
    }

    assert!(
        !find_tag(&nodes, "h1"),
        "<h1 class='sidebar'> should be removed"
    );
    assert!(
        find_tag(&nodes, "h2"),
        "<h2> without negative class should remain"
    );
}

// ── 15. filter_low_density_elements ─────────────────────────────────

#[test]
fn test_has_likely_content_with_content_pattern_class() {
    let div = DomNode::Element {
        tag: "div".into(),
        attrs: vec![("class".into(), "content".into())],
        children: vec![],
        scores: std::collections::HashMap::new(),
        metadata: std::collections::HashMap::new(),
    };
    assert!(
        has_likely_content(&[div]),
        "div with class='content' should be likely content"
    );
}

#[test]
fn test_strip_unlikely_with_content_class_kept() {
    let child_div = DomNode::Element {
        tag: "div".into(),
        attrs: vec![("class".into(), "content".into())],
        children: vec![DomNode::Text("text".into())],
        scores: std::collections::HashMap::new(),
        metadata: std::collections::HashMap::new(),
    };
    let parent_div = DomNode::Element {
        tag: "div".into(),
        attrs: vec![("class".into(), "sidebar".into())],
        children: vec![child_div],
        scores: std::collections::HashMap::new(),
        metadata: std::collections::HashMap::new(),
    };
    let html = crate::generators::gen_html::dom_nodes_to_html(&parent_div);
    let mut nodes = parse_html(&html).expect("valid HTML");
    walk_pre_mut(&mut nodes, &|n| strip_unlikely_candidates(n));

    fn find_tag(node: &DomNode, tag: &str) -> bool {
        match node {
            DomNode::Element { tag: t, .. } if t == tag => true,
            DomNode::Element { children, .. } => children.iter().any(|c| find_tag(c, tag)),
            _ => false,
        }
    }
    assert!(
        find_tag(&nodes, "div"),
        "outer div with content-child should survive unlikely check"
    );
}

#[test]
fn test_strip_unlikely_ok_maybe_its_a_candidate() {
    // An element whose class matches both unlikely and candidate patterns
    // should survive due to the okMaybeItsACandidate self-check.
    // E.g. <mjx-container class="MathJax CtxtMenu_Attached_0"> where
    // "CtxtMenu" matches the unlikely regex but "MathJax" matches CONTENT_CANDIDATE_RE.
    let html = r#"<div class="MathJax CtxtMenu_Attached_0">math content</div>"#;
    let mut nodes = parse_html(html).expect("valid HTML");
    walk_pre_mut(&mut nodes, &|n| strip_unlikely_candidates(n));

    fn find_tag(node: &DomNode, tag: &str) -> bool {
        match node {
            DomNode::Element { tag: t, .. } if t == tag => true,
            DomNode::Element { children, .. } => children.iter().any(|c| find_tag(c, tag)),
            _ => false,
        }
    }

    assert!(
        find_tag(&nodes, "div"),
        "div with MathJax class should survive (okMaybeItsACandidate self-check)"
    );
}

// ── clean_matched_nodes ────────────────────────────────────────────────

#[test]
fn test_clean_matched_nodes_removes_clearfix() {
    let html = r#"<div class="clearfix"></div><p>content</p>"#;
    let mut nodes = parse_html(html).expect("valid HTML");
    walk_pre_mut(&mut nodes, &|n| clean_matched_nodes(n));

    fn find_tag(node: &DomNode, tag: &str) -> bool {
        match node {
            DomNode::Element { tag: t, .. } if t == tag => true,
            DomNode::Element { children, .. } => children.iter().any(|c| find_tag(c, tag)),
            _ => false,
        }
    }
    assert!(!find_tag(&nodes, "div"), "clearfix div should be removed");
    assert!(find_tag(&nodes, "p"), "content should remain");
}

#[test]
fn test_clean_matched_nodes_removes_print_button() {
    let html = r#"<div class="printfriendly"></div><p>text</p>"#;
    let mut nodes = parse_html(html).expect("valid HTML");
    walk_pre_mut(&mut nodes, &|n| clean_matched_nodes(n));

    fn find_tag(node: &DomNode, tag: &str) -> bool {
        match node {
            DomNode::Element { tag: t, .. } if t == tag => true,
            DomNode::Element { children, .. } => children.iter().any(|c| find_tag(c, tag)),
            _ => false,
        }
    }
    assert!(
        !find_tag(&nodes, "div"),
        "printfriendly div should be removed"
    );
    assert!(find_tag(&nodes, "p"), "content should remain");
}

#[test]
fn test_clean_matched_nodes_keeps_content() {
    let html = r#"<div class="content"><p>text</p></div>"#;
    let mut nodes = parse_html(html).expect("valid HTML");
    walk_pre_mut(&mut nodes, &|n| clean_matched_nodes(n));

    fn find_tag(node: &DomNode, tag: &str) -> bool {
        match node {
            DomNode::Element { tag: t, .. } if t == tag => true,
            DomNode::Element { children, .. } => children.iter().any(|c| find_tag(c, tag)),
            _ => false,
        }
    }
    assert!(find_tag(&nodes, "div"), "content div should be kept");
    assert!(find_tag(&nodes, "p"), "<p> should survive");
}

// ── Heuristic functions with known field values ─────────────────────

#[test]
fn test_heuristic_functions_with_known_subtree_counts() {
    let mut metadata = std::collections::HashMap::new();
    metadata.insert("link_density".to_string(), "0.500000".to_string());

    // H1: link_density=0.5 > 0.333 -> true
    assert!(check_high_link_density(&metadata));

    // H2: headings/total = 2/10 = 0.2, not > 0.9 -> false
    assert!(!check_high_heading_density(2, 10, 1));

    // H3: imgs(3) > paras(4) is false -> false
    assert!(!check_img_para_ratio(3, 4, false));

    // H4: weight=0 < 25, link_density=0.5 > 0.2 -> true
    assert!(check_low_weight_link_density(&metadata, 0, false));

    // H5: weight=0 < 25, so this checks weight >= 25 -> false
    assert!(!check_high_weight_link_density(&metadata, 0, false));

    // H6: imgs(3) > paras(4) is false -> false
    assert!(!check_media_heavy(3, 4, 1, false));

    // H7: inputs=0 -> false
    assert!(!check_form_heavy(0, 4));

    // H8: imgs(3) > paras(4) is false -> false
    assert!(!check_gallery(3, 4, false));

    // A: lis=0 -> false
    assert!(!check_list_heavy(0, 4));

    // C: imgs=3 != 0, link_density=0.5 > 0.333, text_chars=200 >= 100 -> false (text_chars < 100 guard)
    assert!(!check_short_content(3, 200, &metadata));

    // D: embeds=1 > 0 -> true
    assert!(check_embed_count(1));

    // E: 200/500 = 0.4, not == 0.0 -> false
    assert!(!check_text_density(200, 500));
}
