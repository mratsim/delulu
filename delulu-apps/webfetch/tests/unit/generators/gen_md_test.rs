use super::*;
use crate::core::types::MarkdownDocument;

// ── Heading ─────────────────────────────────────────────────────────

#[test]
fn test_lower_heading() {
    let nodes = [DomNode::Element {
        tag: "h1".into(),
        attrs: vec![],
        children: vec![DomNode::Text("Title".into())],
        scores: std::collections::HashMap::new(),
        metadata: std::collections::HashMap::new(),
    }];
    let md = MarkdownLowerer::lower(&nodes[0], None);
    assert!(md.contains("# Title"));
}

// ── Link ────────────────────────────────────────────────────────────

#[test]
fn test_lower_link() {
    let nodes = [DomNode::Element {
        tag: "a".into(),
        attrs: vec![("href".into(), "https://example.com".into())],
        children: vec![DomNode::Text("link".into())],
        scores: std::collections::HashMap::new(),
        metadata: std::collections::HashMap::new(),
    }];
    let md = MarkdownLowerer::lower(&nodes[0], None);
    assert!(md.contains("[link](https://example.com)"));
}

// ── Unordered list ──────────────────────────────────────────────────

#[test]
fn test_lower_unordered_list() {
    let nodes = [DomNode::Element {
        tag: "ul".into(),
        attrs: vec![],
        children: vec![
            DomNode::Element {
                tag: "li".into(),
                attrs: vec![],
                children: vec![DomNode::Text("item 1".into())],
                scores: std::collections::HashMap::new(),
                metadata: std::collections::HashMap::new(),
            },
            DomNode::Element {
                tag: "li".into(),
                attrs: vec![],
                children: vec![DomNode::Text("item 2".into())],
                scores: std::collections::HashMap::new(),
                metadata: std::collections::HashMap::new(),
            },
        ],
        scores: std::collections::HashMap::new(),
        metadata: std::collections::HashMap::new(),
    }];
    let md = MarkdownLowerer::lower(&nodes[0], None);
    assert!(md.contains("- item 1"));
    assert!(md.contains("- item 2"));
}

// ── Code block ──────────────────────────────────────────────────────

#[test]
fn test_lower_code_block() {
    let nodes = [DomNode::Element {
        tag: "pre".into(),
        attrs: vec![],
        children: vec![DomNode::Element {
            tag: "code".into(),
            attrs: vec![("class".into(), "language-rust".into())],
            children: vec![DomNode::Text("fn main() {}".into())],
            scores: std::collections::HashMap::new(),
            metadata: std::collections::HashMap::new(),
        }],
        scores: std::collections::HashMap::new(),
        metadata: std::collections::HashMap::new(),
    }];
    let md = MarkdownLowerer::lower(&nodes[0], None);
    assert!(md.contains("```rust"));
    assert!(md.contains("fn main() {}"));
    assert!(md.contains("```"));
}

// ── Table ───────────────────────────────────────────────────────────

#[test]
fn test_lower_table() {
    let nodes = [DomNode::Element {
        tag: "table".into(),
        attrs: vec![],
        children: vec![
            DomNode::Element {
                tag: "thead".into(),
                attrs: vec![],
                children: vec![DomNode::Element {
                    tag: "tr".into(),
                    attrs: vec![],
                    children: vec![
                        DomNode::Element {
                            tag: "th".into(),
                            attrs: vec![],
                            children: vec![DomNode::Text("Name".into())],
                            scores: std::collections::HashMap::new(),
                            metadata: std::collections::HashMap::new(),
                        },
                        DomNode::Element {
                            tag: "th".into(),
                            attrs: vec![],
                            children: vec![DomNode::Text("Age".into())],
                            scores: std::collections::HashMap::new(),
                            metadata: std::collections::HashMap::new(),
                        },
                    ],
                    scores: std::collections::HashMap::new(),
                    metadata: std::collections::HashMap::new(),
                }],
                scores: std::collections::HashMap::new(),
                metadata: std::collections::HashMap::new(),
            },
            DomNode::Element {
                tag: "tbody".into(),
                attrs: vec![],
                children: vec![DomNode::Element {
                    tag: "tr".into(),
                    attrs: vec![],
                    children: vec![
                        DomNode::Element {
                            tag: "td".into(),
                            attrs: vec![],
                            children: vec![DomNode::Text("Alice".into())],
                            scores: std::collections::HashMap::new(),
                            metadata: std::collections::HashMap::new(),
                        },
                        DomNode::Element {
                            tag: "td".into(),
                            attrs: vec![],
                            children: vec![DomNode::Text("30".into())],
                            scores: std::collections::HashMap::new(),
                            metadata: std::collections::HashMap::new(),
                        },
                    ],
                    scores: std::collections::HashMap::new(),
                    metadata: std::collections::HashMap::new(),
                }],
                scores: std::collections::HashMap::new(),
                metadata: std::collections::HashMap::new(),
            },
        ],
        scores: std::collections::HashMap::new(),
        metadata: std::collections::HashMap::new(),
    }];
    let md = MarkdownLowerer::lower(&nodes[0], None);
    assert!(md.contains("| Name"), "should contain header cell Name");
    assert!(md.contains("| ---"), "should contain separator row");
    assert!(md.contains("| Alice"), "should contain data cell Alice");
}

