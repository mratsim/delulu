//! Per-pass length measurement for a single fixture.
//! Run: DIAG_ARGS="politico-com-retirement" cargo test -p delulu-webfetch --test diag_tf_trace --features diagnostic -- --nocapture --ignored

use std::path::PathBuf;

use delulu_webfetch::pipelines::trafilatura::TF_BALANCED;
use delulu_webfetch::pipelines::DomNode;

#[path = "test_utils.rs"]
mod test_utils;
use test_utils::{fixture_dir, load_test_case_tf, tf_count_text_chars};

fn count_text_chars(node: &DomNode) -> usize {
    match node {
        DomNode::Text(t) => t.len(),
        DomNode::Element { children, .. } => children.iter().map(count_text_chars).sum(),
        _ => 0,
    }
}

fn print_tree_stats(node: &DomNode, label: &str) {
    let text_len = count_text_chars(node);
    println!("  {:40} {}", label, text_len);
}

#[test]
#[ignore]
fn diag_tf_trace() {
    let args_str = std::env::var("DIAG_ARGS").unwrap_or_default();
    let fixture_name = args_str.trim();
    if fixture_name.is_empty() {
        eprintln!("Usage: DIAG_ARGS=\"<fixture-name>\" cargo test ...");
        return;
    }

    let dir = fixture_dir();
    let case_dir = dir.join(fixture_name);
    if !case_dir.exists() {
        eprintln!("Fixture '{}' not found", fixture_name);
        return;
    }

    let (mut nodes, _, _) = load_test_case_tf(fixture_name);

    println!("================================================================");
    println!("  PER-PASS LENGTH TRACE: {}", fixture_name);
    println!("================================================================");

    // Measure initial state
    print_tree_stats(&nodes, "Initial (raw DOM)");

    // Apply tf_protect_content_forms
    // (it's in the pipeline but doesn't change output length directly)
    let before = nodes.clone();

    // Run each pass manually with length measurement
    let names = [
        "tf_protect_content_forms",
        "tf_extract_script_templates",
        "tf_remove_cleaned",
        "tf_remove_teaser",
        "tf_remove_unlikely_candidates",
        "tf_strip_unwrapped",
        "tf_remove_empty_cut",
        "tf_filter_by_link_density",
        "tf_convert_headings",
        "tf_convert_lists",
        "tf_convert_quotes",
        "tf_convert_formatting",
        "tf_convert_breaks",
        "tf_convert_refs_and_details",
        "tf_canonicalize_strip_non_content",
        "tf_isolate_content_container",
        "tf_canonicalize_unwrap_containers",
        "tf_filter_tag_catalog",
    ];

    // We need to import the actual functions
    // Since they're module-level functions, let's use the TF_BALANCED passes
    // but measure between each one

    let mut tree = nodes;
    let passes = *TF_BALANCED;

    for (i, pass_fn) in passes.iter().enumerate() {
        let name = if i < names.len() { names[i] } else { "unknown" };
        pass_fn(&mut tree);
        let len = count_text_chars(&tree);
        println!("  After {:40} {}", name, len);
    }

    println!("================================================================");
    println!("  FINAL text length: {}", count_text_chars(&tree));

    // Also run the full filter_trafilatura for comparison
    let (mut nodes2, _, _) = load_test_case_tf(fixture_name);
    delulu_webfetch::pipelines::trafilatura::filter_trafilatura(&mut nodes2);
    println!("  filter_trafilatura output: {}", count_text_chars(&nodes2));
}
