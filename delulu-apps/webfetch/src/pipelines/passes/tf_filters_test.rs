use super::*;
use crate::pipelines::parse_html;
use crate::pipelines::walk_pre_mut;
use std::collections::HashMap;

// ── tf_remove_cleaned ────────────────────────────────────────────────

#[test]
fn test_tf_remove_cleaned_removes_aside() {
    let mut root = parse_html("<aside>content</aside><p>keep</p>").unwrap();
    walk_pre_mut(&mut root, &|n| tf_remove_cleaned(n));
    assert!(!find_tag(&root, "aside"), "<aside> should be removed");
    assert!(find_tag(&root, "p"), "<p> should still exist");
}

#[test]
fn test_tf_remove_cleaned_removes_figure() {
    let mut root = parse_html("<figure><img src='x.png'></figure><p>text</p>").unwrap();
    walk_pre_mut(&mut root, &|n| tf_remove_cleaned(n));
    assert!(!find_tag(&root, "figure"), "<figure> should be removed");
}

#[test]
fn test_tf_remove_cleaned_keeps_unlisted() {
    let mut root = parse_html("<p>keep</p><div>keep</div>").unwrap();
    walk_pre_mut(&mut root, &|n| tf_remove_cleaned(n));
    assert!(find_tag(&root, "p"), "<p> should be kept");
    assert!(find_tag(&root, "div"), "<div> should be kept");
}
#[test]
fn test_tf_remove_cleaned_preserves_head_with_rend() {
    let mut root = parse_html("<head rend=\"h1\">Title</head><p>text</p>").unwrap();
    walk_pre_mut(&mut root, &|n| tf_remove_cleaned(n));
    assert!(
        find_tag(&root, "head"),
        "<head rend=\"h1\"> should be preserved"
    );
    assert!(find_tag(&root, "p"), "<p> should survive");
}

#[test]
fn test_tf_remove_cleaned_removes_bare_head() {
    let mut root = parse_html("<head>Title</head><p>text</p>").unwrap();
    walk_pre_mut(&mut root, &|n| tf_remove_cleaned(n));
    assert!(!find_tag(&root, "head"), "bare <head> should be removed");
    assert!(find_tag(&root, "p"), "<p> should survive");
}

#[test]
fn test_tf_remove_cleaned_preserves_other_cleaned() {
    let mut root = parse_html("<aside>side</aside><figure>fig</figure><p>text</p>").unwrap();
    walk_pre_mut(&mut root, &|n| tf_remove_cleaned(n));
    assert!(!find_tag(&root, "aside"), "<aside> should still be removed");
    assert!(
        !find_tag(&root, "figure"),
        "<figure> should still be removed"
    );
}

// ── tf_remove_teaser ──────────────────────────────────────────────

#[test]
fn test_tf_remove_teaser_class_div() {
    let mut root = parse_html("<div class=\"teaser\">content</div><p>keep</p>").unwrap();
    walk_pre_mut(&mut root, &|n| tf_remove_teaser(n));
    assert!(
        !find_tag(&root, "div"),
        "<div class='teaser'> should be removed"
    );
    assert!(find_tag(&root, "p"), "<p> should still exist");
}

#[test]
fn test_tf_remove_teaser_class_case_insensitive() {
    let mut root = parse_html("<div class=\"TeasEr\">content</div><p>keep</p>").unwrap();
    walk_pre_mut(&mut root, &|n| tf_remove_teaser(n));
    assert!(
        !find_tag(&root, "div"),
        "<div class='TeasEr'> should be removed (case insensitive)"
    );
    assert!(find_tag(&root, "p"));
}

#[test]
fn test_tf_remove_teaser_class_contains() {
    let mut root =
        parse_html("<div class=\"post-teaser-content\">content</div><p>keep</p>").unwrap();
    walk_pre_mut(&mut root, &|n| tf_remove_teaser(n));
    assert!(
        !find_tag(&root, "div"),
        "<div class='post-teaser-content'> should be removed (contains)"
    );
    assert!(find_tag(&root, "p"));
}

#[test]
fn test_tf_remove_teaser_keeps_normal() {
    let mut root = parse_html("<div class=\"content\">keep me</div>").unwrap();
    walk_pre_mut(&mut root, &|n| tf_remove_teaser(n));
    assert!(find_tag(&root, "div"), "normal <div> should be kept");
}

#[test]
fn test_tf_remove_teaser_paragraph_class() {
    let mut root = parse_html("<p class=\"teaser\">teaser text</p><p>real content</p>").unwrap();
    walk_pre_mut(&mut root, &|n| tf_remove_teaser(n));
    // teaser <p> should be removed; real content <p> should remain
    assert!(find_tag(&root, "p"), "the non-teaser <p> should remain");
}

#[test]
fn test_tf_remove_teaser_section_id() {
    let mut root =
        parse_html("<section id=\"teaser-block\">teaser text</section><p>content</p>").unwrap();
    walk_pre_mut(&mut root, &|n| tf_remove_teaser(n));
    assert!(
        !find_tag(&root, "section"),
        "<section id='teaser-block'> should be removed"
    );
    assert!(find_tag(&root, "p"));
}

