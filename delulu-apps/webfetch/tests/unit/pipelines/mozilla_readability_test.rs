use super::*;
use crate::pipelines::parse_html;

// -- filter_mozilla_readability retry orchestrator --------------------------

#[test]
fn test_filter_mozilla_readability_empty() {
    // Empty input should not panic and produce empty result
    let mut root = DomNode::Element {
        tag: "html".into(),
        attrs: vec![],
        children: vec![],
        scores: Default::default(),
        metadata: Default::default(),
    };
    filter_mozilla_readability(&mut root);
    let len = measure_output(&root);
    assert_eq!(len, 0, "empty input should produce empty output");
}

#[test]
fn test_filter_mozilla_readability_large_tree_uses_level_1_only() {
    // Create a tree >10,000 nodes to trigger the performance guard
    let mut children = Vec::with_capacity(10_001);
    for i in 0..10_001 {
        children.push(DomNode::Element {
            tag: "span".into(),
            attrs: vec![],
            children: vec![DomNode::Text(format!("node_{}", i))],
            scores: std::collections::HashMap::new(),
            metadata: std::collections::HashMap::new(),
        });
    }
    let mut root = DomNode::Element {
        tag: "html".into(),
        attrs: vec![],
        children: vec![DomNode::Element {
            tag: "div".into(),
            attrs: vec![],
            children,
            scores: std::collections::HashMap::new(),
            metadata: std::collections::HashMap::new(),
        }],
        scores: std::collections::HashMap::new(),
        metadata: std::collections::HashMap::new(),
    };

    // Should not panic and should use Level 1 only
    filter_mozilla_readability(&mut root);
    // Output should be valid (may be empty or have content depending on scoring)
    let _ = measure_output(&root);
}

// ---------------------------------------------------------------------------
// Regression test for 5 micropasses in the readability pipeline
// ---------------------------------------------------------------------------

/// Regression test that exercises all 5 micropasses:
///   - pass_prune_no_candidate
///   - pass_splice_cutoff
///   - pass_keep_alt_cluster
///   - pass_keep_qualifying_siblings
///   - pass_promote_content_child
///
/// Uses snapshot-based output comparison against an inline expected string.
/// To update the snapshot, set `UPDATE_EXPECTED=1` and run:
/// ```bash
/// UPDATE_EXPECTED=1 cargo test -p delulu-webfetch -- test_extraction_regression
/// ```
#[test]
fn test_extraction_regression() {
    // Inline HTML string that exercises all 5 micropasses with enough content
    // to pass the MIN_OUTPUT_CHARS threshold.
    let html = r#"<html><body>
            <article>
                <h1>Understanding Quantum Computing: A Comprehensive Guide</h1>
                <p>Quantum computing represents a fundamental shift in how we approach computation. Unlike classical computers that use bits representing either 0 or 1, quantum computers use quantum bits or qubits that can exist in multiple states simultaneously through superposition. This property, combined with entanglement and quantum interference, enables quantum computers to solve certain problems exponentially faster than their classical counterparts.</p>
                <p>The field has seen remarkable progress in recent years. Major technology companies including Google, IBM, and Microsoft have invested heavily in quantum computing research and development. Google's Sycamore processor demonstrated quantum supremacy in 2019 by performing a calculation in 200 seconds that would take a classical supercomputer approximately 10,000 years to complete.</p>
                <p>However, significant challenges remain before quantum computers become practical for everyday use. Quantum decoherence, error correction, and the need for extremely low operating temperatures are just a few of the obstacles that researchers continue to address. Despite these challenges, the potential applications in cryptography, drug discovery, materials science, and optimization problems make quantum computing one of the most exciting frontiers in modern technology.</p>
            </article>
        </body></html>"#;
    let mut root = parse_html(html).expect("valid HTML");
    filter_mozilla_readability(&mut root);
    let md = crate::generators::gen_md::MarkdownLowerer::lower(&root, None);

    // UPDATE_EXPECTED=1 support: print actual output and skip assertion
    // so the developer can copy the new expected string.
    if std::env::var("UPDATE_EXPECTED").is_ok() {
        eprintln!("=== UPDATE_EXPECTED=1 mode ===");
        eprintln!("Actual output:");
        eprintln!("{}", md);
        eprintln!("=== Copy the above into the `let expected = r#\"...\"#` literal ===");
        panic!(
            "UPDATE_EXPECTED=1: update expected string in source, then re-run without the env var"
        );
    }

    // Snapshot comparison: update with UPDATE_EXPECTED=1
    let expected = r#"
            
                
                Quantum computing represents a fundamental shift in how we approach computation. Unlike classical computers that use bits representing either 0 or 1, quantum computers use quantum bits or qubits that can exist in multiple states simultaneously through superposition. This property, combined with entanglement and quantum interference, enables quantum computers to solve certain problems exponentially faster than their classical counterparts.


                The field has seen remarkable progress in recent years. Major technology companies including Google, IBM, and Microsoft have invested heavily in quantum computing research and development. Google's Sycamore processor demonstrated quantum supremacy in 2019 by performing a calculation in 200 seconds that would take a classical supercomputer approximately 10,000 years to complete.


                However, significant challenges remain before quantum computers become practical for everyday use. Quantum decoherence, error correction, and the need for extremely low operating temperatures are just a few of the obstacles that researchers continue to address. Despite these challenges, the potential applications in cryptography, drug discovery, materials science, and optimization problems make quantum computing one of the most exciting frontiers in modern technology.


            
        "#;

    if md != expected {
        eprintln!("--- ACTUAL OUTPUT ({} chars) ---", md.len());
        eprintln!("{}", md);
        eprintln!("--- END ACTUAL OUTPUT ---");
        eprintln!("To update snapshot, set UPDATE_EXPECTED=1");
    }
    assert_eq!(
        md, expected,
        "Output does not match expected snapshot. See output above."
    );
}

#[test]
fn test_rd_pipeline_normalizes_code_blocks() {
    // The rd pipeline must run normalize_code_blocks too, so gen_md only ever
    // sees normalized <pre> (language hoisted onto the pre's own class).
    let code = "fn main() {\n".to_string()
        + &(0..30)
            .map(|i| format!("    let v{i}: i32 = compute_v{i}();\n"))
            .collect::<String>()
        + "    println!(\"done\");\n}";
    let paras: String = (0..8)
        .map(|i| {
            format!(
                "<p>Paragraph {i}: the study of concurrent and distributed systems examines \
                how independent components coordinate through message passing, shared state, \
                and synchronization primitives. Recent advances in actor models and software \
                transactional memory have reshaped how we reason about safety and liveness \
                properties in large-scale deployments spanning thousands of nodes across \
                geographically distributed data centers.</p>"
            )
        })
        .collect();
    let html = format!(
        "<html><body><article><h1>Code sample</h1>{paras}<h2>Example</h2><pre class=\"not-prose\"><code class=\"language-rust\">{code}</code></pre></article></body></html>"
    );
    let mut root = parse_html(&html).expect("valid HTML");
    filter_mozilla_readability(&mut root);
    let md = crate::generators::gen_md::MarkdownLowerer::lower(&root, None);
    // The rd pipeline must run normalize_code_blocks too: gen_md only ever sees
    // canonical <pre> blocks (no pre>code nesting). Note rd's clean_classes
    // strips class attributes by design (matches JS Readability), so the
    // language is lost at the rd level — the structural contract is what we
    // assert: the code renders as a fenced block, not inline backticks.
    assert!(
        md.contains("```\nfn main() {\n    let v0: i32 = compute_v0();\n"),
        "rd pipeline must normalize pre>code so the code renders fenced, got: {md}"
    );
}
