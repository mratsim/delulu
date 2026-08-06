use crate::pipelines::DomNode;
use crate::pipelines::passes::rd_filters::{CONTENT_CANDIDATE_RE, is_data_table};
use crate::pipelines::passes::rd_utils::{get_inner_text, is_body_or_html, meta_get_f64};
use crate::pipelines::walkers::walk_post_mut;
use crate::pipelines::walkers::{WalkerAction, walk_post_acc_mut, walk_pre_mut};

// ---------------------------------------------------------------------------
// Sibling qualification constants
// ---------------------------------------------------------------------------

/// Minimum sibling floor value (matches JS: `Math.max(10, ...)`).
const SIBLING_FLOOR_MIN: f64 = 10.0;

/// Ratio of candidate_score used for sibling qualification (matches JS `contentScore * 0.2`).
const SIBLING_FLOOR_RATIO: f64 = 0.2;

const CUTOFF_SCORE_THRESHOLD: f64 = 20.0;

// ---------------------------------------------------------------------------
// ---------------------------------------------------------------------------
// pass_prune_no_candidate — remove nodes with zero/missing scores
// ---------------------------------------------------------------------------

/// Remove nodes where md_rd_subtree_acc_score is 0.0 or missing.
///
/// Uses walk_pre_mut with WalkerAction::Remove.
///
/// # Pre
/// - Scoring has run — every Element node has `md_rd_subtree_acc_score`.
/// # Post
/// - No Element node with score 0.0 or missing remains. Non-Element nodes pass through.
/// # Panics
/// Panics if `md_rd_subtree_acc_score` is missing (pipeline ordering bug) or
/// unparsable (scoring bug).
/// Tags whose subtree scores are meaningful for content detection.
/// Elements NOT in this set may have score 0.0 even when they contain
/// content, because they have no paragraph-scored descendants.
/// Matches JS Readability's paragraph-scored tags.
/// Cross-ref: rd_analysis.rs:112 (add_self) — must stay in sync.
const TAGS_THAT_GET_SCORED: &[&str] = &["p", "td", "pre", "blockquote"];

pub fn pass_prune_no_candidate(node: &mut DomNode) {
    walk_pre_mut(node, &|n| {
        match n {
            DomNode::Element { tag, metadata, .. } => {
                // Data tables: skip children to protect table content from being pruned.
                // mark_data_tables_by_structure() runs before this pass, so is_data_table
                // metadata is already set on qualifying <table> elements.
                if metadata.get("is_data_table").is_some_and(|v| v == "true") {
                    return WalkerAction::SkipChildren;
                }

                // Two-tier check: distinguish missing (pipeline bug) from zero (legitimate).
                // Missing key → pipeline ordering bug (scoring must run before extraction).
                // Zero score → legitimate "no content" state (not an error).
                // Unparseable/NaN/Inf → scoring bug (meta_parse_f64 filters these out).
                let score = match metadata.get("md_rd_subtree_acc_score") {
                    None => {
                        // Missing key: pipeline ordering bug — crash-loudly
                        panic!(
                            "pass_prune_no_candidate: node missing md_rd_subtree_acc_score — scoring must run before extraction"
                        );
                    }
                    Some(raw) => match crate::pipelines::passes::rd_utils::meta_parse_f64(raw) {
                        Some(s) => s, // meta_parse_f64 already guarantees finite, non-NaN
                        None => {
                            // Present but invalid: scoring bug — crash-loudly
                            panic!(
                                "pass_prune_no_candidate: unparsable md_rd_subtree_acc_score: {:?} — scoring bug",
                                raw
                            );
                        }
                    },
                };
                // Only prune scored tags (p, td, pre, blockquote). Non-scored tags
                // (span, a, div, article, etc.) have score 0.0 because they have no
                // paragraph-scored descendants — their zero score is not "no content".
                if score == 0.0 && TAGS_THAT_GET_SCORED.contains(&tag.as_str()) {
                    return WalkerAction::Remove;
                }
                WalkerAction::Continue
            }
            _ => WalkerAction::Continue,
        }
    });
}

// ---------------------------------------------------------------------------
// pass_splice_cutoff — splice thin wrappers with low score
// ---------------------------------------------------------------------------

