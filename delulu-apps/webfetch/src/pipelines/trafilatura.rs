use once_cell::sync::Lazy;
use std::collections::HashMap;
use std::collections::HashSet;

use crate::pipelines::walkers::PassFn;
use crate::pipelines::{DomNode, WalkerAction, walk_post_mut, walk_pre_mut};

use super::passes::tf_analysis::{
    count_non_ws_chars,
    extract_jsonld_article_body,
};
use super::passes::tf_filters::{
    collect_p_elements,
    tf_extract_script_templates, tf_fallback_content_container, tf_filter_by_link_density,
    tf_filter_tag_catalog, tf_isolate_content_container, tf_protect_content_forms,
    tf_remove_cleaned, tf_remove_empty_cut, tf_remove_teaser,
    tf_strip_unwrapped,
};
#[cfg(not(feature = "use-xpath"))]
use super::passes::tf_filters::{
    tf_discard_image_elements,
    tf_remove_unlikely_candidates,
};
#[cfg(feature = "use-xpath")]
use super::passes::tf_filters::{
    tf_discard_image_elements_xpath,
    tf_isolate_content_container_xpath,
    tf_remove_teaser_xpath,
    tf_remove_unlikely_candidates_xpath,
};
use super::passes::tf_transforms::{
    tf_canonicalize_strip_non_content, tf_canonicalize_unwrap_containers,
};
use super::passes::tf_transforms::{
    tf_convert_breaks, tf_convert_formatting, tf_convert_headings, tf_convert_lists,
    tf_convert_quotes, tf_convert_refs_and_details,
};

// ---------------------------------------------------------------------------
// wrap_pass! and wrap_pass_void! macros — Phase 0a
// ---------------------------------------------------------------------------

/// Wrap a `WalkerAction`-returning pass for use in a pipeline array.
///
/// Expands to `(|node| walk_pre_mut(node, &|n| $f(n))) as fn(&mut DomNode)`.
/// Eliminates boilerplate closure cast syntax.
///
/// Pre: `$f` is a function `fn(&mut DomNode) -> WalkerAction`.
/// Post: Returns a `fn(&mut DomNode)` that walks the tree in pre-order applying `$f`.
///
/// Note: No direct trafilatura equivalent — Rust-specific macro.
#[macro_export]
macro_rules! wrap_pass {
    ($f:expr) => {
        (|node| walk_pre_mut(node, &|n| $f(n))) as fn(&mut DomNode)
    };
}

/// Wrap a void-returning pass for use in a pipeline array.
///
/// Expands to `(|node| walk_pre_mut(node, &|n| { $f(n); WalkerAction::Continue })) as fn(&mut DomNode)`.
///
/// Pre: `$f` is a function `fn(&mut DomNode)` (no return value).
/// Post: Returns a `fn(&mut DomNode)` that walks the tree in pre-order applying `$f`.
#[macro_export]
macro_rules! wrap_pass_void {
    ($f:expr) => {
        (|node| walk_pre_mut(node, &|n| { $f(n); WalkerAction::Continue })) as fn(&mut DomNode)
    };
}

// ---------------------------------------------------------------------------
// with_backup — generic backup/restore helper — Phase 0a
// ---------------------------------------------------------------------------

