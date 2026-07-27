//! Shared utilities for the Trafilatura pipeline diagnostic tool.
//!
//! Provides:
//! - `fixture_dir()`: Path to the `tests/fixtures-trafilatura/` directory
//! - `load_test_case_tf()`: Load source.html.zst + expected.md.zst + optional annotations.json
//! - `decompress_zst()`: zstd decompression helper (panics on error)
//! - `try_decompress_zst()`: zstd decompression helper (returns Result)
//! - `Annotations`, `Classification`, `Severity`, `ConfusionMatrix` types
//! - `classify_output()`: Length-ratio classification
//! - `compute_confusion_matrix()`: Substring-based confusion matrix
//! - `tf_count_text_chars()`: Recursive text-node character counter
//! - `normalize_output()`: NFC normalization + whitespace collapse + trim

use std::path::PathBuf;

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
    String::from_utf8(decoded)
        .map_err(|e| format!("invalid UTF-8 in '{}': {e}", path.display()))
}

/// Decompress a zstd-compressed file into a `String`.
///
/// # Panics
///
/// - If the file cannot be read
/// - If the decompressed bytes are not valid UTF-8
pub fn decompress_zst(path: &std::path::Path) -> String {
    try_decompress_zst(path)
        .unwrap_or_else(|e| panic!("{e}"))
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
/// When `expected_len` is 0, returns `OverFiltering(Severe)`.
pub fn classify_output(output_len: usize, expected_len: usize) -> Classification {
    if expected_len == 0 {
        return Classification::OverFiltering(Severity::Severe);
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

    for w in &annotations.with {
        if output_text.contains(w.as_str()) {
            cm.tp += 1;
        } else {
            cm.fn_ += 1;
        }
    }

    for wo in &annotations.without {
        if !output_text.contains(wo.as_str()) {
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
    let dir = fixture_dir().join(name);
    let source_path = dir.join("source.html.zst");
    let expected_path = dir.join("expected.md.zst");
    let annotations_path = dir.join("annotations.json");

    let source_html = decompress_zst(&source_path);
    let expected_md = decompress_zst(&expected_path);

    let root = parse_html(&source_html)
        .unwrap_or_else(|e| panic!("Failed to parse HTML for '{name}': {e}"));

    let annotations = if annotations_path.exists() {
        let content = std::fs::read_to_string(&annotations_path)
            .unwrap_or_else(|e| panic!("Failed to read annotations.json for '{name}': {e}"));
        Some(serde_json::from_str(&content)
            .unwrap_or_else(|e| panic!("Failed to parse annotations.json for '{name}': {e}")))
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
/// Walks all descendant `Text` nodes and sums their character counts.
/// Replicates the private `count_text_chars` in `trafilatura.rs`.
pub fn tf_count_text_chars(node: &DomNode) -> usize {
    match node {
        DomNode::Text(t) => t.len(),
        DomNode::Element { children, .. } => children.iter().map(tf_count_text_chars).sum(),
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
        let longer = if a_chars.len() > b_chars.len() { &a_chars } else { &b_chars };
        let before: String = longer[ctx_start..pos].iter().collect();
        let after: String = longer[pos..ctx_end].iter().collect();
        return Some((pos, before, after));
    }

    None
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
        let tmp = std::env::temp_dir().join("test_decompress_zst.zst");
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
        assert_eq!(
            classify_output(100, 0),
            Classification::OverFiltering(Severity::Severe)
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
        assert!(!expected.is_empty(), "expected markdown should not be empty");
        let ann = annotations.expect("fixture should have annotations.json");
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
        assert!(after.starts_with("x"));
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
}