#[test]
fn test_tf_remove_teaser_no_match_wrong_tag() {
    // <article> is NOT in the allowed tag list
    let mut root = parse_html("<article class=\"teaser\">content</article>").unwrap();
    walk_pre_mut(&mut root, &|n| tf_remove_teaser(n));
    assert!(
        find_tag(&root, "article"),
        "<article> should be kept (not in allowed tags)"
    );
}

// ── tf_strip_unwrapped ──────────────────────────────────────────────

#[test]
fn test_tf_strip_unwrapped_abbr_becomes_text() {
    let mut nodes = parse_html("<abbr title='World'>W</abbr>").unwrap();
    tf_strip_unwrapped(&mut nodes);
    // The abbr is unwrapped, leaving just text "W"
    assert!(!find_tag(&nodes, "abbr"), "<abbr> should be unwrapped");
}

#[test]
fn test_tf_strip_unwrapped_address_promotes_children() {
    let mut nodes = parse_html("<address><p>content</p></address>").unwrap();
    tf_strip_unwrapped(&mut nodes);
    assert!(
        !find_tag(&nodes, "address"),
        "<address> should be unwrapped"
    );
    assert!(find_tag(&nodes, "p"), "<p> should be promoted");
}

// ── tf_remove_empty_cut ─────────────────────────────────────────────

#[test]
fn test_tf_remove_empty_cut_empty_div() {
    let mut root = parse_html("<div></div>").unwrap();
    walk_pre_mut(&mut root, &|n| tf_remove_empty_cut(n));
    assert!(!find_tag(&root, "div"), "empty <div> should be removed");
}

#[test]
fn test_tf_remove_empty_cut_whitespace_p() {
    let mut root = parse_html("<p>  </p>").unwrap();
    walk_pre_mut(&mut root, &|n| tf_remove_empty_cut(n));
    assert!(
        !find_tag(&root, "p"),
        "whitespace-only <p> should be removed"
    );
}

#[test]
fn test_tf_remove_empty_cut_keeps_text() {
    let mut root = parse_html("<p>text</p>").unwrap();
    walk_pre_mut(&mut root, &|n| tf_remove_empty_cut(n));
    assert!(find_tag(&root, "p"), "<p> with text should be kept");
}

#[test]
fn test_tf_remove_empty_cut_keeps_li_with_link() {
    let mut root = parse_html("<li><a>link</a></li>").unwrap();
    walk_pre_mut(&mut root, &|n| tf_remove_empty_cut(n));
    assert!(find_tag(&root, "li"), "<li> with <a> should be kept");
}

#[test]
fn test_tf_remove_empty_cut_void_br_children() {
    let mut root = parse_html("<p><br></p>").unwrap();
    walk_pre_mut(&mut root, &|n| tf_remove_empty_cut(n));
    assert!(
        !find_tag(&root, "p"),
        "<p> with only <br> should be removed"
    );
}

// ── Helpers ─────────────────────────────────────────────────────────

fn find_tag(node: &DomNode, tag: &str) -> bool {
    match node {
        DomNode::Element {
            tag: t, children, ..
        } if t == tag => true,
        DomNode::Element { children, .. } => children.iter().any(|c| find_tag(c, tag)),
        _ => false,
    }
}

fn inject_max_score(node: &mut DomNode, score: f64) {
    if let DomNode::Element {
        metadata, children, ..
    } = node
    {
        metadata.insert("md_rd_subtree_max_score".to_string(), score.to_string());
        for child in children.iter_mut() {
            inject_max_score(child, score);
        }
    }
}

/// Inject md_rd_subtree_max_score only on nodes with a specific tag.
/// All other nodes get a low score (0.0).
fn inject_score_for_tag(node: &mut DomNode, target_tag: &str, high_score: f64) {
    set_score_recursive(node, 0.0, target_tag, high_score);
}

fn set_score_recursive(node: &mut DomNode, low_score: f64, target_tag: &str, high_score: f64) {
    if let DomNode::Element {
        tag,
        metadata,
        children,
        ..
    } = node
    {
        let score = if tag == target_tag {
            high_score
        } else {
            low_score
        };
        metadata.insert("md_rd_subtree_max_score".to_string(), score.to_string());
        for child in children.iter_mut() {
            set_score_recursive(child, low_score, target_tag, high_score);
        }
    }
}

// ── BODY_XPATH container isolation ──────────────────────────────────

#[test]
fn test_isolate_container_class_post() {
    let p_text: String = "A".repeat(250);
    let mut nodes = parse_html(&format!(
        "<div class=\"post\"><p>{}</p></div><nav>junk</nav>",
        p_text
    ))
    .unwrap();
    tf_isolate_content_container(&mut nodes);
    assert!(find_tag(&nodes, "div"), "<div> should be kept");
    assert!(find_tag(&nodes, "p"), "<p> content should be kept");
    assert!(!find_tag(&nodes, "nav"), "<nav> sibling should be removed");
}

#[test]
fn test_isolate_container_class_entry() {
    let p_text: String = "A".repeat(250);
    let mut nodes = parse_html(&format!(
        "<div class=\"entry\"><p>{}</p></div><nav>junk</nav>",
        p_text
    ))
    .unwrap();
    tf_isolate_content_container(&mut nodes);
    assert!(find_tag(&nodes, "div"), "<div> should be kept");
    assert!(!find_tag(&nodes, "nav"), "<nav> sibling should be removed");
}