/// Splice thin wrappers: if a node's score < best child's score / 3.0,
/// replace the node with its children (splice them into the parent Vec).
///
/// Uses walk_post_mut with WalkerAction::ReplaceWithChildren.
/// Body/html nodes are never spliced.
///
/// # Pre
/// Scoring has run.
/// # Post
/// Thin wrappers (score < best_child_score / 3.0) are replaced with their children.
///       Body/html nodes are never spliced.
/// # Panics
/// Panics if `md_rd_subtree_acc_score` is missing (pipeline ordering bug) or
/// unparsable (scoring bug), or if Element children exist but none have valid scores.
pub fn pass_splice_cutoff(node: &mut DomNode) {
    let mut cutoff_filter = |n: &mut DomNode| -> WalkerAction {
        let DomNode::Element {
            children,
            metadata,
            tag,
            ..
        } = n
        else {
            return WalkerAction::Continue;
        };
        if is_body_or_html(tag) {
            return WalkerAction::Continue;
        }
        // Data tables: never splice — their structure is meaningful.
        if metadata.get("is_data_table").is_some_and(|v| v == "true") {
            return WalkerAction::Continue;
        }
        // Read this node's score:
        // - Missing metadata → panic (pipeline ordering bug)
        // - Present but unparsable → panic (scoring bug)
        // - Present and valid → use it
        let my_score = match metadata.get("md_rd_subtree_acc_score") {
            None => {
                panic!("pass_splice_cutoff: node '{}' missing md_rd_subtree_acc_score — pipeline ordering bug", tag);
            }
            Some(raw) => crate::pipelines::passes::rd_utils::meta_parse_f64(raw).unwrap_or_else(|| {
                panic!("pass_splice_cutoff: node '{}' has unparsable md_rd_subtree_acc_score: {:?} — scoring bug", tag, raw);
            }),
        };
        // Find best child's score from children's metadata
        // Check if there are any Element children first — having none is legitimate
        // (e.g., a <p> with only text content). Only panic if Element children exist but
        // none have valid scores (pipeline ordering bug: scoring must run before this pass).
        let has_element_children = children
            .iter()
            .any(|c| matches!(c, DomNode::Element { .. }));
        let best_child_score = if has_element_children {
            children
                .iter()
                .filter_map(|c| match c {
                    DomNode::Element { metadata, .. } => {
                        meta_get_f64(metadata, "md_rd_subtree_acc_score")
                    }
                    _ => None,
                })
                .max_by(f64::total_cmp)
                .unwrap_or_else(|| {
                    panic!(
                        "pass_splice_cutoff: no child with valid score — pipeline ordering bug?"
                    );
                })
        } else {
            0.0
        };
        // Cutoff check: my_score < best_child_score / 3.0
        // CUTOFF_SCORE_THRESHOLD (20.0) implicitly guards against best_child_score == 0.0.
        if best_child_score >= CUTOFF_SCORE_THRESHOLD && my_score < best_child_score / 3.0 {
            // Don't splice if any direct child is a data table — would eject the table
            // from its container, causing it to be removed by sibling qualification.
            let has_data_table_child = children.iter().any(|c| match c {
                DomNode::Element { metadata, .. } => {
                    metadata.get("is_data_table").is_some_and(|v| v == "true")
                }
                _ => false,
            });
            if !has_data_table_child {
                return WalkerAction::ReplaceWithChildren;
            }
        }
        WalkerAction::Continue
    };
    // walk_post_mut expects &mut [&mut dyn FnMut(...)].
    // Using vec! matching existing call sites (type annotation doesn't compile for unsized trait).
    let mut filters: Vec<&mut crate::pipelines::walkers::WalkerFilter> = vec![&mut cutoff_filter];
    // Use is_data_table as should_descend guard to protect data table subtrees.
    // Data tables are structural — splicing them would destroy their content.
    walk_post_mut(node, &mut filters, Some(is_data_table));
}

// ---------------------------------------------------------------------------
// pass_keep_alt_cluster — keep alt clusters with 3+ qualifying children
// ---------------------------------------------------------------------------

