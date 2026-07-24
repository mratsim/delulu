//! Shared utilities for the Readability JS test suite.
//!
//! Provides:
//! - `fixture_dir()`: Path to the 130 test-pages directory
//! - `load_test_case()`: Load source.html.zst and expected.html.zst for a given name
//! - `normalize_html()`: 6-step canonical normalization for comparison

use std::io::Read;
use std::path::PathBuf;

use delulu_webfetch::pipelines::{DomNode, parse_html};
use regex::Regex;

/// Resolve the path to the vendored Readability.js test-pages directory.
///
/// `CARGO_MANIFEST_DIR` = `<workspace>/delulu/delulu-apps/webfetch`
/// Target               = `<workspace>/delulu/delulu-apps/webfetch/tests/fixtures-readability`
pub fn fixture_dir() -> PathBuf {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest.join("tests/fixtures-readability")
}

/// Load a single test case by directory name.
///
/// Returns a tuple of `(parsed DomNode tree, expected HTML string)`.
///
/// # Panics
///
/// - If the fixture directory or required files do not exist
/// - If `source.html.zst` fails to decompress or parse
pub fn load_test_case(name: &str) -> (DomNode, String) {
    let dir = fixture_dir().join(name);
    let source_path = dir.join("source.html.zst");
    let expected_path = dir.join("expected.html.zst");

    let source_compressed = std::fs::read(&source_path)
        .unwrap_or_else(|e| panic!("Failed to read source.html.zst for '{name}': {e}"));
    let mut decoder = zstd::Decoder::new(source_compressed.as_slice()).unwrap_or_else(|e| {
        panic!("Failed to create zstd decoder for source.html.zst '{name}': {e}")
    });
    let mut source_html = String::new();
    decoder
        .read_to_string(&mut source_html)
        .unwrap_or_else(|e| panic!("Failed to decompress source.html.zst for '{name}': {e}"));

    let expected_compressed = std::fs::read(&expected_path)
        .unwrap_or_else(|e| panic!("Failed to read expected.html.zst for '{name}': {e}"));
    let mut decoder = zstd::Decoder::new(expected_compressed.as_slice()).unwrap_or_else(|e| {
        panic!("Failed to create zstd decoder for expected.html.zst '{name}': {e}")
    });
    let mut expected_html = String::new();
    decoder
        .read_to_string(&mut expected_html)
        .unwrap_or_else(|e| panic!("Failed to decompress expected.html.zst for '{name}': {e}"));

    let node = parse_html(&source_html)
        .unwrap_or_else(|e| panic!("Failed to parse HTML for '{name}': {e}"));

    (node, expected_html)
}
/// 6-step HTML canonical normalization for comparison.
///
/// Steps:
/// 1. Trim leading/trailing whitespace
/// 2. Normalize `\r\n` → `\n`
/// 3. Collapse whitespace between tags: `>   <` → `><`
/// 4. Collapse multiple internal whitespace: `{2,}` → single space
/// 5. Trim again after collapsing
///
/// This normalization is designed to eliminate irrelevant whitespace
/// differences between the Rust and JavaScript readability outputs while
/// preserving meaningful content and structure.
pub fn normalize_html(html: &str) -> String {
    let re_between_tags = Regex::new(r">\s+<").expect("valid regex");
    let re_whitespace = Regex::new(r"\s{2,}").expect("valid regex");

    let s = html.trim();
    let s = s.replace("\r\n", "\n");
    let s = re_between_tags.replace_all(&s, "><").to_string();
    let s = re_whitespace.replace_all(&s, " ").to_string();
    s.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixture_dir_exists() {
        let dir = fixture_dir();
        assert!(dir.exists(), "fixture dir should exist: {:?}", dir);
        assert!(dir.is_dir(), "fixture dir should be a directory: {:?}", dir);
    }

    #[test]
    fn fixture_dir_has_expected_count() {
        let dir = fixture_dir();
        let count = std::fs::read_dir(&dir)
            .expect("read fixture dir")
            .filter_map(|e| e.ok())
            .filter(|e| e.path().is_dir())
            .count();
        // Mozilla readability.js has 130 test-pages as of v0.6.0
        assert!(count >= 130, "expected ≥130 test cases, found {count}");
    }

    #[test]
    fn load_001() {
        let (_node, expected) = load_test_case("001");
        // DomNode is always non-empty (it's the root element)
        assert!(!expected.is_empty(), "001 should have expected.html");
    }

    #[test]
    fn normalize_html_collapses_whitespace() {
        let input = "  <div>\n  <p>Hello</p>  \n</div>  ";
        let result = normalize_html(input);
        // After collapse: "<div><p>Hello</p></div>"
        assert_eq!(result, "<div><p>Hello</p></div>");
    }

    #[test]
    fn normalize_html_handles_crlf() {
        let input = "<div>\r\n<p>text</p>\r\n</div>";
        let result = normalize_html(input);
        assert_eq!(result, "<div><p>text</p></div>");
    }

    #[test]
    fn normalize_html_handles_empty() {
        assert_eq!(normalize_html(""), "");
        assert_eq!(normalize_html("   "), "");
    }
}