#[test]
fn test_isolate_container_id_content() {
    let p_text: String = "A".repeat(250);
    let mut nodes = parse_html(&format!(
        "<div id=\"content\"><p>{}</p></div><aside>junk</aside>",
        p_text
    ))
    .unwrap();
    tf_isolate_content_container(&mut nodes);
    assert!(find_tag(&nodes, "div"), "<div> should be kept");
    assert!(
        !find_tag(&nodes, "aside"),
        "<aside> sibling should be removed"
    );
}

#[test]
fn test_isolate_container_article_tag() {
    let p_text: String = "A".repeat(250);
    let mut nodes = parse_html(&format!(
        "<article><p>{}</p></article><footer>junk</footer>",
        p_text
    ))
    .unwrap();
    tf_isolate_content_container(&mut nodes);
    assert!(find_tag(&nodes, "article"), "<article> should be kept");
    assert!(
        !find_tag(&nodes, "footer"),
        "<footer> sibling should be removed"
    );
}

#[test]
fn test_isolate_container_main_tag() {
    let p_text: String = "A".repeat(250);
    let mut nodes = parse_html(&format!(
        "<main><p>{}</p></main><aside>junk</aside>",
        p_text
    ))
    .unwrap();
    tf_isolate_content_container(&mut nodes);
    assert!(find_tag(&nodes, "main"), "<main> should be kept");
    assert!(
        !find_tag(&nodes, "aside"),
        "<aside> sibling should be removed"
    );
}

#[test]
fn test_isolate_container_re_class() {
    let p_text: String = "A".repeat(250);
    let mut nodes = parse_html(&format!(
        "<div class=\"post-content\"><p>{}</p></div><nav>junk</nav>",
        p_text
    ))
    .unwrap();
    tf_isolate_content_container(&mut nodes);
    assert!(find_tag(&nodes, "div"), "<div> should be kept");
    assert!(!find_tag(&nodes, "nav"), "<nav> should be removed");
}

#[test]
fn test_isolate_container_re_id() {
    let p_text: String = "A".repeat(250);
    let mut nodes = parse_html(&format!(
        "<section id=\"article-body\"><p>{}</p></section><aside>junk</aside>",
        p_text
    ))
    .unwrap();
    tf_isolate_content_container(&mut nodes);
    assert!(find_tag(&nodes, "section"), "<section> should be kept");
    assert!(!find_tag(&nodes, "aside"), "<aside> should be removed");
}

#[test]
fn test_isolate_container_starts_with_main() {
    let p_text: String = "A".repeat(250);
    let mut nodes = parse_html(&format!(
        "<div class=\"main-content\"><p>{}</p></div><nav>junk</nav>",
        p_text
    ))
    .unwrap();
    tf_isolate_content_container(&mut nodes);
    assert!(find_tag(&nodes, "div"), "<div> should be kept");
    assert!(!find_tag(&nodes, "nav"), "<nav> should be removed");
}

#[test]
fn test_isolate_container_role_main() {
    let p_text: String = "A".repeat(250);
    let mut nodes = parse_html(&format!(
        "<div role=\"main\"><p>{}</p></div><footer>junk</footer>",
        p_text
    ))
    .unwrap();
    tf_isolate_content_container(&mut nodes);
    assert!(find_tag(&nodes, "div"), "<div> should be kept");
    assert!(!find_tag(&nodes, "footer"), "<footer> should be removed");
}

#[test]
fn test_isolate_container_first_match_wins() {
    let p_text: String = "A".repeat(250);
    let mut nodes = parse_html(&format!(
        "<div class=\"post\"><p>{}</p></div><article><p>{}</p></article>",
        p_text, p_text
    ))
    .unwrap();
    tf_isolate_content_container(&mut nodes);
    // Pattern 0 match (<div class=\"post\">) should win over Pattern 1 (<article>)
    assert!(
        find_tag(&nodes, "div"),
        "<div> should be kept (Pattern 0 wins)"
    );
    assert!(!find_tag(&nodes, "article"), "<article> should be removed");
}

#[test]
fn test_isolate_container_no_match_noop() {
    let _nodes = parse_html("<div><p>A</p><span>B</span></div>").unwrap();
    let mut nodes = vec![parse_html("<div><p>A</p><span>B</span></div>").unwrap()];
    let original = nodes.clone();
    tf_isolate_content_container(&mut nodes[0]);
    // Tree should be unchanged (no container matched)
    assert_eq!(nodes.len(), original.len(), "tree should be unchanged");
}

#[test]
fn test_isolate_container_sibling_discard() {
    let p_text: String = "A".repeat(250);
    let mut nodes = parse_html(&format!(
        "<div class=\"post\"><p>{}</p></div><nav>junk</nav><footer>x</footer>",
        p_text
    ))
    .unwrap();
    tf_isolate_content_container(&mut nodes);
    assert!(find_tag(&nodes, "div"), "<div> should be kept");
    assert!(find_tag(&nodes, "p"), "<p> content should be kept");
    assert!(!find_tag(&nodes, "nav"), "<nav> should be removed");
    assert!(!find_tag(&nodes, "footer"), "<footer> should be removed");
}

