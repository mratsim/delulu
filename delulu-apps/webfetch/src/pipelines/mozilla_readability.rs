use once_cell::sync::Lazy;

use crate::pipelines::{DomNode, PassFn, walk_pre_mut};

use super::passes::code_blocks::normalize_code_blocks;
use super::passes::rd_analysis::{
    rd_score_mozilla_readability, rd_score_mozilla_readability_no_class_weights,
};
use super::passes::rd_extraction::{
    pass_keep_alt_cluster,
    pass_keep_qualifying_siblings,
    // pass_promote_content_child,  // SLOP — does not exist in Readability.js. Keeps only 1 child at every level,
    //                             // recursively narrowing tree to a single leaf. Destroys text nodes.
    //                             // pass_keep_qualifying_siblings already handles sibling selection correctly.
    pass_prune_no_candidate,
    pass_splice_cutoff,
};
use super::passes::rd_filters::{
    clean_matched_nodes, clean_negative_headers, filter_low_density_elements, is_probably_visible,
    rd_strip_analytics, remove_empty_paragraphs, remove_empty_structural_elements,
    remove_garbage_interactive_elements, remove_script_elements, remove_style_elements,
    strip_unlikely_candidates,
};
use super::passes::rd_transforms::{
    clean_classes, clean_styles, collapse_single_child_elements,
    convert_div_containing_phrasing_to_paragraph, convert_double_br_to_paragraph,
    convert_font_to_span, fix_lazy_loaded_images, rd_strip_non_content,
    rd_unwrap_structural_wrappers, remove_br_before_paragraph, replace_h1_with_h2,
    strip_heading_edit_suffixes, unwrap_single_cell_tables, wrap_readability_output,
};

/// Level 1: Strict -- standard Mozilla Readability pipeline.
/// Includes all DOM normalization passes, scoring with class/ID weights,
/// and score-based filtering.
pub static READABILITY_LEVEL_1_STRICT: Lazy<&[PassFn]> = Lazy::new(|| {
    &[
        (|node| walk_pre_mut(node, &|n| remove_style_elements(n))) as fn(&mut DomNode),
        (|node| walk_pre_mut(node, &|n| rd_strip_analytics(n))) as fn(&mut DomNode),
        (|node| walk_pre_mut(node, &|n| remove_script_elements(n))) as fn(&mut DomNode),
        (|node| walk_pre_mut(node, &|n| convert_double_br_to_paragraph(n))) as fn(&mut DomNode),
        (|node| walk_pre_mut(node, &|n| convert_font_to_span(n))) as fn(&mut DomNode),
        (|node| walk_pre_mut(node, &|n| strip_unlikely_candidates(n))) as fn(&mut DomNode),
        (|node| walk_pre_mut(node, &|n| remove_empty_structural_elements(n))) as fn(&mut DomNode),
        (|node| walk_pre_mut(node, &|n| convert_div_containing_phrasing_to_paragraph(n)))
            as fn(&mut DomNode),
        (|node| walk_pre_mut(node, &|n| is_probably_visible(n))) as fn(&mut DomNode),
        rd_score_mozilla_readability,
        pass_prune_no_candidate,
        pass_splice_cutoff,
        pass_keep_alt_cluster,
        pass_keep_qualifying_siblings,
        // pass_promote_content_child,  // SLOP — see import comment
        (|node| walk_pre_mut(node, &|n| fix_lazy_loaded_images(n))) as fn(&mut DomNode),
        (|node| walk_pre_mut(node, &|n| replace_h1_with_h2(n))) as fn(&mut DomNode),
        (|node| walk_pre_mut(node, &|n| remove_br_before_paragraph(n))) as fn(&mut DomNode),
        (|node| walk_pre_mut(node, &|n| unwrap_single_cell_tables(n))) as fn(&mut DomNode),
        collapse_single_child_elements,
        filter_low_density_elements,
        (|node| walk_pre_mut(node, &|n| clean_styles(n))) as fn(&mut DomNode),
        (|node| walk_pre_mut(node, &|n| clean_classes(n))) as fn(&mut DomNode),
        (|node| walk_pre_mut(node, &|n| remove_garbage_interactive_elements(n)))
            as fn(&mut DomNode),
        (|node| walk_pre_mut(node, &|n| clean_negative_headers(n))) as fn(&mut DomNode),
        (|node| walk_pre_mut(node, &|n| remove_empty_paragraphs(n))) as fn(&mut DomNode),
        (|node| walk_pre_mut(node, &|n| clean_matched_nodes(n))) as fn(&mut DomNode),
        (|node| walk_pre_mut(node, &|n| strip_heading_edit_suffixes(n))) as fn(&mut DomNode),
        // Normalize code blocks (pre stays pre; language hoisted) so generators
        // see canonical <pre> blocks (markdown fences / HTML <pre>).
        (|node| walk_pre_mut(node, &|n| normalize_code_blocks(n))) as fn(&mut DomNode),
    ]
});

