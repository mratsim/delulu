//! Trafilatura pipeline diagnostic tool (run via cargo test, not cargo run).
//!
//! # Usage
//!
//! ```bash
//! # Batch mode — classify all available fixtures
//! DIAG_ARGS="batch" cargo test --release --test diag_tf_pipeline -- --nocapture --ignored
//!
//! # Batch with custom fixture directory
//! DIAG_ARGS="batch --fixtures-dir /path/to/fixtures" cargo test --release --test diag_tf_pipeline -- --nocapture --ignored
//!
//! # Deep-dive into a single test case (skeleton — not yet implemented)
//! DIAG_ARGS="deep-dive <fixture-name>" cargo test --release --test diag_tf_pipeline -- --nocapture --ignored
//! ```
//!
//! ## How it works
//!
//! This is a `[[test]]` target with `#[ignore]` so it:
//! - Is NOT compiled by `cargo build` (CI/CD stays clean)
//! - Is NOT run by plain `cargo test`
//! - Only runs explicitly with `--ignored`
//! - Reads arguments from the `DIAG_ARGS` env var
//! - Uses `zstd` from `[dev-dependencies]` only
//!
//! ## Flags
//!
//! - `--nocapture` — lets you see stdout/stderr output (otherwise cargo test hides it)
//! - `--ignored` — required to run `#[ignore]`d tests
//! - `--fixtures-dir <path>` — optional, overrides the default fixture path

use std::path::PathBuf;

use delulu_webfetch::generators::gen_md::MarkdownLowerer;
use delulu_webfetch::pipelines::trafilatura::filter_trafilatura;

#[path = "test_utils.rs"]
mod test_utils;

use test_utils::{
    classify_output, compute_confusion_matrix, fixture_dir,
    normalize_output, tf_count_text_chars, Classification,
};

// ---------------------------------------------------------------------------
// Batch result
// ---------------------------------------------------------------------------

/// Result of processing a single fixture in batch mode.
#[derive(Debug)]
struct BatchResult {
    name: String,
    output_len: usize,
    expected_len: usize,
    ratio: f64,
    classification: Classification,
    precision: f64,
    recall: f64,
    f1: f64,
    truncated: bool,
}

// ---------------------------------------------------------------------------
// Batch mode
// ---------------------------------------------------------------------------

/// Run batch diagnostics on all fixtures.
///
/// NOTE: The spec defines `run_batch(fixture_names: &[String]) -> Vec<BatchResult>`,
/// but this implementation takes a fixtures directory override and prints TSV
/// directly to stdout. The functional behavior is equivalent — all fixtures in
/// the directory are processed and results are printed as TSV — but the API
/// contract was simplified for ergonomics.
fn run_batch(fixtures_arg: &Option<PathBuf>) {
    let dir = if let Some(d) = fixtures_arg {
        d.clone()
    } else {
        fixture_dir()
    };

    let mut entries: Vec<_> = std::fs::read_dir(&dir)
        .expect("fixture dir")
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_dir())
        .map(|e| e.file_name().to_string_lossy().to_string())
        .collect();
    entries.sort();

    if entries.is_empty() {
        eprintln!("No fixture directories found in {:?}", dir);
        eprintln!("Run `convert_eval_fixtures.py` first to seed fixtures.");
        return;
    }

    // Determine column widths from data
    let mut max_name_len = "fixture".len();
    let mut max_class_len = "classification".len();
    for name in &entries {
        if name.len() > max_name_len {
            max_name_len = name.len();
        }
    }
    // Pre-compute classifications for width estimation
    // (we'll collect results first, then print)
    let mut results: Vec<BatchResult> = Vec::new();
    for name in &entries {
        // Load fixture via shared helper
        let (root, expected_md, annotations) = test_utils::load_test_case_tf(name);

        // Parse and run pipeline
        let mut nodes = root;
        filter_trafilatura(&mut nodes);

        // Convert to markdown
        let output_md = MarkdownLowerer::lower(&nodes, None);

        // Normalize for comparison
        let norm_output = normalize_output(&output_md);
        let norm_expected = normalize_output(&expected_md);

        let out_len = norm_output.len();
        let exp_len = norm_expected.len();
        let ratio = if exp_len > 0 {
            out_len as f64 / exp_len as f64
        } else {
            0.0
        };

        let classification = classify_output(out_len, exp_len);

        // Compute confusion matrix if annotations available
        let (precision, recall, f1) = if let Some(ref ann) = annotations {
            let cm = compute_confusion_matrix(&norm_output, &norm_expected, ann);
            (cm.precision(), cm.recall(), cm.f1())
        } else {
            (0.0, 0.0, 0.0)
        };

        let truncated = tf_count_text_chars(&nodes) > out_len;

        results.push(BatchResult {
            name: name.clone(),
            output_len: out_len,
            expected_len: exp_len,
            ratio,
            classification,
            precision,
            recall,
            f1,
            truncated,
        });
    }

    // Update classification width from actual data
    for r in &results {
        let cl = format!("{}", r.classification);
        if cl.len() > max_class_len {
            max_class_len = cl.len();
        }
    }

    // Print padded header
    println!(
        "{name:width$}  {output:>10}  {expected:>10}  {ratio:>6}  {class:class_width$}  {prec:>7}  {rec:>7}  {f1:>7}  {trunc}",
        name = "fixture", width = max_name_len,
        output = "output_len", expected = "expected_len",
        ratio = "ratio",
        class = "classification", class_width = max_class_len,
        prec = "precision", rec = "recall", f1 = "f1",
        trunc = "truncated",
    );

    for r in &results {
        let cl = format!("{}", r.classification);
        println!(
            "{name:width$}  {output:>10}  {expected:>10}  {ratio:>6.4}  {class:class_width$}  {prec:>7.4}  {rec:>7.4}  {f1:>7.4}  {trunc}",
            name = r.name, width = max_name_len,
            output = r.output_len, expected = r.expected_len,
            ratio = r.ratio,
            class = cl, class_width = max_class_len,
            prec = r.precision, rec = r.recall, f1 = r.f1,
            trunc = if r.truncated { "true" } else { "false" },
        );
    }

    // Print summary
    eprintln!();
    eprintln!("================================================================");
    eprintln!("  BATCH CATEGORIZATION SUMMARY");
    eprintln!("================================================================");
    eprintln!("  Total cases:    {}", results.len());

    let mut pass_count = 0u32;
    let mut over_severe = 0u32;
    let mut over_moderate = 0u32;
    let mut over_mild = 0u32;
    let mut under_severe = 0u32;
    let mut under_moderate = 0u32;
    let mut under_mild = 0u32;

    for r in &results {
        match r.classification {
            Classification::Pass => pass_count += 1,
            Classification::OverFiltering(sev) => match sev {
                test_utils::Severity::Severe => over_severe += 1,
                test_utils::Severity::Moderate => over_moderate += 1,
                test_utils::Severity::Mild => over_mild += 1,
            },
            Classification::UnderFiltering(sev) => match sev {
                test_utils::Severity::Severe => under_severe += 1,
                test_utils::Severity::Moderate => under_moderate += 1,
                test_utils::Severity::Mild => under_mild += 1,
            },
        }
    }

    eprintln!("  Passed:         {pass_count}");
    eprintln!();
    eprintln!("  OVER-FILTERING (severe):    {over_severe:>3}  (<30% content)");
    eprintln!("  OVER-FILTERING (moderate):  {over_moderate:>3}  (30-60% content)");
    eprintln!("  OVER-FILTERING (mild):      {over_mild:>3}  (60-80% content)");
    eprintln!("  UNDER-FILTERING (severe):   {under_severe:>3}  (>300% content)");
    eprintln!("  UNDER-FILTERING (moderate): {under_moderate:>3}  (200-300% content)");
    eprintln!("  UNDER-FILTERING (mild):     {under_mild:>3}  (120-200% content)");
    eprintln!();
    let total_over = over_severe + over_moderate + over_mild;
    let total_under = under_severe + under_moderate + under_mild;
    eprintln!("  TOTAL OVER-FILTERING:  {total_over}");
    eprintln!("  TOTAL UNDER-FILTERING: {total_under}");
    eprintln!("================================================================");
}

