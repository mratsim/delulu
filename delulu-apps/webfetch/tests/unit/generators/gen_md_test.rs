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

#[test]
fn test_lower_ref_link() {
    // "ref" is trafilatura's internal link element (<a href> -> <ref target>
    // via tf_convert_refs_and_details). Must render as a markdown link so
    // include_links is on by default.
    let nodes = [DomNode::Element {
        tag: "ref".into(),
        attrs: vec![("target".into(), "https://example.com/page".into())],
        children: vec![DomNode::Text("link".into())],
        scores: std::collections::HashMap::new(),
        metadata: std::collections::HashMap::new(),
    }];
    let md = MarkdownLowerer::lower(&nodes[0], None);
    assert!(md.contains("[link](https://example.com/page)"));
}

#[test]
fn test_lower_ref_link_relative_resolves_base() {
    let nodes = [DomNode::Element {
        tag: "ref".into(),
        attrs: vec![("target".into(), "/blog/2025-09-05-anatomy-of-vllm".into())],
        children: vec![DomNode::Text("Anatomy of vLLM".into())],
        scores: std::collections::HashMap::new(),
        metadata: std::collections::HashMap::new(),
    }];
    let md = MarkdownLowerer::lower(&nodes[0], Some("https://vllm.ai/blog"));
    assert!(md.contains("[Anatomy of vLLM](https://vllm.ai/blog/2025-09-05-anatomy-of-vllm)"));
}

// ── Unordered list ──────────────────────────────────────────────────

#[test]
fn test_lower_head_rend_heading() {
    // tf_convert_headings renames h2 -> <head rend="h2">; must render as
    // a markdown heading or the text jams onto the next paragraph
    // ("FAQQuick answers").
    let nodes = [DomNode::Element {
        tag: "head".into(),
        attrs: vec![("rend".into(), "h2".into())],
        children: vec![DomNode::Text("FAQ".into())],
        scores: std::collections::HashMap::new(),
        metadata: std::collections::HashMap::new(),
    }];
    let md = MarkdownLowerer::lower(&nodes[0], None);
    assert!(
        md.contains("## FAQ"),
        "head rend=h2 -> ## heading, got: {md}"
    );
}

#[test]
fn test_lower_head_without_rend_renders_text() {
    let nodes = [DomNode::Element {
        tag: "head".into(),
        attrs: vec![],
        children: vec![DomNode::Text("plain".into())],
        scores: std::collections::HashMap::new(),
        metadata: std::collections::HashMap::new(),
    }];
    let md = MarkdownLowerer::lower(&nodes[0], None);
    assert!(!md.contains('#'), "no rend -> plain text, got: {md}");
}

#[test]
fn test_lower_list_item_tags() {
    // tf_convert_lists renames ul->list, li->item; both must render as bullets.
    let nodes = [DomNode::Element {
        tag: "list".into(),
        attrs: vec![],
        children: vec![DomNode::Element {
            tag: "item".into(),
            attrs: vec![],
            children: vec![DomNode::Text("bullet".into())],
            scores: std::collections::HashMap::new(),
            metadata: std::collections::HashMap::new(),
        }],
        scores: std::collections::HashMap::new(),
        metadata: std::collections::HashMap::new(),
    }];
    let md = MarkdownLowerer::lower(&nodes[0], None);
    assert!(
        md.contains("- bullet"),
        "list/item -> '- bullet', got: {md}"
    );
}

#[test]
fn test_escape_markdown_keeps_dot_and_plus() {
    // '.' and '+' are not escaped: decimals (3.1) and signs (30%+) stay clean.
    let md = MarkdownLowerer::lower(&DomNode::Text("3.1x and 30%+ gain".into()), None);
    assert!(
        md.contains("3.1x and 30%+ gain"),
        "no \\. or \\+, got: {md}"
    );
}
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

// ── R2: multi-line <code> renders as fenced block ─────────────────────

