use super::*;
use crate::pipelines::{parse_html, walk_pre_mut};

// ── 2. convert_double_br_to_paragraph ─────────────────────────────────

#[test]
fn test_convert_double_br_to_paragraph() {
    let html = "<div>a<br><br>b</div>";
    let mut nodes = vec![parse_html(html).expect("valid HTML")];
    walk_pre_mut(&mut nodes[0], &|n| convert_double_br_to_paragraph(n));

    // Should have <div> containing two <p> elements.
    fn find_p_count(nodes: &[DomNode]) -> usize {
        let mut count = 0;
        for node in nodes {
            if let DomNode::Element { tag, children, .. } = node {
                if tag == "p" {
                    count += 1;
                }
                count += find_p_count(children);
            }
        }
        count
    }

    assert_eq!(
        find_p_count(&nodes),
        2,
        "should create two <p> elements from double <br> split"
    );
}

#[test]
fn test_convert_double_br_no_change_no_br() {
    let html = "<div>hello world</div>";
    let mut nodes = vec![parse_html(html).expect("valid HTML")];
    walk_pre_mut(&mut nodes[0], &|n| convert_double_br_to_paragraph(n));

    fn find_tag(nodes: &[DomNode], tag: &str) -> bool {
        for node in nodes {
            match node {
                DomNode::Element { tag: t, .. } if t == tag => return true,
                DomNode::Element { children, .. } if find_tag(children, tag) => return true,
                _ => {}
            }
        }
        false
    }

    assert!(
        !find_tag(&nodes, "p"),
        "no <p> should be created when there are no <br><br>"
    );
}

// ── 3. convert_font_to_span ───────────────────────────────────────────

#[test]
fn test_convert_font_to_span() {
    let html = r#"<font color="red">text</font>"#;
    let mut nodes = vec![parse_html(html).expect("valid HTML")];
    walk_pre_mut(&mut nodes[0], &|n| convert_font_to_span(n));

    fn find_tag(nodes: &[DomNode], tag: &str) -> bool {
        for node in nodes {
            match node {
                DomNode::Element {
                    tag: t, children, ..
                } if t == tag => return true,
                DomNode::Element { children, .. } if find_tag(children, tag) => return true,
                _ => {}
            }
        }
        false
    }

    assert!(!find_tag(&nodes, "font"), "<font> should be converted");
    assert!(find_tag(&nodes, "span"), "<span> should replace <font>");
}

#[test]
fn test_convert_font_preserves_attrs() {
    let html = r#"<font color="red" face="Arial">text</font>"#;
    let mut nodes = vec![parse_html(html).expect("valid HTML")];
    walk_pre_mut(&mut nodes[0], &|n| convert_font_to_span(n));

    fn find_span_attrs(nodes: &[DomNode]) -> Option<Vec<(String, String)>> {
        for node in nodes {
            match node {
                DomNode::Element { tag, attrs, .. } if tag == "span" => {
                    return Some(attrs.clone());
                }
                DomNode::Element { children, .. } => {
                    if let Some(a) = find_span_attrs(children) {
                        return Some(a);
                    }
                }
                _ => {}
            }
        }
        None
    }

    let attrs = find_span_attrs(&nodes).expect("<span> should exist");
    assert!(attrs.contains(&("color".into(), "red".into())));
    assert!(attrs.contains(&("face".into(), "Arial".into())));
}

// ── 6. convert_div_containing_phrasing_to_paragraph ───────────────────

#[test]
fn test_convert_div_to_paragraph() {
    let html = "<div><span>text</span></div>";
    let mut nodes = vec![parse_html(html).expect("valid HTML")];
    walk_pre_mut(&mut nodes[0], &|n| {
        convert_div_containing_phrasing_to_paragraph(n)
    });

    fn find_tag(nodes: &[DomNode], tag: &str) -> bool {
        for node in nodes {
            match node {
                DomNode::Element {
                    tag: t, children, ..
                } if t == tag => return true,
                DomNode::Element { children, .. } if find_tag(children, tag) => return true,
                _ => {}
            }
        }
        false
    }

    assert!(
        !find_tag(&nodes, "div"),
        "<div> with phrasing content should become <p>"
    );
    assert!(find_tag(&nodes, "p"), "<p> should exist");
    assert!(
        find_tag(&nodes, "span"),
        "<span> child should survive conversion"
    );
}