/// Level 2: Keep unlikely candidates -- skips `strip_unlikely_candidates`.
/// For pages where the main content resides inside an unlikely-candidate element.
pub static READABILITY_LEVEL_2_KEEP_UNLIKELY: Lazy<&[PassFn]> = Lazy::new(|| {
    &[
        (|node| walk_pre_mut(node, &|n| remove_style_elements(n))) as fn(&mut DomNode),
        (|node| walk_pre_mut(node, &|n| rd_strip_analytics(n))) as fn(&mut DomNode),
        (|node| walk_pre_mut(node, &|n| remove_script_elements(n))) as fn(&mut DomNode),
        (|node| walk_pre_mut(node, &|n| convert_double_br_to_paragraph(n))) as fn(&mut DomNode),
        (|node| walk_pre_mut(node, &|n| convert_font_to_span(n))) as fn(&mut DomNode),
        // strip_unlikely_candidates SKIPPED
        (|node| walk_pre_mut(node, &|n| remove_empty_structural_elements(n))) as fn(&mut DomNode),
        (|node| walk_pre_mut(node, &|n| convert_div_containing_phrasing_to_paragraph(n)))
            as fn(&mut DomNode),
        (|node| walk_pre_mut(node, &|n| is_probably_visible(n))) as fn(&mut DomNode),
        rd_score_mozilla_readability,
        pass_prune_no_candidate,
        pass_splice_cutoff,
        pass_keep_alt_cluster,
        pass_keep_qualifying_siblings,
        // pass_promote_content_child,  // SLOP — see import comment
        (|node| walk_pre_mut(node, &|n| fix_lazy_loaded_images(n))) as fn(&mut DomNode),
        (|node| walk_pre_mut(node, &|n| replace_h1_with_h2(n))) as fn(&mut DomNode),
        (|node| walk_pre_mut(node, &|n| remove_br_before_paragraph(n))) as fn(&mut DomNode),
        (|node| walk_pre_mut(node, &|n| unwrap_single_cell_tables(n))) as fn(&mut DomNode),
        collapse_single_child_elements,
        filter_low_density_elements,
        (|node| walk_pre_mut(node, &|n| clean_styles(n))) as fn(&mut DomNode),
        (|node| walk_pre_mut(node, &|n| clean_classes(n))) as fn(&mut DomNode),
        (|node| walk_pre_mut(node, &|n| remove_garbage_interactive_elements(n)))
            as fn(&mut DomNode),
        (|node| walk_pre_mut(node, &|n| clean_negative_headers(n))) as fn(&mut DomNode),
        (|node| walk_pre_mut(node, &|n| remove_empty_paragraphs(n))) as fn(&mut DomNode),
        (|node| walk_pre_mut(node, &|n| clean_matched_nodes(n))) as fn(&mut DomNode),
        (|node| walk_pre_mut(node, &|n| strip_heading_edit_suffixes(n))) as fn(&mut DomNode),
        // Normalize code blocks (pre stays pre; language hoisted) so generators
        // see canonical <pre> blocks (markdown fences / HTML <pre>).
        (|node| walk_pre_mut(node, &|n| normalize_code_blocks(n))) as fn(&mut DomNode),
    ]
});

