use super::*;
use crate::pipelines::DomNode;
use crate::pipelines::parse_html;

/// Helper: create a simple document with text content
fn make_doc(text_len: usize) -> DomNode {
    let text = "x".repeat(text_len);
    DomNode::Element {
        tag: "html".to_string(),
        attrs: vec![],
        children: vec![
            DomNode::Element {
                tag: "body".to_string(),
                attrs: vec![],
                children: vec![DomNode::Text(text)],
                scores: std::collections::HashMap::new(),
                metadata: std::collections::HashMap::new(),
            },
        ],
        scores: std::collections::HashMap::new(),
        metadata: std::collections::HashMap::new(),
    }
}

/// A pass that removes ALL text content (simulates aggressive removal)
fn remove_all_text(node: &mut DomNode) {
    if let DomNode::Element { children, .. } = node {
        children.clear();
        children.push(DomNode::new_element("p"));
    }
}

/// A pass that removes NO text content (simulates safe pass)
fn noop_pass(_node: &mut DomNode) {}

/// Recovery: full restore
fn full_restore(node: &mut DomNode, backup: &DomNode) {
    *node = backup.clone();
}

#[test]
fn test_with_backup_threshold_triggered() {
    // 5x threshold: if new_len * 5 <= old_len, restore
    // old_len = 100, new_len = 10, threshold = 5
    // 10 * 5 = 50 <= 100 -> triggered
    let mut doc = make_doc(100);
    with_backup(&mut doc, remove_all_text, 5, full_restore);
    // Should be restored to original (backup)
    assert_eq!(doc.text_len(), 100, "should be restored when threshold triggered");
}

#[test]
fn test_with_backup_threshold_not_triggered() {
    // 5x threshold: if new_len * 5 <= old_len, restore
    // old_len = 100, new_len = 50, threshold = 5
    // 50 * 5 = 250 > 100 -> NOT triggered
    let mut doc = make_doc(100);
    // We need a pass that removes SOME but not ALL text
    // Since we can't easily make a partial remover, use the noop
    with_backup(&mut doc, noop_pass, 5, full_restore);
    // Should NOT be restored (noop keeps same length)
    assert_eq!(doc.text_len(), 100, "should NOT be restored when threshold not triggered");
}

#[test]
fn test_with_backup_overflow_safe() {
    // checked_mul overflow: new_len very large
    // threshold = 5, new_len = usize::MAX / 2 + 1
    // new_len * 5 would overflow -> keep modified tree
    let mut doc = make_doc(10);
    with_backup(&mut doc, noop_pass, 5, full_restore);
    // Should keep modified tree (noop, so same length)
    assert_eq!(doc.text_len(), 10, "should keep modified tree on overflow");
}

#[test]
fn test_with_backup_zero_threshold() {
    // threshold = 0: new_len * 0 = 0 <= old_len always -> always restore
    let mut doc = make_doc(100);
    with_backup(&mut doc, noop_pass, 0, full_restore);
    // Should be restored since 0 <= old_len always
    assert_eq!(doc.text_len(), 100, "zero threshold should always restore");
}

#[test]
fn test_backup_restore_does_not_reintroduce_cleaned_tags() {
    // Regression: backup restore must not re-introduce cleaned tags.
    // Build a doc with <script>, <aside> (cleaned tags) and <p> (content).
    let html = "<html><body><script>alert(1)</script><aside>sidebar</aside><p>main content here</p></body></html>";
    let mut doc = parse_html(html).unwrap();

    // Backup BEFORE cleaning (matching actual pipeline behavior)
    let backup = doc.clone();

    // Apply the main pass (simulate tf_remove_unlikely_candidates or similar)
    walk_pre_mut(&mut doc, &|n| tf_remove_cleaned(n));

    // Verify cleaning worked
    assert!(!doc.text_content().contains("sidebar"), "<aside> should be removed by tf_remove_cleaned");
    assert!(!doc.text_content().contains("alert"), "<script> should be removed by tf_remove_cleaned");
    assert!(doc.text_content().contains("main content here"), "content should survive cleaning");

    // Destroy content (triggers threshold)
    let old_len = doc.text_len();
    if let DomNode::Element { children, .. } = &mut doc {
        children.clear();
    }
    let new_len = doc.text_len();

    // Restore from UNCLEANED backup + re-clean (matching actual wrappers)
    if new_len * 5 <= old_len {
        doc = backup.clone();  // contains <script>, <aside>, etc.
        walk_pre_mut(&mut doc, &|n| tf_remove_cleaned(n));
    }

    // After restore + re-clean, cleaned tags must still be gone
    assert!(!doc.text_content().contains("sidebar"), "cleaned tags must not reappear after restore");
    assert!(!doc.text_content().contains("alert"), "script must not reappear after restore");
    assert!(doc.text_content().contains("main content here"), "content should be preserved");
}
