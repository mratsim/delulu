//! Readability pipeline diagnostic tool (run via cargo test, not cargo run).
//!
//! # Usage
//!
//! ```bash
//! # Batch mode — classify all 130+ test cases (use --release for ~10x speed)
//! DIAG_ARGS="batch" cargo test --release --test diag_rd_pipeline -- --nocapture --ignored
//!
//! # Batch with custom fixture directory
//! DIAG_ARGS="batch --fixtures-dir /path/to/fixtures" cargo test --release --test diag_rd_pipeline -- --nocapture --ignored
//!
//! # Deep-dive into a single test case
//! DIAG_ARGS="deep-dive bbc-1" cargo test --release --test diag_rd_pipeline -- --nocapture --ignored
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
//!

use std::io::Read;
use std::path::PathBuf;

use delulu_webfetch::generators::gen_html::dom_nodes_to_html;
use delulu_webfetch::pipeline::mozilla_readability::filter_mozilla_readability;
use delulu_webfetch::pipeline::parse_html;

/// Normalize HTML for comparison: strip whitespace between tags, collapse runs of whitespace.
fn normalize_html(html: &str) -> String {
    let s = html.trim();
    let s = s.replace("\r\n", "\n");
    // Collapse whitespace between tags
    let s = s.replace("> ", ">");
    let s = s.replace(" <", "<");
    let s = s.replace(">  ", ">");
    // Collapse remaining multiple whitespace
    let mut result = String::with_capacity(s.len());
    let mut prev_was_space = false;
    for ch in s.chars() {
        if ch.is_whitespace() && !ch.is_control() {
            if !prev_was_space {
                result.push(' ');
                prev_was_space = true;
            }
        } else {
            result.push(ch);
            prev_was_space = false;
        }
    }
    result.trim().to_string()
}

// ---------------------------------------------------------------------------
// Fixture directory resolution
// ---------------------------------------------------------------------------

fn fixture_dir(fixtures_arg: &Option<PathBuf>) -> PathBuf {
    if let Some(dir) = fixtures_arg {
        return dir.clone();
    }
    panic!(
        "--fixtures-dir <path> is required. This binary cannot resolve test fixtures without it."
    );
}

fn load_test_case(
    name: &str,
    fixtures_arg: &Option<PathBuf>,
) -> (delulu_webfetch::pipeline::DomNode, String) {
    let dir = fixture_dir(fixtures_arg).join(name);
    let source_path = dir.join("source.html.zst");
    let expected_path = dir.join("expected.html.zst");

    let source_compressed = std::fs::read(&source_path)
        .unwrap_or_else(|e| panic!("Failed to read source.html.zst for '{name}': {e}"));
    let mut decoder = zstd::Decoder::new(source_compressed.as_slice())
        .unwrap_or_else(|e| panic!("Failed to create zstd decoder for source.html.zst '{name}': {e}"));
    let mut source_html = String::new();
    decoder.read_to_string(&mut source_html)
        .unwrap_or_else(|e| panic!("Failed to decompress source.html.zst for '{name}': {e}"));

    let expected_compressed = std::fs::read(&expected_path)
        .unwrap_or_else(|e| panic!("Failed to read expected.html.zst for '{name}': {e}"));
    let mut decoder = zstd::Decoder::new(expected_compressed.as_slice())
        .unwrap_or_else(|e| panic!("Failed to create zstd decoder for expected.html.zst '{name}': {e}"));
    let mut expected_html = String::new();
    decoder.read_to_string(&mut expected_html)
        .unwrap_or_else(|e| panic!("Failed to decompress expected.html.zst for '{name}': {e}"));

    let root = parse_html(&source_html)
        .unwrap_or_else(|e| panic!("Failed to parse HTML for '{name}': {e}"));

    (root, expected_html)
}

// ---------------------------------------------------------------------------
// Classification
// ---------------------------------------------------------------------------

fn classify(out_len: usize, exp_len: usize) -> &'static str {
    if exp_len == 0 {
        return "EMPTY_EXPECTED";
    }
    let ratio = out_len as f64 / exp_len as f64;
    if out_len < exp_len / 3 {
        "OVER-FILTERING (severe)"
    } else if ratio < 0.50 {
        "OVER-FILTERING (moderate)"
    } else if ratio < 0.75 {
        "OVER-FILTERING (mild)"
    } else if out_len > exp_len * 3 {
        "UNDER-FILTERING (severe)"
    } else if ratio > 2.0 {
        "UNDER-FILTERING (moderate)"
    } else if ratio > 1.5 {
        "UNDER-FILTERING (mild)"
    } else if (ratio - 1.0).abs() < 0.15 {
        "STRUCTURAL/SERIALIZATION"
    } else {
        "MIXED"
    }
}