#[test]
fn test_convert_div_to_paragraph_keeps_div_with_block_children() {
    let html = "<div><p>text</p></div>";
    let mut nodes = vec![parse_html(html).expect("valid HTML")];
    walk_pre_mut(&mut nodes[0], &|n| {
        convert_div_containing_phrasing_to_paragraph(n)
    });

    fn find_tag(nodes: &[DomNode], tag: &str) -> bool {
        for node in nodes {
            match node {
                DomNode::Element { tag: t, .. } if t == tag => return true,
                DomNode::Element { children, .. } if find_tag(children, tag) => return true,
                _ => {}
            }
        }
        false
    }

    assert!(
        find_tag(&nodes, "div"),
        "<div> with block children (<p>) should remain <div>"
    );
}

// ── 13. fix_lazy_loaded_images ────────────────────────────────────────

#[test]
fn test_fix_lazy_loaded_images_promotes_data_src() {
    let html = r#"<img data-src="https://example.com/img.jpg" alt="test">"#;
    let mut nodes = vec![parse_html(html).expect("valid HTML")];
    walk_pre_mut(&mut nodes[0], &|n| fix_lazy_loaded_images(n));

    // Check that src was set from data-src
    fn find_src(nodes: &[DomNode]) -> Option<String> {
        for node in nodes {
            match node {
                DomNode::Element { tag, attrs, .. } if tag == "img" => {
                    for (k, v) in attrs {
                        if k == "src" {
                            return Some(v.clone());
                        }
                    }
                }
                DomNode::Element { children, .. } => {
                    if let Some(s) = find_src(children) {
                        return Some(s);
                    }
                }
                _ => {}
            }
        }
        None
    }

    let src = find_src(&nodes).expect("img should have src after promotion");
    assert_eq!(
        src, "https://example.com/img.jpg",
        "data-src should be promoted to src"
    );
}

#[test]
fn test_fix_lazy_loaded_images_skips_existing_src() {
    // Image with a real src already set should not be modified
    let html =
        r#"<img data-src="https://example.com/lazy.jpg" src="https://example.com/real.jpg">"#;
    let mut nodes = vec![parse_html(html).expect("valid HTML")];
    walk_pre_mut(&mut nodes[0], &|n| fix_lazy_loaded_images(n));

    fn get_src(nodes: &[DomNode]) -> Option<String> {
        for node in nodes {
            match node {
                DomNode::Element { tag, attrs, .. } if tag == "img" => {
                    for (k, v) in attrs {
                        if k == "src" {
                            return Some(v.clone());
                        }
                    }
                }
                DomNode::Element { children, .. } => {
                    if let Some(s) = get_src(children) {
                        return Some(s);
                    }
                }
                _ => {}
            }
        }
        None
    }

    let src = get_src(&nodes).expect("img should have src");
    assert_eq!(
        src, "https://example.com/real.jpg",
        "existing real src should not be overwritten"
    );
}

// ── 14. replace_h1_with_h2 ────────────────────────────────────────────

#[test]
fn test_replace_h1_with_h2() {
    let html = "<h1>Title</h1><h2>Section</h2><h1>Another Title</h1>";
    let mut nodes = vec![parse_html(html).expect("valid HTML")];
    walk_pre_mut(&mut nodes[0], &|n| replace_h1_with_h2(n));

    fn find_tag(nodes: &[DomNode], tag: &str) -> bool {
        for node in nodes {
            match node {
                DomNode::Element {
                    tag: t, children, ..
                } if t == tag => return true,
                DomNode::Element { children, .. } if find_tag(children, tag) => return true,
                _ => {}
            }
        }
        false
    }

    assert!(!find_tag(&nodes, "h1"), "no <h1> should remain");
    assert!(find_tag(&nodes, "h2"), "<h2> elements should exist");
}

// ── 18. unwrap_single_cell_tables ─────────────────────────────────────

#[test]
fn test_unwrap_single_cell_table_to_paragraph() {
    let html = "<table><tr><td>Hello</td></tr></table>";
    let mut nodes = vec![parse_html(html).expect("valid HTML")];

    walk_pre_mut(&mut nodes[0], &|n| unwrap_single_cell_tables(n));

    fn find_tag(nodes: &[DomNode], tag: &str) -> bool {
        for node in nodes {
            match node {
                DomNode::Element {
                    tag: t, children, ..
                } if t == tag => return true,
                DomNode::Element { children, .. } if find_tag(children, tag) => return true,
                _ => {}
            }
        }
        false
    }

    assert!(
        !find_tag(&nodes, "table"),
        "single-cell table should be unwrapped"
    );
}

// ── 19. collapse_single_child_elements ────────────────────────────────