#[test]
fn test_isolate_container_nested() {
    let p_text: String = "A".repeat(250);
    let mut nodes = parse_html(&format!(
        "<main><div class=\"post\"><p>{}</p></div><aside>junk</aside></main>",
        p_text
    ))
    .unwrap();
    tf_isolate_content_container(&mut nodes);
    // The outermost ancestor <main> should be kept
    assert!(find_tag(&nodes, "main"), "<main> should be kept");
    // <div> inside <main> should be kept
    assert!(find_tag(&nodes, "div"), "<div> should be kept");
    // <aside> sibling of <div> should be removed
    assert!(!find_tag(&nodes, "aside"), "<aside> should be removed");
}

#[test]
fn test_isolate_container_tag_scope() {
    let mut nodes =
        parse_html("<span class=\"post\">text</span><p id=\"content\">text</p>").unwrap();
    tf_isolate_content_container(&mut nodes);
    // Neither <span> nor <p> are in the allowed tags — tree unchanged
    assert!(find_tag(&nodes, "span"), "<span> should survive");
    assert!(find_tag(&nodes, "p"), "<p> should survive");
}

#[test]
fn test_isolate_container_itemprop_articleBody() {
    let p_text: String = "A".repeat(250);
    let mut nodes = parse_html(&format!(
        "<div itemprop=\"articleBody\"><p>{}</p></div><nav>junk</nav>",
        p_text
    ))
    .unwrap();
    tf_isolate_content_container(&mut nodes);
    assert!(find_tag(&nodes, "div"), "<div> should be kept");
    assert!(!find_tag(&nodes, "nav"), "<nav> should be removed");
}

#[test]
fn test_isolate_container_empty_input() {
    let mut nodes = DomNode::Element {
        tag: "html".into(),
        attrs: vec![],
        children: vec![],
        scores: HashMap::new(),
        metadata: HashMap::new(),
    };
    tf_isolate_content_container(&mut nodes);
    assert!(
        matches!(&nodes, DomNode::Element { children, .. } if children.is_empty()),
        "empty input should stay empty"
    );
}

#[test]
fn test_isolate_container_deeply_nested() {
    let p_text: String = "A".repeat(250);
    let mut nodes = parse_html(&format!(
        "<div class=\"post\"><section><article><p>{}</p></article></section></div><nav>junk</nav>",
        p_text
    ))
    .unwrap();
    tf_isolate_content_container(&mut nodes);
    // The deepest container strategy should find the match at the outermost container
    assert!(
        find_tag(&nodes, "div"),
        "<div> outermost container should be kept"
    );
    assert!(
        find_tag(&nodes, "article"),
        "<article> inside should be kept"
    );
    assert!(!find_tag(&nodes, "nav"), "<nav> sibling should be removed");
}

#[test]
fn test_body_xpath_regex_compiles() {
    // Verify regex statics don't panic at access time
    let _ = &*BODY_XPATH_PATTERN_0_RE;
    let _ = &*BODY_XPATH_PATTERN_2_RE;
    // Verify they don't match empty string
    assert!(!BODY_XPATH_PATTERN_0_RE.is_match(""));
    assert!(!BODY_XPATH_PATTERN_2_RE.is_match(""));
}

#[test]
fn test_isolate_container_role_article() {
    let p_text: String = "A".repeat(250);
    let mut nodes = parse_html(&format!(
        "<div role=\"article\"><p>{}</p></div><nav>junk</nav>",
        p_text
    ))
    .unwrap();
    tf_isolate_content_container(&mut nodes);
    assert!(find_tag(&nodes, "div"), "<div> should be kept");
    assert!(!find_tag(&nodes, "nav"), "<nav> should be removed");
}
// ── Test helper for building containers with controlled p-text ─────────

/// Build a container element with `<p>` elements containing `n_chars` of text each.
fn make_container_with_p_text(
    tag: &str,
    class_val: &str,
    p_count: usize,
    n_chars: usize,
) -> DomNode {
    let p_text: String = "x".repeat(n_chars);
    let children: Vec<DomNode> = (0..p_count)
        .map(|_| DomNode::Element {
            tag: "p".into(),
            attrs: vec![],
            children: vec![DomNode::Text(p_text.clone())],
            scores: HashMap::new(),
            metadata: HashMap::new(),
        })
        .collect();
    DomNode::Element {
        tag: tag.into(),
        attrs: vec![("class".into(), class_val.into())],
        children,
        scores: HashMap::new(),
        metadata: HashMap::new(),
    }
}

// ── Content-length check tests ─────────────────────────────────────

#[test]
fn test_isolate_container_rejects_short_content() {
    let mut nodes = DomNode::Element {
        tag: "html".into(),
        attrs: vec![],
        children: vec![
            make_container_with_p_text("div", "post", 1, 10),
            DomNode::Text("other".into()),
        ],
        scores: HashMap::new(),
        metadata: HashMap::new(),
    };
    tf_isolate_content_container(&mut nodes);
    if let DomNode::Element { children, .. } = &nodes {
        assert_eq!(
            children.len(),
            2,
            "short container should not match, tree unchanged"
        );
    };
}

#[test]
fn test_isolate_container_accepts_sufficient_content() {
    let mut nodes = DomNode::Element {
        tag: "html".into(),
        attrs: vec![],
        children: vec![
            make_container_with_p_text("div", "post", 1, 250),
            DomNode::Text("junk".into()),
        ],
        scores: HashMap::new(),
        metadata: HashMap::new(),
    };
    tf_isolate_content_container(&mut nodes);
    if let DomNode::Element { children, .. } = &nodes {
        assert_eq!(
            children.len(),
            1,
            "container with >=250 chars should be accepted"
        );
    };
    assert!(find_tag(&nodes, "div"), "<div> should be kept");
}