/// Detect alt clusters: if a parent has 3+ children with score >= alt_threshold,
/// keep all qualifying children and remove non-qualifying ones.
///
/// Uses walk_post_acc_mut<()> (unit accumulator — scores read from metadata).
/// The () accumulator is required by the walker API; values are never consumed.
///
/// Pre: Scoring has run. Pruning and cutoff passes have run.
/// Post: If 3+ children qualify (score >= alt_threshold), non-qualifying children are removed.
///       Body/html children are excluded from the qualifying count.
/// # Panics
/// Panics if `md_rd_subtree_acc_score` is missing (pipeline ordering bug) or
/// invalid (NaN/Inf — scoring bug).
pub fn pass_keep_alt_cluster(node: &mut DomNode) {
    let DomNode::Element { children, .. } = node else {
        return;
    };
    walk_post_acc_mut::<()>(children, Some(is_data_table), &mut |n: &mut DomNode,
                                                                 _child_accs: &[(
    )]| {
        let DomNode::Element {
            children: my_children,
            metadata,
            tag,
            ..
        } = n
        else {
            return (WalkerAction::Continue, ());
        };
        // Read node's score — missing or invalid is a pipeline/scoring bug
        let my_score = meta_get_f64(metadata, "md_rd_subtree_acc_score")
            .unwrap_or_else(|| {
                panic!("pass_keep_alt_cluster: node missing md_rd_subtree_acc_score — pipeline ordering bug");
            });
        // meta_get_f64 already filters NaN/Inf, but guard defensively
        assert!(
            !my_score.is_nan() && !my_score.is_infinite(),
            "pass_keep_alt_cluster: invalid score {} — scoring bug",
            my_score
        );
        if my_score == 0.0 {
            return (WalkerAction::Continue, ());
        }
        // Find best non-body/html child score for alt_threshold
        let top_child_score = my_children
            .iter()
            .filter_map(|c| match c {
                DomNode::Element { tag, metadata, .. } if !is_body_or_html(tag) => {
                    meta_get_f64(metadata, "md_rd_subtree_acc_score")
                }
                _ => None,
            })
            .filter(|s| *s > 0.0)
            .max_by(f64::total_cmp);
        // Use Option<f64> instead of f64::MAX sentinel (see INV-014)
        // Add epsilon for floating-point tolerance (see FC-MED-008)
        let alt_threshold = top_child_score.map(|s| s * 0.75 - 1e-9);
        // Count qualifying children (score >= alt_threshold, exclude body/html)
        // Using Vec::retain for O(n) removal (see FC-MED-001)
        let is_alt_cluster = {
            let qualifying_count = my_children
                .iter()
                .filter(|c| match c {
                    DomNode::Element { tag, metadata, .. } if !is_body_or_html(tag) => {
                        alt_threshold.is_some_and(|threshold| {
                            meta_get_f64(metadata, "md_rd_subtree_acc_score")
                                .is_some_and(|s| s >= threshold)
                        })
                    }
                    _ => false,
                })
                .count();
            qualifying_count >= 3 && !is_body_or_html(tag)
        };
        if is_alt_cluster {
            // Use retain for O(n) removal instead of O(n²) remove(i)
            my_children.retain(|c| match c {
                DomNode::Element {
                    tag,
                    metadata,
                    attrs,
                    ..
                } if !is_body_or_html(tag) => {
                    // Data tables: always preserve — their structure is meaningful.
                    if metadata.get("is_data_table").is_some_and(|v| v == "true") {
                        return true;
                    }
                    // Content-candidate check: preserve elements with content-indicating class/id
                    // (e.g., "MathJax", "content", "article"). Mirrors should_keep_sibling.
                    let class_val = attrs
                        .iter()
                        .find(|(k, _)| k == "class")
                        .map(|(_, v)| v.as_str())
                        .unwrap_or("");
                    let id_val = attrs
                        .iter()
                        .find(|(k, _)| k == "id")
                        .map(|(_, v)| v.as_str())
                        .unwrap_or("");
                    if CONTENT_CANDIDATE_RE.is_match(class_val)
                        || CONTENT_CANDIDATE_RE.is_match(id_val)
                    {
                        return true;
                    }
                    alt_threshold.is_some_and(|threshold| {
                        meta_get_f64(metadata, "md_rd_subtree_acc_score")
                            .is_some_and(|s| s >= threshold)
                    })
                }
                _ => true, // keep non-Element children and body/html
            });
        }
        (WalkerAction::Continue, ())
    });
}

