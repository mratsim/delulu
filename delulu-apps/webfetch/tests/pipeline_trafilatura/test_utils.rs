//! Shared utilities for the Trafilatura pipeline diagnostic tool.
//!
//! Provides:
//! - `fixture_dir()`: Path to the `tests/fixtures-trafilatura/` directory
//! - `load_test_case_tf()`: Load source.html.zst + expected.md.zst + optional annotations.json.zst
//! - `decompress_zst()`: zstd decompression helper (panics on error)
//! - `try_decompress_zst()`: zstd decompression helper (returns Result)
//! - `Annotations`, `Classification`, `Severity`, `ConfusionMatrix` types
//! - `classify_output()`: Length-ratio classification
//! - `compute_confusion_matrix()`: Substring-based confusion matrix
//! - `tf_count_text_chars()`: Recursive text-node character counter
//! - `normalize_output()`: NFC normalization + whitespace collapse + trim

//! - `detect_backup_restore()`: Check if tf_remove_unlikely_candidates backup triggered
//! - `detect_body_xpath_pattern()`: Which BODY_XPATH container pattern matched (0-3)
//! - `detect_retry_level()`: Which retry level produced the best output
use std::path::PathBuf;

use delulu_webfetch::pipelines::passes::tf_filters::PATTERN_CHECKS;
use delulu_webfetch::pipelines::{DomNode, parse_html};
use serde::Deserialize;

// ---------------------------------------------------------------------------
// Fixture directory
// ---------------------------------------------------------------------------

/// Resolve the path to the trafilatura fixture directory.
///
/// `CARGO_MANIFEST_DIR` = `<workspace>/delulu/delulu-apps/webfetch`
/// Target               = `<workspace>/delulu/delulu-apps/webfetch/tests/fixtures-trafilatura`
pub fn fixture_dir() -> PathBuf {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest.join("tests/fixtures-trafilatura")
}

// ---------------------------------------------------------------------------
// zstd decompression
// ---------------------------------------------------------------------------

/// Decompress a zstd-compressed file into a `String`, returning a `Result`.
///
/// This is the batch-safe variant that does not panic on error.
pub fn try_decompress_zst(path: &std::path::Path) -> Result<String, String> {
    let compressed =
        std::fs::read(path).map_err(|e| format!("failed to read '{}': {e}", path.display()))?;
    let decoded = zstd::decode_all(&compressed[..])
        .map_err(|e| format!("failed to decompress '{}': {e}", path.display()))?;
    String::from_utf8(decoded).map_err(|e| format!("invalid UTF-8 in '{}': {e}", path.display()))
}

/// Decompress a zstd-compressed file into a `String`.
///
/// # Panics
///
/// - If the file cannot be read
/// - If the decompressed bytes are not valid UTF-8
pub fn decompress_zst(path: &std::path::Path) -> String {
    try_decompress_zst(path).unwrap_or_else(|e| panic!("{e}"))
}

// ---------------------------------------------------------------------------
// Annotations
// ---------------------------------------------------------------------------

/// Ground-truth annotations for a trafilatura test case.
///
/// Mirrors the `with[]` / `without[]` lists from Python `evaldata.py`.
#[derive(Clone, Debug, Deserialize)]
pub struct Annotations {
    /// Substrings that MUST be present in the extraction output.
    #[serde(default)]
    pub with: Vec<String>,
    /// Substrings that MUST NOT be present in the extraction output.
    #[serde(default)]
    pub without: Vec<String>,
}

// ---------------------------------------------------------------------------
// Classification
// ---------------------------------------------------------------------------

/// Severity level for over/under filtering.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Severity {
    Severe,
    Moderate,
    Mild,
}

impl std::fmt::Display for Severity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Severity::Severe => write!(f, "severe"),
            Severity::Moderate => write!(f, "moderate"),
            Severity::Mild => write!(f, "mild"),
        }
    }
}

/// Classification of a trafilatura extraction result based on length ratio.
#[derive(Debug, Clone, PartialEq)]
pub enum Classification {
    /// Output length is within 80–120% of expected.
    Pass,
    /// Output is significantly shorter than expected.
    OverFiltering(Severity),
    /// Output is significantly longer than expected.
    UnderFiltering(Severity),
}

impl std::fmt::Display for Classification {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Classification::Pass => write!(f, "PASS"),
            Classification::OverFiltering(sev) => write!(f, "OVER-FILTERING ({sev})"),
            Classification::UnderFiltering(sev) => write!(f, "UNDER-FILTERING ({sev})"),
        }
    }
}