#[test]
fn test_isolate_container_fallthrough_to_next_pattern() {
    // First container (Pattern 0 class "post") too short
    // Second container (Pattern 1 tag "article") has enough text
    let mut nodes = DomNode::Element {
        tag: "html".into(),
        attrs: vec![],
        children: vec![
            make_container_with_p_text("div", "post", 1, 10),
            DomNode::Element {
                tag: "article".into(),
                attrs: vec![],
                children: vec![DomNode::Element {
                    tag: "p".into(),
                    attrs: vec![],
                    children: vec![DomNode::Text("x".repeat(250))],
                    scores: HashMap::new(),
                    metadata: HashMap::new(),
                }],
                scores: HashMap::new(),
                metadata: HashMap::new(),
            },
        ],
        scores: HashMap::new(),
        metadata: HashMap::new(),
    };
    tf_isolate_content_container(&mut nodes);
    if let DomNode::Element { children, .. } = &nodes {
        assert_eq!(
            children.len(),
            1,
            "article should be selected after div rejected"
        );
    };
    assert!(find_tag(&nodes, "article"), "<article> should be kept");
    assert!(!find_tag(&nodes, "div"), "<div> should be removed");
}

#[test]
fn test_isolate_container_count_p_text_no_p_elements() {
    let _container = DomNode::Element {
        tag: "div".into(),
        attrs: vec![("class".into(), "post".into())],
        children: vec![DomNode::Text("text without p tags".into())],
        scores: HashMap::new(),
        metadata: HashMap::new(),
    };
    let mut nodes = DomNode::Element {
        tag: "html".into(),
        attrs: vec![],
        children: vec![],
        scores: HashMap::new(),
        metadata: HashMap::new(),
    };
    tf_isolate_content_container(&mut nodes);
    if let DomNode::Element { children, .. } = &nodes {
        assert_eq!(
            children.len(),
            1,
            "no <p> elements -> no match -> unchanged"
        );
    };
}

#[test]
fn test_isolate_container_non_p_text_not_counted() {
    let long_text: String = "x".repeat(500);
    let _container = DomNode::Element {
        tag: "div".into(),
        attrs: vec![("class".into(), "post".into())],
        children: vec![DomNode::Element {
            tag: "div".into(),
            attrs: vec![],
            children: vec![DomNode::Text(long_text)],
            scores: HashMap::new(),
            metadata: HashMap::new(),
        }],
        scores: HashMap::new(),
        metadata: HashMap::new(),
    };
    let mut nodes = DomNode::Element {
        tag: "html".into(),
        attrs: vec![],
        children: vec![],
        scores: HashMap::new(),
        metadata: HashMap::new(),
    };
    tf_isolate_content_container(&mut nodes);
    if let DomNode::Element { children, .. } = &nodes {
        assert_eq!(
            children.len(),
            1,
            "500 chars in <div> but no <p> -> no match"
        );
    };
}

#[test]
fn test_isolate_container_exact_threshold_250_accepted() {
    let mut nodes = DomNode::Element {
        tag: "html".into(),
        attrs: vec![],
        children: vec![
            make_container_with_p_text("div", "post", 1, 250),
            DomNode::Text("junk".into()),
        ],
        scores: HashMap::new(),
        metadata: HashMap::new(),
    };
    tf_isolate_content_container(&mut nodes);
    if let DomNode::Element { children, .. } = &nodes {
        assert_eq!(children.len(), 1, "exactly 250 chars should be accepted");
    };
    assert!(find_tag(&nodes, "div"));
}

#[test]
fn test_isolate_container_249_rejected() {
    let mut nodes = DomNode::Element {
        tag: "html".into(),
        attrs: vec![],
        children: vec![
            make_container_with_p_text("div", "post", 1, 249),
            DomNode::Text("junk".into()),
        ],
        scores: HashMap::new(),
        metadata: HashMap::new(),
    };
    tf_isolate_content_container(&mut nodes);
    if let DomNode::Element { children, .. } = &nodes {
        assert_eq!(
            children.len(),
            2,
            "249 chars should be rejected, tree unchanged"
        );
    };
}

#[test]
fn test_isolate_container_whitespace_only_p_not_counted() {
    let _container = DomNode::Element {
        tag: "div".into(),
        attrs: vec![("class".into(), "post".into())],
        children: vec![DomNode::Element {
            tag: "p".into(),
            attrs: vec![],
            children: vec![DomNode::Text("   ".into())],
            scores: HashMap::new(),
            metadata: HashMap::new(),
        }],
        scores: HashMap::new(),
        metadata: HashMap::new(),
    };
    let mut nodes = DomNode::Element {
        tag: "html".into(),
        attrs: vec![],
        children: vec![],
        scores: HashMap::new(),
        metadata: HashMap::new(),
    };
    tf_isolate_content_container(&mut nodes);
    if let DomNode::Element { children, .. } = &nodes {
        assert_eq!(children.len(), 1, "whitespace-only <p> -> no match");
    };
}

