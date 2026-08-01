use crate::pipelines::parse_html;

// ── Anti-regression: UTF-8 byte-slice panic in JSON-LD error path ─────────
//
// The bug (line 59 in tf_analysis.rs, now fixed):
//   let preview = &text[..text.len().min(200)];
// This panics when byte index `text.len().min(200)` falls in the middle of a
// multi-byte UTF-8 character (e.g., Chinese, emoji, accented chars).
//
// The fix:
//   text.chars().take(200).collect()
// This operates on Unicode scalar values, not byte indices, so it never panics.
//
// These tests verify that the fix works by feeding malformed JSON-LD with
// multi-byte content where the 200-byte boundary cuts through a multi-byte
// character.

#[test]
fn test_extract_jsonld_unicode_malformed_no_panic_chinese() {
    // Construct malformed JSON-LD where byte 200 falls in the middle of '中'
    // (3-byte UTF-8 character U+4E2D, encoded as E4 B8 AD).
    //
    // Layout:
    //   "articleBody"  (11 bytes, 0..11)
    //   "x".repeat(188)       (188 bytes, 11..199)
    //   "中"                  (3 bytes, 199..202: E4 B8 AD)
    //
    // text.len() = 202
    // text.len().min(200) = 200
    // text[..200] would include bytes 0..199, splitting '中' at byte 199/200.
    // Byte 200 is 0xB8 (continuation byte) → would panic with old code.
    let unicode_payload = format!("articleBody{}中", "x".repeat(188));
    assert!(
        unicode_payload.len() > 200,
        "payload must be >200 bytes to trigger the bug; got {}",
        unicode_payload.len()
    );

    let html = format!(
        r#"<html><body><script type="application/ld+json">{}</script></body></html>"#,
        unicode_payload
    );
    let root = parse_html(&html).unwrap();
    let result =
        crate::pipelines::passes::tf_analysis::extract_jsonld_article_body(&root);
    assert!(
        result.is_none(),
        "malformed JSON-LD with multi-byte Chinese should return None (not panic)"
    );
}

#[test]
fn test_extract_jsonld_unicode_malformed_no_panic_emoji() {
    // Same regression but with emoji (4-byte UTF-8: F0 9F 98 80 for U+1F600).
    //
    // Layout:
    //   "articleBody"         (11 bytes, 0..11)
    //   49 × "😀"               (196 bytes, 11..207)
    //   "ab"                  (2 bytes, 207..209)
    //   "中"                  (3 bytes, 209..212: E4 B8 AD)
    //
    // text.len() = 212
    // text.len().min(200) = 200
    // text[..200] includes bytes 0..199, which includes:
    //   - "articleBody" (0..11)
    //   - 47 × "😀"      (11..199) — 47 × 4 = 188 bytes
    //   - byte 199 = first byte of 48th 😀 (0xF0)
    // Byte 200 = second byte of 48th 😀 (0x9F, continuation byte) → would panic.
    let emoji_block: String = std::iter::repeat_n("😀", 49).collect();
    let unicode_payload = format!("articleBody{}ab中", emoji_block);
    assert!(
        unicode_payload.len() > 200,
        "payload must be >200 bytes to trigger the bug; got {}",
        unicode_payload.len()
    );

    let html = format!(
        r#"<html><body><script type="application/ld+json">{}</script></body></html>"#,
        unicode_payload
    );
    let root = parse_html(&html).unwrap();
    let result =
        crate::pipelines::passes::tf_analysis::extract_jsonld_article_body(&root);
    assert!(
        result.is_none(),
        "malformed JSON-LD with emoji should return None (not panic)"
    );
}