/// Level 3: Ignore class/ID weights -- uses `compute_mozilla_readability_score_no_class_weights`
/// instead of the standard scorer. Only tag bonuses contribute to scores,
/// ignoring positive/negative candidate class/id patterns.
pub static READABILITY_LEVEL_3_IGNORE_CLASS_WEIGHTS: Lazy<&[PassFn]> = Lazy::new(|| {
    &[
        (|node| walk_pre_mut(node, &|n| remove_style_elements(n))) as fn(&mut DomNode),
        (|node| walk_pre_mut(node, &|n| rd_strip_analytics(n))) as fn(&mut DomNode),
        (|node| walk_pre_mut(node, &|n| remove_script_elements(n))) as fn(&mut DomNode),
        (|node| walk_pre_mut(node, &|n| convert_double_br_to_paragraph(n))) as fn(&mut DomNode),
        (|node| walk_pre_mut(node, &|n| convert_font_to_span(n))) as fn(&mut DomNode),
        (|node| walk_pre_mut(node, &|n| strip_unlikely_candidates(n))) as fn(&mut DomNode),
        (|node| walk_pre_mut(node, &|n| remove_empty_structural_elements(n))) as fn(&mut DomNode),
        (|node| walk_pre_mut(node, &|n| convert_div_containing_phrasing_to_paragraph(n)))
            as fn(&mut DomNode),
        (|node| walk_pre_mut(node, &|n| is_probably_visible(n))) as fn(&mut DomNode),
        rd_score_mozilla_readability_no_class_weights,
        pass_prune_no_candidate,
        pass_splice_cutoff,
        pass_keep_alt_cluster,
        pass_keep_qualifying_siblings,
        // pass_promote_content_child,  // SLOP — see import comment
        (|node| walk_pre_mut(node, &|n| fix_lazy_loaded_images(n))) as fn(&mut DomNode),
        (|node| walk_pre_mut(node, &|n| replace_h1_with_h2(n))) as fn(&mut DomNode),
        (|node| walk_pre_mut(node, &|n| remove_br_before_paragraph(n))) as fn(&mut DomNode),
        (|node| walk_pre_mut(node, &|n| unwrap_single_cell_tables(n))) as fn(&mut DomNode),
        collapse_single_child_elements,
        filter_low_density_elements,
        (|node| walk_pre_mut(node, &|n| clean_styles(n))) as fn(&mut DomNode),
        (|node| walk_pre_mut(node, &|n| clean_classes(n))) as fn(&mut DomNode),
        (|node| walk_pre_mut(node, &|n| remove_garbage_interactive_elements(n)))
            as fn(&mut DomNode),
        (|node| walk_pre_mut(node, &|n| clean_negative_headers(n))) as fn(&mut DomNode),
        (|node| walk_pre_mut(node, &|n| remove_empty_paragraphs(n))) as fn(&mut DomNode),
        (|node| walk_pre_mut(node, &|n| clean_matched_nodes(n))) as fn(&mut DomNode),
        (|node| walk_pre_mut(node, &|n| strip_heading_edit_suffixes(n))) as fn(&mut DomNode),
        // Normalize code blocks (pre stays pre; language hoisted) so generators
        // see canonical <pre> blocks (markdown fences / HTML <pre>).
        (|node| walk_pre_mut(node, &|n| normalize_code_blocks(n))) as fn(&mut DomNode),
        rd_strip_non_content,
        rd_unwrap_structural_wrappers,
    ]
});

/// Level 4: No score filter -- runs all normalization passes but includes 5 micropasses.
/// 5 micropasses active -- prunes zero-score nodes and splices thin wrappers even in Level 4.
pub static READABILITY_LEVEL_4_NO_SCORE_FILTER: Lazy<&[PassFn]> = Lazy::new(|| {
    &[
        (|node| walk_pre_mut(node, &|n| remove_style_elements(n))) as fn(&mut DomNode),
        (|node| walk_pre_mut(node, &|n| rd_strip_analytics(n))) as fn(&mut DomNode),
        (|node| walk_pre_mut(node, &|n| remove_script_elements(n))) as fn(&mut DomNode),
        (|node| walk_pre_mut(node, &|n| convert_double_br_to_paragraph(n))) as fn(&mut DomNode),
        (|node| walk_pre_mut(node, &|n| convert_font_to_span(n))) as fn(&mut DomNode),
        (|node| walk_pre_mut(node, &|n| strip_unlikely_candidates(n))) as fn(&mut DomNode),
        (|node| walk_pre_mut(node, &|n| remove_empty_structural_elements(n))) as fn(&mut DomNode),
        (|node| walk_pre_mut(node, &|n| convert_div_containing_phrasing_to_paragraph(n)))
            as fn(&mut DomNode),
        (|node| walk_pre_mut(node, &|n| is_probably_visible(n))) as fn(&mut DomNode),
        rd_score_mozilla_readability,
        // 5 micropasses active -- prunes zero-score nodes and splices thin wrappers even in Level 4
        pass_prune_no_candidate,
        pass_splice_cutoff,
        pass_keep_alt_cluster,
        pass_keep_qualifying_siblings,
        // pass_promote_content_child,  // SLOP — see import comment
        (|node| walk_pre_mut(node, &|n| fix_lazy_loaded_images(n))) as fn(&mut DomNode),
        (|node| walk_pre_mut(node, &|n| replace_h1_with_h2(n))) as fn(&mut DomNode),
        (|node| walk_pre_mut(node, &|n| remove_br_before_paragraph(n))) as fn(&mut DomNode),
        (|node| walk_pre_mut(node, &|n| unwrap_single_cell_tables(n))) as fn(&mut DomNode),
        collapse_single_child_elements,
        filter_low_density_elements,
        (|node| walk_pre_mut(node, &|n| clean_styles(n))) as fn(&mut DomNode),
        (|node| walk_pre_mut(node, &|n| clean_classes(n))) as fn(&mut DomNode),
        (|node| walk_pre_mut(node, &|n| remove_garbage_interactive_elements(n)))
            as fn(&mut DomNode),
        (|node| walk_pre_mut(node, &|n| clean_negative_headers(n))) as fn(&mut DomNode),
        (|node| walk_pre_mut(node, &|n| remove_empty_paragraphs(n))) as fn(&mut DomNode),
        (|node| walk_pre_mut(node, &|n| clean_matched_nodes(n))) as fn(&mut DomNode),
        (|node| walk_pre_mut(node, &|n| strip_heading_edit_suffixes(n))) as fn(&mut DomNode),
        // Normalize code blocks (pre stays pre; language hoisted) so generators
        // see canonical <pre> blocks (markdown fences / HTML <pre>).
        (|node| walk_pre_mut(node, &|n| normalize_code_blocks(n))) as fn(&mut DomNode),
        rd_strip_non_content,
        rd_unwrap_structural_wrappers,
    ]
});