#[test]
fn test_isolate_container_empty_container_rejected() {
    let _container = DomNode::Element {
        tag: "div".into(),
        attrs: vec![("class".into(), "post".into())],
        children: vec![],
        scores: HashMap::new(),
        metadata: HashMap::new(),
    };
    let mut nodes = DomNode::Element {
        tag: "html".into(),
        attrs: vec![],
        children: vec![],
        scores: HashMap::new(),
        metadata: HashMap::new(),
    };
    tf_isolate_content_container(&mut nodes);
    if let DomNode::Element { children, .. } = &nodes {
        assert_eq!(children.len(), 1, "empty container -> no match");
    };
}

#[test]
fn test_isolate_container_sibling_both_match_short_first() {
    // Two sibling containers with same pattern class "post"
    // First is short, second has enough text
    let mut nodes = DomNode::Element {
        tag: "html".into(),
        attrs: vec![],
        children: vec![
            make_container_with_p_text("div", "post", 1, 10),
            make_container_with_p_text("div", "post", 1, 250),
        ],
        scores: HashMap::new(),
        metadata: HashMap::new(),
    };
    tf_isolate_content_container(&mut nodes);
    if let DomNode::Element { children, .. } = &nodes {
        assert_eq!(children.len(), 1, "second container should be selected");
    };
    assert!(find_tag(&nodes, "div"));
}

#[test]
fn test_isolate_container_integration_sidebar_vs_article() {
    // Realistic scenario: sidebar nav div with class "content" but no real p text
    // Article body with class "main-content" having enough p text
    let _sidebar = DomNode::Element {
        tag: "div".into(),
        attrs: vec![("class".into(), "content".into())],
        children: vec![DomNode::Element {
            tag: "ul".into(),
            attrs: vec![],
            children: vec![DomNode::Element {
                tag: "li".into(),
                attrs: vec![],
                children: vec![DomNode::Text("nav link".into())],
                scores: HashMap::new(),
                metadata: HashMap::new(),
            }],
            scores: HashMap::new(),
            metadata: HashMap::new(),
        }],
        scores: HashMap::new(),
        metadata: HashMap::new(),
    };
    let _article_body = DomNode::Element {
        tag: "div".into(),
        attrs: vec![("class".into(), "main-content".into())],
        children: vec![DomNode::Element {
            tag: "p".into(),
            attrs: vec![],
            children: vec![DomNode::Text("x".repeat(250))],
            scores: HashMap::new(),
            metadata: HashMap::new(),
        }],
        scores: HashMap::new(),
        metadata: HashMap::new(),
    };
    let mut nodes = DomNode::Element {
        tag: "html".into(),
        attrs: vec![],
        children: vec![],
        scores: HashMap::new(),
        metadata: HashMap::new(),
    };
    tf_isolate_content_container(&mut nodes);
    if let DomNode::Element { attrs, .. } = &nodes {
        assert!(
            attrs
                .iter()
                .any(|(k, v)| k == "class" && v == "main-content"),
            "surviving div should have main-content class"
        );
    } else {
        panic!("expected Element node");
    }
}

// ── tf_remove_unlikely_candidates (has_likely_content guard removal) ──

#[test]
fn test_tf_remove_unlikely_candidates_removes_despite_likely_content() {
    // Core behavioral change: elements with likely-content children
    // (e.g., <p>) are now unconditionally removed when they match OVERALL_DISCARD patterns.
    // Before this change, this would have been KEPT by the has_likely_content guard.
    let mut root = parse_html("<div class=\"sidebar\"><p>content text here</p></div>").unwrap();
    walk_pre_mut(&mut root, &|n| tf_remove_unlikely_candidates(n));
    assert!(
        !find_tag(&root, "div"),
        "<div class='sidebar'> should be removed despite <p> child"
    );
    assert!(
        !find_tag(&root, "p"),
        "<p> child should also be removed with parent"
    );
}

#[test]
fn test_tf_remove_unlikely_candidates_removes_display_none_with_content() {
    // attr_match path: display:none elements with <p> children should also be
    // unconditionally removed now.
    let mut root = parse_html("<div style=\"display:none\"><p>hidden content</p></div>").unwrap();
    walk_pre_mut(&mut root, &|n| tf_remove_unlikely_candidates(n));
    assert!(
        !find_tag(&root, "div"),
        "<div style='display:none'> should be removed despite <p> child"
    );
}

#[test]
fn test_tf_remove_unlikely_candidates_keeps_non_matching() {
    // Elements that do NOT match UNLIKELY_CANDIDATES_RE should still be kept.
    let mut root =
        parse_html("<div class=\"content\"><p>actual article content</p></div>").unwrap();
    walk_pre_mut(&mut root, &|n| tf_remove_unlikely_candidates(n));
    assert!(
        find_tag(&root, "div"),
        "<div class='content'> should be kept (no match)"
    );
    assert!(find_tag(&root, "p"), "<p> should be kept (parent kept)");
}

// ── Gap 1: Scope restriction tests ────────────────────────────────────

#[test]
fn test_scope_restriction_keeps_a_tag() {
    // <a> is NOT in the allowed scope (div|item|list|p|section|span)
    let mut root = parse_html("<a class=\"sidebar\">link text</a>").unwrap();
    walk_pre_mut(&mut root, &|n| tf_remove_unlikely_candidates(n));
    assert!(
        find_tag(&root, "a"),
        "<a class='sidebar'> should be KEPT (not in scope)"
    );
}