// ── Special characters ──────────────────────────────────────────────

#[test]
fn test_lower_special_chars() {
    let nodes = [DomNode::Element {
        tag: "p".into(),
        attrs: vec![],
        children: vec![DomNode::Text("escape *stars* and _underscores_".into())],
        scores: std::collections::HashMap::new(),
        metadata: std::collections::HashMap::new(),
    }];
    let md = MarkdownLowerer::lower(&nodes[0], None);
    assert!(md.contains(r"\*"));
    assert!(md.contains(r"\_"));
}

// ── Document with frontmatter ──────────────────────────────────────

#[test]
fn test_lower_document_with_frontmatter() {
    let nodes = [DomNode::Element {
        tag: "p".into(),
        attrs: vec![],
        children: vec![DomNode::Text("Hello, world!".into())],
        scores: std::collections::HashMap::new(),
        metadata: std::collections::HashMap::new(),
    }];
    let metadata = MarkdownDocument {
        frontmatter: "title: Test\ndate: 2025-01-01".into(),
        body: String::new(),
    };
    let body = MarkdownLowerer::lower(&nodes[0], None);
    let md = format!("---\n{}---\n\n{}", metadata.frontmatter, body);
    assert!(md.starts_with("---\n"), "should start with frontmatter");
    assert!(md.contains("title: Test"));
    assert!(md.contains("Hello, world!"));
}

// ── Output size cap ─────────────────────────────────────────────────

#[test]
fn test_output_size_cap() {
    // Create a huge paragraph of text
    let huge_text = "A".repeat(600_000);
    let nodes = [DomNode::Element {
        tag: "p".into(),
        attrs: vec![],
        children: vec![DomNode::Text(huge_text.clone())],
        scores: std::collections::HashMap::new(),
        metadata: std::collections::HashMap::new(),
    }];
    let md = MarkdownLowerer::lower(&nodes[0], None);
    assert!(
        md.len() <= MAX_OUTPUT_SIZE + 100,
        "output should be capped at MAX_OUTPUT_SIZE"
    );
    assert!(md.contains("[truncated: output exceeded 500 KiB]"));
}

// ── DOM nodes to HTML ───────────────────────────────────────────────

#[test]
fn test_dom_nodes_to_html() {
    let nodes = [DomNode::Element {
        tag: "div".into(),
        attrs: vec![("class".into(), "container".into())],
        children: vec![
            DomNode::Element {
                tag: "h1".into(),
                attrs: vec![],
                children: vec![DomNode::Text("Hello".into())],
                scores: std::collections::HashMap::new(),
                metadata: std::collections::HashMap::new(),
            },
            DomNode::Element {
                tag: "p".into(),
                attrs: vec![],
                children: vec![DomNode::Text("World".into())],
                scores: std::collections::HashMap::new(),
                metadata: std::collections::HashMap::new(),
            },
            DomNode::Element {
                tag: "img".into(),
                attrs: vec![
                    ("src".into(), "pic.png".into()),
                    ("alt".into(), "A picture".into()),
                ],
                children: vec![],
                scores: std::collections::HashMap::new(),
                metadata: std::collections::HashMap::new(),
            },
        ],
        scores: std::collections::HashMap::new(),
        metadata: std::collections::HashMap::new(),
    }];
    let html = crate::generators::gen_html::dom_nodes_to_html(&nodes[0]);
    assert!(html.contains("<div"), "should open div");
    assert!(
        html.contains("class=\"container\""),
        "should have class attr"
    );
    assert!(html.contains("<h1>Hello</h1>"), "should have h1");
    assert!(html.contains("<p>World</p>"), "should have p");
    assert!(
        html.contains("<img src=\"pic.png\" alt=\"A picture\">"),
        "should have img tag"
    );
}