// ---------------------------------------------------------------------------
// pass_keep_qualifying_siblings — keep qualifying siblings of best child
// ---------------------------------------------------------------------------

/// Keep qualifying siblings of the best child. At each parent level, identify
/// the best non-body/html child by score, then keep siblings that meet the
/// sibling_floor threshold or pass the p-sibling heuristic.
///
/// Uses walk_post_acc_mut<()> (unit accumulator — scores read from metadata).
///
/// Pre: Scoring has run. All prior micropasses have run.
///      Root node has `md_rd_subtree_max_score` (set by scoring pass).
/// Post: Siblings of the best child that meet the sibling_floor threshold or pass the
///       p-sibling heuristic are kept. All other siblings are removed.
/// # Panics
/// Panics if `md_rd_subtree_max_score` is missing on root (pipeline ordering bug),
/// or if `md_rd_subtree_acc_score` is missing (pipeline ordering bug) or
/// invalid (NaN/Inf — scoring bug).
///
/// NOTE: Do NOT cache global_max in a static variable — each pass must be stateless.
pub fn pass_keep_qualifying_siblings(node: &mut DomNode) {
    let DomNode::Element {
        children, metadata, ..
    } = node
    else {
        return;
    };
    // Read global_max from root metadata (set by scoring pass).
    // This is O(1) — the scoring pass already computed md_rd_subtree_max_score.
    // Do NOT walk the tree to find max (see INV-019).
    // global_max is now only used as a defensive sanity check.
    let global_max = meta_get_f64(metadata, "md_rd_subtree_max_score")
        .unwrap_or_else(|| {
            panic!(
                "pass_keep_qualifying_siblings: md_rd_subtree_max_score missing on root — scoring must run before extraction"
            );
        });

    walk_post_acc_mut::<()>(children, Some(is_data_table), &mut |n: &mut DomNode,
                                                                 _child_accs: &[(
    )]| {
        let DomNode::Element {
            children: my_children,
            metadata,
            ..
        } = n
        else {
            return (WalkerAction::Continue, ());
        };
        let my_score = meta_get_f64(metadata, "md_rd_subtree_acc_score")
            .unwrap_or_else(|| {
                panic!("pass_keep_qualifying_siblings: missing md_rd_subtree_acc_score — pipeline ordering bug");
            });
        // meta_get_f64 already filters NaN/Inf, but guard defensively
        assert!(
            !my_score.is_nan() && !my_score.is_infinite(),
            "pass_keep_qualifying_siblings: invalid score {} — scoring bug",
            my_score
        );
        if my_score == 0.0 || my_children.is_empty() {
            return (WalkerAction::Continue, ());
        }
        // Find best child (highest score, exclude body/html)
        let best_idx = my_children
            .iter()
            .enumerate()
            .filter(|(_, c)| match c {
                DomNode::Element { tag, .. } => !is_body_or_html(tag),
                _ => false,
            })
            .filter_map(|(i, c)| match c {
                DomNode::Element { metadata, .. } => {
                    meta_get_f64(metadata, "md_rd_subtree_acc_score").map(|s| (i, s))
                }
                _ => None,
            })
            .max_by(|(_, a), (_, b)| a.total_cmp(b))
            .map(|(i, _)| i);
        let Some(best_idx) = best_idx else {
            return (WalkerAction::Continue, ());
        };
        // Get candidate's class for same-class bonus
        // class attribute is genuinely optional — empty string is the correct fallback
        let candidate_class = match &my_children[best_idx] {
            DomNode::Element { attrs, .. } => attrs
                .iter()
                .find(|(k, _)| k == "class")
                .map(|(_, v)| v.clone())
                .unwrap_or_default(),
            _ => String::new(),
        };
        // FC-HIGH-004: Add debug_assert before unwrap_or(0.0) on candidate_score
        // The best child was selected BY score, so its score should always be available
        debug_assert!(
            meta_get_f64(
                match &my_children[best_idx] {
                    DomNode::Element { metadata, .. } => metadata,
                    _ => {
                        return (WalkerAction::Continue, ());
                    }
                },
                "md_rd_subtree_acc_score"
            )
            .is_some(),
            "pass_keep_qualifying_siblings: best child has no score — pipeline ordering bug"
        );
        let candidate_score = match &my_children[best_idx] {
            DomNode::Element { metadata, .. } =>
                meta_get_f64(metadata, "md_rd_subtree_acc_score").unwrap_or_else(|| {
                    panic!("pass_keep_qualifying_siblings: best child missing md_rd_subtree_acc_score — pipeline ordering bug");
                }),
            _ => 0.0,
        };
        debug_assert!(
            !candidate_score.is_nan() && candidate_score <= global_max,
            "pass_keep_qualifying_siblings: candidate_score ({}) > global_max ({}) or NaN — inconsistent scoring",
            candidate_score,
            global_max
        );
        // Compute sibling floor relative to candidate_score, not global_max.
        // This ensures siblings in different branches all get a fair threshold.
        let sibling_floor = (candidate_score * SIBLING_FLOOR_RATIO).max(SIBLING_FLOOR_MIN);
        // Keep best child, remove non-qualifying siblings (in reverse order)
        // Non-Element siblings (Text, Comment) are always preserved.
        let mut i = my_children.len();
        while i > 0 {
            i -= 1;
            if i == best_idx {
                continue;
            }
            if !matches!(my_children[i], DomNode::Element { .. }) {
                continue; // non-Element siblings preserved
            }
            // Data table siblings: always preserve — their structure is meaningful.
            if let DomNode::Element { metadata, .. } = &my_children[i]
                && metadata.get("is_data_table").is_some_and(|v| v == "true")
            {
                continue;
            }
            let keep = should_keep_sibling(
                &my_children[i],
                candidate_score,
                &candidate_class,
                sibling_floor,
            );
            if !keep {
                my_children.remove(i);
            }
        }
        (WalkerAction::Continue, ())
    });
}