fn code_node(attrs: &[(&str, &str)], text: &str) -> DomNode {
    DomNode::Element {
        tag: "code".into(),
        attrs: attrs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect(),
        children: vec![DomNode::Text(text.into())],
        scores: std::collections::HashMap::new(),
        metadata: std::collections::HashMap::new(),
    }
}

#[test]
fn test_lower_multiline_code_fenced_block() {
    // Multi-line <code> (from pre->code conversion) must render as a fenced
    // block, not inline backticks, or the lines collapse onto one line.
    let node = code_node(
        &[],
        "pip install sglang[all]\npython -m sglang.launch_server --model-path meta-llama/Llama-3.1-8B-Instruct --port 8000",
    );
    let md = MarkdownLowerer::lower(&node, None);
    assert!(
        md.contains(
            "```\npip install sglang[all]\npython -m sglang.launch_server --model-path meta-llama/Llama-3.1-8B-Instruct --port 8000\n```\n\n"
        ),
        "multi-line code must be a fenced block, got: {md}"
    );
}

#[test]
fn test_lower_multiline_code_fenced_block_with_trailing_newline() {
    // Source text already ends with '\n': no extra newline is inserted before
    // the closing fence (mirrors the "pre" case).
    let node = code_node(&[], "line1\nline2\n");
    let md = MarkdownLowerer::lower(&node, None);
    assert_eq!(md, "```\nline1\nline2\n```\n\n", "got: {md}");
}

#[test]
fn test_lower_multiline_code_fenced_block_with_language() {
    // A `language-*` class on the code element is appended to the opening fence.
    let node = code_node(
        &[("class", "language-bash")],
        "pip install vllm\nvllm serve",
    );
    let md = MarkdownLowerer::lower(&node, None);
    assert!(
        md.contains("```bash\npip install vllm\nvllm serve\n```\n\n"),
        "language-* class must be on the opening fence, got: {md}"
    );
}

#[test]
fn test_lower_single_line_code_inline_backticks() {
    // Single-line <code> keeps inline backticks (no fence).
    let node = code_node(&[], "inline");
    let md = MarkdownLowerer::lower(&node, None);
    assert_eq!(md, "`inline`", "single-line code stays inline, got: {md}");
}

// ── SPEC-001: headings must land on a fresh line after loose text ─────────

#[test]
fn test_lower_head_after_loose_text_starts_fresh_line() {
    // Regression (SPEC-001): a <head rend="h3"> following loose text must
    // start on a fresh line, or the `### ` marker is glued mid-line and is
    // not a CommonMark heading (it renders as body text).
    let container = DomNode::Element {
        tag: "div".into(),
        attrs: vec![],
        children: vec![
            DomNode::Text("prefix caching provides no benefit.".into()),
            DomNode::Element {
                tag: "head".into(),
                attrs: vec![("rend".into(), "h3".into())],
                children: vec![DomNode::Text(
                    "Why does DeepSeek recommend SGLang over vLLM?".into(),
                )],
                scores: std::collections::HashMap::new(),
                metadata: std::collections::HashMap::new(),
            },
        ],
        scores: std::collections::HashMap::new(),
        metadata: std::collections::HashMap::new(),
    };
    let md = MarkdownLowerer::lower(&container, None);
    assert!(
        md.contains("\n### Why does DeepSeek recommend SGLang over vLLM?"),
        "heading must start on a fresh line, got: {md}"
    );
    assert!(
        !md.contains("benefit.###"),
        "heading must not glue onto the preceding text, got: {md}"
    );
}

// ── GOAL-001: fenced code opener must land at line-start after loose text ──