/// Generic backup/restore wrapper for destructive passes.
///
/// Clones the node before applying `pass`, then checks if `new_len * threshold <= old_len`.
/// If true (too much text removed), applies `recovery` to restore from backup.
///
/// Uses `checked_mul()` to prevent integer overflow. On overflow, keeps the
/// modified tree (conservative: "not enough removed").
///
/// Pre: `node` is a valid DOM tree. `pass` is a destructive pass that may remove content.
///      `threshold` is the backup trigger multiplier (e.g., 5 for >=80% removal).
///      `recovery` is a function that takes `(node, backup)` and restores content.
/// Post: If too much text was removed, `node` is restored via `recovery`.
///       Otherwise, `pass`'s effects remain.
///
/// Reference: Trafilatura `main_extractor.py` line 710: `tree = prune_unwanted_nodes(tree, OVERALL_DISCARD_XPATH, with_backup=True)`
pub fn with_backup<F, R>(node: &mut DomNode, pass: F, threshold: usize, recovery: R)
where
    F: Fn(&mut DomNode),
    R: Fn(&mut DomNode, &DomNode),
{
    let old_len = node.text_len();
    let backup = node.clone();

    pass(node);

    let new_len = node.text_len();

    // Use checked_mul to prevent integer overflow on large documents
    match new_len.checked_mul(threshold) {
        Some(product) if product <= old_len => {
            tracing::warn!(
                "backup triggered ({} -> {} chars, threshold={}), restoring",
                old_len,
                new_len,
                threshold,
            );
            recovery(node, &backup);
        }
        Some(_) => {
            tracing::debug!(
                "pass safe: {} -> {} chars (threshold={})",
                old_len,
                new_len,
                threshold,
            );
        }
        None => {
            // Overflow: keep modified tree (conservative)
            tracing::warn!(
                "checked_mul overflow in with_backup: {} * {} overflowed, keeping modified tree",
                new_len,
                threshold,
            );
        }
    }
}

/// Macro: generate a backup/restore wrapper for a destructive pass.
/// Recovery does full restore + re-applies tf_remove_cleaned.
macro_rules! with_backup_wrapper {
    ($name:ident, $pass:expr, $threshold:expr) => {
        pub fn $name(node: &mut DomNode) {
            with_backup(node, $pass, $threshold, |node, backup| {
                *node = backup.clone();
                walk_pre_mut(node, &|n| tf_remove_cleaned(n));
            });
        }
    };
}


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
/// Reference: Trafilatura `main_extractor.py` line 710:
///   `tree = prune_unwanted_nodes(tree, OVERALL_DISCARD_XPATH, with_backup=True)`
#[cfg(not(feature = "use-xpath"))]
with_backup_wrapper!(
    apply_tf_remove_unlikely_candidates_with_backup,
    |n| walk_pre_mut(n, &|n| tf_remove_unlikely_candidates(n)),
    5
);

/// Filter by link density with backup/restore safety net.
///
/// Pre: `node` is a valid DOM tree. Unlikely candidates have been removed.
/// Post: Elements with link density >50% are removed. If >=95% of text is removed (threshold: 19×), the node is restored to the backup state. Uses `*node = backup` (full restore).
///
/// Reference: Trafilatura `main_extractor.py` line 710 pattern.
#[cfg(not(feature = "use-xpath"))]
with_backup_wrapper!(
    apply_tf_filter_by_link_density_with_backup,
    |n| walk_pre_mut(n, &|n| tf_filter_by_link_density(n)),
    19
);

/// Isolate content container with backup/recovery safety net.
///
/// Pre: `node` is a valid DOM tree. Link density filtering has been applied.
/// Post: Content container is isolated via BODY_XPATH cascade. If >=90% of text is removed (threshold: 10×), wild `<p>` elements are recovered from the backup (not full restore).
///
/// Reference: Trafilatura `main_extractor.py` line 710 pattern.
pub fn apply_tf_isolate_container_with_backup(node: &mut DomNode) {
    use super::passes::tf_filters::recover_wild_p_elements;
    with_backup(
        node,
        |n| {
            tf_isolate_content_container(n);
            tf_fallback_content_container(n);
        },
        10,
        |node, backup| {
            let existing_text = node.text_content();
            recover_wild_p_elements(node, backup, &existing_text);
        },
    );
}

/// Remove unlikely candidates with backup/restore safety net (XPath version).
///
/// Pre: `node` is a valid DOM tree. The tree has been parsed and basic cleaning applied.
/// Post: Elements matching OVERALL_DISCARD_XPATH patterns are removed. If >=80% of text is removed (threshold: 5×), the node is restored to the backup state.
#[cfg(feature = "use-xpath")]
with_backup_wrapper!(
    apply_tf_remove_unlikely_candidates_xpath_with_backup,
    |n| walk_pre_mut(n, &|n| tf_remove_unlikely_candidates_xpath(n)),
    5
);