/// Determine whether a sibling should be kept alongside the best child.
///
/// Pre: sibling is an Element node. Non-Element nodes return false.
/// Post: Returns true if sibling meets the score floor, class bonus, or p-sibling heuristic.
fn should_keep_sibling(
    sibling: &DomNode,
    candidate_score: f64,
    candidate_class: &str,
    sibling_floor: f64,
) -> bool {
    let DomNode::Element {
        tag,
        metadata,
        attrs,
        ..
    } = sibling
    else {
        return false;
    };
    // Use f64::MAX as fallback to preserve sibling on missing score in release mode.
    // In debug mode, assert that scored-tag siblings have scores (pipeline ordering check).
    let sibling_score = meta_get_f64(metadata, "md_rd_subtree_acc_score")
        .unwrap_or_else(|| {
            debug_assert!(
                !TAGS_THAT_GET_SCORED.contains(&tag.as_str()),
                "should_keep_sibling: scored-tag sibling missing md_rd_subtree_acc_score — pipeline ordering bug"
            );
            f64::MAX  // Preserve sibling on missing score (safe default — see INV-016)
        });
    // Same-class bonus: +20% if same class as candidate
    // class attribute is genuinely optional — empty string means no bonus
    let sibling_class = attrs
        .iter()
        .find(|(k, _)| k == "class")
        .map(|(_, v)| v.as_str())
        .unwrap_or("");
    let class_bonus = if !candidate_class.is_empty() && sibling_class == candidate_class {
        candidate_score * 0.2
    } else {
        0.0
    };
    let effective_score = sibling_score + class_bonus;
    if effective_score >= sibling_floor {
        return true;
    }
    // P-sibling heuristic
    if tag == "p" {
        let text = get_inner_text(sibling);
        let node_length = text.len();
        let link_density = metadata
            .get("link_density")
            .and_then(|s| crate::pipelines::passes::rd_utils::meta_parse_f64(s))
            .unwrap_or(0.0);
        // Long <p> heuristic: length > 80 AND link_density < 0.25
        if node_length > 80 && link_density < 0.25 {
            return true;
        }
        // Short sentence heuristic: length > 0 AND length ≤ 80 AND link_density == 0.0
        // AND (text contains ". " or ends with '.')
        if node_length > 0
            && node_length <= 80
            && link_density == 0.0
            && (text.contains(". ") || text.ends_with('.'))
        {
            return true;
        }
    }
    // Image preservation: <img> elements with a valid src should survive even with
    // zero score. Images don't get paragraph scores (not p/td/pre/blockquote).
    // Also check data-src/data-original for lazy-loaded images that may not have src set.
    if tag == "img" {
        let has_src = attrs.iter().any(|(k, v)| {
            matches!(
                k.as_str(),
                "src" | "data-src" | "data-original" | "data-lazy-src"
            ) && !v.is_empty()
        });
        if has_src {
            return true;
        }
    }
    // Content-candidate check: if the sibling's class/id matches CONTENT_CANDIDATE_RE
    // (e.g., "MathJax", "content", "article"), preserve it even with zero score.
    // This mirrors the okMaybeItsACandidate check in strip_unlikely_candidates (Phase 1).
    // Without this, elements like <mjx-container class="MathJax ..."> survive
    // strip_unlikely_candidates but get removed here because they have score 0.0
    // (not a scored tag like p/td/pre/blockquote).
    let class_val = attrs
        .iter()
        .find(|(k, _)| k == "class")
        .map(|(_, v)| v.as_str())
        .unwrap_or("");
    let id_val = attrs
        .iter()
        .find(|(k, _)| k == "id")
        .map(|(_, v)| v.as_str())
        .unwrap_or("");
    if CONTENT_CANDIDATE_RE.is_match(class_val) || CONTENT_CANDIDATE_RE.is_match(id_val) {
        return true;
    }
    false
}