#[test]
fn test_lower_multiline_code_after_loose_text_fence_opener_at_line_start() {
    // Regression (GOAL-001): a block-level multi-line <code> following loose
    // text (e.g. a "BASH" code-block header label) must have its opening
    // fence at line-start — `BASH```` is not a valid CommonMark fence opener.
    let container = DomNode::Element {
        tag: "div".into(),
        attrs: vec![],
        children: vec![
            DomNode::Text("BASH".into()),
            code_node(
                &[],
                "pip install sglang[all]\npython -m sglang.launch_server --model-path meta-llama/Llama-3.1-8B-Instruct --port 8000",
            ),
        ],
        scores: std::collections::HashMap::new(),
        metadata: std::collections::HashMap::new(),
    };
    let md = MarkdownLowerer::lower(&container, None);
    assert!(
        md.contains("BASH\n```\npip install sglang[all]\n"),
        "fence opener must be at line-start after loose text, got: {md}"
    );
    assert!(!md.contains("BASH```"), "no `BASH```` jam, got: {md}");
}

// ── SLOP-004: multi-line inline code inside a paragraph stays inline ──────

#[test]
fn test_lower_multiline_inline_code_in_paragraph_stays_inline() {
    // Regression (SLOP-004): a multi-line inline <code> inside a <p> must NOT
    // emit a mid-paragraph fence; newlines normalize to spaces and the code
    // stays inline backticks (valid markdown).
    let p = DomNode::Element {
        tag: "p".into(),
        attrs: vec![],
        children: vec![
            DomNode::Text("Type ".into()),
            code_node(&[], "foo\nbar"),
            DomNode::Text(" in the terminal to continue.".into()),
        ],
        scores: std::collections::HashMap::new(),
        metadata: std::collections::HashMap::new(),
    };
    let md = MarkdownLowerer::lower(&p, None);
    assert!(
        md.contains("Type `foo bar` in the terminal to continue."),
        "inline code must stay inline with normalized newlines, got: {md}"
    );
    assert!(!md.contains("```"), "no mid-paragraph fence, got: {md}");
}

// ── SLOP-101: multi-line code in a formatting wrapper inside a paragraph ────

#[test]
fn test_lower_multiline_code_in_hi_wrapper_in_paragraph_stays_inline() {
    // Regression (SLOP-101): tf_convert_formatting renames <strong> -> <hi>;
    // <hi> is an unknown container for gen_md and hits the `_` fallback. The
    // fallback must thread the incoming inline_ctx through, or a multi-line
    // <code> nested in the wrapper emits a mid-paragraph fence.
    let p = DomNode::Element {
        tag: "p".into(),
        attrs: vec![],
        children: vec![
            DomNode::Text("Type ".into()),
            DomNode::Element {
                tag: "hi".into(),
                attrs: vec![("rend".into(), "#b".into())],
                children: vec![code_node(&[], "foo\nbar")],
                scores: std::collections::HashMap::new(),
                metadata: std::collections::HashMap::new(),
            },
            DomNode::Text(" to continue.".into()),
        ],
        scores: std::collections::HashMap::new(),
        metadata: std::collections::HashMap::new(),
    };
    let md = MarkdownLowerer::lower(&p, None);
    assert!(
        md.contains("Type `foo bar` to continue."),
        "inline code in a hi wrapper must stay inline, got: {md}"
    );
    assert!(!md.contains("```"), "no mid-paragraph fence, got: {md}");
}

#[test]
fn test_lower_multiline_code_in_strong_wrapper_in_paragraph_stays_inline() {
    // Direct <strong> wrapper: gen_md's strong arm lowers children inline, so
    // the multi-line code renders as `foo bar` inside ** ** — no fence.
    let p = DomNode::Element {
        tag: "p".into(),
        attrs: vec![],
        children: vec![
            DomNode::Text("Type ".into()),
            DomNode::Element {
                tag: "strong".into(),
                attrs: vec![],
                children: vec![code_node(&[], "foo\nbar")],
                scores: std::collections::HashMap::new(),
                metadata: std::collections::HashMap::new(),
            },
            DomNode::Text(" to continue.".into()),
        ],
        scores: std::collections::HashMap::new(),
        metadata: std::collections::HashMap::new(),
    };
    let md = MarkdownLowerer::lower(&p, None);
    assert!(
        md.contains("Type **`foo bar`** to continue."),
        "strong-wrapped code must stay inline in bold, got: {md}"
    );
    assert!(!md.contains("```"), "no mid-paragraph fence, got: {md}");
}