/// Classify an extraction output based on its length ratio vs expected.
///
/// Ratios:
/// - PASS (0.8–1.2)
/// - OVER_FILTERING (<0.8): severe (<0.3), moderate (0.3–0.6), mild (0.6–0.8)
/// - UNDER_FILTERING (>1.2): severe (>3.0), moderate (2.0–3.0), mild (1.2–2.0)
///
/// When `expected_len` is 0: `Pass` if `output_len` is also 0 (exact match),
/// else `UnderFiltering(Severe)` (Rust produced content where Python expected
/// none — never over-filtering).
pub fn classify_output(output_len: usize, expected_len: usize) -> Classification {
    if expected_len == 0 {
        // No expected content: can't compute a ratio. Both empty is an exact
        // match (Pass); Rust producing content where Python expected none is a
        // divergence in the under-filtering direction, never over-filtering.
        return if output_len == 0 {
            Classification::Pass
        } else {
            Classification::UnderFiltering(Severity::Severe)
        };
    }
    let ratio = output_len as f64 / expected_len as f64;

    if (0.8..=1.2).contains(&ratio) {
        return Classification::Pass;
    }

    if ratio < 0.8 {
        if ratio < 0.3 {
            Classification::OverFiltering(Severity::Severe)
        } else if ratio < 0.6 {
            Classification::OverFiltering(Severity::Moderate)
        } else {
            Classification::OverFiltering(Severity::Mild)
        }
    } else {
        // ratio > 1.2 (guaranteed by early return above)
        if ratio > 3.0 {
            Classification::UnderFiltering(Severity::Severe)
        } else if ratio > 2.0 {
            Classification::UnderFiltering(Severity::Moderate)
        } else {
            Classification::UnderFiltering(Severity::Mild)
        }
    }
}

// ---------------------------------------------------------------------------
// Confusion Matrix
// ---------------------------------------------------------------------------

/// Confusion matrix based on `with[]`/`without[]` annotation presence.
#[derive(Debug, Clone, Copy, Default)]
pub struct ConfusionMatrix {
    pub tp: usize,
    pub fp: usize,
    pub tn: usize,
    pub fn_: usize,
}

impl ConfusionMatrix {
    /// Precision: TP / (TP + FP). Returns 0.0 if denominator is 0.
    pub fn precision(&self) -> f64 {
        let denom = self.tp + self.fp;
        if denom == 0 {
            0.0
        } else {
            self.tp as f64 / denom as f64
        }
    }

    /// Recall: TP / (TP + FN). Returns 0.0 if denominator is 0.
    pub fn recall(&self) -> f64 {
        let denom = self.tp + self.fn_;
        if denom == 0 {
            0.0
        } else {
            self.tp as f64 / denom as f64
        }
    }

    /// Accuracy: (TP + TN) / (TP + TN + FP + FN). Returns 0.0 if denominator is 0.
    pub fn accuracy(&self) -> f64 {
        let denom = self.tp + self.tn + self.fp + self.fn_;
        if denom == 0 {
            0.0
        } else {
            (self.tp + self.tn) as f64 / denom as f64
        }
    }

    /// F1 score: harmonic mean of precision and recall. Returns 0.0 if p+r is 0.
    pub fn f1(&self) -> f64 {
        let p = self.precision();
        let r = self.recall();
        let denom = p + r;
        if denom == 0.0 {
            0.0
        } else {
            2.0 * p * r / denom
        }
    }
}

/// Compute a confusion matrix by checking `with[]`/`without[]` substrings
/// in the output and expected text.
///
/// - `with` items expected in output → TP if present, FN if absent
/// - `without` items expected absent from output → TN if absent, FP if present
///
/// `expected_text` — reserved for future cross-validation against expected output.
pub fn compute_confusion_matrix(
    output_text: &str,
    _expected_text: &str,
    annotations: &Annotations,
) -> ConfusionMatrix {
    let mut cm = ConfusionMatrix::default();

    // `output_text` is already normalized (via `normalize_output`), so each
    // annotation must be normalized too before matching (Issue F). Otherwise an
    // annotation containing a newline, double space, NBSP, or decomposed char
    // never matches the normalized output, inflating fn_/fp and skewing
    // precision/recall.
    for w in &annotations.with {
        let w_norm = normalize_output(w);
        if output_text.contains(&w_norm) {
            cm.tp += 1;
        } else {
            cm.fn_ += 1;
        }
    }

    for wo in &annotations.without {
        let wo_norm = normalize_output(wo);
        if !output_text.contains(&wo_norm) {
            cm.tn += 1;
        } else {
            cm.fp += 1;
        }
    }

    cm
}

// ---------------------------------------------------------------------------
// Fixture loading
// ---------------------------------------------------------------------------

