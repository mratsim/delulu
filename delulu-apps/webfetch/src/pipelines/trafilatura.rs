use once_cell::sync::Lazy;
use std::collections::HashMap;

use crate::pipelines::{DomNode, WalkerAction, walk_post_mut, walk_pre_mut};

use super::passes::tf_filters::{
    collect_p_elements, count_non_ws_chars, count_p_text, count_text_chars,
    extract_jsonld_article_body, get_inner_text, recover_wild_p_elements,
    tf_extract_script_templates, tf_fallback_content_container, tf_filter_by_link_density,
    tf_filter_tag_catalog, tf_isolate_content_container, tf_protect_content_forms,
    tf_remove_cleaned, tf_remove_empty_cut, tf_remove_teaser,
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

/// Remove unlikely candidates with backup/restore safety net.
///
/// Pre: `node` is a valid DOM tree. The tree has been parsed and basic cleaning applied.
/// Post: Elements matching OVERALL_DISCARD_XPATH are removed. If >=80% of text is removed (threshold: 5×), the node is restored to the backup state. Uses `*node = backup` (full restore).
///
/// Matches Trafilatura's `prune_unwanted_nodes(tree, OVERALL_DISCARD_XPATH,
/// with_backup=True)` (trafilatura/htmlprocessing.py:prune_unwanted_nodes).
///
/// Trafilatura logic: `return tree if new_len > old_len / 5 else backup`
/// Our equivalent: `if new_len * 5 <= old_len { restore }`
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

    // Adjusted threshold: restore if new_len * 5 <= old_len (>=80% removed)
    // Uses multiplication to avoid integer division edge case
    if new_len * 5 <= old_len {
        tracing::warn!(
            "tf_remove_unlikely_candidates removed >=80% of text ({} -> {} chars), restoring backup",
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

/// Filter by link density with backup/restore safety net.
///
/// Pre: `node` is a valid DOM tree. Unlikely candidates have been removed.
/// Post: Elements with link density >50% are removed. If >=95% of text is removed (threshold: 19×), the node is restored to the backup state. Uses `*node = backup` (full restore).
///
/// This prevents catastrophic content loss when the full-page link density
/// filter removes the main content container (e.g., dailymail.co.uk where
/// the top-level wrapper has high link density from navigation elements).
pub fn apply_tf_filter_by_link_density_with_backup(node: &mut DomNode) {
    let old_len = measure_output(node);
    let backup = node.clone();

    // Apply link density filter
    walk_pre_mut(node, &|n| tf_filter_by_link_density(n));

    let new_len = measure_output(node);

    // If >=95% of text was removed by link density filtering, restore from backup
    if new_len * 19 <= old_len {
        tracing::warn!(
            "link density filter removed >=95% of text ({} -> {} chars), restoring backup",
            old_len,
            new_len
        );
        *node = backup;
    } else {
        tracing::debug!(
            "link density filter: {} -> {} chars (safe, {:.1}% removed)",
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

/// Isolate content container with backup/recovery safety net.
///
/// Pre: `node` is a valid DOM tree. Link density filtering has been applied.
/// Post: Content container is isolated via BODY_XPATH cascade. If >=90% of text is removed (threshold: 10×), wild `<p>` elements are recovered from the backup (not full restore).
///
/// This is a simplified version of Python's `recover_wild_text()` which scans
/// the backup tree for wild `<p>`, `<code>`, `<quote>`, `<table>` elements.
pub fn apply_tf_isolate_container_with_backup(node: &mut DomNode) {
    let old_len = measure_output(node);
    let backup = node.clone();

    // Apply container isolation passes
    tf_isolate_content_container(node);
    tf_fallback_content_container(node);

    let new_len = measure_output(node);

    // If >=90% of text was removed by container isolation, recover content
    if new_len * 10 <= old_len {
        tracing::warn!(
            "container isolation removed >=90% of text ({} -> {} chars), recovering wild p-elements",
            old_len,
            new_len
        );
        // Recover <p> elements from the backup tree that aren't already in node
        // This is a simplified version of Python's recover_wild_text()
        let existing_text = get_inner_text(node);
        recover_wild_p_elements(node, &backup, &existing_text);
        let recovered_len = measure_output(node);
        tracing::info!(
            "recovered wild p-elements: {} -> {} chars",
            new_len,
            recovered_len
        );
    } else {
        tracing::debug!(
            "container isolation: {} -> {} chars (safe, {:.1}% removed)",
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
/// Pre: `node` is a valid DOM tree. `rd_analysis::mark_data_tables_by_structure` has been called.
/// Post: All 18 passes are applied in order. `node` is mutated in-place. Output contains only tags in TAG_CATALOG.
///
/// Order:
/// 1. Remove MANUALLY_CLEANED tags (figure, script, nav, etc.)
/// 2. Remove TEASER_DISCARD elements (teaser in class/id)
/// 3. Remove UNLIKELY_CANDIDATES elements (class/id matches OVERALL_DISCARD_XPATH)
/// 4. Unwrap MANUALLY_STRIPPED tags (abbr, address, etc.)
/// 5. Remove CUT_EMPTY_ELEMS (empty p, div, li, etc.)
/// 6. Remove high-link-density elements (sidebar ads, nav blocks, etc.)
/// 7. Tag conversion passes (headings, lists, quotes, formatting, breaks, refs)
/// 8. Canonicalization (strip non-content, unwrap containers)
/// 9. DISCARD_IMAGE_ELEMENTS (remove caption elements)
/// 10. TAG_CATALOG filter (whitelist allowed output tags)
fn apply_tf_filter_tag_catalog(node: &mut DomNode) {
    use crate::pipelines::walkers::WalkerFilter;
    let mut filter = |n: &mut DomNode| -> WalkerAction { tf_filter_tag_catalog(n) };
    let mut filters: Vec<&mut WalkerFilter> = vec![&mut filter];
    walk_post_mut(node, &mut filters, None);
}

/// Apply tag catalog filter — remove all tags not in TAG_CATALOG.
///
/// Pre: `node` is a valid DOM tree. All other passes have been applied.
/// Post: Only tags in TAG_CATALOG survive. Unknown tags are replaced with their children (ReplaceWithChildren). Uses `walk_post_mut` (ReplaceWithChildren panics in pre-order).
pub static TF_BALANCED: Lazy<&[PassFn]> = Lazy::new(|| {
    &[
        tf_protect_content_forms,
        tf_extract_script_templates,
        (|node| walk_pre_mut(node, &|n| tf_remove_cleaned(n))) as fn(&mut DomNode),
        (|node| walk_pre_mut(node, &|n| tf_remove_teaser(n))) as fn(&mut DomNode),
        apply_tf_remove_unlikely_candidates_with_backup,
        tf_strip_unwrapped,
        (|node| walk_pre_mut(node, &|n| tf_remove_empty_cut(n))) as fn(&mut DomNode),
        apply_tf_filter_by_link_density_with_backup,
        (|node| walk_pre_mut(node, &|n| tf_convert_headings(n))) as fn(&mut DomNode),
        (|node| walk_pre_mut(node, &|n| tf_convert_lists(n))) as fn(&mut DomNode),
        (|node| walk_pre_mut(node, &|n| tf_convert_quotes(n))) as fn(&mut DomNode),
        (|node| walk_pre_mut(node, &|n| tf_convert_formatting(n))) as fn(&mut DomNode),
        (|node| walk_pre_mut(node, &|n| tf_convert_breaks(n))) as fn(&mut DomNode),
        (|node| walk_pre_mut(node, &|n| tf_convert_refs_and_details(n))) as fn(&mut DomNode),
        tf_canonicalize_strip_non_content,
        apply_tf_isolate_container_with_backup,
        tf_canonicalize_unwrap_containers,
        // Final tag whitelist — remove any tags not in TAG_CATALOG
        apply_tf_filter_tag_catalog,
    ]
});

/// Level: Recall — same as Balanced but WITHOUT `tf_remove_empty_cut` and WITHOUT `apply_tf_filter_tag_catalog`.
///
/// Pre: `node` is a valid DOM tree. `rd_analysis::mark_data_tables_by_structure` has been called.
/// Post: All 16 passes are applied in order. `node` is mutated in-place. `tf_remove_empty_cut` and `apply_tf_filter_tag_catalog` are NOT applied.
///
/// Less aggressive filtering. Use as fallback when Balanced produces too little output.
pub static TF_RECALL: Lazy<&[PassFn]> = Lazy::new(|| {
    &[
        tf_protect_content_forms,
        tf_extract_script_templates,
        (|node| walk_pre_mut(node, &|n| tf_remove_cleaned(n))) as fn(&mut DomNode),
        (|node| walk_pre_mut(node, &|n| tf_remove_teaser(n))) as fn(&mut DomNode),
        apply_tf_remove_unlikely_candidates_with_backup,
        tf_strip_unwrapped,
        // tf_remove_empty_cut SKIPPED -- preserve all content
        apply_tf_filter_by_link_density_with_backup,
        (|node| walk_pre_mut(node, &|n| tf_convert_headings(n))) as fn(&mut DomNode),
        (|node| walk_pre_mut(node, &|n| tf_convert_lists(n))) as fn(&mut DomNode),
        (|node| walk_pre_mut(node, &|n| tf_convert_quotes(n))) as fn(&mut DomNode),
        (|node| walk_pre_mut(node, &|n| tf_convert_formatting(n))) as fn(&mut DomNode),
        (|node| walk_pre_mut(node, &|n| tf_convert_breaks(n))) as fn(&mut DomNode),
        (|node| walk_pre_mut(node, &|n| tf_convert_refs_and_details(n))) as fn(&mut DomNode),
        tf_canonicalize_strip_non_content,
        apply_tf_isolate_container_with_backup,
        tf_canonicalize_unwrap_containers,
    ]
});

// ---------------------------------------------------------------------------
// Orchestrator
// ---------------------------------------------------------------------------

/// Minimum output length (in characters) for a successful tf_* extraction.
/// Uses the same constant as the readability pipeline for consistency.
pub const TF_MIN_OUTPUT_CHARS: usize = 1000;

/// Measure output length by delegating to `tf_filters::count_text_chars`.
///
/// Pre: `node` is a valid DOM tree (may be empty).
/// Post: Returns the total number of text characters in `node` (same as `count_text_chars`).
///
/// Thin wrapper that delegates to `count_text_chars(node)` with no additional logic.
/// Kept in orchestrator for measurement single-point-of-change.
///
/// Reference: Trafilatura `len(tree.text_content())` in `htmlprocessing.py:95,106`
fn measure_output(node: &DomNode) -> usize {
    count_text_chars(node)
}


/// Recover `<p>` elements from the original tree that were lost during pipeline processing.
///
/// Pre: `best_tree` is the current extraction result (may contain partial content).
///      `original` is a clone of the original tree (pre-pipeline).
///      `min_p_len` is the minimum paragraph length in characters (0 = no filter).
/// Post: `<p>` elements from the cleaned original tree that don't duplicate existing text
///       are appended to `best_tree.children`. Dedup uses substring `contains()` (matching
///       Python behavior, not exact match). Paragraphs shorter than `min_p_len` are filtered out
///       to avoid recovering boilerplate/sidebar snippets (see OVERALL_DISCARD_XPATH).
/// Returns: The number of paragraphs appended to `best_tree`.
///
/// Note: This function is orchestrator-level, coordinating cleaning passes, element collection,
///       and tree manipulation. It does NOT use recursion — stack depth is bounded by the
///       DOM tree traversal in `collect_p_elements` and `walk_pre_mut`.
///
/// Reference: Trafilatura `recover_wild_text()` in `main_extractor.py:536-560`
fn recover_wild_paragraphs(best_tree: &mut DomNode, original: &DomNode, min_p_len: usize) -> usize {
    let mut recovery_tree = original.clone();
    // Apply tf_protect_content_forms first to protect form-wrapped content
    tf_protect_content_forms(&mut recovery_tree);
    // Then apply cleaning passes to remove boilerplate
    walk_pre_mut(&mut recovery_tree, &|n| tf_remove_teaser(n));
    // Also remove script, style, svg, template, iframe, canvas
    walk_pre_mut(&mut recovery_tree, &|n| {
        match n {
            DomNode::Element { tag, .. } if matches!(
                tag.as_str(),
                "script" | "style" | "svg" | "template" | "iframe" | "canvas"
            ) => WalkerAction::Remove,
            _ => WalkerAction::Continue,
        }
    });
    // Collect <p> elements from the cleaned tree (boilerplate already removed)
    let mut recovered_ps: Vec<DomNode> = Vec::new();
    collect_p_elements(&recovery_tree, &mut recovered_ps);
    // Get existing text in best_tree for dedup
    let existing_text = get_inner_text(best_tree);
    // Add recovered <p> elements that aren't already in best_tree
    let mut appended = 0usize;
    if let DomNode::Element { children, .. } = best_tree {
        for p_node in &recovered_ps {
            let p_text = get_inner_text(p_node);
            let trimmed = p_text.trim();
            if trimmed.len() >= min_p_len && !trimmed.is_empty() && !existing_text.contains(&p_text) {
                children.push(p_node.clone());
                appended += 1;
            }
        }
    }
    appended
}







/// Run the Trafilatura extraction pipeline on a parsed DOM tree.
///
/// Pre: `node` is a fully parsed DOM tree with `<html>` root.
///      `rd_analysis::mark_data_tables_by_structure` has NOT yet been called
///      (caller must call it before this function).
/// Post: `node` is mutated to contain the best available extraction result;
///       benchmark output is byte-identical for all 961 fixtures.
///       The retry cascade (TF_BALANCED → TF_RECALL → wild p-recovery → JSON-LD rescue)
///       has been applied. `node` may contain a subset of original elements.
///       Never panics — all errors are silently recovered.
/// Side effects: Clones the DOM tree up to 7 times per invocation (pre-existing behavior).
///               Calls `rd_analysis::mark_data_tables_by_structure` on the input node.
///
/// Reference: Trafilatura `trafilatura_sequence()` in `core.py:95-122`
pub fn filter_trafilatura(node: &mut DomNode) {
    // Run analysis passes first to populate metadata
    crate::pipelines::passes::rd_analysis::mark_data_tables_by_structure(node);

    // TODO: Add fuzzing guard for large DOM trees (e.g., skip retry cascade when nodes > INPUT_NODE_LIMIT)
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

    // If best output is below threshold, try recovering <p> elements
    // Uses a cleaned copy with boilerplate containers removed first.
    if best_len < 500 || (best_len < 2200 && count_non_ws_chars(&best_tree) < 250) {
        let old_len = best_len;
        let n = recover_wild_paragraphs(&mut best_tree, &original, 0);
        best_len = measure_output(&best_tree);
        if best_len > old_len {
            tracing::info!(
                "recover_wild_text: {} -> {} chars (recovered {} p-elements)",
                old_len,
                best_len,
                n
            );
        } else {
            tracing::debug!("recover_wild_text: no improvement ({} chars)", best_len);
        }
    } else if best_len < 800 {
        let old_len = best_len;
        let n = recover_wild_paragraphs(&mut best_tree, &original, 100);
        best_len = measure_output(&best_tree);
        if best_len > old_len {
            tracing::info!(
                "recover_wild_text (filtered): {} -> {} chars (recovered {} p-elements, >={} char filter)",
                old_len,
                best_len,
                n,
                100
            );
        } else {
            tracing::debug!("recover_wild_text (filtered): no improvement ({} chars)", best_len);
        }
    }

    // JSON-LD recovery: try to extract articleBody from original tree as rescue fallback.
    // Uses the original tree (which still has JSON-LD scripts) to extract articleBody
    // directly from script elements, then adds a <p> with the text to best_tree.
    let p_text = count_p_text(std::slice::from_ref(&best_tree));
    // Trigger JSON-LD recovery when pipeline produces little content:
    // either low total chars (<500) or no real <p> content (<250)
    if best_len < 500 || p_text < 250 {
        // Walk the original tree looking for JSON-LD script elements with articleBody
        let article_body = extract_jsonld_article_body(&original);
        if let Some(body) = article_body {
            let trimmed = body.trim();
            if trimmed.len() >= 100 {
                let existing_text = get_inner_text(&best_tree);
                if !existing_text.contains(trimmed) {
                    tracing::info!(
                        "jsonld recovery: adding articleBody ({} chars) to best_tree (current {} chars)",
                        trimmed.len(),
                        best_len
                    );
                    // Create a <p> element with the extracted text and add to best_tree
                    let p_node = DomNode::Element {
                        tag: "p".to_string(),
                        attrs: vec![],
                        children: vec![DomNode::Text(trimmed.to_string())],
                        scores: HashMap::new(),
                        metadata: HashMap::new(),
                    };
                    if let DomNode::Element { children, .. } = &mut best_tree {
                        children.push(p_node);
                    }
                    best_len = measure_output(&best_tree);
                    tracing::info!("jsonld recovery: best_tree now {} chars", best_len);
                } else {
                    tracing::debug!("jsonld recovery: articleBody already present in best_tree");
                }
            }
        } else {
            tracing::debug!("jsonld recovery: no articleBody found");
        }
    }

    *node = best_tree;
}