/// Filter by link density with backup/restore safety net (XPath version).
///
/// Pre: `node` is a valid DOM tree. Unlikely candidates have been removed.
/// Post: Elements with link density >50% are removed. If >=95% of text is removed (threshold: 19×), the node is restored to the backup state.
#[cfg(feature = "use-xpath")]
with_backup_wrapper!(
    apply_tf_filter_by_link_density_xpath_with_backup,
    |n| walk_pre_mut(n, &|n| tf_filter_by_link_density(n)),
    19
);

/// Isolate content container with backup/recovery safety net (XPath version).
///
/// Pre: `node` is a valid DOM tree. Link density filtering has been applied.
/// Post: Content container is isolated via XPath BODY_XPATH cascade. If >=90% of text is removed (threshold: 10×), wild `<p>` elements are recovered from the backup.
#[cfg(feature = "use-xpath")]
pub fn apply_tf_isolate_container_xpath_with_backup(node: &mut DomNode) {
    use super::passes::tf_filters::recover_wild_p_elements;
    with_backup(
        node,
        |n| {
            tf_isolate_content_container_xpath(n);
            tf_fallback_content_container(n);
        },
        10,
        |node, backup| {
            let existing_text = node.text_content();
            recover_wild_p_elements(node, backup, &existing_text);
        },
    );
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
        wrap_pass!(tf_remove_cleaned),
        #[cfg(not(feature = "use-xpath"))]
        wrap_pass!(tf_remove_teaser),
        #[cfg(feature = "use-xpath")]
        wrap_pass!(tf_remove_teaser_xpath),
        #[cfg(not(feature = "use-xpath"))]
        apply_tf_remove_unlikely_candidates_with_backup,
        #[cfg(feature = "use-xpath")]
        apply_tf_remove_unlikely_candidates_xpath_with_backup,
        tf_strip_unwrapped,
        wrap_pass!(tf_remove_empty_cut),
        #[cfg(not(feature = "use-xpath"))]
        apply_tf_filter_by_link_density_with_backup,
        #[cfg(feature = "use-xpath")]
        apply_tf_filter_by_link_density_xpath_with_backup,
        wrap_pass!(tf_convert_headings),
        wrap_pass!(tf_convert_lists),
        wrap_pass!(tf_convert_quotes),
        wrap_pass!(tf_convert_formatting),
        wrap_pass!(tf_convert_breaks),
        wrap_pass!(tf_convert_refs_and_details),
        tf_canonicalize_strip_non_content,
        #[cfg(not(feature = "use-xpath"))]
        apply_tf_isolate_container_with_backup,
        #[cfg(feature = "use-xpath")]
        apply_tf_isolate_container_xpath_with_backup,
        #[cfg(not(feature = "use-xpath"))]
        tf_fallback_content_container,
        #[cfg(feature = "use-xpath")]
        tf_fallback_content_container,
        #[cfg(not(feature = "use-xpath"))]
        wrap_pass!(tf_discard_image_elements),
        #[cfg(feature = "use-xpath")]
        wrap_pass!(tf_discard_image_elements_xpath),
        tf_canonicalize_unwrap_containers,
        // Final tag whitelist — remove any tags not in TAG_CATALOG
        apply_tf_filter_tag_catalog,
    ]
});