/// Load a single trafilatura test case by directory name.
///
/// Returns a tuple of `(parsed DomNode tree, expected markdown string, optional annotations)`.
///
/// # Panics
///
/// - If the fixture directory or required files do not exist
/// - If `source.html.zst` fails to decompress or parse
pub fn load_test_case_tf(name: &str) -> (DomNode, String, Option<Annotations>) {
    load_test_case_tf_from(&fixture_dir(), name)
}

/// Load a single trafilatura test case from an explicit base directory.
///
/// Same as [`load_test_case_tf`] but resolves the fixture under `base_dir`
/// instead of the default [`fixture_dir`], so the diag's `--fixtures-dir`
/// override actually reaches the load path.
pub fn load_test_case_tf_from(
    base_dir: &std::path::Path,
    name: &str,
) -> (DomNode, String, Option<Annotations>) {
    let dir = base_dir.join(name);
    let source_path = dir.join("source.html.zst");
    let expected_path = dir.join("expected.md.zst");
    let annotations_path = dir.join("annotations.json.zst");

    let source_html = decompress_zst(&source_path);
    let expected_md = decompress_zst(&expected_path);

    let root = parse_html(&source_html)
        .unwrap_or_else(|e| panic!("Failed to parse HTML for '{name}': {e}"));

    let annotations =
        if annotations_path.exists() {
            let content = try_decompress_zst(&annotations_path).unwrap_or_else(|e| {
                panic!("Failed to decompress annotations.json.zst for '{name}': {e}")
            });
            Some(serde_json::from_str(&content).unwrap_or_else(|e| {
                panic!("Failed to parse annotations.json.zst for '{name}': {e}")
            }))
        } else {
            None
        };

    (root, expected_md, annotations)
}

// ---------------------------------------------------------------------------
// Text utilities
// ---------------------------------------------------------------------------

/// Count total text characters in a DOM tree recursively.
///
/// Walks all descendant `Text` nodes and sums their character counts (via
/// `str::chars().count()`), so a CJK char counts as 1 (not 3 bytes).
/// Replicates the private `count_text_chars` in `trafilatura.rs`.
pub fn tf_count_text_chars(node: &DomNode) -> usize {
    match node {
        DomNode::Text(t) => t.chars().count(),
        DomNode::Element { children, .. } => children.iter().map(tf_count_text_chars).sum(),
        _ => 0,
    }
}

/// Count total text BYTES in a DOM tree recursively.
///
/// Walks all descendant `Text` nodes and sums their byte lengths (via `str::len()`).
/// For CJK/non-ASCII text this diverges from `tf_count_text_chars` (e.g. "日本語" is
/// 9 bytes but 3 chars); reporting both surfaces potential CJK bugs.
pub fn tf_count_text_bytes(node: &DomNode) -> usize {
    match node {
        DomNode::Text(t) => t.len(),
        DomNode::Element { children, .. } => children.iter().map(tf_count_text_bytes).sum(),
        _ => 0,
    }
}