#[allow(dead_code)]
fn find_first_diff(a: &str, b: &str) -> Option<usize> {
    let chars_a: Vec<char> = a.chars().collect();
    let chars_b: Vec<char> = b.chars().collect();
    let min_len = chars_a.len().min(chars_b.len());
    for i in 0..min_len {
        if chars_a[i] != chars_b[i] {
            return Some(i);
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Batch mode
// ---------------------------------------------------------------------------

fn run_batch(fixtures_arg: &Option<PathBuf>) {
    let dir = fixture_dir(fixtures_arg);
    let mut entries: Vec<_> = std::fs::read_dir(&dir)
        .expect("fixture dir")
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_dir())
        .map(|e| e.file_name().to_string_lossy().to_string())
        .collect();
    entries.sort();

    let mut total = 0u32;
    let mut passed = 0u32;
    let mut failed = 0u32;
    let mut over_filtering_severe = 0u32;
    let mut over_filtering_moderate = 0u32;
    let mut over_filtering_mild = 0u32;
    let mut under_filtering_severe = 0u32;
    let mut under_filtering_moderate = 0u32;
    let mut under_filtering_mild = 0u32;
    let mut structural = 0u32;
    let mut mixed = 0u32;

    println!("case_name\tstatus\tclassification\toutput_len\texpected_len\tratio_pct");

    for name in &entries {
        total += 1;
        let source_path = dir.join(name).join("source.html.zst");
        let expected_path = dir.join(name).join("expected.html.zst");

        let source_compressed = match std::fs::read(&source_path) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("ERROR: {name} - failed to read source: {e}");
                continue;
            }
        };
        let source_html = match zstd::decode_all(source_compressed.as_slice()) {
            Ok(d) => match String::from_utf8(d) {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("ERROR: {name} - source is not valid UTF-8: {e}");
                    continue;
                }
            },
            Err(e) => {
                eprintln!("ERROR: {name} - failed to decompress source: {e}");
                continue;
            }
        };
        let expected_compressed = match std::fs::read(&expected_path) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("ERROR: {name} - failed to read expected: {e}");
                continue;
            }
        };
        let expected_html = match zstd::decode_all(expected_compressed.as_slice()) {
            Ok(d) => match String::from_utf8(d) {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("ERROR: {name} - expected is not valid UTF-8: {e}");
                    continue;
                }
            },
            Err(e) => {
                eprintln!("ERROR: {name} - failed to decompress expected: {e}");
                continue;
            }
        };
        let mut nodes = match parse_html(&source_html) {
            Ok(n) => n,
            Err(e) => {
                eprintln!("ERROR: {name} - failed to parse: {e}");
                continue;
            }
        };

        filter_mozilla_readability(&mut nodes);
        let output_html = dom_nodes_to_html(&nodes);
        let norm_output = normalize_html(&output_html);
        let norm_expected = normalize_html(&expected_html);

        let out_len = norm_output.len();
        let exp_len = norm_expected.len();

        if norm_output == norm_expected {
            passed += 1;
            println!("{name}\tPASS\t-\t{out_len}\t{exp_len}\t100.0");
            continue;
        }

        failed += 1;
        let classification = classify(out_len, exp_len);
        let ratio_pct = if exp_len > 0 {
            format!("{:.1}", 100.0 * out_len as f64 / exp_len as f64)
        } else {
            "N/A".to_string()
        };

        match classification {
            c if c.starts_with("OVER-FILTERING (severe)") => over_filtering_severe += 1,
            c if c.starts_with("OVER-FILTERING (moderate)") => over_filtering_moderate += 1,
            c if c.starts_with("OVER-FILTERING (mild)") => over_filtering_mild += 1,
            c if c.starts_with("UNDER-FILTERING (severe)") => under_filtering_severe += 1,
            c if c.starts_with("UNDER-FILTERING (moderate)") => under_filtering_moderate += 1,
            c if c.starts_with("UNDER-FILTERING (mild)") => under_filtering_mild += 1,
            c if c.starts_with("STRUCTURAL") => structural += 1,
            c if c.starts_with("MIXED") => mixed += 1,
            _ => {}
        }

        println!("{name}\tFAIL\t{classification}\t{out_len}\t{exp_len}\t{ratio_pct}");
    }

    eprintln!();
    eprintln!("================================================================");
    eprintln!("  BATCH CATEGORIZATION SUMMARY");
    eprintln!("================================================================");
    eprintln!("  Total cases:    {total}");
    eprintln!("  Passed:         {passed}");
    eprintln!("  Failed:         {failed}");
    eprintln!();
    eprintln!("  OVER-FILTERING (severe):    {over_filtering_severe:>3}  (<33% content)");
    eprintln!("  OVER-FILTERING (moderate):  {over_filtering_moderate:>3}  (33-50% content)");
    eprintln!("  OVER-FILTERING (mild):      {over_filtering_mild:>3}  (50-75% content)");
    eprintln!("  UNDER-FILTERING (severe):   {under_filtering_severe:>3}  (>300% content)");
    eprintln!("  UNDER-FILTERING (moderate): {under_filtering_moderate:>3}  (200-300% content)");
    eprintln!("  UNDER-FILTERING (mild):     {under_filtering_mild:>3}  (150-200% content)");
    eprintln!(
        "  STRUCTURAL/SERIALIZATION:   {structural:>3}  (85-115% content, different structure)"
    );
    eprintln!("  MIXED:                      {mixed:>3}");
    eprintln!();
    let total_over = over_filtering_severe + over_filtering_moderate + over_filtering_mild;
    let total_under = under_filtering_severe + under_filtering_moderate + under_filtering_mild;
    eprintln!("  TOTAL OVER-FILTERING:  {total_over}");
    eprintln!("  TOTAL UNDER-FILTERING: {total_under}");
    eprintln!("  TOTAL STRUCTURAL:      {structural}");
    eprintln!("  TOTAL MIXED:           {mixed}");
    eprintln!("================================================================");
}