#[test]
fn test_collapse_single_child_div() {
    let html = "<div><section><p>text</p></section></div>";
    let mut nodes = vec![parse_html(html).expect("valid HTML")];
    collapse_single_child_elements(&mut nodes[0]);

    fn find_tag(nodes: &[DomNode], tag: &str) -> bool {
        for node in nodes {
            match node {
                DomNode::Element {
                    tag: t, children, ..
                } if t == tag => return true,
                DomNode::Element { children, .. } if find_tag(children, tag) => return true,
                _ => {}
            }
        }
        false
    }

    // After collapse, only <p> should remain (div and section unwrapped)
    assert!(find_tag(&nodes, "p"), "p should remain");
}

#[test]
fn test_remove_empty_paragraph() {
    let html = "<p></p><p>content</p><p>  </p>";
    let mut nodes = vec![parse_html(html).expect("valid HTML")];
    walk_pre_mut(&mut nodes[0], &|n| remove_empty_paragraphs(n));

    fn count_tag(nodes: &[DomNode], tag: &str) -> usize {
        let mut count = 0;
        for node in nodes {
            match node {
                DomNode::Element {
                    tag: t, children, ..
                } if t == tag => count += 1 + count_tag(children, tag),
                DomNode::Element { children, .. } => count += count_tag(children, tag),
                _ => {}
            }
        }
        count
    }

    assert_eq!(count_tag(&nodes, "p"), 1, "only one <p> should remain");
}

// ── 21. rd_strip_non_content ──────────────────────────────────

#[test]
fn test_strip_removes_script() {
    let html = "<div><script>alert(1)</script><p>text</p></div>";
    let mut nodes = vec![parse_html(html).expect("valid HTML")];
    rd_strip_non_content(&mut nodes[0]);

    fn find_tag(nodes: &[DomNode], tag: &str) -> bool {
        for node in nodes {
            match node {
                DomNode::Element {
                    tag: t, children, ..
                } if t == tag => return true,
                DomNode::Element { children, .. } if find_tag(children, tag) => return true,
                _ => {}
            }
        }
        false
    }

    assert!(!find_tag(&nodes, "script"), "<script> should be removed");
    assert!(find_tag(&nodes, "p"), "<p> should survive");
}

#[test]
fn test_strip_removes_multiple_non_content() {
    let html = "<div><script>a</script><style>.c{}</style><nav>menu</nav><footer>copy</footer><aside>side</aside><form>f</form><p>text</p></div>";
    let mut nodes = vec![parse_html(html).expect("valid HTML")];
    rd_strip_non_content(&mut nodes[0]);

    fn find_tag(nodes: &[DomNode], tag: &str) -> bool {
        for node in nodes {
            match node {
                DomNode::Element {
                    tag: t, children, ..
                } if t == tag => return true,
                DomNode::Element { children, .. } if find_tag(children, tag) => return true,
                _ => {}
            }
        }
        false
    }

    assert!(!find_tag(&nodes, "script"), "<script> removed");
    assert!(!find_tag(&nodes, "style"), "<style> removed");
    assert!(!find_tag(&nodes, "nav"), "<nav> removed");
    assert!(!find_tag(&nodes, "footer"), "<footer> removed");
    assert!(!find_tag(&nodes, "aside"), "<aside> removed");
    assert!(!find_tag(&nodes, "form"), "<form> removed");
    assert!(find_tag(&nodes, "p"), "<p> should survive");
}