/// Normalize output text for comparison.
///
/// Steps:
/// 1. NFC normalization
/// 2. Collapse runs of whitespace (including newlines) into a single space
/// 3. Trim leading/trailing whitespace
pub fn normalize_output(text: &str) -> String {
    use std::borrow::Cow;
    // NFC normalization
    let nfc = unicode_normalization::UnicodeNormalization::nfc(text);
    let s: Cow<'_, str> = nfc.collect::<String>().into();

    // Collapse whitespace runs
    let mut result = String::with_capacity(s.len());
    let mut prev_was_space = false;
    for ch in s.chars() {
        if ch.is_whitespace() {
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

/// Find the first character position where two strings differ,
/// with surrounding context window.
/// Returns `(pos, context_before, context_after)` where pos is the
/// character index of the first difference, or None if strings are identical.
pub fn first_diff_position(a: &str, b: &str) -> Option<(usize, String, String)> {
    let a_chars: Vec<char> = a.chars().collect();
    let b_chars: Vec<char> = b.chars().collect();

    let min_len = a_chars.len().min(b_chars.len());
    for i in 0..min_len {
        if a_chars[i] != b_chars[i] {
            let ctx_start = i.saturating_sub(40);
            let ctx_end = (i + 40).min(a_chars.len());
            let before: String = a_chars[ctx_start..i].iter().collect();
            let after: String = a_chars[i..ctx_end].iter().collect();
            return Some((i, before, after));
        }
    }

    // One string is a prefix of the other
    if a_chars.len() != b_chars.len() {
        let pos = min_len;
        let ctx_start = pos.saturating_sub(40);
        let ctx_end = (pos + 40).min(a_chars.len().max(b_chars.len()));
        let longer = if a_chars.len() > b_chars.len() {
            &a_chars
        } else {
            &b_chars
        };
        let before: String = longer[ctx_start..pos].iter().collect();
        let after: String = longer[pos..ctx_end].iter().collect();
        return Some((pos, before, after));
    }

    None
}

// ---------------------------------------------------------------------------
// Pipeline Introspection
// ---------------------------------------------------------------------------
//
// These functions probe the pipeline's internal behavior. They exist because
// the pipeline doesn't expose these details through its public API — the
// diagnostic needs to detect them externally to help debug divergence between
// the Rust tf_* pipeline and the Python trafilatura reference.
//
// Backup/restore: Trafilatura's safety mechanism. If OVERALL_DISCARD_XPATH
//   removes ≥80% of text (threshold 5×), the pass restores from a pre-removal clone.
//   Detecting this in the diagnostic tells you whether the discard patterns
//   are too aggressive for a given page — the pipeline is "working by accident"
//   and relying on backup as a crutch rather than correct pattern matching.
//
// BODY_XPATH patterns: 4 cascading patterns (0-3) that identify the main
//   content container. Pattern 0 is most specific (exact class/id matches),
//   Pattern 3 is most general (starts-with "main"). Reporting which pattern
//   matched tells you how precisely the container was identified — a Pattern 3
//   match is a weak signal and may indicate the page structure is unexpected.
//
// Retry level: The pipeline tries Balanced first, then Recall if output is
//   <500 chars. Reporting which level won tells you if the page needed relaxed
//   filtering — frequent Recall wins suggest the Balanced pass is over-filtering.

/// Detect whether the backup/restore safety mechanism triggered during
/// `tf_remove_unlikely_candidates`.
///
/// Runs the pass on a clone and compares text content before vs after.
/// If backup triggered, the clone is restored to its original state, so
/// text content before == text content after (but the pass DID remove items
/// and then restored). To distinguish from "nothing was removed", also runs
/// the pass WITHOUT backup on another clone.
///
/// - `backup_triggered`: true if ≥80% text was removed and restored
/// - `items_removed_count`: how many elements `tf_remove_unlikely_candidates`
///   removed (before any restore) — 0 means nothing matched
#[allow(dead_code)]
pub fn detect_backup_restore(html: &str) -> (bool, u32) {
    use delulu_webfetch::pipelines::passes::tf_filters::tf_remove_cleaned;
    #[cfg(not(feature = "use-xpath"))]
    use delulu_webfetch::pipelines::passes::tf_filters::tf_remove_unlikely_candidates;
    #[cfg(feature = "use-xpath")]
    use delulu_webfetch::pipelines::passes::tf_filters::tf_remove_unlikely_candidates_xpath as tf_remove_unlikely_candidates;
    use delulu_webfetch::pipelines::trafilatura::with_backup;
    use delulu_webfetch::pipelines::walk_pre_mut;

    let root = parse_html(html).expect("parse_html failed");
    let original_len = tf_count_text_chars(&root);

    // Clone A: run WITH backup (the real pipeline behavior)
    let mut guarded = root.clone();
    {
        with_backup(
            &mut guarded,
            |n| walk_pre_mut(n, &|n| tf_remove_unlikely_candidates(n)),
            // 5x threshold: >=80% text removed triggers restore (matches production
            // apply_tf_remove_unlikely_candidates_with_backup, trafilatura.rs:155-159)
            5,
            // Production restore closure (trafilatura.rs:133-136): full restore + clean
            |node, backup| {
                *node = backup.clone();
                walk_pre_mut(node, &|n| tf_remove_cleaned(n));
            },
        );
    }
    let after_backup_len = tf_count_text_chars(&guarded);

    // Clone B: run WITHOUT backup to count actual removals
    let mut without_backup = root;
    let before_count = count_elements(&without_backup);
    walk_pre_mut(&mut without_backup, &|n| tf_remove_unlikely_candidates(n));
    let after_count = count_elements(&without_backup);
    let items_removed = before_count.saturating_sub(after_count);

    // If with_backup restored to original length, backup triggered
    let backup_triggered = after_backup_len == original_len && items_removed > 0;

    (backup_triggered, items_removed)
}

/// Count element nodes in a DOM tree (recursive).
#[allow(dead_code)]
fn count_elements(node: &DomNode) -> u32 {
    match node {
        DomNode::Element { children, .. } => 1 + children.iter().map(count_elements).sum::<u32>(),
        _ => 0,
    }
}

/// Detect which BODY_XPATH pattern (0-4) matches for the given HTML.
///
/// BODY_XPATH identifies the main content container. The 5 patterns are
/// checked in cascade order — the first match wins:
///
/// - Pattern 0 (most specific): exact class/id matches like "post", "entry",
///   "article-body", itemprop="articleBody", role="article"
/// - Pattern 1: bare `<article>` or `<main>` tag (no class/id requirement)
/// - Pattern 2: specific content class/id patterns like "postarea", "text", "story"
/// - Pattern 3: broader content patterns like "content", "main-content", "page-content"
/// - Pattern 4 (most general): starts-with "main" in class, id, role, or tag
///
/// Returns Some(0-4) if a match is found, or None if no container identified.
/// A Pattern 0 match = strong signal (page structure is well-known).
/// A Pattern 4 match = weak signal (page may have unusual structure).
#[allow(dead_code)]
pub fn detect_body_xpath_pattern(html: &str) -> Option<usize> {
    #[cfg(not(feature = "use-xpath"))]
    use delulu_webfetch::pipelines::passes::tf_filters::tf_isolate_content_container;
    #[cfg(feature = "use-xpath")]
    use delulu_webfetch::pipelines::passes::tf_filters::tf_isolate_content_container_xpath as tf_isolate_content_container;

    let mut root = parse_html(html).expect("parse_html failed");
    let original_count = count_elements(&root);
    tf_isolate_content_container(&mut root);
    let after_count = count_elements(&root);

    // If element count changed, a container was isolated
    if after_count < original_count {
        // Walk the tree to find which BODY_XPATH pattern matched
        // by checking the surviving container's attributes
        find_matching_pattern(&root)
    } else {
        None
    }
}

/// Walk the tree to determine which BODY_XPATH pattern matched the
/// surviving container after `tf_isolate_content_container`.
#[allow(dead_code)]
fn find_matching_pattern(node: &DomNode) -> Option<usize> {
    match node {
        DomNode::Element {
            tag,
            attrs,
            children,
            ..
        } => {
            // Delegate to the SAME canonical PATTERN_CHECKS the production
            // tf_isolate_content_container uses, so the reported index always
            // matches which pattern production actually applied.
            let class_val = get_attr(attrs, "class").unwrap_or("");
            let id_val = get_attr(attrs, "id").unwrap_or("");
            let role_val = get_attr(attrs, "role").unwrap_or("");
            let itemprop_val = get_attr(attrs, "itemprop").unwrap_or("");
            for (i, check) in PATTERN_CHECKS.iter().enumerate() {
                if check(tag, class_val, id_val, role_val, itemprop_val) {
                    return Some(i);
                }
            }
            // Recurse into children
            for child in children {
                if let Some(p) = find_matching_pattern(child) {
                    return Some(p);
                }
            }
            None
        }
        _ => None,
    }
}

/// Get an attribute value by name from an attribute list.
#[allow(dead_code)]
fn get_attr<'a>(attrs: &'a [(String, String)], name: &str) -> Option<&'a str> {
    attrs
        .iter()
        .find(|(k, _)| k == name)
        .map(|(_, v)| v.as_str())
}

/// Detect which retry level (0=Balanced, 1=Recall) produced the best output.
///
/// The pipeline tries Balanced first, then Recall if output <500 chars.
/// Reporting which level won tells you whether the page needed relaxed
/// filtering. Frequent Recall wins suggest the Balanced pass is over-filtering
/// and may need pattern refinement.
#[allow(dead_code)]
pub fn detect_retry_level(html: &str) -> usize {
    use delulu_webfetch::pipelines::trafilatura::{TF_BALANCED, TF_MIN_OUTPUT_CHARS, TF_RECALL};

    let original = parse_html(html).expect("parse_html failed");

    // Run Balanced level only
    let mut balanced_tree = original.clone();
    for pass in *TF_BALANCED {
        pass(&mut balanced_tree);
    }
    let balanced_len = tf_count_text_chars(&balanced_tree);

    // If Balanced produced enough, that's the winner
    if balanced_len >= TF_MIN_OUTPUT_CHARS {
        return 0;
    }

    // Otherwise, run Recall passes directly and compare against Balanced.
    // (We intentionally avoid filter_trafilatura here, since its recovery/
    // fallback paths can inflate output length without Recall contributing.)
    let mut recall_tree = original;
    for pass in *TF_RECALL {
        pass(&mut recall_tree);
    }
    let recall_len = tf_count_text_chars(&recall_tree);

    if recall_len > balanced_len { 1 } else { 0 }
}

// ---------------------------------------------------------------------------
// Per-Pass Timing (diagnostic feature)
// ---------------------------------------------------------------------------
//
// Runs each pipeline pass individually with Instant::now() timing.
// Feature-gated with #[cfg(feature = "diagnostic")] — zero overhead in production.
//
// Note: passes mutate the tree in place, so each pass operates on the output
// of the previous one, matching real pipeline behavior.

/// Run each pass in the TF_BALANCED pipeline with timing, returning
/// `(pass_name, duration)` pairs. Only available with the `diagnostic` feature.
#[cfg(feature = "diagnostic")]
#[allow(dead_code)]
pub fn time_passes(html: &str) -> Vec<(String, std::time::Duration)> {
    use delulu_webfetch::pipelines::trafilatura::TF_BALANCED;
    use std::time::Instant;

    let mut tree = parse_html(html).expect("parse_html failed");
    let mut timings = Vec::new();

    let names = [
        "tf_remove_cleaned",
        "tf_remove_teaser",
        "tf_remove_unlikely_candidates (with backup)",
        "tf_strip_unwrapped",
        "tf_remove_empty_cut",
        "tf_convert_headings",
        "tf_convert_lists",
        "tf_convert_quotes",
        "tf_convert_formatting",
        "tf_convert_breaks",
        "tf_convert_refs_and_details",
        "tf_canonicalize_strip_non_content",
        "tf_isolate_content_container",
        "tf_canonicalize_unwrap_containers",
    ];

    for (pass, name) in (*TF_BALANCED).iter().zip(names.iter()) {
        let start = Instant::now();
        pass(&mut tree);
        timings.push((name.to_string(), start.elapsed()));
    }

    timings
}

/// Run a slice of passes with retry logic, returning output + metadata.
/// Mimics `filter_trafilatura`'s retry cascade but allows passing arbitrary passes.
#[allow(clippy::type_complexity)]
#[allow(dead_code)]
pub fn run_passes_with_retry(
    html: &str,
    levels: &[&[&dyn Fn(&mut DomNode)]],
    min_output_chars: usize,
) -> (DomNode, usize, usize) {
    let original = parse_html(html).expect("parse_html failed");
    let mut best_tree = original.clone();
    let mut best_len = 0usize;
    let mut best_level = 0usize;

    for (i, level) in levels.iter().enumerate() {
        let mut attempt = original.clone();
        for pass_fn in *level {
            pass_fn(&mut attempt);
        }
        let len = tf_count_text_chars(&attempt);
        if len > best_len {
            best_len = len;
            best_tree = attempt;
            best_level = i;
        }
        if len >= min_output_chars {
            break;
        }
    }

    (best_tree, best_len, best_level)
}
// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // ── fixture_dir ───────────────────────────────────────────────────────

    #[test]
    fn fixture_dir_resolves() {
        let dir = fixture_dir();
        // The directory may not exist yet (before seeding), but the path should
        // be under tests/fixtures-trafilatura relative to CARGO_MANIFEST_DIR.
        let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        assert_eq!(dir, manifest.join("tests/fixtures-trafilatura"));
    }

    // ── decompress_zst ────────────────────────────────────────────────────

    #[test]
    fn decompress_zst_roundtrip() {
        let data = "Hello, trafilatura diagnostic!";
        let compressed = zstd::encode_all(data.as_bytes(), 3).expect("compress");
        // Unique per-process path to avoid collisions across concurrent test runs.
        let tmp =
            std::env::temp_dir().join(format!("test_decompress_zst_{}.zst", std::process::id()));
        std::fs::write(&tmp, &compressed).unwrap();
        let result = decompress_zst(&tmp);
        assert_eq!(result, data);
        let _ = std::fs::remove_file(&tmp);
    }

    #[test]
    fn try_decompress_zst_returns_err_on_missing() {
        let tmp = std::env::temp_dir().join("nonexistent.zst");
        let result = try_decompress_zst(&tmp);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("failed to read"));
    }

    // ── classify_output ───────────────────────────────────────────────────

    #[test]
    fn classify_output_exact_match() {
        assert_eq!(classify_output(100, 100), Classification::Pass);
        assert_eq!(classify_output(80, 100), Classification::Pass);
        assert_eq!(classify_output(120, 100), Classification::Pass);
    }

    #[test]
    fn classify_output_over_filtering() {
        assert_eq!(
            classify_output(20, 100),
            Classification::OverFiltering(Severity::Severe)
        );
        assert_eq!(
            classify_output(50, 100),
            Classification::OverFiltering(Severity::Moderate)
        );
        assert_eq!(
            classify_output(70, 100),
            Classification::OverFiltering(Severity::Mild)
        );
    }

    #[test]
    fn classify_output_under_filtering() {
        assert_eq!(
            classify_output(400, 100),
            Classification::UnderFiltering(Severity::Severe)
        );
        assert_eq!(
            classify_output(250, 100),
            Classification::UnderFiltering(Severity::Moderate)
        );
        assert_eq!(
            classify_output(150, 100),
            Classification::UnderFiltering(Severity::Mild)
        );
    }

    #[test]
    fn classify_output_zero_expected() {
        // Both empty: Rust matches Python exactly (both extract nothing) -> Pass.
        assert_eq!(classify_output(0, 0), Classification::Pass);
        // Rust produced content where Python expected none -> a divergence in the
        // under-filtering direction (Rust keeps content Python discards), never
        // over-filtering.
        assert_eq!(
            classify_output(1541, 0),
            Classification::UnderFiltering(Severity::Severe)
        );
    }

    // ── ConfusionMatrix ───────────────────────────────────────────────────

    #[test]
    fn confusion_matrix_all_correct() {
        let cm = ConfusionMatrix {
            tp: 5,
            fp: 0,
            tn: 3,
            fn_: 0,
        };
        assert!((cm.precision() - 1.0).abs() < 1e-9);
        assert!((cm.recall() - 1.0).abs() < 1e-9);
        assert!((cm.accuracy() - 1.0).abs() < 1e-9);
        assert!((cm.f1() - 1.0).abs() < 1e-9);
    }

    #[test]
    fn confusion_matrix_zero_denominator() {
        let cm = ConfusionMatrix::default();
        assert!((cm.precision() - 0.0).abs() < 1e-9);
        assert!((cm.recall() - 0.0).abs() < 1e-9);
        assert!((cm.accuracy() - 0.0).abs() < 1e-9);
        assert!((cm.f1() - 0.0).abs() < 1e-9);
    }

    #[test]
    fn confusion_matrix_partial() {
        let cm = ConfusionMatrix {
            tp: 4,
            fp: 1,
            tn: 2,
            fn_: 2,
        };
        // precision = 4/5 = 0.8
        assert!((cm.precision() - 0.8).abs() < 1e-9);
        // recall = 4/6 ≈ 0.6667
        assert!((cm.recall() - 4.0 / 6.0).abs() < 1e-9);
        // accuracy = 6/9 ≈ 0.6667
        assert!((cm.accuracy() - 6.0 / 9.0).abs() < 1e-9);
        // f1 = 2 * 0.8 * 0.6667 / (0.8 + 0.6667) ≈ 0.7273
        let expected_f1 = 2.0 * 0.8 * (4.0 / 6.0) / (0.8 + 4.0 / 6.0);
        assert!((cm.f1() - expected_f1).abs() < 1e-9);
    }

    #[test]
    fn compute_confusion_matrix_tp_fn() {
        let ann = Annotations {
            with: vec!["hello".to_string(), "world".to_string()],
            without: vec![],
        };
        let cm = compute_confusion_matrix("hello there", "", &ann);
        assert_eq!(cm.tp, 1); // "hello" present
        assert_eq!(cm.fn_, 1); // "world" absent
    }

    #[test]
    fn compute_confusion_matrix_tn_fp() {
        let ann = Annotations {
            with: vec![],
            without: vec!["spam".to_string(), "eggs".to_string()],
        };
        let cm = compute_confusion_matrix("hello world", "", &ann);
        assert_eq!(cm.tn, 2); // both absent
        assert_eq!(cm.fp, 0);
    }

    #[test]
    fn compute_confusion_matrix_mixed() {
        let ann = Annotations {
            with: vec!["hello".to_string()],
            without: vec!["spam".to_string()],
        };
        let cm = compute_confusion_matrix("hello spam", "", &ann);
        assert_eq!(cm.tp, 1); // "hello" present
        assert_eq!(cm.fp, 1); // "spam" present (should be absent)
        assert_eq!(cm.tn, 0);
        assert_eq!(cm.fn_, 0);
    }

    #[test]
    fn compute_confusion_matrix_normalizes_annotations() {
        // Annotations contain a double space and a non-breaking space; the output
        // is normalized (single spaces). Raw substring matching would miss them;
        // normalizing each annotation makes them match.
        let ann = Annotations {
            with: vec!["hello  world".to_string(), "foo\u{00a0}bar".to_string()],
            without: vec!["spam  eggs".to_string()],
        };
        let output = normalize_output("hello  world  foo\u{00a0}bar");
        let cm = compute_confusion_matrix(&output, "", &ann);
        assert_eq!(
            cm.tp, 2,
            "normalized annotations should match normalized output"
        );
        assert_eq!(cm.fn_, 0);
        assert_eq!(cm.tn, 1, "'spam eggs' absent from normalized output");
        assert_eq!(cm.fp, 0);
    }
    // ── load_test_case_tf ─────────────────────────────────────────────────

    #[test]
    fn load_test_case_tf_loads_fixture() {
        let (root, expected, annotations) = load_test_case_tf("adac-de-kindersitze");
        match &root {
            DomNode::Element { tag, children, .. } => {
                assert!(!tag.is_empty(), "root element should have a tag");
                assert!(!children.is_empty(), "root should have children");
            }
            _ => panic!("expected DomNode::Element root"),
        }
        assert!(
            !expected.is_empty(),
            "expected markdown should not be empty"
        );
        let ann = annotations.expect("fixture should have annotations.json.zst");
        assert!(!ann.with.is_empty(), "should have with[] annotations");
        assert!(!ann.without.is_empty(), "should have without[] annotations");
    }

    // ── tf_count_text_chars ───────────────────────────────────────────────

    #[test]
    fn tf_count_text_chars_counts_text() {
        use delulu_webfetch::pipelines::DomNode;
        let node = DomNode::Element {
            tag: "div".to_string(),
            attrs: vec![],
            children: vec![
                DomNode::Text("Hello".to_string()),
                DomNode::Element {
                    tag: "span".to_string(),
                    attrs: vec![],
                    children: vec![DomNode::Text("World".to_string())],
                    scores: std::collections::HashMap::new(),
                    metadata: std::collections::HashMap::new(),
                },
            ],
            scores: std::collections::HashMap::new(),
            metadata: std::collections::HashMap::new(),
        };
        assert_eq!(tf_count_text_chars(&node), 10); // "Hello" + "World" = 10 chars
    }

    #[test]
    fn tf_count_text_chars_empty() {
        use delulu_webfetch::pipelines::DomNode;
        let node = DomNode::Element {
            tag: "div".to_string(),
            attrs: vec![],
            children: vec![],
            scores: std::collections::HashMap::new(),
            metadata: std::collections::HashMap::new(),
        };
        assert_eq!(tf_count_text_chars(&node), 0);
    }

    // ── normalize_output ──────────────────────────────────────────────────

    #[test]
    fn normalize_output_collapses_whitespace() {
        let input = "  Hello   World\n\nNext line.\t";
        let result = normalize_output(input);
        assert_eq!(result, "Hello World Next line.");
    }

    #[test]
    fn normalize_output_empty() {
        assert_eq!(normalize_output(""), "");
        assert_eq!(normalize_output("   "), "");
    }

    #[test]
    fn normalize_output_nfc() {
        // "é" composed form (U+00E9) vs decomposed (U+0065 U+0301)
        let composed = "\u{00e9}"; // é in NFC
        let decomposed = "e\u{0301}"; // é in NFD
        assert_eq!(normalize_output(composed), normalize_output(decomposed));
    }

    // ── first_diff_position ───────────────────────────────────────────────

    #[test]
    fn first_diff_position_identical() {
        assert_eq!(first_diff_position("hello", "hello"), None);
    }

    #[test]
    fn first_diff_position_differs() {
        let (pos, _before, after) = first_diff_position("hello", "hxllo").unwrap();
        assert_eq!(pos, 1);
        assert!(after.starts_with("e"), "after is from first arg: {after}");
    }

    #[test]
    fn first_diff_position_prefix() {
        let (pos, ..) = first_diff_position("hello", "hello world").unwrap();
        assert_eq!(pos, 5);
    }

    #[test]
    fn first_diff_position_empty() {
        assert!(first_diff_position("", "a").is_some());
        assert!(first_diff_position("a", "").is_some());
        assert_eq!(first_diff_position("", ""), None);
    }

    // ── tf_count_text_chars / tf_count_text_bytes ────────────────────────

    fn text_node(s: &str) -> DomNode {
        DomNode::Text(s.to_string())
    }

    #[test]
    fn tf_count_text_chars_counts_chars_not_bytes() {
        // "日本語" is 9 bytes but 3 chars — must count chars.
        let node = text_node("日本語");
        assert_eq!(
            tf_count_text_chars(&node),
            3,
            "CJK must count as 3 chars, not 9 bytes"
        );
        // "héllo" is 6 bytes but 5 chars.
        assert_eq!(tf_count_text_chars(&text_node("héllo")), 5);
        // Recursive over children.
        let parent = DomNode::Element {
            tag: "p".to_string(),
            attrs: vec![],
            children: vec![text_node("日本"), text_node("語")],
            scores: Default::default(),
            metadata: Default::default(),
        };
        assert_eq!(tf_count_text_chars(&parent), 3);
    }

    #[test]
    fn tf_count_text_bytes_counts_bytes_not_chars() {
        // "日本語" is 9 bytes but 3 chars — must count bytes.
        let node = text_node("日本語");
        assert_eq!(
            tf_count_text_bytes(&node),
            9,
            "日本語 must count as 9 bytes, not 3 chars"
        );
        assert_eq!(tf_count_text_bytes(&text_node("héllo")), 6);
        // Recursive over children.
        let parent = DomNode::Element {
            tag: "p".to_string(),
            attrs: vec![],
            children: vec![text_node("a"), text_node("bcd")],
            scores: Default::default(),
            metadata: Default::default(),
        };
        assert_eq!(tf_count_text_bytes(&parent), 4);
    }
}
