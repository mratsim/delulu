//! Per-pass length measurement for a single fixture.
//! Run: DIAG_ARGS="politico-com-retirement" cargo test -p delulu-webfetch --test diag_tf_trace --features diagnostic -- --nocapture --ignored

use delulu_webfetch::pipelines::DomNode;
use delulu_webfetch::pipelines::trafilatura::{TF_BALANCED, TF_BALANCED_NAMES};

#[path = "test_utils.rs"]
mod test_utils;
use test_utils::{fixture_dir, load_test_case_tf};

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

    let (nodes, _, _) = load_test_case_tf(fixture_name);

    println!("================================================================");
    println!("  PER-PASS LENGTH TRACE: {}", fixture_name);
    println!("================================================================");

    // Measure initial state
    print_tree_stats(&nodes, "Initial (raw DOM)");

    // Name labels come from the same source as the pass list so they cannot
    // drift (Issue D: the old hardcoded 17-entry list diverged from the real
    // TF_BALANCED set, which includes *_with_backup wrappers).
    let names = *TF_BALANCED_NAMES;

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
