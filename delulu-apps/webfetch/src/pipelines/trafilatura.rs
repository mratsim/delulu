use once_cell::sync::Lazy;

use crate::pipelines::{DomNode, walk_pre_mut};

use super::passes::tf_filters::{
    tf_isolate_content_container, tf_remove_cleaned, tf_remove_empty_cut, tf_remove_teaser,
    tf_remove_unlikely_candidates, tf_strip_unwrapped,
};
use super::passes::tf_transforms::{
    tf_canonicalize_strip_non_content, tf_canonicalize_unwrap_containers,
};
use super::passes::tf_transforms::{
    tf_convert_breaks, tf_convert_formatting, tf_convert_headings, tf_convert_lists,
    tf_convert_quotes, tf_convert_refs_and_details,
};

/// Function pointer type for Trafilatura-equivalent pipeline passes.
///
/// Takes a `&mut DomNode` representing the root node
/// and mutates the tree in-place. Passes should handle all DomNode
/// variants and should not panic on well-formed input.
pub type PassFn = fn(&mut DomNode);

/// Apply tf_remove_unlikely_candidates with backup/restore safety mechanism.
///
/// Measures text content before/after pruning. If >=86% of text was removed
/// (i.e., new_len * 7 <= old_len), restores from backup copy.
///
/// Matches Trafilatura's `prune_unwanted_nodes(tree, OVERALL_DISCARD_XPATH,
/// with_backup=True)` (trafilatura/htmlprocessing.py:prune_unwanted_nodes).
///
/// Trafilatura logic: `return tree if new_len > old_len / 7 else backup`
/// Our equivalent: `if new_len * 7 <= old_len { restore }`
///
/// The backup only reverts the unlikely-candidates pass. Other passes
/// (cleaned removal, teaser removal) are NOT reverted. Subsequent passes
/// (canonicalization, link density) still apply to the (possibly restored) tree.
///
/// This is a safety mechanism, not a substitute for pattern refinement.
///
/// Reference: Trafilatura `main_extractor.py` line 710:
///   `tree = prune_unwanted_nodes(tree, OVERALL_DISCARD_XPATH, with_backup=True)`
pub fn apply_tf_remove_unlikely_candidates_with_backup(node: &mut DomNode) {
    let old_len = measure_output(node);
    let backup = node.clone();

    // Apply tf_remove_unlikely_candidates
    walk_pre_mut(node, &|n| tf_remove_unlikely_candidates(n));

    let new_len = measure_output(node);

    // Trafilatura threshold: restore if new_len * 7 <= old_len (>=86% removed)
    // Uses multiplication to avoid integer division edge case
    if new_len * 7 <= old_len {
        tracing::warn!(
            "tf_remove_unlikely_candidates removed >=86% of text ({} -> {} chars), restoring backup",
            old_len,
            new_len
        );
        *node = backup;
    } else {
        tracing::debug!(
            "tf_remove_unlikely_candidates: {} -> {} chars (safe, {:.1}% removed)",
            old_len,
            new_len,
            if old_len > 0 {
                100.0 - (new_len as f64 / old_len as f64 * 100.0)
            } else {
                0.0
            }
        );
    }
}

// ---------------------------------------------------------------------------
// Retry Level Constants
// ---------------------------------------------------------------------------

/// Level: Balanced — standard Trafilatura-equivalent pipeline.
///
/// Order:
/// 1. Remove MANUALLY_CLEANED tags (figure, script, nav, etc.)
/// 2. Remove TEASER_DISCARD elements (teaser in class/id)
/// 3. Remove UNLIKELY_CANDIDATES elements (class/id matches OVERALL_DISCARD_XPATH)
/// 4. Unwrap MANUALLY_STRIPPED tags (abbr, address, etc.)
/// 5. Remove CUT_EMPTY_ELEMS (empty p, div, li, etc.)
/// 6-11. Tag conversion passes (headings, lists, quotes, formatting, breaks, refs)
/// 12-13. Canonicalization (strip non-content, unwrap containers)
pub static TF_BALANCED: Lazy<&[PassFn]> = Lazy::new(|| {
    &[
        (|node| walk_pre_mut(node, &|n| tf_remove_cleaned(n))) as fn(&mut DomNode),
        (|node| walk_pre_mut(node, &|n| tf_remove_teaser(n))) as fn(&mut DomNode),
        apply_tf_remove_unlikely_candidates_with_backup,
        tf_strip_unwrapped,
        (|node| walk_pre_mut(node, &|n| tf_remove_empty_cut(n))) as fn(&mut DomNode),
        (|node| walk_pre_mut(node, &|n| tf_convert_headings(n))) as fn(&mut DomNode),
        (|node| walk_pre_mut(node, &|n| tf_convert_lists(n))) as fn(&mut DomNode),
        (|node| walk_pre_mut(node, &|n| tf_convert_quotes(n))) as fn(&mut DomNode),
        (|node| walk_pre_mut(node, &|n| tf_convert_formatting(n))) as fn(&mut DomNode),
        (|node| walk_pre_mut(node, &|n| tf_convert_breaks(n))) as fn(&mut DomNode),
        (|node| walk_pre_mut(node, &|n| tf_convert_refs_and_details(n))) as fn(&mut DomNode),
        tf_canonicalize_strip_non_content,
        tf_isolate_content_container,
        tf_canonicalize_unwrap_containers,
    ]
});