#[test]
fn test_strip_preserves_title() {
    let mut nodes = vec![DomNode::Element {
        tag: "html".into(),
        attrs: vec![],
        children: vec![
            DomNode::Element {
                tag: "title".into(),
                attrs: vec![],
                children: vec![DomNode::Text("My Page".into())],
                scores: std::collections::HashMap::new(),
                metadata: std::collections::HashMap::new(),
            },
            DomNode::Element {
                tag: "body".into(),
                attrs: vec![],
                children: vec![
                    DomNode::Element {
                        tag: "p".into(),
                        attrs: vec![],
                        children: vec![DomNode::Text("content".into())],
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
                ],
                scores: std::collections::HashMap::new(),
                metadata: std::collections::HashMap::new(),
            },
        ],
        scores: std::collections::HashMap::new(),
        metadata: std::collections::HashMap::new(),
    }];
    rd_strip_non_content(&mut nodes[0]);

    fn find_tag(nodes: &[DomNode], tag: &str) -> bool {
        for node in nodes {
            match node {
                DomNode::Element {
                    tag: t, children, ..
                } if t == tag => return true,
                DomNode::Element { children, .. } if find_tag(children, tag) => return true,
                _ => {}
            }
        }
        false
    }

    assert!(find_tag(&nodes, "title"), "<title> should be preserved");
    assert!(!find_tag(&nodes, "script"), "<script> should be removed");
}

#[test]
fn test_strip_preserves_content_elements() {
    let html = "<div><p>hello</p><h1>title</h1><a href='x'>link</a><img src='x.png'><table><tr><td>data</td></tr></table></div>";
    let mut nodes = vec![parse_html(html).expect("valid HTML")];
    rd_strip_non_content(&mut nodes[0]);

    fn find_tag(nodes: &[DomNode], tag: &str) -> bool {
        for node in nodes {
            match node {
                DomNode::Element {
                    tag: t, children, ..
                } if t == tag => return true,
                DomNode::Element { children, .. } if find_tag(children, tag) => return true,
                _ => {}
            }
        }
        false
    }

    assert!(find_tag(&nodes, "p"), "<p> should survive");
    assert!(find_tag(&nodes, "h1"), "<h1> should survive");
    assert!(find_tag(&nodes, "a"), "<a> should survive");
    assert!(find_tag(&nodes, "img"), "<img> should survive");
    assert!(find_tag(&nodes, "table"), "<table> should survive");
}

#[test]
fn test_strip_preserves_unknown_tag() {
    let html = "<div><custom-x>value</custom-x><p>text</p></div>";
    let mut nodes = vec![parse_html(html).expect("valid HTML")];
    rd_strip_non_content(&mut nodes[0]);

    fn find_tag(nodes: &[DomNode], tag: &str) -> bool {
        for node in nodes {
            match node {
                DomNode::Element {
                    tag: t, children, ..
                } if t == tag => return true,
                DomNode::Element { children, .. } if find_tag(children, tag) => return true,
                _ => {}
            }
        }
        false
    }

    assert!(find_tag(&nodes, "custom-x"), "<custom-x> should survive");
    assert!(find_tag(&nodes, "p"), "<p> should survive");
}

// ── 22. rd_unwrap_structural_wrappers ───────────────────────────────────

#[test]
fn test_unwrap_single_container() {
    let mut nodes = vec![parse_html("<html><p>text</p></html>").expect("valid HTML")];
    rd_unwrap_structural_wrappers(&mut nodes[0]);

    fn find_tag(nodes: &[DomNode], tag: &str) -> bool {
        for node in nodes {
            match node {
                DomNode::Element {
                    tag: t, children, ..
                } if t == tag => return true,
                DomNode::Element { children, .. } if find_tag(children, tag) => return true,
                _ => {}
            }
        }
        false
    }

    assert!(!find_tag(&nodes, "html"), "<html> should be unwrapped");
    assert!(find_tag(&nodes, "p"), "<p> should survive");
}
#[test]
fn test_unwrap_nested_containers() {
    let mut nodes = vec![
        parse_html("<html><head><title>Test</title></head><body><p>text</p></body></html>")
            .expect("valid HTML"),
    ];
    rd_unwrap_structural_wrappers(&mut nodes[0]);

    fn find_tag(nodes: &[DomNode], tag: &str) -> bool {
        for node in nodes {
            match node {
                DomNode::Element {
                    tag: t, children, ..
                } if t == tag => return true,
                DomNode::Element { children, .. } if find_tag(children, tag) => return true,
                _ => {}
            }
        }
        false
    }

    assert!(!find_tag(&nodes, "html"), "<html> should be unwrapped");
    assert!(!find_tag(&nodes, "head"), "<head> should be unwrapped");
    assert!(!find_tag(&nodes, "body"), "<body> should be unwrapped");
    assert!(find_tag(&nodes, "p"), "<p> should survive");
    assert!(find_tag(&nodes, "title"), "<title> should survive");
}

#[test]
fn test_unwrap_preserves_data_table() {
    let mut nodes = vec![DomNode::Element {
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
        metadata: {
            let mut m = std::collections::HashMap::new();
            m.insert("is_data_table".to_string(), "true".to_string());
            m
        },
    }];
    rd_unwrap_structural_wrappers(&mut nodes[0]);

    fn find_tag(nodes: &[DomNode], tag: &str) -> bool {
        for node in nodes {
            match node {
                DomNode::Element {
                    tag: t, children, ..
                } if t == tag => return true,
                DomNode::Element { children, .. } if find_tag(children, tag) => return true,
                _ => {}
            }
        }
        false
    }

    assert!(find_tag(&nodes, "table"), "data table should be preserved");
    assert!(
        find_tag(&nodes, "td"),
        "<td> inside data table should be preserved"
    );
}

#[test]
fn test_unwrap_layout_table() {
    let mut nodes = vec![DomNode::Element {
        tag: "table".into(),
        attrs: vec![],
        children: vec![DomNode::Element {
            tag: "tr".into(),
            attrs: vec![],
            children: vec![DomNode::Element {
                tag: "td".into(),
                attrs: vec![],
                children: vec![DomNode::Text("layout".into())],
                scores: std::collections::HashMap::new(),
                metadata: std::collections::HashMap::new(),
            }],
            scores: std::collections::HashMap::new(),
            metadata: std::collections::HashMap::new(),
        }],
        scores: std::collections::HashMap::new(),
        metadata: {
            let mut m = std::collections::HashMap::new();
            m.insert("is_data_table".to_string(), "false".to_string());
            m
        },
    }];
    rd_unwrap_structural_wrappers(&mut nodes[0]);

    fn find_tag(nodes: &[DomNode], tag: &str) -> bool {
        for node in nodes {
            match node {
                DomNode::Element {
                    tag: t, children, ..
                } if t == tag => return true,
                DomNode::Element { children, .. } if find_tag(children, tag) => return true,
                _ => {}
            }
        }
        false
    }

    assert!(
        !find_tag(&nodes, "table"),
        "layout <table> should be unwrapped"
    );
    assert!(find_tag(&nodes, "td"), "<td> should be preserved");
    assert!(find_tag(&nodes, "tr"), "<tr> should survive");
}
#[test]
fn test_unwrap_consecutive_containers() {
    let mut nodes =
        vec![parse_html("<html><body><p>a</p><p>b</p></body></html>").expect("valid HTML")];
    rd_unwrap_structural_wrappers(&mut nodes[0]);

    fn find_tag(nodes: &[DomNode], tag: &str) -> bool {
        for node in nodes {
            match node {
                DomNode::Element {
                    tag: t, children, ..
                } if t == tag => return true,
                DomNode::Element { children, .. } if find_tag(children, tag) => return true,
                _ => {}
            }
        }
        false
    }

    fn count_tag(nodes: &[DomNode], tag: &str) -> usize {
        let mut count = 0;
        for node in nodes {
            match node {
                DomNode::Element {
                    tag: t, children, ..
                } if t == tag => count += 1 + count_tag(children, tag),
                DomNode::Element { children, .. } => count += count_tag(children, tag),
                _ => {}
            }
        }
        count
    }

    assert!(!find_tag(&nodes, "html"), "<html> should be unwrapped");
    assert!(!find_tag(&nodes, "body"), "<body> should be unwrapped");
    assert_eq!(count_tag(&nodes, "p"), 2, "both <p> should survive");
}

#[test]
fn test_unwrap_is_data_table_case_insensitive() {
    let mut nodes = vec![DomNode::Element {
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
        metadata: {
            let mut m = std::collections::HashMap::new();
            m.insert("is_data_table".to_string(), "True".to_string());
            m
        },
    }];
    rd_unwrap_structural_wrappers(&mut nodes[0]);

    fn find_tag(nodes: &[DomNode], tag: &str) -> bool {
        for node in nodes {
            match node {
                DomNode::Element {
                    tag: t, children, ..
                } if t == tag => return true,
                DomNode::Element { children, .. } if find_tag(children, tag) => return true,
                _ => {}
            }
        }
        false
    }

    assert!(
        find_tag(&nodes, "table"),
        "data table with 'True' should be preserved"
    );
    assert!(
        find_tag(&nodes, "td"),
        "<td> inside data table should be preserved"
    );
}
#[test]
fn test_unwrap_header_and_li() {
    // header/li are NOT structural wrappers — they should be preserved
    let html = "<header><p>heading</p></header><ul><li><p>item</p></li></ul>";
    let mut nodes = vec![parse_html(html).expect("valid HTML")];
    rd_unwrap_structural_wrappers(&mut nodes[0]);

    fn find_tag(nodes: &[DomNode], tag: &str) -> bool {
        for node in nodes {
            match node {
                DomNode::Element {
                    tag: t, children, ..
                } if t == tag => return true,
                DomNode::Element { children, .. } if find_tag(children, tag) => return true,
                _ => {}
            }
        }
        false
    }

    // Now only html/head/body are unwrapped; header/li etc. are preserved
    assert!(find_tag(&nodes, "header"), "<header> should be preserved");
    assert!(find_tag(&nodes, "li"), "<li> should be preserved");
    assert!(find_tag(&nodes, "p"), "<p> should survive");
    assert!(find_tag(&nodes, "ul"), "<ul> should survive");
}

// ── 23. clean_styles ────────────────────────────────────────────

#[test]
fn test_clean_styles_removes_style_attr() {
    let html = r#"<div style="color:red">text</div>"#;
    let mut nodes = vec![parse_html(html).expect("valid HTML")];
    walk_pre_mut(&mut nodes[0], &|n| clean_styles(n));
    fn find_tag(nodes: &[DomNode], tag: &str) -> bool {
        for node in nodes {
            match node {
                DomNode::Element {
                    tag: t, children, ..
                } if t == tag => return true,
                DomNode::Element { children, .. } if find_tag(children, tag) => return true,
                _ => {}
            }
        }
        false
    }
    // After clean_styles, the <div> should still exist but without style attr
    assert!(find_tag(&nodes, "div"), "<div> should be kept");
    // Verify style attr is gone
    fn has_style_attr(nodes: &[DomNode]) -> bool {
        for node in nodes {
            match node {
                DomNode::Element { tag, attrs, .. } if tag == "div" => {
                    return attrs.iter().any(|(k, _)| k == "style");
                }
                DomNode::Element { children, .. } if has_style_attr(children) => {
                    return true;
                }
                _ => {}
            }
        }
        false
    }
    assert!(!has_style_attr(&nodes), "style attr should be removed");
}

#[test]
fn test_clean_styles_removes_event_handler() {
    let html = r#"<button onclick="doSomething()">click</button>"#;
    let mut nodes = vec![parse_html(html).expect("valid HTML")];
    walk_pre_mut(&mut nodes[0], &|n| clean_styles(n));
    fn find_tag(nodes: &[DomNode], tag: &str) -> bool {
        for node in nodes {
            match node {
                DomNode::Element {
                    tag: t, children, ..
                } if t == tag => return true,
                DomNode::Element { children, .. } if find_tag(children, tag) => return true,
                _ => {}
            }
        }
        false
    }
    assert!(find_tag(&nodes, "button"), "<button> should be kept");
    fn has_onclick(nodes: &[DomNode]) -> bool {
        for node in nodes {
            match node {
                DomNode::Element { tag, attrs, .. } if tag == "button" => {
                    return attrs.iter().any(|(k, _)| k == "onclick");
                }
                DomNode::Element { children, .. } if has_onclick(children) => {
                    return true;
                }
                _ => {}
            }
        }
        false
    }
    assert!(!has_onclick(&nodes), "onclick attr should be removed");
}

#[test]
fn test_clean_styles_preserves_other_attrs() {
    let html = r#"<a href="/test" style="color:blue" onclick="track()">link</a>"#;
    let mut nodes = vec![parse_html(html).expect("valid HTML")];
    walk_pre_mut(&mut nodes[0], &|n| clean_styles(n));
    fn find_tag(nodes: &[DomNode], tag: &str) -> bool {
        for node in nodes {
            match node {
                DomNode::Element {
                    tag: t, children, ..
                } if t == tag => return true,
                DomNode::Element { children, .. } if find_tag(children, tag) => return true,
                _ => {}
            }
        }
        false
    }
    assert!(find_tag(&nodes, "a"), "<a> should be kept");
    fn get_href(nodes: &[DomNode]) -> Option<String> {
        for node in nodes {
            match node {
                DomNode::Element { tag, attrs, .. } if tag == "a" => {
                    for (k, v) in attrs {
                        if k == "href" {
                            return Some(v.clone());
                        }
                    }
                }
                DomNode::Element { children, .. } => {
                    if let Some(h) = get_href(children) {
                        return Some(h);
                    }
                }
                _ => {}
            }
        }
        None
    }
    assert_eq!(
        get_href(&nodes).as_deref(),
        Some("/test"),
        "href attr should be preserved"
    );
}

// ── 24. clean_classes ────────────────────────────────────────────

#[test]
fn test_clean_classes_removes_class_attr() {
    let html = r#"<div class="sidebar">text</div>"#;
    let mut nodes = vec![parse_html(html).expect("valid HTML")];
    walk_pre_mut(&mut nodes[0], &|n| clean_classes(n));
    fn find_tag(nodes: &[DomNode], tag: &str) -> bool {
        for node in nodes {
            match node {
                DomNode::Element {
                    tag: t, children, ..
                } if t == tag => return true,
                DomNode::Element { children, .. } if find_tag(children, tag) => return true,
                _ => {}
            }
        }
        false
    }
    assert!(find_tag(&nodes, "div"), "<div> should be kept");
    fn has_class_attr(nodes: &[DomNode]) -> bool {
        for node in nodes {
            match node {
                DomNode::Element { tag, attrs, .. } if tag == "div" => {
                    return attrs.iter().any(|(k, _)| k == "class");
                }
                DomNode::Element { children, .. } if has_class_attr(children) => {
                    return true;
                }
                _ => {}
            }
        }
        false
    }
    assert!(!has_class_attr(&nodes), "class attr should be removed");
}

#[test]
fn test_clean_classes_preserves_other_attrs() {
    let html = r#"<div id="main" class="content" data-x="test">text</div>"#;
    let mut nodes = vec![parse_html(html).expect("valid HTML")];
    walk_pre_mut(&mut nodes[0], &|n| clean_classes(n));
    fn find_tag(nodes: &[DomNode], tag: &str) -> bool {
        for node in nodes {
            match node {
                DomNode::Element {
                    tag: t, children, ..
                } if t == tag => return true,
                DomNode::Element { children, .. } if find_tag(children, tag) => return true,
                _ => {}
            }
        }
        false
    }
    assert!(find_tag(&nodes, "div"), "<div> should be kept");
    fn get_id(nodes: &[DomNode]) -> Option<String> {
        for node in nodes {
            match node {
                DomNode::Element { tag, attrs, .. } if tag == "div" => {
                    for (k, v) in attrs {
                        if k == "id" {
                            return Some(v.clone());
                        }
                    }
                }
                DomNode::Element { children, .. } => {
                    if let Some(id) = get_id(children) {
                        return Some(id);
                    }
                }
                _ => {}
            }
        }
        None
    }
    assert_eq!(
        get_id(&nodes).as_deref(),
        Some("main"),
        "id attr should be preserved"
    );
    fn has_class_attr(nodes: &[DomNode]) -> bool {
        for node in nodes {
            match node {
                DomNode::Element { tag, attrs, .. } if tag == "div" => {
                    return attrs.iter().any(|(k, _)| k == "class");
                }
                DomNode::Element { children, .. } if has_class_attr(children) => {
                    return true;
                }
                _ => {}
            }
        }
        false
    }
    assert!(!has_class_attr(&nodes), "class attr should be removed");
}
// ── 19. collapse_single_child_elements: enhancements ────────────────

#[test]
fn test_collapse_empty_div_removed() {
    let html = "<div></div>";
    let mut nodes = vec![parse_html(html).expect("valid HTML")];
    collapse_single_child_elements(&mut nodes[0]);
    fn find_tag(nodes: &[DomNode], tag: &str) -> bool {
        for node in nodes {
            match node {
                DomNode::Element {
                    tag: t, children, ..
                } if t == tag => return true,
                DomNode::Element { children, .. } if find_tag(children, tag) => return true,
                _ => {}
            }
        }
        false
    }
    assert!(!find_tag(&nodes, "div"), "empty div should be removed");
}

#[test]
fn test_collapse_whitespace_only_div_removed() {
    let html = "<div>   </div>";
    let mut nodes = vec![parse_html(html).expect("valid HTML")];
    collapse_single_child_elements(&mut nodes[0]);
    fn find_tag(nodes: &[DomNode], tag: &str) -> bool {
        for node in nodes {
            match node {
                DomNode::Element {
                    tag: t, children, ..
                } if t == tag => return true,
                DomNode::Element { children, .. } if find_tag(children, tag) => return true,
                _ => {}
            }
        }
        false
    }
    assert!(
        !find_tag(&nodes, "div"),
        "whitespace-only div should be removed"
    );
}

#[test]
fn test_collapse_empty_nested_divs_removed() {
    let html = "<div><section><div></div></section></div>";
    let mut nodes = vec![parse_html(html).expect("valid HTML")];
    collapse_single_child_elements(&mut nodes[0]);
    fn find_tag(nodes: &[DomNode], tag: &str) -> bool {
        for node in nodes {
            match node {
                DomNode::Element {
                    tag: t, children, ..
                } if t == tag => return true,
                DomNode::Element { children, .. } if find_tag(children, tag) => return true,
                _ => {}
            }
        }
        false
    }
    assert!(
        !find_tag(&nodes, "div"),
        "empty nested div should be removed"
    );
}

#[test]
fn test_collapse_attribute_transfer() {
    let html = "<div class='wrapper'><p id='child'>text</p></div>";
    let mut nodes = vec![parse_html(html).expect("valid HTML")];
    collapse_single_child_elements(&mut nodes[0]);
    fn find_p_attrs(nodes: &[DomNode]) -> Vec<(String, String)> {
        for node in nodes {
            match node {
                DomNode::Element { tag, attrs, .. } if tag == "p" => return attrs.clone(),
                DomNode::Element { children, .. } => {
                    let result = find_p_attrs(children);
                    if !result.is_empty() {
                        return result;
                    }
                }
                _ => {}
            }
        }
        vec![]
    }
    let attrs = find_p_attrs(&nodes);
    assert!(
        attrs.iter().any(|(k, v)| k == "class" && v == "wrapper"),
        "parent class attr should transfer to child"
    );
    assert!(
        attrs.iter().any(|(k, v)| k == "id" && v == "child"),
        "child's own id attr should be preserved"
    );
}

#[test]
fn test_collapse_non_empty_div_kept() {
    let html = "<div><p>content</p></div>";
    let mut nodes = vec![parse_html(html).expect("valid HTML")];
    collapse_single_child_elements(&mut nodes[0]);
    fn find_tag(nodes: &[DomNode], tag: &str) -> bool {
        for node in nodes {
            match node {
                DomNode::Element {
                    tag: t, children, ..
                } if t == tag => return true,
                DomNode::Element { children, .. } if find_tag(children, tag) => return true,
                _ => {}
            }
        }
        false
    }
    assert!(find_tag(&nodes, "p"), "p should remain");
}

// ── 13. fix_lazy_loaded_images: enhancements ─────────────────────────

#[test]
fn test_fix_lazy_loaded_images_data_original() {
    let html = r##"<img data-original='https://example.com/img.jpg' src='data:image/gif;base64,R0lGODlhAQABAAAAACH5BAEKAAEALAAAAAABAAEAAAICTAEAOw=='>"##;
    let mut nodes = vec![parse_html(html).expect("valid HTML")];
    walk_pre_mut(&mut nodes[0], &|n| fix_lazy_loaded_images(n));
    fn get_src(nodes: &[DomNode]) -> Option<String> {
        for node in nodes {
            match node {
                DomNode::Element { tag, attrs, .. } if tag == "img" => {
                    for (k, v) in attrs {
                        if k == "src" {
                            return Some(v.clone());
                        }
                    }
                }
                DomNode::Element { children, .. } => {
                    if let Some(s) = get_src(children) {
                        return Some(s);
                    }
                }
                _ => {}
            }
        }
        None
    }
    let src = get_src(&nodes).expect("img should have src after promotion");
    assert_eq!(
        src, "https://example.com/img.jpg",
        "data-original should be promoted to src"
    );
}

#[test]
fn test_fix_lazy_loaded_images_data_fallback() {
    let html = r##"<img data-fallback='https://example.com/fallback.jpg'>"##;
    let mut nodes = vec![parse_html(html).expect("valid HTML")];
    walk_pre_mut(&mut nodes[0], &|n| fix_lazy_loaded_images(n));
    fn get_src(nodes: &[DomNode]) -> Option<String> {
        for node in nodes {
            match node {
                DomNode::Element { tag, attrs, .. } if tag == "img" => {
                    for (k, v) in attrs {
                        if k == "src" {
                            return Some(v.clone());
                        }
                    }
                }
                DomNode::Element { children, .. } => {
                    if let Some(s) = get_src(children) {
                        return Some(s);
                    }
                }
                _ => {}
            }
        }
        None
    }
    let src = get_src(&nodes).expect("img should have src after promotion");
    assert_eq!(
        src, "https://example.com/fallback.jpg",
        "data-fallback should be promoted to src"
    );
}

#[test]
fn test_fix_lazy_loaded_images_data_lazy_src() {
    let html = r##"<img data-lazy-src='https://example.com/lazy.jpg'>"##;
    let mut nodes = vec![parse_html(html).expect("valid HTML")];
    walk_pre_mut(&mut nodes[0], &|n| fix_lazy_loaded_images(n));
    fn get_src(nodes: &[DomNode]) -> Option<String> {
        for node in nodes {
            match node {
                DomNode::Element { tag, attrs, .. } if tag == "img" => {
                    for (k, v) in attrs {
                        if k == "src" {
                            return Some(v.clone());
                        }
                    }
                }
                DomNode::Element { children, .. } => {
                    if let Some(s) = get_src(children) {
                        return Some(s);
                    }
                }
                _ => {}
            }
        }
        None
    }
    let src = get_src(&nodes).expect("img should have src after promotion");
    assert_eq!(
        src, "https://example.com/lazy.jpg",
        "data-lazy-src should be promoted to src"
    );
}

#[test]
fn test_fix_lazy_loaded_images_data_src_preferred_over_original() {
    let html = r##"<img data-src='https://example.com/src.jpg' data-original='https://example.com/original.jpg'>"##;
    let mut nodes = vec![parse_html(html).expect("valid HTML")];
    walk_pre_mut(&mut nodes[0], &|n| fix_lazy_loaded_images(n));
    fn get_src(nodes: &[DomNode]) -> Option<String> {
        for node in nodes {
            match node {
                DomNode::Element { tag, attrs, .. } if tag == "img" => {
                    for (k, v) in attrs {
                        if k == "src" {
                            return Some(v.clone());
                        }
                    }
                }
                DomNode::Element { children, .. } => {
                    if let Some(s) = get_src(children) {
                        return Some(s);
                    }
                }
                _ => {}
            }
        }
        None
    }
    let src = get_src(&nodes).expect("img should have src after promotion");
    assert_eq!(
        src, "https://example.com/src.jpg",
        "data-src should be preferred over data-original"
    );
}
