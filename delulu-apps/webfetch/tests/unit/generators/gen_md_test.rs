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
// ── Heading text: inline-lowered, newline-collapsed, trimmed, links neutralized ──

#[test]
fn test_heading_link_in_heading_is_escaped() {
    // A heading that itself contains a link element (`<ref target>`, i.e.
    // trafilatura's `<a href>`) must NOT render as a live markdown link out
    // of the ATX line. The inline lowering produces `[docs](url)` and
    // escape_heading_links neutralizes the unescaped `[`/`]`/`(`/`)` so a
    // javascript:/data: target can never become clickable.
    let nodes = [DomNode::Element {
        tag: "h2".into(),
        attrs: vec![],
        children: vec![DomNode::Element {
            tag: "ref".into(),
            attrs: vec![("target".into(), "javascript:alert(1)".into())],
            children: vec![DomNode::Text("docs".into())],
            scores: std::collections::HashMap::new(),
            metadata: std::collections::HashMap::new(),
        }],
        scores: std::collections::HashMap::new(),
        metadata: std::collections::HashMap::new(),
    }];
    let md = MarkdownLowerer::lower(&nodes[0], None);
    assert!(
        !md.contains("](javascript:"),
        "live javascript: link must be neutralized, got: {md}"
    );
    assert!(
        md.contains("## "),
        "heading marker must still be emitted, got: {md}"
    );
}

#[test]
fn test_heading_escapes_raw_markdown_specials() {
    // Raw text special characters in a heading must not break the ATX line
    // or inject document-level markdown. lower_inline escapes raw specials
    // ONCE (so `*c*` is `\*c\*`, not emphasis); we must NOT re-escape them
    // (that would show a visible backslash). escape_heading_links keeps this
    // single-escaped raw text intact while neutralising only constructed
    // link delimiters.
    let nodes = [DomNode::Element {
        tag: "h3".into(),
        attrs: vec![],
        children: vec![DomNode::Text("a [b] *c* _d_".into())],
        scores: std::collections::HashMap::new(),
        metadata: std::collections::HashMap::new(),
    }];
    let md = MarkdownLowerer::lower(&nodes[0], None);
    assert!(
        !md.contains(" *c* "),
        "raw asterisk emphasis must not render as emphasis, got: {md}"
    );
    assert!(
        md.contains("##") || md.contains("###"),
        "heading marker must be present, got: {md}"
    );
}

#[test]
fn test_heading_inline_lowering_preserves_structure() {
    // Heading text must be built via the inline lowering path so inline
    // structure (code, links) survives. `<code>` renders as a live backtick
    // code span (raw backticks are not a link vector); only constructed link
    // delimiters are neutralized by escape_heading_links.
    let nodes = [DomNode::Element {
        tag: "h2".into(),
        attrs: vec![],
        children: vec![
            DomNode::Text("Install ".into()),
            DomNode::Element {
                tag: "code".into(),
                attrs: vec![],
                children: vec![DomNode::Text("--port".into())],
                scores: std::collections::HashMap::new(),
                metadata: std::collections::HashMap::new(),
            },
            DomNode::Text(" now".into()),
        ],
        scores: std::collections::HashMap::new(),
        metadata: std::collections::HashMap::new(),
    }];
    let md = MarkdownLowerer::lower(&nodes[0], None);
    assert!(
        md.contains("## Install `--port` now"),
        "inline code must survive as a live backtick span in the heading, got: {md}"
    );
}

#[test]
fn test_heading_collapses_newlines_and_trims() {
    // Attacker/pretty-printed newlines in heading text must collapse to
    // spaces and leading/trailing whitespace must be trimmed so the ATX line
    // stays a single valid heading.
    let nodes = [DomNode::Element {
        tag: "head".into(),
        attrs: vec![("rend".into(), "h3".into())],
        children: vec![DomNode::Text("\nMulti\nLine heading\n".into())],
        scores: std::collections::HashMap::new(),
        metadata: std::collections::HashMap::new(),
    }];
    let md = MarkdownLowerer::lower(&nodes[0], None);
    assert!(
        md.contains("### Multi Line heading"),
        "newlines collapse and edges trim, got: {md}"
    );
}

#[test]
fn test_heading_plain_parens_not_double_escaped() {
    // Plain parenthesized headings ("(2024)", "(Part 2)", function sigs) must
    // NOT be double-escaped into a visible literal `\(`. lower_inline escapes
    // raw parens once (`\(2024\)`); escape_heading_links leaves that intact.
    // The buggy double-escape (`escape_markdown(lower_inline(..))`) produced
    // `\(2024\)`, which renders with a stray visible backslash.
    let nodes = [DomNode::Element {
        tag: "h2".into(),
        attrs: vec![],
        children: vec![DomNode::Text("Quantum Computing (2024) Guide".into())],
        scores: std::collections::HashMap::new(),
        metadata: std::collections::HashMap::new(),
    }];
    let md = MarkdownLowerer::lower(&nodes[0], None);
    assert!(
        !md.contains("\\\\("),
        "parens must not be double-escaped (visible backslash), got: {md}"
    );
    assert!(
        md.contains("## Quantum Computing"),
        "heading text must be present, got: {md}"
    );
}

#[test]
fn test_heading_code_span_parens_not_escaped() {
    // Parens/brackets INSIDE an inline `<code>` span in a heading are genuine
    // function/array syntax, not link delimiters. escape_heading_links must be
    // code-span aware and leave them untouched — otherwise the user sees a
    // visible `\(` inside the backtick span; such pairs must be left untouched.
    let nodes = [DomNode::Element {
        tag: "h2".into(),
        attrs: vec![],
        children: vec![
            DomNode::Text("Use ".into()),
            DomNode::Element {
                tag: "code".into(),
                attrs: vec![],
                children: vec![DomNode::Text("func(a, b)".into())],
                scores: std::collections::HashMap::new(),
                metadata: std::collections::HashMap::new(),
            },
            DomNode::Text(" now".into()),
        ],
        scores: std::collections::HashMap::new(),
        metadata: std::collections::HashMap::new(),
    }];
    let md = MarkdownLowerer::lower(&nodes[0], None);
    assert!(
        md.contains("`func(a, b)`"),
        "code span must survive with no escaping, got: {md}"
    );
    assert!(
        !md.contains("func\\("),
        "no backslash injected inside the code span, got: {md}"
    );
    assert!(
        md.contains("## Use"),
        "heading marker must be present, got: {md}"
    );
}