// ---------------------------------------------------------------------------
// pass_promote_content_child — final pass to keep only the best child
// ---------------------------------------------------------------------------

/// Promote the best non-body/html content child at each level by removing
/// all other children. If no qualifying child exists, clear all children.
///
/// Uses walk_pre_mut with WalkerAction::Continue (inline removal via children.remove(i)).
///
/// Pre: Scoring has run. All prior micropasses have run.
/// Post: Only the best non-body/html child remains at each level. If no qualifying child
///       exists, all children are cleared. Single-child nodes are unchanged.
/// # Panics
/// Never panics. Non-Element children are treated as score 0.0.
///
/// Priority rule for zero-score children:
/// - If at least one Element child has a positive score, that child is promoted.
/// - If no Element child has a positive score, children are cleared (same as current behavior).
/// - Non-Element children with score 0.0 do NOT prevent clearing.
pub fn pass_promote_content_child(node: &mut DomNode) {
    walk_pre_mut(node, &|n| {
        let DomNode::Element { children, .. } = n else {
            return WalkerAction::Continue;
        };
        if children.len() <= 1 {
            return WalkerAction::Continue;
        }
        // Find best non-body/html Element child
        // Non-Element children (Text, Comment, Doctype) are excluded — they cannot be promoted
        let best_idx = children
            .iter()
            .enumerate()
            .filter(|(_, c)| match c {
                DomNode::Element { tag, .. } => !is_body_or_html(tag),
                _ => false, // Non-Element children excluded (FC-CRIT-004)
            })
            .filter_map(|(i, c)| match c {
                DomNode::Element { metadata, .. } => {
                    meta_get_f64(metadata, "md_rd_subtree_acc_score").map(|s| (i, s))
                }
                _ => None,
            })
            .filter(|(_, s)| *s > 0.0) // Only positive scores qualify (zero-score → cleared)
            .max_by(|(_, a), (_, b)| a.total_cmp(b))
            .map(|(i, _)| i);
        match best_idx {
            Some(idx) => {
                // Remove all other children (reverse order)
                let mut i = children.len();
                while i > 0 {
                    i -= 1;
                    if i != idx {
                        children.remove(i);
                    }
                }
            }
            None => {
                // No qualifying child — clear all
                children.clear();
            }
        }
        WalkerAction::Continue // Returns Continue, not Remove (FC-HIGH-008)
    });
}
// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[path = "../../../tests/unit/pipelines/passes/rd_extraction_test.rs"]
mod tests;