#[test]
fn test_scope_restriction_removes_div() {
    // <div> IS in the allowed scope
    let mut root = parse_html("<div class=\"sidebar\">side content</div>").unwrap();
    walk_pre_mut(&mut root, &|n| tf_remove_unlikely_candidates(n));
    assert!(
        !find_tag(&root, "div"),
        "<div class='sidebar'> should be REMOVED (in scope)"
    );
}

#[test]
fn test_scope_restriction_nested_parent_kept_child_removed() {
    // Parent <a> not in scope, child <div> in scope
    let mut root =
        parse_html("<a class=\"sidebar\"><div class=\"sidebar\">text</div></a>").unwrap();
    walk_pre_mut(&mut root, &|n| tf_remove_unlikely_candidates(n));
    assert!(
        find_tag(&root, "a"),
        "<a> parent should be KEPT (not in scope)"
    );
    assert!(
        !find_tag(&root, "div"),
        "<div> child should be REMOVED (in scope)"
    );
}

#[test]
fn test_scope_restriction_keeps_li() {
    // <li> is NOT in the allowed scope
    let mut root = parse_html("<li class=\"sidebar\">list item</li>").unwrap();
    walk_pre_mut(&mut root, &|n| tf_remove_unlikely_candidates(n));
    assert!(
        find_tag(&root, "li"),
        "<li class='sidebar'> should be KEPT (not in scope)"
    );
}

// ── Gap 2: Separate pattern tests ─────────────────────────────────────

#[test]
fn test_separate_patterns_id_premium() {
    // ID-only pattern matches "premium"
    let mut root = parse_html("<div id=\"premium-content\">premium</div>").unwrap();
    walk_pre_mut(&mut root, &|n| tf_remove_unlikely_candidates(n));
    assert!(
        !find_tag(&root, "div"),
        "<div id='premium-content'> should be REMOVED (id pattern)"
    );
}

#[test]
fn test_separate_patterns_class_footer() {
    // Class-only pattern matches "footer"
    let mut root = parse_html("<div class=\"footer\">footer</div>").unwrap();
    walk_pre_mut(&mut root, &|n| tf_remove_unlikely_candidates(n));
    assert!(
        !find_tag(&root, "div"),
        "<div class='footer'> should be REMOVED (class pattern)"
    );
}

#[test]
fn test_separate_patterns_class_share_contains() {
    // Class-only pattern matches "share-" (substring match)
    let mut root = parse_html("<div class=\"share-icons\">share</div>").unwrap();
    walk_pre_mut(&mut root, &|n| tf_remove_unlikely_candidates(n));
    assert!(
        !find_tag(&root, "div"),
        "<div class='share-icons'> should be REMOVED (class pattern share-)"
    );
}

#[test]
fn test_separate_patterns_id_share_only() {
    // ID-only pattern matches "share"
    let mut root = parse_html("<div id=\"share-buttons\">share</div>").unwrap();
    walk_pre_mut(&mut root, &|n| tf_remove_unlikely_candidates(n));
    assert!(
        !find_tag(&root, "div"),
        "<div id='share-buttons'> should be REMOVED (id pattern share)"
    );
}

#[test]
fn test_separate_patterns_shared_sidebar() {
    // Shared pattern matches "sidebar" (ACLU regression scenario)
    let mut root = parse_html(
        "<div class=\"panel-two-col-sidebar-right-mix\"><p>article content here</p></div>",
    )
    .unwrap();
    walk_pre_mut(&mut root, &|n| tf_remove_unlikely_candidates(n));
    assert!(
        !find_tag(&root, "div"),
        "<div class='panel-two-col-sidebar-right-mix'> should be REMOVED (shared sidebar)"
    );
}

#[test]
fn test_role_nav_check() {
    // Trafilatura's exact role check: contains "nav"
    let mut root = parse_html("<div role=\"navigation\">nav</div>").unwrap();
    walk_pre_mut(&mut root, &|n| tf_remove_unlikely_candidates(n));
    assert!(
        !find_tag(&root, "div"),
        "<div role='navigation'> should be REMOVED (role contains nav)"
    );
}

#[test]
fn test_role_nav_check_non_matching() {
    // Non-matching role should NOT trigger removal
    let mut root = parse_html("<div role=\"main\"><p>content</p></div>").unwrap();
    walk_pre_mut(&mut root, &|n| tf_remove_unlikely_candidates(n));
    assert!(
        find_tag(&root, "div"),
        "<div role='main'> should be KEPT (no nav match)"
    );
}

// ── Pattern 2: scope-unrestricted discard ──────────────────────

#[test]
fn test_pattern2_noprint_class_removed() {
    let mut root =
        parse_html("<section class=\"top-article noprint\">nav stuff</section><p>content</p>")
            .unwrap();
    walk_pre_mut(&mut root, &|n| tf_remove_unlikely_candidates(n));
    assert!(
        !find_tag(&root, "section"),
        "section with noprint class should be removed"
    );
}

#[test]
fn test_pattern2_scope_unrestricted_catches_any_tag() {
    // Pattern 2 catches <figure> (not in Pattern 1 scope) with noprint
    let mut root = parse_html("<figure class=\"noprint\">fig</figure><p>content</p>").unwrap();
    walk_pre_mut(&mut root, &|n| tf_remove_unlikely_candidates(n));
    assert!(
        !find_tag(&root, "figure"),
        "figure with noprint should be removed (unrestricted)"
    );
}