// ---------------------------------------------------------------------------
// Retry Orchestrator: filter_mozilla_readability
// ---------------------------------------------------------------------------

/// Minimum output length (in Markdown characters) to consider extraction successful.
/// If readability output is below this threshold, the next retry level is tried.
pub const MIN_OUTPUT_CHARS: usize = 500;

/// Measure the rendered Markdown output length of a DOM tree.
///
/// Uses `MarkdownLowerer::lower` with `base_url=None` to measure content volume.
/// NOTE: `base_url=None` is acceptable because the comparison is relative across
/// retry levels -- the same bias applies to all. `MarkdownLowerer` caps output at
/// 500 KiB; for pages exceeding this cap, Level 1 almost certainly passes the
/// threshold anyway.
pub fn measure_output(node: &DomNode) -> usize {
    let md = crate::generators::gen_md::MarkdownLowerer::lower(node, None);
    md.len()
}

/// Set a string metadata attribute on a root element node.
///
/// Used to inject retry-level information (e.g., `md_retry_level`) so that
/// callers can determine which retry level was ultimately selected.
fn inject_metadata_flag(root: &mut DomNode, key: &str, value: &str) {
    if let DomNode::Element { metadata, .. } = root {
        metadata.insert(key.to_string(), value.to_string());
    }
}

/// Runs the readability pipeline with progressive relaxation until output
/// reaches MIN_OUTPUT_CHARS, or keeps the longest result across all levels.
///
/// Pre: node is a parsed DOM tree root.
/// Post:
///    - If output >= MIN_OUTPUT_CHARS at any level, that level's tree is used.
///    - If no level reaches the threshold, the longest output is kept and
///      a `tracing::warn!` is emitted; `md_retry_level` metadata is injected
///      on the root node indicating which level was selected.
pub fn filter_mozilla_readability(node: &mut DomNode) {
    // Mark data tables by structure (walk tree, set is_data_table metadata)
    crate::pipelines::passes::rd_analysis::mark_data_tables_by_structure(node);
    // Contextual: mark tables inside <figure> as data tables
    crate::pipelines::passes::rd_analysis::mark_data_tables_inside_figures(node);

    // TODO: Add fuzzing guard for large DOM trees
    let levels: &[&[PassFn]] = &[
        *READABILITY_LEVEL_1_STRICT,
        *READABILITY_LEVEL_2_KEEP_UNLIKELY,
        *READABILITY_LEVEL_3_IGNORE_CLASS_WEIGHTS,
        *READABILITY_LEVEL_4_NO_SCORE_FILTER,
    ];

    let original = node.clone();

    // baseline is 0: raw unprocessed tree is NOT eligible as output.
    let mut best_tree = original.clone();
    let mut best_len = 0usize;
    let mut best_level: usize = 0;

    for (i, level) in levels.iter().enumerate() {
        let mut attempt = original.clone();
        for pass in *level {
            pass(&mut attempt);
        }

        let len = measure_output(&attempt);
        // TODO side-effect to push to main: tracing::* logging in lib
        tracing::debug!(
            "filter_mozilla_readability: level {} produced {} chars",
            i + 1,
            len
        );

        if len >= MIN_OUTPUT_CHARS {
            *node = attempt;
            wrap_readability_output(node);
            return; // Early return — first level that meets threshold wins.
        }

        if len > best_len {
            best_tree = attempt;
            best_len = len;
            best_level = i + 1;
        }
    }

    // Fallback: none reached threshold -- keep longest.
    tracing::warn!(
        "filter_mozilla_readability: no level reached {} chars, keeping level {} ({} chars)",
        MIN_OUTPUT_CHARS,
        best_level,
        best_len,
    );
    *node = best_tree;

    // Inject metadata flag so callers can distinguish fallback from clean extraction.
    inject_metadata_flag(node, "md_retry_level", &best_level.to_string());

    // Wrap output in the readability page div.
    wrap_readability_output(node);
}

#[cfg(test)]
#[path = "../../tests/unit/pipelines/mozilla_readability_test.rs"]
mod tests;