/// Level: Recall — same as Balanced but WITHOUT `tf_remove_empty_cut` and WITH `apply_tf_filter_tag_catalog`.
///
/// Pre: `node` is a valid DOM tree. `rd_analysis::mark_data_tables_by_structure` has been called.
/// Post: All 16 passes are applied in order. `node` is mutated in-place. `tf_remove_empty_cut` is NOT applied, but `apply_tf_filter_tag_catalog` IS applied.
///
/// Less aggressive filtering. Use as fallback when Balanced produces too little output.
pub static TF_RECALL: Lazy<&[PassFn]> = Lazy::new(|| {
    &[
        tf_protect_content_forms,
        tf_extract_script_templates,
        wrap_pass!(tf_remove_cleaned),
        #[cfg(not(feature = "use-xpath"))]
        wrap_pass!(tf_remove_teaser),
        #[cfg(feature = "use-xpath")]
        wrap_pass!(tf_remove_teaser_xpath),
        #[cfg(not(feature = "use-xpath"))]
        apply_tf_remove_unlikely_candidates_with_backup,
        #[cfg(feature = "use-xpath")]
        apply_tf_remove_unlikely_candidates_xpath_with_backup,
        tf_strip_unwrapped,
        // tf_remove_empty_cut SKIPPED -- preserve all content
        #[cfg(not(feature = "use-xpath"))]
        apply_tf_filter_by_link_density_with_backup,
        #[cfg(feature = "use-xpath")]
        apply_tf_filter_by_link_density_xpath_with_backup,
        wrap_pass!(tf_convert_headings),
        wrap_pass!(tf_convert_lists),
        wrap_pass!(tf_convert_quotes),
        wrap_pass!(tf_convert_formatting),
        wrap_pass!(tf_convert_breaks),
        wrap_pass!(tf_convert_refs_and_details),
        tf_canonicalize_strip_non_content,
        #[cfg(not(feature = "use-xpath"))]
        apply_tf_isolate_container_with_backup,
        #[cfg(feature = "use-xpath")]
        apply_tf_isolate_container_xpath_with_backup,
        #[cfg(not(feature = "use-xpath"))]
        tf_fallback_content_container,
        #[cfg(feature = "use-xpath")]
        tf_fallback_content_container,
        #[cfg(not(feature = "use-xpath"))]
        wrap_pass!(tf_discard_image_elements),
        #[cfg(feature = "use-xpath")]
        wrap_pass!(tf_discard_image_elements_xpath),
        tf_canonicalize_unwrap_containers,
        // Final tag whitelist — remove any tags not in TAG_CATALOG
        apply_tf_filter_tag_catalog,
    ]
});

// ---------------------------------------------------------------------------
// Orchestrator
// ---------------------------------------------------------------------------

/// Minimum output length (in characters) for a successful tf_* extraction.
/// Uses the same constant as the readability pipeline for consistency.
pub const TF_MIN_OUTPUT_CHARS: usize = 1000;

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
    // Build a HashSet of existing paragraph texts for exact-match dedup
    let mut paragraph_set: HashSet<String> = HashSet::new();
    {
        let mut existing_ps: Vec<DomNode> = Vec::new();
        collect_p_elements(best_tree, &mut existing_ps);
        for p in &existing_ps {
            paragraph_set.insert(p.text_content());
        }
    }
    // Add recovered <p> elements that aren't already in best_tree
    let mut appended = 0usize;
    if let DomNode::Element { children, .. } = best_tree {
        for p_node in &recovered_ps {
            let p_text = p_node.text_content();
            let trimmed = p_text.trim();
            if trimmed.len() >= min_p_len && !trimmed.is_empty() && paragraph_set.insert(p_text.clone()) {
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

        let len = attempt.text_len();
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
        best_len = best_tree.text_len();
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
        best_len = best_tree.text_len();
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
    let p_text = best_tree.text_stats().0;
    // Trigger JSON-LD recovery when pipeline produces little content:
    // either low total chars (<500) or no real <p> content (<250)
    if best_len < 500 || p_text < 250 {
        // Walk the original tree looking for JSON-LD script elements with articleBody
        let article_body = extract_jsonld_article_body(&original);
        if let Some(body) = article_body {
            let trimmed = body.trim();
            if trimmed.len() >= 100 {
                let existing_text = best_tree.text_content();
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
                    best_len = best_tree.text_len();
                    tracing::info!("jsonld recovery: best_tree now {} chars", best_len);
                } else {
                    tracing::debug!("jsonld recovery: articleBody already present in best_tree");
                }
            }
        } else {
            tracing::debug!("jsonld recovery: no articleBody found");
        }
    }

    // Last-resort fallback: if all cascade levels (Balanced, Recall, recovery) produce
    // little to no content, return the original tree rather than an empty result.
    // This matches Python's behavior of returning whatever survived the pipeline.
    if best_len < 500 {
        tracing::warn!(
            "filter_trafilatura: all cascade levels produced <500 chars ({}), falling back to original tree",
            best_len,
        );
        *node = original;
    } else {
        *node = best_tree;
    }
}


#[cfg(test)]
#[path = "../../tests/unit/pipelines/trafilatura_test.rs"]
mod tests;
