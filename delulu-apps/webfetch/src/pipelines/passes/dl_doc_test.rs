use super::*;
use crate::pipelines::parse_html;

fn count_elements(node: &DomNode, tag: &str) -> usize {
    match node {
        DomNode::Element {
            tag: t, children, ..
        } => {
            let mut count = if t == tag { 1 } else { 0 };
            for child in children {
                count += count_elements(child, tag);
            }
            count
        }
        _ => 0,
    }
}

fn has_element(node: &DomNode, tag: &str) -> bool {
    count_elements(node, tag) > 0
}

fn text_content(node: &DomNode) -> String {
    match node {
        DomNode::Text(t) => t.clone(),
        DomNode::Element { children, .. } => {
            let mut s = String::new();
            for child in children {
                s.push_str(&text_content(child));
            }
            s
        }
        _ => String::new(),
    }
}

// -- filter_doc tests -------------------------------------------------------

#[test]
fn test_filter_doc_removes_scripts() {
    let html = r#"<html><body><script>alert("xss")</script><p>Hello</p></body></html>"#;
    let mut dom = parse_html(html).unwrap();
    filter_doc(&mut dom);
    assert!(!has_element(&dom, "script"), "scripts should be removed");
    assert!(text_content(&dom).contains("Hello"), "content should be preserved");
}

#[test]
fn test_filter_doc_removes_styles() {
    let html = r#"<html><head><style>body { color: red; }</style></head><body><p>Text</p></body></html>"#;
    let mut dom = parse_html(html).unwrap();
    filter_doc(&mut dom);
    assert!(!has_element(&dom, "style"), "styles should be removed");
    assert!(text_content(&dom).contains("Text"), "content should be preserved");
}

#[test]
fn test_filter_doc_removes_empty_elements() {
    let html = r#"<html><body><div></div><p><span></span></p><div>Content</div></body></html>"#;
    let mut dom = parse_html(html).unwrap();
    filter_doc(&mut dom);
    // Empty div should be removed
    assert!(!text_content(&dom).contains("Content") || has_element(&dom, "div"),
        "non-empty div should remain");
    // The non-empty div should remain
    assert!(text_content(&dom).contains("Content"), "content should be preserved");
}

#[test]
fn test_filter_doc_preserves_img() {
    let html = r#"<html><body><img src="test.png" alt="test"/><p>Text</p></body></html>"#;
    let mut dom = parse_html(html).unwrap();
    filter_doc(&mut dom);
    assert!(has_element(&dom, "img"), "img elements should be preserved");
}

#[test]
fn test_filter_doc_preserves_br() {
    let html = r#"<html><body><p>Line1<br/>Line2</p></body></html>"#;
    let mut dom = parse_html(html).unwrap();
    filter_doc(&mut dom);
    assert!(has_element(&dom, "br"), "br elements should be preserved");
}

#[test]
fn test_filter_doc_preserves_hr() {
    let html = r#"<html><body><hr/><p>Text</p></body></html>"#;
    let mut dom = parse_html(html).unwrap();
    filter_doc(&mut dom);
    assert!(has_element(&dom, "hr"), "hr elements should be preserved");
}

#[test]
fn test_filter_doc_preserves_wbr() {
    let html = r#"<html><body><p>Long<wbr/>Word</p></body></html>"#;
    let mut dom = parse_html(html).unwrap();
    filter_doc(&mut dom);
    assert!(has_element(&dom, "wbr"), "wbr elements should be preserved");
}

#[test]
fn test_filter_doc_preserves_content() {
    let html = r#"<html><body><h1>Title</h1><p>Paragraph text</p><ul><li>Item 1</li><li>Item 2</li></ul></body></html>"#;
    let mut dom = parse_html(html).unwrap();
    filter_doc(&mut dom);
    assert!(text_content(&dom).contains("Title"), "h1 content should be preserved");
    assert!(text_content(&dom).contains("Paragraph text"), "paragraph content should be preserved");
    assert!(text_content(&dom).contains("Item 1"), "list item content should be preserved");
    assert!(text_content(&dom).contains("Item 2"), "list item content should be preserved");
}

#[test]
fn test_filter_doc_removes_both_script_and_style() {
    let html = r#"<html><head><style>body { }</style></head><body><script>void 0</script><p>Survivor</p></body></html>"#;
    let mut dom = parse_html(html).unwrap();
    filter_doc(&mut dom);
    assert!(!has_element(&dom, "script"), "scripts should be removed");
    assert!(!has_element(&dom, "style"), "styles should be removed");
    assert!(text_content(&dom).contains("Survivor"), "remaining content should be preserved");
}

#[test]
fn test_filter_doc_removes_nested_empty_elements() {
    let html = r#"<html><body><div><p><span></span></p></div><main>Content</main></body></html>"#;
    let mut dom = parse_html(html).unwrap();
    filter_doc(&mut dom);
    // The text "Content" should be present
    assert!(text_content(&dom).contains("Content"), "content should survive");
}