#[test]
fn test_extract_jsonld_unicode_malformed_no_panic_accented() {
    // Same regression but with accented Latin characters (2-byte UTF-8).
    // 'é' is U+00E9, encoded as C3 A9 (2 bytes).
    //
    // Layout:
    //   "articleBody"         (11 bytes, 0..11)
    //   "x".repeat(188)       (188 bytes, 11..199)
    //   "é"                   (2 bytes, 199..201: C3 A9)
    //
    // text.len() = 201
    // text.len().min(200) = 200
    // text[..200] includes bytes 0..199, which includes:
    //   - "articleBody" (0..11)
    //   - 188 × 'x'     (11..199)
    //   - byte 199 = first byte of 'é' (0xC3)
    // Byte 200 = second byte of 'é' (0xA9, continuation byte) → would panic.
    let unicode_payload = format!("articleBody{}é", "x".repeat(188));
    assert!(
        unicode_payload.len() > 200,
        "payload must be >200 bytes to trigger the bug; got {}",
        unicode_payload.len()
    );

    let html = format!(
        r#"<html><body><script type="application/ld+json">{}</script></body></html>"#,
        unicode_payload
    );
    let root = parse_html(&html).unwrap();
    let result =
        crate::pipelines::passes::tf_analysis::extract_jsonld_article_body(&root);
    assert!(
        result.is_none(),
        "malformed JSON-LD with accented chars should return None (not panic)"
    );
}

#[test]
fn test_extract_jsonld_unicode_malformed_no_panic_smart_quotes() {
    // Same regression but with smart/curly quotes (multi-byte UTF-8).
    // '\u{201C}' (LEFT DOUBLE QUOTATION MARK) is E2 80 9C (3 bytes).
    //
    // Layout:
    //   "articleBody"         (11 bytes, 0..11)
    //   "x".repeat(187)       (187 bytes, 11..198)
    //   "\u{201C}"            (3 bytes, 198..201: E2 80 9C)
    //
    // text.len() = 201
    // text.len().min(200) = 200
    // text[..200] includes bytes 0..199, which includes:
    //   - "articleBody" (0..11)
    //   - 187 × 'x'     (11..198)
    //   - bytes 198..199 = first two bytes of '\u{201C}' (E2 80)
    // Byte 200 = third byte of '\u{201C}' (0x9C, continuation byte) → would panic.
    let unicode_payload = format!("articleBody{}\u{201C}", "x".repeat(187));
    assert!(
        unicode_payload.len() > 200,
        "payload must be >200 bytes to trigger the bug; got {}",
        unicode_payload.len()
    );

    let html = format!(
        r#"<html><body><script type="application/ld+json">{}</script></body></html>"#,
        unicode_payload
    );
    let root = parse_html(&html).unwrap();
    let result =
        crate::pipelines::passes::tf_analysis::extract_jsonld_article_body(&root);
    assert!(
        result.is_none(),
        "malformed JSON-LD with smart quotes should return None (not panic)"
    );
}

// ── Baseline: ASCII-only malformed JSON-LD ──────────────────────────────

#[test]
fn test_extract_jsonld_ascii_malformed_returns_none() {
    // Re-verify that ASCII-only malformed JSON-LD still returns None
    // (no panic expected even with old code, but confirm the fix didn't
    // break the ASCII path).
    let malformed_json = r#"{invalid json here"#;
    let html = format!(
        r#"<html><body><script type="application/ld+json">{}</script></body></html>"#,
        malformed_json
    );
    let root = parse_html(&html).unwrap();
    let result =
        crate::pipelines::passes::tf_analysis::extract_jsonld_article_body(&root);
    assert!(
        result.is_none(),
        "malformed JSON-LD should return None (not panic)"
    );
}

// ── Valid JSON-LD still works ──────────────────────────────────────────

#[test]
fn test_extract_jsonld_valid_article_body_returns_some() {
    // Verify that valid JSON-LD with articleBody still works correctly
    // after the fix (no regression on the happy path).
    let body_text = "A".repeat(100);
    let valid_json = format!(r#"{{"articleBody": "{}"}}"#, body_text);
    let html = format!(
        r#"<html><body><script type="application/ld+json">{}</script></body></html>"#,
        valid_json
    );
    let root = parse_html(&html).unwrap();
    let result =
        crate::pipelines::passes::tf_analysis::extract_jsonld_article_body(&root);
    assert!(
        result.is_some(),
        "valid JSON-LD with articleBody should return Some"
    );
    assert_eq!(result.unwrap(), body_text);
}