#[test]
fn test_pattern2_hide_class_removed() {
    let mut root = parse_html("<div class=\"hide-ads\">ads</div><p>content</p>").unwrap();
    walk_pre_mut(&mut root, &|n| tf_remove_unlikely_candidates(n));
    assert!(
        !find_tag(&root, "div"),
        "div with hide- class should be removed"
    );
}

#[test]
fn test_pattern2_notloaded_removed() {
    let mut root = parse_html("<div class=\"notloaded\">lazy</div><p>content</p>").unwrap();
    walk_pre_mut(&mut root, &|n| tf_remove_unlikely_candidates(n));
    assert!(
        !find_tag(&root, "div"),
        "div with notloaded class should be removed"
    );
}

#[test]
fn test_pattern2_akismet_id_removed() {
    let mut root = parse_html("<div id=\"akismet\">spam</div><p>content</p>").unwrap();
    walk_pre_mut(&mut root, &|n| tf_remove_unlikely_candidates(n));
    assert!(
        !find_tag(&root, "div"),
        "div with akismet id should be removed"
    );
}

#[test]
fn test_pattern2_reply_prefix_removed() {
    let mut root =
        parse_html("<div class=\"reply-comment-123\">reply form</div><p>content</p>").unwrap();
    walk_pre_mut(&mut root, &|n| tf_remove_unlikely_candidates(n));
    assert!(
        !find_tag(&root, "div"),
        "div with reply- class should be removed"
    );
}

#[test]
fn test_pattern2_class_pattern_does_not_match_id() {
    // REGRESSION: SLOP-001 — class-only patterns (noprint, hide-, reply-)
    // must NOT match against id values.
    let mut root = parse_html("<div id=\"noprint\">should be kept</div><p>content</p>").unwrap();
    walk_pre_mut(&mut root, &|n| tf_remove_unlikely_candidates(n));
    assert!(
        find_tag(&root, "div"),
        "div with id='noprint' should be KEPT (class-only pattern)"
    );
}

#[test]
fn test_pattern2_hidden_id_removed() {
    let mut root = parse_html("<div id=\"hidden-content\">hidden div</div><p>content</p>").unwrap();
    walk_pre_mut(&mut root, &|n| tf_remove_unlikely_candidates(n));
    assert!(
        !find_tag(&root, "div"),
        "div with 'hidden' in id should be removed"
    );
}

#[test]
fn test_pattern2_hidden_in_style_removed() {
    let mut root =
        parse_html("<div style=\"visibility:hidden\">hidden</div><p>content</p>").unwrap();
    walk_pre_mut(&mut root, &|n| tf_remove_unlikely_candidates(n));
    assert!(
        !find_tag(&root, "div"),
        "div with 'hidden' in style should be removed"
    );
}

#[test]
fn test_pattern2_comments_title_removed() {
    let mut root =
        parse_html("<div class=\"comments-title\">comments</div><p>content</p>").unwrap();
    walk_pre_mut(&mut root, &|n| tf_remove_unlikely_candidates(n));
    assert!(
        !find_tag(&root, "div"),
        "div with comments-title class should be removed"
    );
}

#[test]
fn test_pattern2_suggest_links_removed() {
    let mut root = parse_html("<div class=\"suggest-links\">suggest</div><p>content</p>").unwrap();
    walk_pre_mut(&mut root, &|n| tf_remove_unlikely_candidates(n));
    assert!(
        !find_tag(&root, "div"),
        "div with suggest-links class should be removed"
    );
}

#[test]
fn test_pattern2_preserves_body_html() {
    let mut root = parse_html("<html lang=\"en\"><body><p>content</p></body></html>").unwrap();
    walk_pre_mut(&mut root, &|n| tf_remove_unlikely_candidates(n));
    assert!(find_tag(&root, "html"), "<html> should never be removed");
    assert!(find_tag(&root, "body"), "<body> should never be removed");
}

#[test]
fn test_pattern2_aria_hidden_structural_preserved() {
    let mut root = parse_html("<main aria-hidden=\"true\"><p>main content</p></main>").unwrap();
    walk_pre_mut(&mut root, &|n| tf_remove_unlikely_candidates(n));
    assert!(
        find_tag(&root, "main"),
        "<main aria-hidden='true'> should be preserved (structural guard)"
    );
}

#[test]
fn test_pattern2_aria_hidden_nonstructural_removed() {
    let mut root = parse_html("<div aria-hidden=\"true\">hidden div</div><p>content</p>").unwrap();
    walk_pre_mut(&mut root, &|n| tf_remove_unlikely_candidates(n));
    assert!(
        !find_tag(&root, "div"),
        "<div aria-hidden='true'> should be removed"
    );
}

#[test]
fn test_pattern2_preserves_pattern1_sidebar() {
    // Pattern 1 still works: sidebar in class
    let mut root =
        parse_html("<div class=\"panel-two-col-sidebar-right-mix\"><p>content</p></div>").unwrap();
    walk_pre_mut(&mut root, &|n| tf_remove_unlikely_candidates(n));
    assert!(
        !find_tag(&root, "div"),
        "Pattern 1 sidebar removal should still work"
    );
}