#[test]
fn test_heading_code_span_brackets_and_link_not_escaped_inside() {
    // A heading containing BOTH an inline code span (with brackets) and a
    // constructed link: the code span must stay literal (no escaping) while the
    // link delimiters OUTSIDE it remain neutralized.
    let nodes = [DomNode::Element {
        tag: "h2".into(),
        attrs: vec![],
        children: vec![
            DomNode::Text("Access ".into()),
            DomNode::Element {
                tag: "code".into(),
                attrs: vec![],
                children: vec![DomNode::Text("a[0]".into())],
                scores: std::collections::HashMap::new(),
                metadata: std::collections::HashMap::new(),
            },
            DomNode::Text(" ".into()),
            DomNode::Element {
                tag: "ref".into(),
                attrs: vec![("target".into(), "javascript:x".into())],
                children: vec![DomNode::Text("docs".into())],
                scores: std::collections::HashMap::new(),
                metadata: std::collections::HashMap::new(),
            },
        ],
        scores: std::collections::HashMap::new(),
        metadata: std::collections::HashMap::new(),
    }];
    let md = MarkdownLowerer::lower(&nodes[0], None);
    assert!(
        md.contains("`a[0]`"),
        "code span brackets must be preserved unescaped, got: {md}"
    );
    assert!(
        !md.contains("`a\\[0]`"),
        "no backslash injected inside the code span, got: {md}"
    );
    assert!(
        !md.contains("](javascript:"),
        "constructed link must still be neutralized, got: {md}"
    );
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
    // Contract: generators receive normalized <pre> blocks — the language is
    // hoisted onto the pre's own class by normalize_code_blocks (every
    // pipeline runs it), and the pre holds plain text.
    let nodes = [DomNode::Element {
        tag: "pre".into(),
        attrs: vec![("class".into(), "language-rust".into())],
        children: vec![DomNode::Text("fn main() {}".into())],
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
    assert!(
        md.contains("|-----"),
        "should contain dash-only separator row"
    );
    assert!(md.contains("| Alice"), "should contain data cell Alice");
}

#[test]
fn test_lower_table_wide_cell_bounds_padding() {
    // PERF-F-2: a single pathological wide cell must NOT right-pad every other
    // cell in its column to the max width (O(N * max_cell_width)). The
    // col_widths cap (min(64)) bounds the padding while the wide content itself
    // is preserved in full.
    let cell = |text: String, tag: &str| DomNode::Element {
        tag: tag.into(),
        attrs: vec![],
        children: vec![DomNode::Text(text)],
        scores: std::collections::HashMap::new(),
        metadata: std::collections::HashMap::new(),
    };
    let row = |cells: Vec<DomNode>| DomNode::Element {
        tag: "tr".into(),
        attrs: vec![],
        children: cells,
        scores: std::collections::HashMap::new(),
        metadata: std::collections::HashMap::new(),
    };
    let to = |tag: &str, rows: Vec<DomNode>| DomNode::Element {
        tag: tag.into(),
        attrs: vec![],
        children: rows,
        scores: std::collections::HashMap::new(),
        metadata: std::collections::HashMap::new(),
    };

    let wide = "x".repeat(10_000);
    let table = to(
        "table",
        vec![
            to(
                "thead",
                vec![row(vec![cell("h".into(), "th"), cell("w".into(), "th")])],
            ),
            to(
                "tbody",
                vec![
                    row(vec![cell("a".into(), "td"), cell(wide.clone(), "td")]),
                    row(vec![cell("b".into(), "td"), cell("c".into(), "td")]),
                    row(vec![cell("d".into(), "td"), cell("e".into(), "td")]),
                    row(vec![cell("f".into(), "td"), cell("g".into(), "td")]),
                ],
            ),
        ],
    );

    let md = MarkdownLowerer::lower(&table, None);
    // The full wide cell content must be preserved (no data loss)...
    assert!(md.contains(&wide), "wide cell content must be emitted");
    assert!(md.contains("---"), "dash separator row must survive");
    // ...but the output must be bounded: the wide cell itself (~10k) plus a
    // small constant for the short cells/separators. Without the col_widths
    // min(64) cap, the three short second-column cells would each be
    // right-padded to ~10k, blowing output up to ~40k+.
    assert!(
        md.len() < 10_000 + 2_048,
        "output must be bounded to ~max_cell_width + constant, not amplified, got {}",
        md.len()
    );
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

#[test]
fn test_cap_size_multibyte_no_panic() {
    // Pure multibyte string with NO ASCII prefix ("€" is 3 bytes) so byte
    // 512,000 (MAX_OUTPUT_SIZE) lands MID-character: 512000 % 3 == 2. The old
    // `s.truncate(MAX_OUTPUT_SIZE)` would therefore panic; cap_size must walk
    // back to the last char boundary. (A "head\n" prefix would shift the byte
    // offset onto a char boundary and make this regression guard tautological.)
    let huge = "€".repeat(200_000); // 600_000 bytes raw
    let out = MarkdownLowerer::cap_size(huge);
    assert!(
        out.len() <= MAX_OUTPUT_SIZE + 64,
        "output should be capped near MAX_OUTPUT_SIZE, got {}",
        out.len()
    );
    assert!(
        out.ends_with("[truncated: output exceeded 500 KiB]"),
        "must end with the truncation marker, got: {:?}",
        &out[out.len().saturating_sub(60)..]
    );
    // Truncation must land on a char boundary: the portion before the marker
    // is valid UTF-8 (no panic already proves this, but be explicit).
    let body_end = out.find("\n\n[truncated").expect("marker present");
    assert!(
        out.is_char_boundary(body_end),
        "marker must start at a char boundary"
    );
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
    // Language read from the <pre>'s own class (tf normalization path).
    let pre = DomNode::Element {
        tag: "pre".into(),
        attrs: vec![("class".into(), "language-python".into())],
        children: vec![DomNode::Text("print('hello')".into())],
        scores: std::collections::HashMap::new(),
        metadata: std::collections::HashMap::new(),
    };
    let (lang, code) = extract_code_block(&pre);
    assert_eq!(lang, "python");
    assert_eq!(code, "print('hello')");
}

#[test]
fn test_extract_code_block_ignores_un_normalized_shape() {
    // Contract: generators only ever receive normalized <pre> blocks (the
    // normalize_code_blocks pass runs in every pipeline). A pre>code shape
    // reaching gen_md is a pipeline bug; the generator reads only the pre's
    // own class and renders the text — no nested-code language lookup.
    let pre = DomNode::Element {
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
    };
    let (lang, code) = extract_code_block(&pre);
    assert_eq!(lang, "", "no nested-code language lookup in the generator");
    assert_eq!(code, "fn main() {}");
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

/// Build a `<pre>` block node (block code is structurally `<pre>`).
fn pre_node(attrs: &[(&str, &str)], text: &str) -> DomNode {
    DomNode::Element {
        tag: "pre".into(),
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
    // A multi-line `<pre>` (block code) must render as a fenced block, not
    // inline backticks, or the lines collapse onto one line.
    let node = pre_node(
        &[],
        "pip install sglang[all]\npython -m sglang.launch_server --model-path meta-llama/Llama-3.1-8B-Instruct --port 8000",
    );
    let md = MarkdownLowerer::lower(&node, None);
    assert!(
        md.contains(
            "```\npip install sglang[all]\npython -m sglang.launch_server --model-path meta-llama/Llama-3.1-8B-Instruct --port 8000\n```\n\n"
        ),
        "multi-line pre must be a fenced block, got: {md}"
    );
}

#[test]
fn test_lower_multiline_code_fenced_block_with_trailing_newline() {
    // Source text already ends with '\n': no extra newline is inserted before
    // the closing fence.
    let node = pre_node(&[], "line1\nline2\n");
    let md = MarkdownLowerer::lower(&node, None);
    assert_eq!(md, "```\nline1\nline2\n```\n\n", "got: {md}");
}

#[test]
fn test_lower_multiline_code_fenced_block_with_language() {
    // A `language-*` class on the pre is appended to the opening fence.
    let node = pre_node(
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

#[test]
fn test_lower_multiline_code_fence_longer_than_interior_backtick_run() {
    // Regression: a `<pre>` whose content contains a line with 4 consecutive
    // backticks must be fenced with a 5-backtick delimiter so the interior
    // run cannot close the block early and leak the rest as markdown.
    let node = pre_node(
        &[("class", "language-bash")],
        "line1\n````dangerous```\nline2",
    );
    let md = MarkdownLowerer::lower(&node, None);
    assert!(
        md.contains("`````bash\nline1\n````dangerous```\nline2\n`````\n\n"),
        "fence must be longer than the interior 4-backtick run, got: {md}",
    );
    assert!(
        !md.lines()
            .any(|l| l.starts_with("dangerous") && !l.starts_with("`````")),
        "no content may leak outside the fenced block, got: {md}",
    );
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

#[test]
fn test_lower_html_heading_after_loose_text_starts_fresh_line() {
    // An <h1>..<h6> element following loose text must start on a fresh line,
    // or the `## ` marker is glued mid-line and is not a CommonMark heading.
    let container = DomNode::Element {
        tag: "div".into(),
        attrs: vec![],
        children: vec![
            DomNode::Text("intro sentence.".into()),
            DomNode::Element {
                tag: "h2".into(),
                attrs: vec![],
                children: vec![DomNode::Text("Deep dive".into())],
                scores: std::collections::HashMap::new(),
                metadata: std::collections::HashMap::new(),
            },
        ],
        scores: std::collections::HashMap::new(),
        metadata: std::collections::HashMap::new(),
    };
    let md = MarkdownLowerer::lower(&container, None);
    assert!(
        md.contains("\n## Deep dive"),
        "heading must start on a fresh line, got: {md}",
    );
    assert!(
        !md.contains("sentence.##"),
        "heading must not glue onto the preceding text, got: {md}",
    );
}

// ── GOAL-001: fenced code opener must land at line-start after loose text ──

#[test]
fn test_lower_multiline_code_after_loose_text_fence_opener_at_line_start() {
    // Regression (GOAL-001): a block `<pre>` following loose text (e.g. a
    // "BASH" code-block header label) must have its opening fence at
    // line-start — `BASH```` is not a valid CommonMark fence opener.
    let container = DomNode::Element {
        tag: "div".into(),
        attrs: vec![],
        children: vec![
            DomNode::Text("BASH".into()),
            pre_node(
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

// ── Multi-line inline code inside a paragraph stays inline ──────────────

#[test]
fn test_lower_multiline_inline_code_in_paragraph_stays_inline() {
    // Regression: a multi-line inline <code> inside a <p> must NOT
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

// ── Multi-line code in a formatting wrapper inside a paragraph ──────────

#[test]
fn test_lower_multiline_code_in_hi_wrapper_in_paragraph_stays_inline() {
    // Regression: tf_convert_formatting renames <strong> -> <hi>;
    // <hi> is an unknown container for gen_md and hits the `_` fallback. The
    // fallback must keep the nested <code> inline, or a multi-line <code>
    // nested in the wrapper emits a mid-paragraph fence.
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
    // Regression: <del> is an unknown container for gen_md (tf
    // renames <del>/<s>/<strike> -> <del>); the `_` fallback must keep the
    // nested multi-line code inline so it stays inline backticks.
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
fn test_lower_code_in_unknown_container_is_structurally_inline() {
    // Structural rule: block-ness lives in the tag. A <code> is inline even at
    // root inside an unknown container (the tf pipeline normalizes block code
    // to <pre>, so a root-level <code> is a degenerate shape) — newlines
    // normalize to spaces and it renders as inline backticks, never a fence.
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
        !md.contains("```"),
        "code is structurally inline, got: {md}"
    );
    assert!(
        md.contains("`pip install sglang[all] python -m sglang.launch_server --port 8000`"),
        "inline code with normalized newlines, got: {md}"
    );
}

// ── Language comes from the pre's own hoisted class ─────────────────────

#[test]
fn test_lower_multiline_code_language_from_pre_class() {
    // The normalize_code_blocks pass hoists the language onto the pre's own
    // class before lowering; the generator reads exactly that.
    let pre = DomNode::Element {
        tag: "pre".into(),
        attrs: vec![("class".into(), "language-rust".into())],
        children: vec![DomNode::Text("fn main() {\n}".into())],
        scores: std::collections::HashMap::new(),
        metadata: std::collections::HashMap::new(),
    };
    let md = MarkdownLowerer::lower(&pre, None);
    assert!(
        md.contains("```rust\nfn main() {\n}\n```\n\n"),
        "language comes from the pre's own class, got: {md}"
    );
}

// ── Multi-class values yield only the first language token ─────────────

#[test]
fn test_lower_multiline_code_language_token_split_on_whitespace() {
    // Regression: a multi-class value like "language-python highlight"
    // must yield "python" — not the bogus token "python highlight".
    let node = pre_node(
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

// ── FAQ: <details><summary> raw-HTML block (GFM collapsible) ─────────

#[test]
fn test_lower_details_summary_block() {
    let nodes = [DomNode::Element {
        tag: "details".into(),
        attrs: vec![],
        children: vec![
            DomNode::Element {
                tag: "summary".into(),
                attrs: vec![],
                children: vec![DomNode::Text("Is SGLang faster than vLLM?".into())],
                scores: std::collections::HashMap::new(),
                metadata: std::collections::HashMap::new(),
            },
            DomNode::Element {
                tag: "p".into(),
                attrs: vec![],
                children: vec![DomNode::Text("Yes, on prefix-heavy workloads.".into())],
                scores: std::collections::HashMap::new(),
                metadata: std::collections::HashMap::new(),
            },
        ],
        scores: std::collections::HashMap::new(),
        metadata: std::collections::HashMap::new(),
    }];
    let md = MarkdownLowerer::lower(&nodes[0], None);
    assert!(
        md.contains("<details>\n<summary>Is SGLang faster than vLLM?</summary>\n"),
        "summary must render inside <details>, got: {md}"
    );
    assert!(
        md.contains("\n\nYes, on prefix-heavy workloads.\n\n</details>"),
        "answer must be a markdown paragraph inside the block, got: {md}"
    );
    assert!(
        md.ends_with("</details>\n\n"),
        "block must close, got: {md}"
    );
}

#[test]
fn test_lower_details_summary_html_escapes_angle_brackets() {
    // Regression: summary text is raw-HTML content, so attacker `<>` and `&`
    // must be HTML-escaped (not markdown-escaped) so a `<` cannot break out
    // of the raw-HTML summary and no literal backslashes appear.
    let nodes = [DomNode::Element {
        tag: "details".into(),
        attrs: vec![],
        children: vec![DomNode::Element {
            tag: "summary".into(),
            attrs: vec![],
            children: vec![DomNode::Text(
                "Is SGLang (v0.3) <fast> & <reliable>?".into(),
            )],
            scores: std::collections::HashMap::new(),
            metadata: std::collections::HashMap::new(),
        }],
        scores: std::collections::HashMap::new(),
        metadata: std::collections::HashMap::new(),
    }];
    let md = MarkdownLowerer::lower(&nodes[0], None);
    assert!(
        md.contains("<summary>Is SGLang (v0.3) &lt;fast&gt; &amp; &lt;reliable&gt;?</summary>"),
        "summary must HTML-escape < > &, got: {md}",
    );
    assert!(
        !md.contains("<summary>Is SGLang (v0.3) <fast>"),
        "raw < must not survive in the summary, got: {md}",
    );
    assert!(
        !md.contains('\\'),
        "no markdown backslashes in the summary, got: {md}",
    );
}

// ── Table cells: parens must NOT be backslash-escaped ────────────────

#[test]
fn test_lower_table_cell_parens_not_escaped() {
    let nodes = [DomNode::Element {
        tag: "table".into(),
        attrs: vec![],
        children: vec![DomNode::Element {
            tag: "tr".into(),
            attrs: vec![],
            children: vec![DomNode::Element {
                tag: "td".into(),
                attrs: vec![],
                children: vec![DomNode::Text("vLLM (PagedAttention)".into())],
                scores: std::collections::HashMap::new(),
                metadata: std::collections::HashMap::new(),
            }],
            scores: std::collections::HashMap::new(),
            metadata: std::collections::HashMap::new(),
        }],
        scores: std::collections::HashMap::new(),
        metadata: std::collections::HashMap::new(),
    }];
    let md = MarkdownLowerer::lower(&nodes[0], None);
    assert!(
        md.lines()
            .any(|l| l.starts_with("|") && l.contains("vLLM (PagedAttention)")),
        "cell parens must not be escaped, got: {md}"
    );
    assert!(
        !md.contains("\\("),
        "no \\( in table cells (some renderers show the backslash literally), got: {md}"
    );
}

#[test]
fn test_lower_table_cell_collapses_embedded_newline() {
    // A cell whose TEXT contains an embedded newline must render as a single
    // pipe row — the newline is collapsed to a space so it cannot break the
    // GFM table structure.
    let nodes = [DomNode::Element {
        tag: "table".into(),
        attrs: vec![],
        children: vec![DomNode::Element {
            tag: "tr".into(),
            attrs: vec![],
            children: vec![DomNode::Element {
                tag: "td".into(),
                attrs: vec![],
                children: vec![DomNode::Text("hello\nworld".into())],
                scores: std::collections::HashMap::new(),
                metadata: std::collections::HashMap::new(),
            }],
            scores: std::collections::HashMap::new(),
            metadata: std::collections::HashMap::new(),
        }],
        scores: std::collections::HashMap::new(),
        metadata: std::collections::HashMap::new(),
    }];
    let md = MarkdownLowerer::lower(&nodes[0], None);
    let content_row = md
        .lines()
        .find(|l| l.starts_with('|') && l.contains("hello world"))
        .unwrap_or_else(|| panic!("a pipe row carrying the cell text must exist, got: {md}"));
    assert!(
        !content_row.contains('\n'),
        "content row must not carry an embedded newline, content row: {content_row}",
    );
}

// ── Heading code spans must not break the link/image neutralizer ──────
//
// The `<code>`/`<img>` arms emit raw content. If that content contains an odd
// number of backticks, escape_heading_links' backtick-toggle ends desynced and a
// following constructed javascript:/data:/vbscript:/file: link is left LIVE. The
// inline `<code>` arm sizes its backtick delimiter to one more than the longest
// interior backtick run so inner runs stay literal, and for the leading/
// trailing-backtick edge case falls back to canonicalized literal text. Either
// way the emitted span is always balanced, so the security invariant (NO live
// dangerous-scheme link out of a heading) holds regardless of code/alt content.

/// Build an `<h2>` with the given inline children (elements or text).
fn heading_h2(children: Vec<DomNode>) -> DomNode {
    DomNode::Element {
        tag: "h2".into(),
        attrs: vec![],
        children,
        scores: std::collections::HashMap::new(),
        metadata: std::collections::HashMap::new(),
    }
}

fn code_el(text: &str) -> DomNode {
    DomNode::Element {
        tag: "code".into(),
        attrs: vec![],
        children: vec![DomNode::Text(text.into())],
        scores: std::collections::HashMap::new(),
        metadata: std::collections::HashMap::new(),
    }
}

fn ref_el(target: &str, text: &str) -> DomNode {
    DomNode::Element {
        tag: "ref".into(),
        attrs: vec![("target".into(), target.into())],
        children: vec![DomNode::Text(text.into())],
        scores: std::collections::HashMap::new(),
        metadata: std::collections::HashMap::new(),
    }
}

fn img_el(src: &str, alt: &str) -> DomNode {
    DomNode::Element {
        tag: "img".into(),
        attrs: vec![("src".into(), src.into()), ("alt".into(), alt.into())],
        children: vec![],
        scores: std::collections::HashMap::new(),
        metadata: std::collections::HashMap::new(),
    }
}

#[test]
fn test_heading_code_odd_backtick_no_live_javascript_link() {
    // <code>a`b</code> emits an odd (3) backtick count. If that odd/even
    // toggle were allowed to desync (the emitted span `a`b` is unbalanced), the
    // trailing javascript: link would be left unescaped. The delimiter sizing
    // must keep the span balanced so the link stays neutralized.
    let h = heading_h2(vec![code_el("a`b"), ref_el("javascript:alert(1)", "go")]);
    let md = MarkdownLowerer::lower(&h, None);
    assert!(
        !md.contains("](javascript:"),
        "javascript: link must be neutralized after odd-backtick code span, got: {md}"
    );
    // The code content must survive as readable text (escaped inner backtick).
    assert!(
        md.contains("`a\\`b`") || md.contains("a`b"),
        "code content a`b must be preserved, got: {md}"
    );
}

#[test]
fn test_heading_two_code_spans_one_with_backtick_then_link() {
    // Two consecutive code spans, the second containing a backtick, then a link.
    // The toggle must end balanced so the trailing link is neutralized.
    let h = heading_h2(vec![
        code_el("x"),
        code_el("y`z"),
        ref_el("javascript:z", "go"),
    ]);
    let md = MarkdownLowerer::lower(&h, None);
    assert!(
        !md.contains("](javascript:"),
        "javascript: link must be neutralized after two code spans, got: {md}"
    );
}

#[test]
fn test_heading_code_odd_backtick_then_safe_https_link() {
    // The security posture neutralizes ALL constructed heading links, not just
    // dangerous-scheme ones. A https link after a backtick code span must also be
    // neutralized (no live link marker out of the heading).
    let h = heading_h2(vec![code_el("a`b"), ref_el("https://example.com", "docs")]);
    let md = MarkdownLowerer::lower(&h, None);
    assert!(
        !md.contains("](https:"),
        "https link must also be neutralized after odd-backtick code span, got: {md}"
    );
}

#[test]
fn test_heading_code_brackets_still_not_escaped() {
    // A regression must NOT re-break: brackets/parens INSIDE a code span
    // (e.g. func(a, b)) are genuine content and must not gain a backslash.
    let h = heading_h2(vec![code_el("func(a, b)"), ref_el("javascript:x", "docs")]);
    let md = MarkdownLowerer::lower(&h, None);
    assert!(
        md.contains("`func(a, b)`"),
        "code span must survive with no escaping, got: {md}"
    );
    assert!(
        !md.contains("func\\"),
        "no backslash inside the code span, got: {md}"
    );
    assert!(
        !md.contains("](javascript:"),
        "link must still be neutralized, got: {md}"
    );
}

#[test]
fn test_heading_code_odd_backtick_data_vbscript_file_links() {
    // data:/vbscript:/file: schemes after an odd-backtick code span must all be
    // neutralized (each fails on the previous code).
    for scheme in [
        "data:text/html,<script>1</script>",
        "vbscript:msgbox",
        "file:///etc/passwd",
    ] {
        let h = heading_h2(vec![code_el("a`b"), ref_el(scheme, "go")]);
        let md = MarkdownLowerer::lower(&h, None);
        let needle = format!("]({}:", scheme.split(':').next().unwrap());
        assert!(
            !md.contains(&needle),
            "{scheme} link must be neutralized after odd-backtick code span, got: {md}"
        );
    }
}

#[test]
fn test_heading_img_alt_with_backtick_then_link() {
    // <img alt> with a backtick can desync the toggle just like <code>; the img
    // arm must escape inner backticks so a following link is still neutralized.
    let h = heading_h2(vec![
        img_el("javascript:z", "a`b"),
        ref_el("javascript:y", "go"),
    ]);
    let md = MarkdownLowerer::lower(&h, None);
    assert!(
        !md.contains("](javascript:"),
        "javascript: link must be neutralized after backtick img alt, got: {md}"
    );
}

// ════════════════════════════════════════════════════════════════════════
// Heading neutralizer: no dangerous-scheme link or image destination may
// NEVER be defeated by code-span content, no matter how degenerate (odd
// backticks, backslash+backtick, trailing backslash, bare backslash,
// brackets). The security invariant for every case below is:
//     NO live `javascript:` / `data:` / `vbscript:` / `file:` link or
//     image src may survive outside a heading.
// Every *_leak_* test expects the DANGEROUS-scheme link to be dead; the
// *_clean_* tests additionally assert code renders without a visible
// backslash. Tests are written FIRST (TDD); each leak test FAILS on the
// buggy pre-fix code and PASSES after the structural refactor.
// ════════════════════════════════════════════════════════════════════════

const DANGEROUS_TARGETS: [&str; 4] = [
    "javascript:alert(1)",
    "data:text/html,<script>1</script>",
    "vbscript:msgbox",
    "file:///etc/passwd",
];

/// Assert no live dangerous-scheme link survives out of the heading.
/// A neutralized link is `\[label\]\(scheme:...)` (inert literal text);
/// a LIVE one is `[label](scheme:...)` / `![alt](src)`.
fn assert_no_live_danger(prefix: &str, md: &str) {
    // A LIVE markdown destination is `](scheme:...)` — for a link `[x](s:...)`
    // OR an image `![alt](s:...)` (the `]` is always immediately followed by
    // `(`). After the neutralizer every bracket/paren gets escaped, so the
    // literal substring `](scheme:` can no longer appear. This single check
    // therefore covers links AND image srcs for every dangerous scheme.
    for t in DANGEROUS_TARGETS {
        let scheme = t.split(':').next().unwrap();
        let live = format!("]({t}");
        assert!(
            !md.contains(&live),
            "{prefix}: live {scheme} link/image destination leaked out of heading: {md}"
        );
    }
}

// ── Odd number of backticks in <code> + a javascript: link ─────────
#[test]
fn test_heading_odd_backtick_code_block_javascript_link_neutralized() {
    let h = heading_h2(vec![code_el("a`b"), ref_el("javascript:alert(1)", "go")]);
    let md = MarkdownLowerer::lower(&h, None);
    assert_no_live_danger("odd-backtick code + javascript link", &md);
}

// ── Odd backtick in <code> + data:/vbscript:/file: links ───────────────
#[test]
fn test_heading_odd_backtick_code_block_other_schemes_neutralized() {
    for &t in &DANGEROUS_TARGETS[1..] {
        let h = heading_h2(vec![code_el("a`b"), ref_el(t, "go")]);
        let md = MarkdownLowerer::lower(&h, None);
        let scheme = t.split(':').next().unwrap();
        assert!(
            !md.contains(&format!("]({t}")),
            "odd-backtick code + {scheme} link leaked: {md}"
        );
    }
}

// ── Backslash immediately before a backtick in <code> ─────────────
#[test]
fn test_heading_backslash_backtick_code_block_javascript_link_neutralized() {
    let h = heading_h2(vec![code_el("a\\`b"), ref_el("javascript:alert(1)", "go")]);
    let md = MarkdownLowerer::lower(&h, None);
    assert_no_live_danger("backslash+backtick code + javascript link", &md);
}

// ── <code> content ending in a single backslash (e.g. C:\ path) ────
#[test]
fn test_heading_trailing_backslash_code_block_javascript_link_neutralized() {
    let h = heading_h2(vec![code_el("C:\\"), ref_el("javascript:alert(1)", "go")]);
    let md = MarkdownLowerer::lower(&h, None);
    assert_no_live_danger("trailing-backslash code (C:\\) + javascript link", &md);
    // Windows-path realism: the code span must still render cleanly (a code
    // span is literal in CommonMark, so a single trailing backslash is fine).
    assert!(
        md.contains("`C:\\`"),
        "code span C:\\ must render cleanly, got: {md}"
    );
}

#[test]
fn test_heading_trailing_backslash_code_block_other_schemes_neutralized() {
    for &t in &DANGEROUS_TARGETS[1..] {
        let h = heading_h2(vec![code_el("C:\\"), ref_el(t, "go")]);
        let md = MarkdownLowerer::lower(&h, None);
        let scheme = t.split(':').next().unwrap();
        assert!(
            !md.contains(&format!("]({t}")),
            "trailing-backslash code + {scheme} link leaked: {md}"
        );
    }
}

// ── Bare backslash in <code> + <img src=javascript:q> (image src leak) ──
#[test]
fn test_heading_bare_backslash_code_then_image_src_neutralized() {
    let h = heading_h2(vec![code_el("\\"), img_el("javascript:q", "pic")]);
    let md = MarkdownLowerer::lower(&h, None);
    assert!(
        !md.contains("](javascript:q)"),
        "bare-backslash code must not leave image src live, got: {md}"
    );
}

// ── Brackets inside <code> + link: must not leak, must stay clean ───────
#[test]
fn test_heading_code_brackets_rendered_clean_link_dead() {
    let h = heading_h2(vec![code_el("x[0]"), ref_el("javascript:z", "docs")]);
    let md = MarkdownLowerer::lower(&h, None);
    assert_no_live_danger("code-brackets+link", &md);
    // Brackets inside a code span are literal content; they must render
    // cleanly with NO visible backslash injected by the neutralizer.
    assert!(
        md.contains("`x[0]`"),
        "code brackets must stay clean, got: {md}"
    );
    assert!(
        !md.contains("x\\["),
        "no backslash inside code span for brackets, got: {md}"
    );
}

// ── <img alt> with an odd backtick + link ───────────────────────────────
#[test]
fn test_heading_img_alt_odd_backtick_link_neutralized() {
    let h = heading_h2(vec![
        img_el("javascript:v", "tick`back"),
        ref_el("javascript:w", "go"),
    ]);
    let md = MarkdownLowerer::lower(&h, None);
    assert_no_live_danger("img-alt odd backtick", &md);
}

// ── <img alt> with a trailing backslash + link ──────────────────────────
#[test]
fn test_heading_img_alt_trailing_backslash_link_neutralized() {
    let h = heading_h2(vec![
        img_el("javascript:u", "C:\\"),
        ref_el("javascript:t", "go"),
    ]);
    let md = MarkdownLowerer::lower(&h, None);
    assert_no_live_danger("img-alt trailing backslash C:\\", &md);
}

// ── Plain heading (2024): must NOT gain a visible \( ─────────────────────
#[test]
fn test_heading_plain_parens_render_without_backslashes() {
    let h = heading_h2(vec![DomNode::Text("(2024)".into())]);
    let md = MarkdownLowerer::lower(&h, None);
    // `lower_inline` single-escapes raw parens -> `\(2024\)`, which CommonMark
    // RENDERS as `(2024)` (the backslash is an invisible escape prefix, NOT a
    // visible glyph). The regression guard is: NO **double** backslash before a
    // paren, which WOULD render a visible backslash.
    assert!(md.contains("## "), "heading marker present, got: {md}");
    assert!(
        !md.contains(r"\\(2024\\)"),
        "plain parens must NOT be double-escaped (visible backslash), got: {md}"
    );
}

// ── <code>func(a, b)</code>: must NOT gain a visible \( inside code ─────
#[test]
fn test_heading_code_parens_render_without_backslashes() {
    let h = heading_h2(vec![
        DomNode::Text("Use ".into()),
        code_el("func(a, b)"),
        DomNode::Text(" now".into()),
    ]);
    let md = MarkdownLowerer::lower(&h, None);
    assert!(
        md.contains("`func(a, b)`"),
        "func(a, b) code must stay clean (no \\(), got: {md}"
    );
    assert!(
        !md.contains("func\\"),
        "no backslash inside func code, got: {md}"
    );
}

// ── Safe https link in a heading: inert text posture ─────────────────────
#[test]
fn test_heading_safe_https_link_rendered_inert() {
    let h = heading_h2(vec![ref_el("https://example.com", "docs")]);
    let md = MarkdownLowerer::lower(&h, None);
    // All constructed links are neutralized to inert literal text in headings.
    assert!(
        md.contains("\\[docs\\]"),
        "https link must be inert text, got: {md}"
    );
    assert!(
        md.contains("https://example.com"),
        "url text preserved, got: {md}"
    );
}

// ── Multiple code spans + a link ────────────────────────────────────────
#[test]
fn test_heading_multiple_code_spans_then_link_neutralized() {
    let h = heading_h2(vec![
        code_el("a\\`b"),
        code_el("x"),
        code_el("C:\\"),
        ref_el("javascript:alert(1)", "go"),
    ]);
    let md = MarkdownLowerer::lower(&h, None);
    assert_no_live_danger("multiple code spans + link", &md);
}

// ── escape_heading_links as a pure function (stateless-bracket pass) ────
#[test]
fn test_escape_heading_links_pure_stateless() {
    // Empty input stays empty.
    assert_eq!(escape_heading_links(""), "");
    // Bracket set all neutralized.
    assert_eq!(escape_heading_links("["), "\\[");
    assert_eq!(escape_heading_links("]"), "\\]");
    assert_eq!(escape_heading_links("("), "\\(");
    assert_eq!(escape_heading_links(")"), "\\)");
    // Already-escaped (raw text) brackets are NOT double-escaped.
    // Realistic already-escaped raw text is preserved exactly:
    assert_eq!(escape_heading_links(r"\(2024\)"), r"\(2024\)");
    // Backslash escape pairs are preserved verbatim.
    assert_eq!(escape_heading_links(r"a\.b\*c"), r"a\.b\*c");
    // A code span's interior is preserved verbatim (brackets stay clean).
    assert_eq!(escape_heading_links("`func(a, b)`"), "`func(a, b)`");
    assert_eq!(escape_heading_links("`x[0]`"), "`x[0]`");
    // Code span + trailing link: delimiter escaped, code intact.
    assert_eq!(
        escape_heading_links("`C:\\`[docs](javascript:x)"),
        "`C:\\`\\[docs\\]\\(javascript:x\\)"
    );
    // Leading/trailing backslash outside a span is preserved (no crash).
    assert_eq!(escape_heading_links(r"\ end"), r"\ end");
    assert_eq!(escape_heading_links(r"start \"), r"start \");
    // Mixed: raw escaped text + constructed link + code span.
    assert_eq!(
        escape_heading_links(r"a \(b\) [c](url) `d[0]`"),
        r"a \(b\) \[c\]\(url\) `d[0]`"
    );
}

// ── Double code span with inner backtick stays balanced + link dead ─────
#[test]
fn test_heading_double_backtick_code_span_link_neutralized() {
    // Content with an inner backtick must be wrapped in MORE backticks so the
    // span is well-formed; the following link must still be neutralized.
    let h = heading_h2(vec![code_el("a`b`c"), ref_el("javascript:m", "go")]);
    let md = MarkdownLowerer::lower(&h, None);
    assert_no_live_danger("double-backtick code span + link", &md);
}

// ── Regression: img alt backtick + javascript link must not leak ────────
#[test]
fn test_heading_img_alt_backtick_javascript_link_neutralized() {
    let h = heading_h2(vec![
        img_el("javascript:z", "a`b"),
        ref_el("javascript:y", "go"),
    ]);
    let md = MarkdownLowerer::lower(&h, None);
    assert_no_live_danger("img-alt backtick + js link", &md);
}

// ════════════════════════════════════════════════════════════════════════
// The image alt attribute shares the same desync class as <code>: a
// backslash immediately before a backtick in the alt desyncs the neutralizer
// attribute) must NOT leave a following dangerous link / image src LIVE
// out of a heading. The <code> arm was fixed earlier; the <img alt> arm
// still used the fragile `.replace('`', "\\`")` escape (which emits a
// bare backtick after consuming the `\\` pair), so the neutralizer must
// survive these inputs and keep every dangerous destination inert.
// and pass after the escape_inline_fragment canonicalization.
// ════════════════════════════════════════════════════════════════════════
//
// ── Backslash+backtick in <img alt> + a dangerous link ────────────
#[test]
fn test_heading_img_alt_backslash_backtick_javascript_link_neutralized() {
    // alt = a, backslash, backtick, b — a backslash before a backtick must
    let h = heading_h2(vec![
        img_el("javascript:v", "a\\`b"),
        ref_el("javascript:alert(1)", "go"),
    ]);
    let md = MarkdownLowerer::lower(&h, None);
    assert_no_live_danger("img-alt backslash+backtick + javascript link", &md);
}

// ── Backslash+backtick in <img alt> + a dangerous image src ────────
#[test]
fn test_heading_img_alt_backslash_backtick_image_src_neutralized() {
    // A following <img src="javascript:q"> must also be neutralized.
    let h = heading_h2(vec![
        img_el("javascript:v", "a\\`b"),
        img_el("javascript:q", "pic"),
    ]);
    let md = MarkdownLowerer::lower(&h, None);
    assert!(
        !md.contains("](javascript:q)"),
        "img-alt backslash+backtick must not leave image src live, got: {md}"
    );
    assert_no_live_danger("img-alt backslash+backtick + image dst", &md);
}

// ── Backslash+backtick in <img alt> + data/vbscript/file links ─────
#[test]
fn test_heading_img_alt_backslash_backtick_other_schemes_neutralized() {
    for &t in &DANGEROUS_TARGETS[1..] {
        let h = heading_h2(vec![img_el("javascript:v", "a\\`b"), ref_el(t, "go")]);
        let md = MarkdownLowerer::lower(&h, None);
        let scheme = t.split(':').next().unwrap();
        assert!(
            !md.contains(&format!("]({t}")),
            "img-alt backslash+backtick + {scheme} link leaked: {md}"
        );
    }
}

// ── Trailing backslash in <img alt> + data/vbscript/file links ─────
#[test]
fn test_heading_img_alt_trailing_backslash_other_schemes_neutralized() {
    for &t in &DANGEROUS_TARGETS[1..] {
        let h = heading_h2(vec![img_el("javascript:v", "C:\\"), ref_el(t, "go")]);
        let md = MarkdownLowerer::lower(&h, None);
        let scheme = t.split(':').next().unwrap();
        assert!(
            !md.contains(&format!("]({t}")),
            "img-alt trailing backslash + {scheme} link leaked: {md}"
        );
    }
}

// ── Property / fuzz sweep (deterministic): the invariant must hold for any
// raw text / code / img-alt built from arbitrary `\`, backticks, and
// brackets, followed by ANY dangerous-scheme <ref> / <a> / <img src> link.
// No live dangerous link/image may survive out of a heading.
#[test]
fn test_heading_property_sweep_no_dangerous_link_or_image_leaks() {
    // Adversarial raw fragments drawn from every desync vector: odd backticks,
    // backslash+backtick, trailing backslash, bare backslash, closing-only
    // brackets, long backtick runs, leading/trailing delimiters.
    let alt_fragments = [
        "a\\`b",     // backslash immediately before backtick
        "tick`back", // odd single backtick
        "C:\\",      // trailing backslash (Windows path)
        "\\",        // bare single backslash
        "`",         // lone backtick
        "```",       // triple backtick run
        "`x[",       // open bracket after backtick
        "x[0]",      // brackets
        "a`,b\\`",   // mixed backtick/backslash
        "\\\\`",     // escaped backslash then backtick (two backslashes then a backtick)
        "",          // empty
        "plain",     // no specials
    ];
    // Every dangerous destination form: <ref> link, <a> link, <img src>.
    for &frag in &alt_fragments {
        for &t in &DANGEROUS_TARGETS {
            // Dangerous <ref> link after an img with this alt.
            let h = heading_h2(vec![img_el("javascript:v", frag), ref_el(t, "go")]);
            let md = MarkdownLowerer::lower(&h, None);
            assert!(
                !md.contains(&format!("]({t}")),
                "alt={frag:?} + {t} ref link leaked: {md}"
            );
            // Dangerous <img src> after an img with this alt.
            let h2 = heading_h2(vec![img_el("javascript:v", frag), img_el(t, "pic")]);
            let md2 = MarkdownLowerer::lower(&h2, None);
            assert!(
                !md2.contains(&format!("]({t}")),
                "alt={frag:?} + {t} img src leaked: {md2}"
            );
        }
        // Dangerous <ref> link after a <code> with this content.
        for &t in &DANGEROUS_TARGETS {
            let h = heading_h2(vec![code_el(frag), ref_el(t, "go")]);
            let md = MarkdownLowerer::lower(&h, None);
            assert!(
                !md.contains(&format!("]({t}")),
                "code={frag:?} + {t} ref link leaked: {md}"
            );
            let h2 = heading_h2(vec![code_el(frag), img_el(t, "pic")]);
            let md2 = MarkdownLowerer::lower(&h2, None);
            assert!(
                !md2.contains(&format!("]({t}")),
                "code={frag:?} + {t} img src leaked: {md2}"
            );
        }
    }
}

// ── Opposite: no OVER-escaping. Plain parens and code-with-brackets render
// with NO visible backslash, and a safe https link is preserved.
#[test]
fn test_heading_neutralizer_does_not_over_escape() {
    // Plain (2024) — single-escaped, no double backslash (no visible \().
    let h = heading_h2(vec![DomNode::Text("Quantum (2024)".into())]);
    let md = MarkdownLowerer::lower(&h, None);
    assert!(
        !md.contains(r"\\(2024\\)"),
        "plain parens must not be double-escaped, got: {md}"
    );
    // Code with brackets/parens stays clean (no backslash inside the span).
    let h2 = heading_h2(vec![code_el("func(a, b)"), code_el("x[0]")]);
    let md2 = MarkdownLowerer::lower(&h2, None);
    assert!(
        md2.contains("`func(a, b)`"),
        "func parens clean, got: {md2}"
    );
    assert!(md2.contains("`x[0]`"), "code brackets clean, got: {md2}");
    assert!(
        !md2.contains("func\\"),
        "no visible backslash in func, got: {md2}"
    );
    // Safe https link in heading stays inert literal text.
    let h3 = heading_h2(vec![ref_el("https://example.com", "docs")]);
    let md3 = MarkdownLowerer::lower(&h3, None);
    assert!(
        md3.contains("https://example.com"),
        "url text kept, got: {md3}"
    );
    assert!(
        !md3.contains("](https://"),
        "no live https link, got: {md3}"
    );
}

// ════════════════════════════════════════════════════════════════════════
// Angle-bracket canonicalization (autolink / raw-HTML injection)
//
// `escape_inline_fragment` is the single canonicalization point for raw
// inline content. A raw `<`/`>` can form a LIVE CommonMark autolink
// `<scheme:...>` or raw HTML out of a heading or body line. These tests
// pin the fix: `<`/`>` are escaped (`\<`/`\>`) so angle-bracket content
// can never reconstruct an autolink/HTML tag, and a trailing backslash never
// survives as a lone escape-pair opener. Written FIRST (TDD); the
// angle-bracket cases FAIL on the pre-fix stub and PASS after the fix.
// ════════════════════════════════════════════════════════════════════════

/// Assert no LIVE `<scheme:...>` autolink opener survives. After the fix every
/// `<` is backslash-escaped (`\<`), so a live opener would be a `<` NOT
/// preceded by a backslash. A bare substring check is insufficient because
/// `\<javascript:` contains `<javascript:` as a substring; we verify the
/// character immediately before each `<` is a backslash.
fn assert_no_live_autolink(prefix: &str, md: &str) {
    let needle = "<javascript:";
    let mut from = 0;
    while let Some(pos) = md[from..].find(needle) {
        let abs = from + pos;
        let prev = if abs == 0 {
            None
        } else {
            md[..abs].chars().last()
        };
        assert!(
            prev == Some('\\'),
            "{prefix}: live autolink opener <javascript: present with prev={prev:?}, got: {md}"
        );
        from = abs + 1;
    }
}

/// Assert no RAW (unescaped) `<tag` opener survives anywhere in `md`.
fn assert_no_live_tag(prefix: &str, md: &str, tag: &str) {
    let needle = format!("<{tag}");
    let mut from = 0;
    while let Some(pos) = md[from..].find(&needle) {
        let abs = from + pos;
        let prev = if abs == 0 {
            None
        } else {
            md[..abs].chars().last()
        };
        assert!(
            prev == Some('\\'),
            "{prefix}: raw <{tag} tag present with prev={prev:?}, got: {md}"
        );
        from = abs + 1;
    }
}

// ── escape_inline_fragment as a canonicalizer ───────────────────────────
#[test]
fn test_escape_inline_fragment_escapes_angle_brackets() {
    // Exact equality: both angle brackets are backslash-escaped and the parens
    // retain escape_markdown's single-escape.
    assert_eq!(
        escape_inline_fragment("<javascript:alert(1)>"),
        r"\<javascript:alert\(1\)\>"
    );
}

#[test]
fn test_escape_inline_fragment_escapes_html_tag() {
    let out = escape_inline_fragment("<img src=x onerror=alert(1)>");
    assert_eq!(
        out, r"\<img src=x onerror=alert\(1\)\>",
        "raw HTML tag must be backslash-escaped, got: {out:?}"
    );
}

#[test]
fn test_escape_inline_fragment_trailing_backslash_doubled() {
    // A lone trailing backslash must be doubled so it cannot form an
    // escape-pair that eats a following closing delimiter.
    let out = escape_inline_fragment("C:\\");
    assert!(
        out.ends_with(r"\\"),
        "trailing backslash must be escaped as literal \\\\, got: {out:?}"
    );
    assert!(
        !out.ends_with('\\') || out.ends_with(r"\\"),
        "no lone unescaped trailing backslash, got: {out:?}"
    );
}

#[test]
fn test_escape_inline_fragment_keeps_escape_markdown_escaping() {
    // Must retain all prior escape_markdown behavior (no regression).
    assert_eq!(escape_inline_fragment("(2024)"), r"\(2024\)");
    assert_eq!(escape_inline_fragment("a`b"), r"a\`b");
    assert_eq!(escape_inline_fragment("a\\b"), r"a\\b");
    // `.` and `+` stay unescaped (not line-start list markers here).
    assert_eq!(escape_inline_fragment("3.1 30%+"), "3.1 30%+");
}

// ── Heading text raw angle-bracket autolink (primary vector) ────────────
#[test]
fn test_heading_angle_bracket_autolink_escaped() {
    let h = heading_h2(vec![DomNode::Text("<javascript:alert(1)>".into())]);
    let md = MarkdownLowerer::lower(&h, None);
    assert_no_live_autolink("heading raw text", &md);
    assert!(
        md.contains(r"\<javascript:"),
        "< must be backslash-escaped in heading text, got: {md}"
    );
    assert!(md.contains("## "), "heading marker present, got: {md}");
}

#[test]
fn test_heading_angle_bracket_html_tag_escaped() {
    // Partial-tag HTML in heading text must not become raw HTML out of the
    // ATX line (e.g. `<img src=x onerror=...>`).
    let h = heading_h2(vec![DomNode::Text("<img src=x onerror=alert(1)>".into())]);
    let md = MarkdownLowerer::lower(&h, None);
    assert_no_live_tag("heading html", &md, "img");
    assert!(md.contains(r"\<img"), "tag opener escaped, got: {md}");
}

#[test]
fn test_head_rend_heading_angle_bracket_autolink_escaped() {
    // tf_convert_headings path (head rend=hX) must also escape angle brackets.
    let nodes = [DomNode::Element {
        tag: "head".into(),
        attrs: vec![("rend".into(), "h2".into())],
        children: vec![DomNode::Text("<javascript:alert(1)>".into())],
        scores: std::collections::HashMap::new(),
        metadata: std::collections::HashMap::new(),
    }];
    let md = MarkdownLowerer::lower(&nodes[0], None);
    assert_no_live_autolink("head-rend heading", &md);
    assert!(
        md.contains(r"\<javascript:"),
        "< escaped in head-rend heading, got: {md}"
    );
}

// ── <img alt> raw angle-bracket content (escape_inline_fragment path) ───
#[test]
fn test_heading_img_alt_angle_brackets_escaped() {
    let h = heading_h2(vec![img_el("pic.png", "<script>alert(1)</script>")]);
    let md = MarkdownLowerer::lower(&h, None);
    assert_no_live_tag("img-alt", &md, "script");
    assert!(
        md.contains(r"\<script"),
        "alt < escaped via escape_inline_fragment, got: {md}"
    );
}

// ── Body (paragraph) raw angle-bracket content ─────────────────────────
#[test]
fn test_paragraph_angle_bracket_html_escaped() {
    // Body text must also be canonicalized so a raw <img x> cannot inject
    // HTML / autolinks below a heading.
    let p = DomNode::Element {
        tag: "p".into(),
        attrs: vec![],
        children: vec![DomNode::Text("<img src=x onerror=alert(1)>".into())],
        scores: std::collections::HashMap::new(),
        metadata: std::collections::HashMap::new(),
    };
    let md = MarkdownLowerer::lower(&p, None);
    assert_no_live_tag("body html", &md, "img");
    assert!(md.contains(r"\<img"), "body < escaped, got: {md}");
}

#[test]
fn test_paragraph_angle_bracket_autolink_escaped() {
    let p = DomNode::Element {
        tag: "p".into(),
        attrs: vec![],
        children: vec![DomNode::Text("<javascript:alert(1)>".into())],
        scores: std::collections::HashMap::new(),
        metadata: std::collections::HashMap::new(),
    };
    let md = MarkdownLowerer::lower(&p, None);
    assert_no_live_autolink("body text", &md);
    assert!(md.contains(r"\<javascript:"), "body < escaped, got: {md}");
}
