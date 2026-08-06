use super::*;
use crate::pipelines::parse_html;
use crate::pipelines::passes::tf_filters::tf_remove_cleaned;
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
fn test_tf_convert_quotes_pre_stays_pre() {
    // Block code is structurally `<pre>`: tf_convert_quotes no longer renames
    // pre -> code (that rename lost block-ness and forced backend hacks).
    let mut root = parse_html("<pre>code here</pre>").unwrap();
    walk_pre_mut_test(&mut root, &|n| tf_convert_quotes(n));
    assert!(find_tag(&root, "pre"), "pre stays pre (block code)");
    assert!(!find_tag(&root, "code"), "no spurious code element");
}

#[test]
fn test_tf_convert_quotes_pre_with_code_child_unwraps_and_hoists_language() {
    // Canonical pre>code.language-x shape: the code child is unwrapped and the
    // language is hoisted onto the pre's own class.
    let mut root = parse_html(
        r#"<pre class="not-prose"><code class="language-rust">fn main() {}</code></pre>"#,
    )
    .unwrap();
    walk_pre_mut_test(&mut root, &|n| tf_convert_quotes(n));
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
fn test_tf_convert_quotes_pre_own_class_language_wins() {
    // The pre's own language class takes precedence over a nested code's.
    let mut root = parse_html(
        r#"<pre class="language-python"><code class="language-rust">print('x')</code></pre>"#,
    )
    .unwrap();
    walk_pre_mut_test(&mut root, &|n| tf_convert_quotes(n));
    let pre = find_node_matching(&root, "pre").expect("pre exists");
    assert_eq!(get_attr(pre, "class"), Some("language-python"));
}

#[test]
fn test_tf_convert_quotes_inline_code_untouched() {
    // Inline <code> is left as-is.
    let mut root = parse_html("<p>run <code>cmd</code> now</p>").unwrap();
    walk_pre_mut_test(&mut root, &|n| tf_convert_quotes(n));
    assert!(find_tag(&root, "code"), "inline code stays code");
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
fn test_tf_convert_refs_drops_duplicate_target_attr() {
    // <a href="URL" target="_blank"> must convert to a single
    // <ref target="URL"> — a leftover target="_blank" would make
    // attr("target") return "_blank" (markdown link [text](_blank)).
    let mut root =
        parse_html(r#"<a href="/blog/post" target="_blank" rel="noopener">Post</a>"#).unwrap();
    walk_pre_mut_test(&mut root, &|n| tf_convert_refs_and_details(n));
    let ref_node = find_node_matching(&root, "ref").expect("should find <ref>");
    assert_eq!(get_attr(ref_node, "target"), Some("/blog/post"));
    let targets: Vec<&str> = match ref_node {
        crate::pipelines::DomNode::Element { attrs, .. } => attrs
            .iter()
            .filter(|(k, _)| k == "target")
            .map(|(_, v)| v.as_str())
            .collect(),
        _ => vec![],
    };
    assert_eq!(targets, vec!["/blog/post"], "exactly one target = the URL");
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

// ── tf_convert_figure_with_table ──────────────────────────────────────

#[test]
fn test_tf_convert_figure_with_table_converts_to_div() {
    // <figure> with a descendant <table> must become <div> so the table
    // survives tf_remove_cleaned (figure is in TF_CLEANED_TAGS).
    let mut root = parse_html(
        "<figure><div><div><table><thead><tr><th>A</th></tr></thead><tbody><tr><td>1</td></tr></tbody></table></div></div></figure>",
    )
    .unwrap();
    tf_convert_figure_with_table(&mut root);
    assert!(
        !find_tag(&root, "figure"),
        "<figure> with table should be renamed to <div>"
    );
    assert!(find_tag(&root, "div"), "renamed <div> should exist");
    assert!(find_tag(&root, "table"), "<table> should survive");
}

#[test]
fn test_tf_convert_figure_with_table_keeps_attributes() {
    // Python's `elem.tag = "div"` keeps attributes — we must too.
    let mut root = parse_html(
        r#"<figure class="w-full" data-x="y"><div><table><tr><td>1</td></tr></table></div></figure>"#,
    )
    .unwrap();
    tf_convert_figure_with_table(&mut root);
    let div = find_node_matching(&root, "div").expect("renamed <div> should exist");
    assert_eq!(get_attr(div, "class"), Some("w-full"), "class preserved");
    assert_eq!(get_attr(div, "data-x"), Some("y"), "data-x preserved");
}

#[test]
fn test_tf_convert_figure_without_table_still_removed_by_cleaning() {
    // <figure> WITHOUT a descendant table stays <figure> and is still removed
    // by tf_remove_cleaned (existing behavior must be preserved).
    let mut root = parse_html(
        "<figure><img src='x.png'></figure><figure><div><table><tr><td>1</td></tr></table></div></figure><p>keep</p>",
    )
    .unwrap();
    tf_convert_figure_with_table(&mut root);
    // Plain figure (image only) stays a figure at this point.
    let mut figures = 0;
    collect_tag_count(&root, "figure", &mut figures);
    assert_eq!(figures, 1, "figure-without-table must stay <figure>");
    // Then tf_remove_cleaned removes the remaining figure but keeps the div.
    walk_pre_mut_test(&mut root, &|n| tf_remove_cleaned(n));
    assert!(
        !find_tag(&root, "figure"),
        "plain <figure> removed by cleaning"
    );
    assert!(find_tag(&root, "table"), "figure-with-table table survives");
    assert!(find_tag(&root, "p"), "<p> should survive");
}

#[test]
fn test_tf_convert_figure_with_table_nested_figures_both_convert() {
    // figure > figure > table: ALL figures containing a table must convert.
    let mut root = parse_html(
        r#"<figure class="outer"><figure class="inner"><div><table><tr><td>1</td></tr></table></div></figure></figure>"#,
    )
    .unwrap();
    tf_convert_figure_with_table(&mut root);
    assert!(
        !find_tag(&root, "figure"),
        "both nested figures should convert to <div>"
    );
    assert!(find_tag(&root, "table"), "table should survive");
}

// ── tf_convert_accordion_to_details ───────────────────────────────────

#[test]
fn test_tf_convert_accordion_to_details_basic() {
    // div[button[aria-expanded] + content element] -> details + summary.
    let mut root = parse_html(
        r#"<div class="rounded-dropdown"><button class="w-full" aria-expanded="false"><span>Question?</span><span aria-hidden="true"><svg><path d="x"/></svg></span></button><div class="grid"><div><div>Answer text</div></div></div></div>"#,
    )
    .unwrap();
    walk_pre_mut_test(&mut root, &|n| tf_convert_accordion_to_details(n));
    assert!(
        find_tag(&root, "details"),
        "container should become <details>"
    );
    assert!(
        find_tag(&root, "summary"),
        "<summary> should replace button"
    );
    assert!(!find_tag(&root, "button"), "button should not remain");
    // The details' first child is the summary; the content sibling stays in
    // place as the details body (inner divs are NOT renamed).
    let details = find_node_matching(&root, "details").expect("<details> exists");
    if let DomNode::Element { children, .. } = details {
        assert_eq!(children.len(), 2, "details = summary + one content sibling");
        assert!(
            matches!(&children[0], DomNode::Element { tag, .. } if tag == "summary"),
            "first child must be <summary>"
        );
        assert!(
            matches!(&children[1], DomNode::Element { .. }),
            "second child must be the content panel"
        );
    }
    assert!(
        root.text_content().contains("Answer text"),
        "answer content must remain"
    );
}

#[test]
fn test_tf_convert_accordion_summary_text_excludes_icons() {
    // The <summary> must hold the button's TEXT CONTENT ONLY — svg/path/rect
    // and aria-hidden elements are dropped. The icon fixtures CARRY text
    // (<title> inside the svg, a glyph in an aria-hidden span) so this test
    // detects any leak: text_content() alone would include "Chevron down"
    // and the glyph.
    let mut root = parse_html(
        r#"<div><button aria-expanded="true"><span>Is SGLang faster than vLLM?</span><span aria-hidden="true"><svg><title>Chevron down</title><path d="M2 4l4 4"/><rect x="0"/></svg></span><span aria-hidden="true">▾</span></button><div><div>Yes.</div></div></div>"#,
    )
    .unwrap();
    walk_pre_mut_test(&mut root, &|n| tf_convert_accordion_to_details(n));
    let summary = find_node_matching(&root, "summary").expect("<summary> should exist");
    assert_eq!(
        summary.text_content(),
        "Is SGLang faster than vLLM?",
        "summary text must be the question text only (svg title/aria-hidden glyph must not leak)"
    );
    // The summary has no classes and no aria-* attributes.
    if let DomNode::Element { attrs, .. } = summary {
        assert!(
            attrs.iter().all(|(k, _)| !k.starts_with("aria-")),
            "aria-* attributes must be dropped"
        );
        assert!(
            attrs.iter().all(|(k, _)| *k != "class"),
            "classes must be dropped"
        );
    }
}

#[test]
fn test_tf_convert_accordion_button_without_aria_expanded_left_alone() {
    // A real button (no aria-expanded) as first child must NOT be converted.
    let mut root =
        parse_html("<div><button>Subscribe</button><div><p>content</p></div></div>").unwrap();
    walk_pre_mut_test(&mut root, &|n| tf_convert_accordion_to_details(n));
    assert!(find_tag(&root, "div"), "container must stay <div>");
    assert!(find_tag(&root, "button"), "real button must be left alone");
    assert!(!find_tag(&root, "details"), "no <details> should appear");
    assert!(!find_tag(&root, "summary"), "no <summary> should appear");
}

#[test]
fn test_tf_convert_accordion_button_without_content_sibling_left_alone() {
    // A lone button (e.g. a mobile-menu toggle) with NO following sibling
    // element must be left alone.
    let mut root =
        parse_html(r#"<div><button type="button" aria-expanded="false">Menu</button></div>"#)
            .unwrap();
    walk_pre_mut_test(&mut root, &|n| tf_convert_accordion_to_details(n));
    assert!(find_tag(&root, "div"), "container must stay <div>");
    assert!(find_tag(&root, "button"), "lone button must be left alone");
    assert!(!find_tag(&root, "details"));
    assert!(!find_tag(&root, "summary"));
}

#[test]
fn test_tf_convert_accordion_native_details_untouched() {
    // Native <details>/<summary> in the HTML are untouched by this pass.
    let mut root =
        parse_html("<details><summary>Native</summary><div>body</div></details>").unwrap();
    walk_pre_mut_test(&mut root, &|n| tf_convert_accordion_to_details(n));
    assert!(find_tag(&root, "details"), "native <details> untouched");
    assert!(find_tag(&root, "summary"), "native <summary> untouched");
    assert_eq!(
        find_node_matching(&root, "summary")
            .expect("summary exists")
            .text_content(),
        "Native"
    );
}

// ── SLOP-002: pretty-printed HTML (whitespace text / comment before button) ──

#[test]
fn test_tf_convert_accordion_pretty_printed_leading_whitespace_and_comment() {
    // Regression (SLOP-002): pretty-printed HTML inserts whitespace text nodes
    // (and possibly comments) before the button. Detection must locate the
    // first ELEMENT child by index, not the literal first DOM node, or the
    // pass silently no-ops and tf_remove_cleaned destroys the question text.
    let mut root = parse_html(
        "<div class=\"rounded-dropdown\">\n  <!-- FAQ item -->\n  <button class=\"w-full\" aria-expanded=\"false\"><span>Question pretty?</span></button>\n  <div class=\"grid\"><div>Answer text.</div></div>\n</div>",
    )
    .unwrap();
    walk_pre_mut_test(&mut root, &|n| tf_convert_accordion_to_details(n));
    let details = find_node_matching(&root, "details").expect("<details> should exist");
    assert!(
        matches!(&details, DomNode::Element { .. }),
        "container must become <details>"
    );
    let summary = find_node_matching(&root, "summary").expect("<summary> should exist");
    assert_eq!(
        summary.text_content(),
        "Question pretty?",
        "question text must survive to the summary despite leading whitespace/comment"
    );
    assert!(
        !find_tag(&root, "button"),
        "button must be replaced by <summary>"
    );
    assert!(
        root.text_content().contains("Answer text."),
        "answer content must remain"
    );
}

// ── SLOP-005: text-less / icon-only toggle buttons are left alone ─────────

#[test]
fn test_tf_convert_accordion_textless_toggle_button_left_alone() {
    // Regression (SLOP-005): a button with no visible text (empty, or
    // icon-only) must NOT be converted — it would produce an empty `### `
    // heading. The container stays a div and tf_remove_cleaned handles the
    // button as before.
    let mut root = parse_html(
        r#"<div><button type="button" aria-expanded="false"></button><div class="panel">panel content</div></div>"#,
    )
    .unwrap();
    walk_pre_mut_test(&mut root, &|n| tf_convert_accordion_to_details(n));
    assert!(
        !find_tag(&root, "details"),
        "no <details> for a text-less toggle"
    );
    assert!(
        !find_tag(&root, "summary"),
        "no <summary> for a text-less toggle"
    );
    assert!(
        find_tag(&root, "button"),
        "text-less button must be left alone"
    );

    // Icon-only button: the svg/aria-hidden icon carries the only content.
    let mut root = parse_html(
        r#"<div><button type="button" aria-expanded="false"><span aria-hidden="true"><svg><title>Chevron down</title><path d="M2 4l4 4"/></svg></span></button><div class="panel">panel content</div></div>"#,
    )
    .unwrap();
    walk_pre_mut_test(&mut root, &|n| tf_convert_accordion_to_details(n));
    assert!(
        !find_tag(&root, "details"),
        "no <details> for an icon-only toggle (svg title must not count as text)"
    );
    assert!(
        !find_tag(&root, "summary"),
        "no <summary> for an icon-only toggle"
    );
}

// ── SLOP-001: icon text nested in a NON-aria-hidden wrapper is still dropped ──

#[test]
fn test_tf_convert_accordion_drops_svg_text_even_without_aria_hidden_wrapper() {
    // The svg icon is dropped by TAG, not only by aria-hidden: an svg (with
    // real text content) nested inside a plain span must not leak into the
    // summary either.
    let mut root = parse_html(
        r#"<div><button aria-expanded="false"><span>Question svg?</span><span><svg><title>Chevron down</title><path d="M2 4l4 4"/></svg></span></button><div><div>Answer.</div></div></div>"#,
    )
    .unwrap();
    walk_pre_mut_test(&mut root, &|n| tf_convert_accordion_to_details(n));
    let summary = find_node_matching(&root, "summary").expect("<summary> should exist");
    assert_eq!(
        summary.text_content(),
        "Question svg?",
        "svg title text must not leak into the summary even without aria-hidden"
    );
}

// ── SLOP-102: aria-hidden="false" (explicitly VISIBLE) text must survive ───

#[test]
fn test_tf_convert_accordion_aria_hidden_false_text_survives() {
    // Regression (SLOP-102): collect_visible_text matched aria-hidden by KEY
    // presence, so aria-hidden="false" (explicitly VISIBLE per ARIA) text was
    // silently destroyed — the trimmed summary went empty and the conversion
    // was skipped, letting tf_remove_cleaned delete the question entirely.
    // Only aria-hidden="true" is hidden.
    let mut root = parse_html(
        r#"<div><button aria-expanded="false"><span aria-hidden="false">Real visible question?</span></button><div><div>Answer text.</div></div></div>"#,
    )
    .unwrap();
    walk_pre_mut_test(&mut root, &|n| tf_convert_accordion_to_details(n));
    let summary = find_node_matching(&root, "summary").expect(
        "aria-hidden=false text is VISIBLE — conversion must proceed and summary must exist",
    );
    assert_eq!(
        summary.text_content(),
        "Real visible question?",
        "aria-hidden=false text must survive into the summary"
    );
}

#[test]
fn test_tf_convert_accordion_aria_hidden_false_kept_in_summary() {
    // Regression (SLOP-102): an aria-hidden="false" span among the question
    // text (e.g. "(updated 2024)") must be kept in the summary, not dropped.
    let mut root = parse_html(
        r#"<div><button aria-expanded="false"><span>Question?</span><span aria-hidden="false">(updated 2024)</span></button><div><div>Answer text.</div></div></div>"#,
    )
    .unwrap();
    walk_pre_mut_test(&mut root, &|n| tf_convert_accordion_to_details(n));
    let summary = find_node_matching(&root, "summary").expect("<summary> should exist");
    let text = summary.text_content();
    assert!(
        text.contains("Question?"),
        "question text must survive, got: {text}"
    );
    assert!(
        text.contains("(updated 2024)"),
        "aria-hidden=false span must be kept in the summary, got: {text}"
    );
}

// ── R3a: summary -> head rend="h3" ────────────────────────────────────

#[test]
fn test_summary_to_head_gains_rend_h3() {
    let mut root = parse_html("<details><summary>Info</summary><p>text</p></details>").unwrap();
    walk_pre_mut_test(&mut root, &|n| tf_convert_refs_and_details(n));
    let head = find_head_with_rend(&root, "h3").expect(r#"summary should become <head rend="h3">"#);
    assert_eq!(get_attr(head, "rend"), Some("h3"));
    assert_eq!(head.text_content(), "Info");
}

#[test]
fn test_summary_to_head_keeps_existing_rend() {
    // A summary that already carries a rend keeps it (h3 is only the default).
    let mut root =
        parse_html(r#"<details><summary rend="h2">Info</summary><p>text</p></details>"#).unwrap();
    walk_pre_mut_test(&mut root, &|n| tf_convert_refs_and_details(n));
    let head = find_head_with_rend(&root, "h2").expect("existing rend must be kept");
    assert_eq!(get_attr(head, "rend"), Some("h2"));
}

// ── Helper: count tags in a tree ──────────────────────────────────────

fn collect_tag_count(node: &DomNode, tag: &str, count: &mut usize) {
    if let DomNode::Element {
        tag: t, children, ..
    } = node
    {
        if t == tag {
            *count += 1;
        }
        for child in children {
            collect_tag_count(child, tag, count);
        }
    }
}