// ---------------------------------------------------------------------------
// Deep-dive mode
// ---------------------------------------------------------------------------

fn run_deep_dive(case_name: &str, fixtures_arg: &Option<PathBuf>) {
    let (mut root, expected_html) = load_test_case(case_name, fixtures_arg);
    println!("=== Analyzing test case: {case_name} ===");
    println!("Expected HTML length: {} chars", expected_html.len());

    filter_mozilla_readability(&mut root);
    let output_html = dom_nodes_to_html(&root);
    println!("Output HTML length: {} chars", output_html.len());

    let norm_output = normalize_html(&output_html);
    let norm_expected = normalize_html(&expected_html);
    println!("Normalized output length: {} chars", norm_output.len());
    println!("Normalized expected length: {} chars", norm_expected.len());

    if norm_output == norm_expected {
        println!("\nRESULT: PASS -- outputs match!");
    } else {
        let out_len = norm_output.len();
        let exp_len = norm_expected.len();

        println!("\nRESULT: FAIL -- outputs differ");

        let chars_out: Vec<char> = norm_output.chars().collect();
        let chars_exp: Vec<char> = norm_expected.chars().collect();
        let min_len = chars_out.len().min(chars_exp.len());
        let mut first_diff = None;
        for i in 0..min_len {
            if chars_out[i] != chars_exp[i] {
                first_diff = Some(i);
                break;
            }
        }
        if let Some(pos) = first_diff {
            let start = pos.saturating_sub(150);
            let end_out = (pos + 150).min(chars_out.len());
            let end_exp = (pos + 150).min(chars_exp.len());
            println!("\nFirst difference at position {pos}:");
            println!(
                "  Output:   ...{}...",
                String::from_iter(&chars_out[start..end_out])
            );
            println!(
                "  Expected: ...{}...",
                String::from_iter(&chars_exp[start..end_exp])
            );
        }

        let classification = classify(out_len, exp_len);
        println!("\nCLASSIFICATION: {classification}");
    }

    // Save output to temp file
    let tmp_dir = std::env::temp_dir().join("readability_actual");
    std::fs::create_dir_all(&tmp_dir).ok();
    let out_path = tmp_dir.join(format!("{case_name}.html"));
    std::fs::write(&out_path, &output_html)
        .unwrap_or_else(|e| eprintln!("Warning: failed to save output: {e}"));
    println!("\nRaw output saved to: {}", out_path.display());
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
        eprintln!("       DIAG_ARGS=\"deep-dive <case-name> [--fixtures-dir <path>]\" cargo test ...");
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
        eprintln!("Example: DIAG_ARGS=\"batch\" cargo test --test diag_rd_pipeline -- --nocapture --ignored");
        return;
    }

    match mode_args[0].as_str() {
        "batch" => run_batch(&fixtures_arg),
        "deep-dive" => {
            if mode_args.len() < 2 {
                eprintln!("Error: deep-dive requires a test case name.");
                eprintln!("Example: DIAG_ARGS=\"deep-dive bbc-1\" cargo test --test diag_rd_pipeline -- --nocapture --ignored");
                return;
            }
            run_deep_dive(&mode_args[1], &fixtures_arg);
        }
        _ => {
            eprintln!("Error: unknown mode '{}'. Use 'batch' or 'deep-dive'.", mode_args[0]);
            return;
        }
    }
}
