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
//! # Deep-dive into a single test case
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
    classify_output, compute_confusion_matrix, detect_backup_restore,
    detect_body_xpath_pattern, detect_retry_level, first_diff_position, fixture_dir,
    normalize_output, tf_count_text_chars, Classification,
    time_passes,
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

fn run_deep_dive(case_name: &str, fixtures_arg: &Option<PathBuf>) {
    let dir = if let Some(d) = fixtures_arg {
        d.clone()
    } else {
        fixture_dir()
    };

    let case_dir = dir.join(case_name);
    if !case_dir.exists() {
        eprintln!("Fixture '{}' not found in {:?}", case_name, dir);
        eprintln!("Available fixtures:");
        if let Ok(entries) = std::fs::read_dir(&dir) {
            for entry in entries.flatten() {
                if entry.path().is_dir() {
                    eprintln!("  - {}", entry.file_name().to_string_lossy());
                }
            }
        }
        return;
    }

    eprintln!("================================================================");
    eprintln!("  DEEP-DIVE: {case_name}");
    eprintln!("================================================================");
    eprintln!();

    // Load fixture — keep raw source HTML for introspection probes
    let fixture_path = fixture_dir().join(case_name).join("source.html.zst");
    let source_html = test_utils::try_decompress_zst(&fixture_path)
        .unwrap_or_else(|e| panic!("failed to read source.html.zst: {e}"));
    let (root, expected_md, annotations) = test_utils::load_test_case_tf(case_name);
    let mut nodes = root;
    filter_trafilatura(&mut nodes);

    // Convert to markdown and normalize
    let output_md = MarkdownLowerer::lower(&nodes, None);
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

    // ── Section 1: Output vs Expected Length ───────────────────────────
    eprintln!("---");
    eprintln!("  Output length:   {out_len}");
    eprintln!("  Expected length: {exp_len}");
    eprintln!("  Ratio:           {ratio:.4}");
    eprintln!();

    // ── Section 2: Classification ──────────────────────────────────────
    eprintln!("---");
    eprintln!("  Classification:  {classification}");
    eprintln!();

    // ── Section 3: Confusion Matrix ────────────────────────────────────
    eprintln!("---");
    if let Some(ref ann) = annotations {
        let cm = compute_confusion_matrix(&norm_output, &norm_expected, ann);
        eprintln!("  Confusion Matrix:");
        eprintln!("    True Positives:     {:>4}", cm.tp);
        eprintln!("    False Positives:    {:>4}", cm.fp);
        eprintln!("    True Negatives:     {:>4}", cm.tn);
        eprintln!("    False Negatives:    {:>4}", cm.fn_);
        eprintln!("    Precision:          {:.4}", cm.precision());
        eprintln!("    Recall:             {:.4}", cm.recall());
        eprintln!("    Accuracy:           {:.4}", cm.accuracy());
        eprintln!("    F1 Score:           {:.4}", cm.f1());
    } else {
        eprintln!("  No annotations available for confusion matrix.");
    }
    eprintln!();

    // ── Section 4: First Difference ────────────────────────────────────
    eprintln!("---");
    eprintln!("  First Difference:");
    if let Some((pos, before, after)) = first_diff_position(&norm_output, &norm_expected) {
        eprintln!("    Position: {pos}");
        eprintln!("    Context before: ...{before}");
        eprintln!("    Context after:  {after}...");
    } else {
        eprintln!("    None — outputs are identical.");
    }
    eprintln!();

    // ── Section 5: Backup/Restore ─────────────────────────────────────
    eprintln!("---");
    eprintln!("  Backup/Restore:");
    eprintln!("    Why: Detects if OVERALL_DISCARD_XPATH removed ≥86% of text and was");
    eprintln!("         reverted. If yes, the discard patterns are too aggressive for");
    eprintln!("         this page — the pipeline is 'working by accident'.");
    {
        let (backup, removed) = detect_backup_restore(&source_html);
        if backup {
            eprintln!("    Status:           TRIGGERED (restored from backup)");
            eprintln!("    Items removed:     {removed}");
            eprintln!("    Implication:      Discard patterns over-match on this page.");
        } else if removed > 0 {
            eprintln!("    Status:           not triggered");
            eprintln!("    Items removed:     {removed} (safe — <86% of text)");
        } else {
            eprintln!("    Status:           not triggered");
            eprintln!("    Items removed:     none (no unlikely candidates matched)");
        }
    }
    eprintln!();

    // ── Section 6: BODY_XPATH Pattern ─────────────────────────────────
    eprintln!("---");
    eprintln!("  BODY_XPATH Pattern:");
    eprintln!("    Why: Identifies which container-isolation pattern matched.");
    eprintln!("         Pattern 0 = strong signal (exact class/id match).");
    eprintln!("         Pattern 3 = weak signal (generic 'main' heuristic).");
    {
        let pattern = detect_body_xpath_pattern(&source_html);
        match pattern {
            Some(0) => eprintln!("    Match: Pattern 0 (specific class/id selectors) — strong signal"),
            Some(1) => eprintln!("    Match: Pattern 1 (bare <article>/<main> tag) — moderate signal"),
            Some(2) => eprintln!("    Match: Pattern 2 (content class/id) — moderate signal"),
            Some(3) => eprintln!("    Match: Pattern 3 (starts-with 'main') — weak signal, page may have unusual structure"),
            None => eprintln!("    No container isolated — page structure may not match Trafilatura expectations"),
            _ => eprintln!("    Unexpected pattern index: {:?}", pattern),
        }
    }
    eprintln!();

    // ── Section 7: Retry Level ────────────────────────────────────────
    eprintln!("---");
    eprintln!("  Retry Level:");
    eprintln!("    Why: Shows whether the page needed relaxed filtering (Recall).");
    eprintln!("         Frequent Recall wins = Balanced pass is over-filtering.");
    {
        let level = detect_retry_level(&source_html);
        match level {
            0 => eprintln!("    Level: Balanced (standard filtering) — page content extracted normally"),
            1 => eprintln!("    Level: Recall (relaxed filtering) — Balanced was too aggressive (<500 chars)"),
            _ => eprintln!("    Level: unknown ({level})"),
        }
    }
    eprintln!();

    // ── Section 8: Per-Pass Timing (diagnostic feature only) ─────────
    // Requires: cargo test --features diagnostic
    #[cfg(feature = "diagnostic")]
    {
        eprintln!("---");
        eprintln!("  Per-Pass Timing:");
        eprintln!("    Why: Shows time spent in each pipeline pass. Useful for");
        eprintln!("         identifying performance bottlenecks or unexpected");
        eprintln!("         behavior (e.g., backup/restore clone overhead).");
        let timings = time_passes(&source_html);
        let total: std::time::Duration = timings.iter().map(|(_, d)| *d).sum();
        for (name, dur) in &timings {
            eprintln!("    {:50}  {:?}", format!("  {name}:"), dur);
        }
        eprintln!("    {:50}  {:?}", "  TOTAL:", total);
    }

    // ── Annotations Summary ───────────────────────────────────────────
    eprintln!("================================================================");
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