#[test]
fn test_lower_multiline_code_in_del_wrapper_in_paragraph_stays_inline() {
    // Regression (SLOP-101): <del> is an unknown container for gen_md (tf
    // renames <del>/<s>/<strike> -> <del>); the `_` fallback must thread
    // inline_ctx through so the multi-line code stays inline backticks.
    let p = DomNode::Element {
        tag: "p".into(),
        attrs: vec![],
        children: vec![
            DomNode::Text("Run ".into()),
            DomNode::Element {
                tag: "del".into(),
                attrs: vec![("rend".into(), "overstrike".into())],
                children: vec![code_node(&[], "old\nnew")],
                scores: std::collections::HashMap::new(),
                metadata: std::collections::HashMap::new(),
            },
            DomNode::Text(" command.".into()),
        ],
        scores: std::collections::HashMap::new(),
        metadata: std::collections::HashMap::new(),
    };
    let md = MarkdownLowerer::lower(&p, None);
    assert!(
        md.contains("Run `old new` command."),
        "del-wrapped code must stay inline, got: {md}"
    );
    assert!(!md.contains("```"), "no mid-paragraph fence, got: {md}");
}

#[test]
fn test_lower_unknown_container_at_root_multiline_code_still_fences() {
    // Control (GOAL-001): an unknown container at ROOT (inline_ctx=false)
    // holding a multi-line <code> must still emit a block-level fence at
    // line-start. The inline_ctx pass-through must not regress this.
    let div = DomNode::Element {
        tag: "div".into(),
        attrs: vec![],
        children: vec![
            DomNode::Text("BASH".into()),
            code_node(
                &[],
                "pip install sglang[all]\npython -m sglang.launch_server --port 8000",
            ),
        ],
        scores: std::collections::HashMap::new(),
        metadata: std::collections::HashMap::new(),
    };
    let md = MarkdownLowerer::lower(&div, None);
    assert!(
        md.contains("BASH\n```\npip install sglang[all]\n"),
        "block-level code in a root unknown container must still fence, got: {md}"
    );
    assert!(!md.contains("BASH```"), "no `BASH```` jam, got: {md}");
}

// ── SLOP-003: language class on a nested <code> child ─────────────────────

#[test]
fn test_lower_multiline_code_language_from_nested_code_child() {
    // Regression (SLOP-003): after tf_convert_quotes renames <pre> to <code>,
    // pre>code.language-rust reaches gen_md as code>code.language-rust — the
    // language class sits on the INNER code. The "code" case must fall back to
    // scanning children for the language class.
    let outer = DomNode::Element {
        tag: "code".into(),
        attrs: vec![],
        children: vec![code_node(&[("class", "language-rust")], "fn main() {\n}")],
        scores: std::collections::HashMap::new(),
        metadata: std::collections::HashMap::new(),
    };
    let md = MarkdownLowerer::lower(&outer, None);
    assert!(
        md.contains("```rust\nfn main() {\n}\n```\n\n"),
        "language must come from the nested <code> class, got: {md}"
    );
}

// ── SLOP-003: multi-class values yield only the first language token ──────

#[test]
fn test_lower_multiline_code_language_token_split_on_whitespace() {
    // Regression (SLOP-003): a multi-class value like "language-python highlight"
    // must yield "python" — not the bogus token "python highlight".
    let node = code_node(
        &[("class", "language-python highlight")],
        "print('x')\nprint('y')",
    );
    let md = MarkdownLowerer::lower(&node, None);
    assert!(
        md.contains("```python\n"),
        "only the first whitespace-delimited token, got: {md}"
    );
    assert!(
        !md.contains("python highlight"),
        "no bogus 'python highlight' token, got: {md}"
    );
}
