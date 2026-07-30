use once_cell::sync::Lazy;
use std::collections::HashMap;

use crate::pipelines::{DomNode, WalkerAction, walk_post_mut, walk_pre_mut};

use super::passes::tf_filters::{
    count_p_text, tf_discard_image_elements, tf_extract_script_templates,
    tf_fallback_content_container, tf_filter_by_link_density, tf_filter_tag_catalog,
    tf_isolate_content_container, tf_precision_discard, tf_protect_content_forms,
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

/// Apply tf_filter_by_link_density with backup/restore safety mechanism.
///
/// Measures text content before/after link density filtering. If >=95% of text
/// was removed (i.e., new_len * 19 <= old_len), restores from backup copy.
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

/// Apply container isolation with recovery mechanism.
///
/// Measures text content before/after container isolation. If >=95% of text
/// was removed (i.e., new_len * 19 <= old_len), recovers `<p>` elements from
/// the backup tree instead of restoring the full tree with boilerplate.
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
        let existing_text = collect_text_from_node(node);
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

/// Collect all text content from a DOM node tree.
fn collect_text_from_node(node: &DomNode) -> String {
    match node {
        DomNode::Text(t) => t.clone(),
        DomNode::Element { children, .. } => {
            let mut result = String::new();
            for child in children {
                result.push_str(&collect_text_from_node(child));
            }
            result
        }
        _ => String::new(),
    }
}

/// Recover `<p>` elements from a backup tree that aren't already in the current node.
/// This is a simplified version of Python's recover_wild_text().
fn recover_wild_p_elements(node: &mut DomNode, backup: &DomNode, existing_text: &str) {
    // Collect all <p> element text from the backup tree
    let mut recovered: Vec<DomNode> = Vec::new();
    collect_p_elements(backup, &mut recovered);

    // Add recovered <p> elements that aren't already in the current tree
    if let DomNode::Element { children, .. } = node {
        for p_node in recovered {
            let p_text = collect_text_from_node(&p_node);
            if !p_text.trim().is_empty() && !existing_text.contains(&p_text) {
                children.push(p_node);
            }
        }
    }
}

/// Collect all `<p>` element subtrees from a DOM tree,
/// skipping `<p>` elements inside boilerplate containers.
fn collect_p_elements(node: &DomNode, result: &mut Vec<DomNode>) {
    match node {
        // Skip boilerplate containers entirely
        DomNode::Element { tag, .. }
            if matches!(tag.as_str(), "nav" | "footer" | "header" | "form") =>
        {
            // Don't descend into boilerplate containers
        }
        DomNode::Element { tag, children, .. } if tag == "p" => {
            result.push(node.clone());
        }
        DomNode::Element { children, .. } => {
            for child in children {
                collect_p_elements(child, result);
            }
        }
        _ => {}
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
        // RC-10: Final tag whitelist — remove any tags not in TAG_CATALOG
        apply_tf_filter_tag_catalog,
    ]
});

/// Level: Recall — same as Balanced but WITHOUT `tf_remove_empty_cut`.
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

/// Count non-whitespace text characters in a DOM tree recursively.
/// This gives a better estimate of actual useful content than raw text length,
/// which includes whitespace from HTML formatting (newlines, indentation).
fn count_non_ws_chars(node: &DomNode) -> usize {
    match node {
        DomNode::Text(t) => t.chars().filter(|c| !c.is_whitespace()).count(),
        DomNode::Element { children, .. } => children.iter().map(count_non_ws_chars).sum(),
        _ => 0,
    }
}


/// Extract `articleBody` from JSON-LD scripts in the DOM tree.
/// Returns `None` if no JSON-LD script with `articleBody` is found.
fn extract_jsonld_article_body(node: &DomNode) -> Option<String> {
    match node {
        DomNode::Text(_) => None,
        DomNode::Element { tag, attrs, children, .. } if tag == "script" => {
            // Check if type attribute is exactly "application/ld+json"
            let is_jsonld = attrs.iter().any(|(k, v)| {
                k.eq_ignore_ascii_case("type")
                    && v == "application/ld+json"
            });
            if is_jsonld {
                let text = super::passes::tf_filters::collect_text(children);
                if text.contains("articleBody") {
                    if let Ok(val) = serde_json::from_str::<serde_json::Value>(&text) {
                        if let Some(body) = val.get("articleBody").and_then(|v| v.as_str()) {
                            let trimmed = body.trim();
                            if trimmed.len() >= 100 {
                                // If articleBody contains <p> tags, parse as HTML and extract text
                                let text = if trimmed.contains("<p>") {
                                    let fragment = scraper::Html::parse_fragment(trimmed);
                                    fragment.root_element().text().collect::<String>().trim().to_string()
                                } else {
                                    trimmed.to_string()
                                };
                                return Some(text);
                            }
                        }
                    }
                }
            }
            // Not a JSON-LD script — recurse into children
            for child in children {
                if let Some(body) = extract_jsonld_article_body(child) {
                    return Some(body);
                }
            }
            None
        }
        DomNode::Element { children, .. } => {
            for child in children {
                if let Some(body) = extract_jsonld_article_body(child) {
                    return Some(body);
                }
            }
            None
        }
        _ => None,
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

    // If best output is below threshold, try recovering <p> elements
    // Uses a cleaned copy with boilerplate containers removed first.
    if best_len < 500 || (best_len < 2200 && count_non_ws_chars(&best_tree) < 250) {
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
        let existing_text = collect_text_from_node(&best_tree);
        // Add recovered <p> elements that aren't already in best_tree
        if let DomNode::Element { children, .. } = &mut best_tree {
            for p_node in &recovered_ps {
                let p_text = collect_text_from_node(p_node);
                if !p_text.trim().is_empty() && !existing_text.contains(&p_text) {
                    children.push(p_node.clone());
                }
            }
        }
        let recovered_len = measure_output(&best_tree);
        if recovered_len > best_len {
            tracing::info!(
                "recover_wild_text: {} -> {} chars (recovered {} p-elements)",
                best_len,
                recovered_len,
                recovered_ps.len()
            );
            best_len = recovered_len;
        } else {
            tracing::debug!("recover_wild_text: no improvement ({} chars)", best_len);
        }
    } else if best_len < 800 {
        // Recovery with paragraph length filter (>= 100 chars) to avoid boilerplate
        // This handles cases where pipeline output is reasonable but content was
        // removed by OVERALL_DISCARD_XPATH. Only recovers paragraphs with meaningful
        // text content to avoid adding boilerplate/sidebar snippets.
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
        let existing_text = collect_text_from_node(&best_tree);
        // Add recovered <p> elements that aren't already in best_tree
        // Only add paragraphs with >= 100 chars of meaningful text (filters boilerplate)
        if let DomNode::Element { children, .. } = &mut best_tree {
            for p_node in &recovered_ps {
                let p_text = collect_text_from_node(p_node);
                let trimmed = p_text.trim();
                if trimmed.len() >= 100 && !existing_text.contains(&p_text) {
                    children.push(p_node.clone());
                }
            }
        }
        let recovered_len = measure_output(&best_tree);
        if recovered_len > best_len {
            tracing::info!(
                "recover_wild_text (filtered): {} -> {} chars (recovered {} p-elements, >=100 char filter)",
                best_len,
                recovered_len,
                recovered_ps.len()
            );
            best_len = recovered_len;
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
                let existing_text = collect_text_from_node(&best_tree);
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