// ── Helper tests ─────────────────────────────────────────────────────

#[test]
fn test_collect_text() {
    let nodes = [
        DomNode::Text("Hello ".into()),
        DomNode::Element {
            tag: "b".into(),
            attrs: vec![],
            children: vec![DomNode::Text("World".into())],
            scores: std::collections::HashMap::new(),
            metadata: std::collections::HashMap::new(),
        },
    ];
    let text: String = nodes.iter().map(|n| n.text_content()).collect();
    assert_eq!(text, "Hello World");
}

#[test]
fn test_get_attr() {
    let node = DomNode::Element {
        tag: "a".to_string(),
        attrs: vec![
            ("href".to_string(), "https://x.com".to_string()),
            ("class".to_string(), "link".to_string()),
        ],
        children: vec![],
        scores: std::collections::HashMap::new(),
        metadata: std::collections::HashMap::new(),
    };
    assert_eq!(node.attr("href"), Some("https://x.com"));
    assert_eq!(node.attr("id"), None);
}

#[test]
fn test_escape_markdown() {
    let result = escape_markdown("a * b _ c ` d");
    assert_eq!(result, r"a \* b \_ c \` d");
}

#[test]
fn test_resolve_url_absolute() {
    assert_eq!(
        resolve_url("https://example.com/page", Some("https://base.com")),
        "https://example.com/page"
    );
}

#[test]
fn test_resolve_url_relative() {
    assert_eq!(
        resolve_url("page", Some("https://base.com/path/")),
        "https://base.com/path/page"
    );
}

#[test]
fn test_resolve_url_root_relative() {
    assert_eq!(
        resolve_url("/page", Some("https://base.com/path/")),
        "https://base.com/page"
    );
}

#[test]
fn test_resolve_url_no_base() {
    assert_eq!(resolve_url("relative", None), "relative");
}

#[test]
fn test_extract_code_block() {
    let children = vec![DomNode::Element {
        tag: "code".into(),
        attrs: vec![("class".into(), "language-python".into())],
        children: vec![DomNode::Text("print('hello')".into())],
        scores: std::collections::HashMap::new(),
        metadata: std::collections::HashMap::new(),
    }];
    let (lang, code) = extract_code_block(&children);
    assert_eq!(lang, "python");
    assert_eq!(code, "print('hello')");
}

// ---------------------------------------------------------------------------
// Math (LaTeXML MathML) tests
// ---------------------------------------------------------------------------

#[test]
fn test_lower_inline_math() {
    let node = DomNode::Element {
        tag: "math".into(),
        attrs: vec![
            ("alttext".into(), "E=mc^2".into()),
            ("display".into(), "inline".into()),
        ],
        children: vec![],
        scores: std::collections::HashMap::new(),
        metadata: std::collections::HashMap::new(),
    };
    let md = MarkdownLowerer::lower(&node, None);
    assert_eq!(md, "$E=mc^2$");
}

#[test]
fn test_lower_display_math() {
    let node = DomNode::Element {
        tag: "math".into(),
        attrs: vec![
            ("alttext".into(), "\\sum_{i=1}^n".into()),
            ("display".into(), "block".into()),
        ],
        children: vec![],
        scores: std::collections::HashMap::new(),
        metadata: std::collections::HashMap::new(),
    };
    let md = MarkdownLowerer::lower(&node, None);
    assert!(
        md.contains("$$\\displaystyle \\sum_{i=1}^n$$"),
        "display math: {}",
        md
    );
}

#[test]
fn test_lower_math_no_alttext_falls_back_to_text() {
    let node = DomNode::Element {
        tag: "math".into(),
        attrs: vec![],
        children: vec![DomNode::Text("E=mc^2".into())],
        scores: std::collections::HashMap::new(),
        metadata: std::collections::HashMap::new(),
    };
    let md = MarkdownLowerer::lower(&node, None);
    assert_eq!(md, "E=mc^2");
}

#[test]
fn test_serialize_math_to_latex() {
    let node = DomNode::Element {
        tag: "math".into(),
        attrs: vec![
            ("alttext".into(), "x^2".into()),
            ("display".into(), "inline".into()),
        ],
        children: vec![],
        scores: std::collections::HashMap::new(),
        metadata: std::collections::HashMap::new(),
    };
    let html = serialize_node_to_html(&node);
    assert_eq!(html, "$x^2$");
}