// ---------------------------------------------------------------------------
// Deep-dive mode (skeleton)
// ---------------------------------------------------------------------------

fn run_deep_dive(case_name: &str, _fixtures_arg: &Option<PathBuf>) {
    eprintln!("Deep-dive mode for '{case_name}' is not yet implemented.");
    eprintln!("This will be implemented in a future phase to provide detailed");
    eprintln!("per-pass breakdown, intermediate DOM states, and diff output.");
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

#[test]
#[ignore]
fn diag_main() {
    let args_str = std::env::var("DIAG_ARGS").unwrap_or_default();
    let args: Vec<String> = args_str.split_whitespace().map(String::from).collect();
    if args.is_empty() {
        eprintln!("Usage: DIAG_ARGS=\"batch [--fixtures-dir <path>]\" cargo test ...");
        eprintln!(
            "       DIAG_ARGS=\"deep-dive <case-name> [--fixtures-dir <path>]\" cargo test ..."
        );
        eprintln!("See the top of this file for full instructions.");
        return;
    }

    let mut fixtures_arg: Option<PathBuf> = None;
    let mut mode_args: Vec<String> = Vec::new();
    let mut i = 0;
    while i < args.len() {
        if args[i] == "--fixtures-dir" {
            i += 1;
            fixtures_arg = Some(PathBuf::from(args[i].clone()));
        } else {
            mode_args.push(args[i].clone());
        }
        i += 1;
    }

    if mode_args.is_empty() {
        eprintln!("Error: missing mode. Use 'batch' or 'deep-dive'.");
        eprintln!(
            "Example: DIAG_ARGS=\"batch\" cargo test --test diag_tf_pipeline -- --nocapture --ignored"
        );
        return;
    }

    match mode_args[0].as_str() {
        "batch" => run_batch(&fixtures_arg),
        "deep-dive" => {
            if mode_args.len() < 2 {
                eprintln!("Error: deep-dive requires a test case name.");
                eprintln!(
                    "Example: DIAG_ARGS=\"deep-dive <fixture-name>\" cargo test --test diag_tf_pipeline -- --nocapture --ignored"
                );
                return;
            }
            run_deep_dive(&mode_args[1], &fixtures_arg);
        }
        _ => {
            eprintln!(
                "Error: unknown mode '{}'. Use 'batch' or 'deep-dive'.",
                mode_args[0]
            );
        }
    }
}