/// Level: Recall — same as Balanced but WITHOUT `tf_remove_empty_cut`.
///
/// Less aggressive filtering. Use as fallback when Balanced produces too little output.
pub static TF_RECALL: Lazy<&[PassFn]> = Lazy::new(|| {
    &[
        (|node| walk_pre_mut(node, &|n| tf_remove_cleaned(n))) as fn(&mut DomNode),
        (|node| walk_pre_mut(node, &|n| tf_remove_teaser(n))) as fn(&mut DomNode),
        apply_tf_remove_unlikely_candidates_with_backup,
        tf_strip_unwrapped,
        // tf_remove_empty_cut SKIPPED -- preserve all content
        (|node| walk_pre_mut(node, &|n| tf_convert_headings(n))) as fn(&mut DomNode),
        (|node| walk_pre_mut(node, &|n| tf_convert_lists(n))) as fn(&mut DomNode),
        (|node| walk_pre_mut(node, &|n| tf_convert_quotes(n))) as fn(&mut DomNode),
        (|node| walk_pre_mut(node, &|n| tf_convert_formatting(n))) as fn(&mut DomNode),
        (|node| walk_pre_mut(node, &|n| tf_convert_breaks(n))) as fn(&mut DomNode),
        (|node| walk_pre_mut(node, &|n| tf_convert_refs_and_details(n))) as fn(&mut DomNode),
        tf_canonicalize_strip_non_content,
        tf_isolate_content_container,
        tf_canonicalize_unwrap_containers,
    ]
});

// ---------------------------------------------------------------------------
// Orchestrator
// ---------------------------------------------------------------------------

/// Minimum output length (in characters) for a successful tf_* extraction.
/// Uses the same constant as the readability pipeline for consistency.
pub const TF_MIN_OUTPUT_CHARS: usize = 500;

/// Measure the rendered text content length of a DOM tree.
fn measure_output(node: &DomNode) -> usize {
    // Count text characters in the tree
    count_text_chars(node)
}

/// Count total text characters in a DOM tree recursively.
fn count_text_chars(node: &DomNode) -> usize {
    match node {
        DomNode::Text(t) => t.len(),
        DomNode::Element { children, .. } => children.iter().map(count_text_chars).sum(),
        _ => 0,
    }
}

/// Run the tf_* pipeline with a retry cascade.
///
/// Tries `TF_BALANCED` first. If output < `TF_MIN_OUTPUT_CHARS`, tries `TF_RECALL`.
/// Logs fallbacks with `tracing::info!`.
pub fn filter_trafilatura(node: &mut DomNode) {
    // Run analysis passes first to populate metadata
    crate::pipelines::passes::rd_analysis::mark_data_tables_by_structure(node);

    // TODO: Add fuzzing guard for large DOM trees
    let levels: &[&[PassFn]] = &[*TF_BALANCED, *TF_RECALL];

    let original = node.clone();
    let mut best_tree = original.clone();
    let mut best_len = 0usize;

    for (i, level) in levels.iter().enumerate() {
        let mut attempt = original.clone();
        for pass in *level {
            pass(&mut attempt);
        }

        let len = measure_output(&attempt);
        tracing::debug!("filter_trafilatura: level {} produced {} chars", i + 1, len,);

        if len > best_len {
            best_len = len;
            best_tree = attempt;
        }

        // Early exit: if balanced produced enough output, skip recall
        if len >= TF_MIN_OUTPUT_CHARS {
            break;
        }
    }

    tracing::info!(
        "filter_trafilatura: best output {} chars (from level {})",
        best_len,
        if best_len >= TF_MIN_OUTPUT_CHARS {
            "balanced"
        } else {
            "recall"
        },
    );

    *node = best_tree;
}
