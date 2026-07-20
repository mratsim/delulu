use crate::pipeline::DomNode;
use crate::pipeline::walkers::{WalkerAction, walk_pre_mut, walk_post_acc_mut};
use crate::pipeline::walkers::walk_post_mut;
use crate::pipeline::passes::rd_utils::{meta_get_f64, is_body_or_html, get_inner_text};
use crate::pipeline::passes::rd_filters::{is_data_table, CONTENT_CANDIDATE_RE};


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
/// unparseable (scoring bug).
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
                    Some(raw) => match crate::pipeline::passes::rd_utils::meta_parse_f64(raw) {
                        Some(s) => s,  // meta_parse_f64 already guarantees finite, non-NaN
                        None => {
                            // Present but invalid: scoring bug — crash-loudly
                            panic!(
                                "pass_prune_no_candidate: unparseable md_rd_subtree_acc_score: {:?} — scoring bug",
                                raw
                            );
                        }
                    }
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
/// unparseable (scoring bug), or if Element children exist but none have valid scores.
pub fn pass_splice_cutoff(node: &mut DomNode) {
    let mut cutoff_filter = |n: &mut DomNode| -> WalkerAction {
        let DomNode::Element { children, metadata, tag, .. } = n else {
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
        // - Present but unparseable → panic (scoring bug)
        // - Present and valid → use it
        let my_score = match metadata.get("md_rd_subtree_acc_score") {
            None => {
                panic!("pass_splice_cutoff: node '{}' missing md_rd_subtree_acc_score — pipeline ordering bug", tag);
            }
            Some(raw) => crate::pipeline::passes::rd_utils::meta_parse_f64(raw).unwrap_or_else(|| {
                panic!("pass_splice_cutoff: node '{}' has unparseable md_rd_subtree_acc_score: {:?} — scoring bug", tag, raw);
            }),
        };
        // Find best child's score from children's metadata
        // Check if there are any Element children first — having none is legitimate
        // (e.g., a <p> with only text content). Only panic if Element children exist but
        // none have valid scores (pipeline ordering bug: scoring must run before this pass).
        let has_element_children = children.iter().any(|c| matches!(c, DomNode::Element { .. }));
        let best_child_score = if has_element_children {
            children.iter()
                .filter_map(|c| match c {
                    DomNode::Element { metadata, .. } =>
                        meta_get_f64(metadata, "md_rd_subtree_acc_score"),
                    _ => None,
                })
                .max_by(f64::total_cmp)
                .unwrap_or_else(|| {
                    panic!("pass_splice_cutoff: no child with valid score — pipeline ordering bug?");
                })
        } else {
            0.0
        };
        // Cutoff check: my_score < best_child_score / 3.0
        // CUTOFF_SCORE_THRESHOLD (20.0) implicitly guards against best_child_score == 0.0.
        if best_child_score >= CUTOFF_SCORE_THRESHOLD
            && my_score < best_child_score / 3.0
        {
            // Don't splice if any direct child is a data table — would eject the table
            // from its container, causing it to be removed by sibling qualification.
            let has_data_table_child = children.iter().any(|c| match c {
                DomNode::Element { metadata, .. } =>
                    metadata.get("is_data_table").is_some_and(|v| v == "true"),
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
    let mut filters: Vec<&mut crate::pipeline::walkers::WalkerFilter> = vec![&mut cutoff_filter];
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
    let DomNode::Element { children, .. } = node else { return };
    walk_post_acc_mut::<()>(children, Some(is_data_table), &mut |n: &mut DomNode, _child_accs: &[()]| {
        let DomNode::Element { children: my_children, metadata, tag, .. } = n else {
            return (WalkerAction::Continue, ());
        };
        // Read node's score — missing or invalid is a pipeline/scoring bug
        let my_score = meta_get_f64(metadata, "md_rd_subtree_acc_score")
            .unwrap_or_else(|| {
                panic!("pass_keep_alt_cluster: node missing md_rd_subtree_acc_score — pipeline ordering bug");
            });
        // meta_get_f64 already filters NaN/Inf, but guard defensively
        assert!(!my_score.is_nan() && !my_score.is_infinite(),
            "pass_keep_alt_cluster: invalid score {} — scoring bug", my_score);
        if my_score == 0.0 {
            return (WalkerAction::Continue, ());
        }
        // Find best non-body/html child score for alt_threshold
        let top_child_score = my_children.iter()
            .filter_map(|c| match c {
                DomNode::Element { tag, metadata, .. } if !is_body_or_html(tag) =>
                    meta_get_f64(metadata, "md_rd_subtree_acc_score"),
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
            let qualifying_count = my_children.iter()
                .filter(|c| match c {
                    DomNode::Element { tag, metadata, .. } if !is_body_or_html(tag) =>
                        alt_threshold.is_some_and(|threshold|
                            meta_get_f64(metadata, "md_rd_subtree_acc_score")
                                .is_some_and(|s| s >= threshold)),
                    _ => false,
                })
                .count();
            qualifying_count >= 3 && !is_body_or_html(tag)
        };
        if is_alt_cluster {
            // Use retain for O(n) removal instead of O(n²) remove(i)
            my_children.retain(|c| match c {
                DomNode::Element { tag, metadata, attrs, .. } if !is_body_or_html(tag) => {
                    // Data tables: always preserve — their structure is meaningful.
                    if metadata.get("is_data_table").is_some_and(|v| v == "true") {
                        return true;
                    }
                    // Content-candidate check: preserve elements with content-indicating class/id
                    // (e.g., "MathJax", "content", "article"). Mirrors should_keep_sibling.
                    let class_val = attrs.iter()
                        .find(|(k, _)| k == "class")
                        .map(|(_, v)| v.as_str())
                        .unwrap_or("");
                    let id_val = attrs.iter()
                        .find(|(k, _)| k == "id")
                        .map(|(_, v)| v.as_str())
                        .unwrap_or("");
                    if CONTENT_CANDIDATE_RE.is_match(class_val) || CONTENT_CANDIDATE_RE.is_match(id_val) {
                        return true;
                    }
                    alt_threshold.is_some_and(|threshold|
                        meta_get_f64(metadata, "md_rd_subtree_acc_score")
                            .is_some_and(|s| s >= threshold))
                },
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
    let DomNode::Element { children, metadata, .. } = node else { return };
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

    walk_post_acc_mut::<()>(children, Some(is_data_table), &mut |n: &mut DomNode, _child_accs: &[()]| {
        let DomNode::Element { children: my_children, metadata, tag: _, .. } = n else {
            return (WalkerAction::Continue, ());
        };
        let my_score = meta_get_f64(metadata, "md_rd_subtree_acc_score")
            .unwrap_or_else(|| {
                panic!("pass_keep_qualifying_siblings: missing md_rd_subtree_acc_score — pipeline ordering bug");
            });
        // meta_get_f64 already filters NaN/Inf, but guard defensively
        assert!(!my_score.is_nan() && !my_score.is_infinite(),
            "pass_keep_qualifying_siblings: invalid score {} — scoring bug", my_score);
        if my_score == 0.0 || my_children.is_empty() {
            return (WalkerAction::Continue, ());
        }
        // Find best child (highest score, exclude body/html)
        let best_idx = my_children.iter().enumerate()
            .filter(|(_, c)| match c {
                DomNode::Element { tag, .. } => !is_body_or_html(tag),
                _ => false,
            })
            .filter_map(|(i, c)| match c {
                DomNode::Element { metadata, .. } =>
                    meta_get_f64(metadata, "md_rd_subtree_acc_score")
                        .map(|s| (i, s)),
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
            DomNode::Element { attrs, .. } => attrs.iter()
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
                    _ => { return (WalkerAction::Continue, ()); }
                },
                "md_rd_subtree_acc_score"
            ).is_some(),
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
            candidate_score, global_max
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
            let keep = should_keep_sibling(&my_children[i], candidate_score, &candidate_class, sibling_floor);
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
    let DomNode::Element { tag, metadata, attrs, .. } = sibling else {
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
    let sibling_class = attrs.iter()
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
            .and_then(|s| crate::pipeline::passes::rd_utils::meta_parse_f64(s))
            .unwrap_or(0.0);
        // Long <p> heuristic: length > 80 AND link_density < 0.25
        if node_length > 80 && link_density < 0.25 {
            return true;
        }
        // Short sentence heuristic: length > 0 AND length ≤ 80 AND link_density == 0.0
        // AND (text contains ". " or ends with '.')
        if node_length > 0 && node_length <= 80 && link_density == 0.0
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
            matches!(k.as_str(), "src" | "data-src" | "data-original" | "data-lazy-src")
                && !v.is_empty()
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
    let class_val = attrs.iter()
        .find(|(k, _)| k == "class")
        .map(|(_, v)| v.as_str())
        .unwrap_or("");
    let id_val = attrs.iter()
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
        let best_idx = children.iter().enumerate()
            .filter(|(_, c)| match c {
                DomNode::Element { tag, .. } => !is_body_or_html(tag),
                _ => false,  // Non-Element children excluded (FC-CRIT-004)
            })
            .filter_map(|(i, c)| match c {
                DomNode::Element { metadata, .. } =>
                    meta_get_f64(metadata, "md_rd_subtree_acc_score")
                        .map(|s| (i, s)),
                _ => None,
            })
            .filter(|(_, s)| *s > 0.0)  // Only positive scores qualify (zero-score → cleared)
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
        WalkerAction::Continue  // Returns Continue, not Remove (FC-HIGH-008)
    });
}
// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    /// Helper struct that captures tracing output into a shared buffer.
    struct CaptureWriter(Arc<Mutex<Vec<u8>>>);

    impl std::io::Write for CaptureWriter {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    /// Run a closure with a tracing subscriber that captures output into a buffer.
    /// Returns the captured buffer so callers can assert on its contents.
    fn with_captured_tracing<F: FnOnce()>(f: F) -> Arc<Mutex<Vec<u8>>> {
        let buf: Arc<Mutex<Vec<u8>>> = Arc::new(Mutex::new(Vec::new()));
        let buf_clone = buf.clone();
        let subscriber = tracing_subscriber::fmt()
            .with_writer(move || CaptureWriter(buf_clone.clone()))
            .finish();
        let _guard = tracing::subscriber::set_default(subscriber);
        f();
        buf
    }
    // ── pass_keep_qualifying_siblings ─────────────────────────────────────

    #[test]

    #[test]
    fn test_qualifying_sibling_candidate_relative_floor() {
        // Best child score = 50, sibling score = 12.
        // Old floor (global_max * 0.2): if global_max > 60, floor > 12 → removed.
        // New floor (candidate_score * 0.2): floor = 50 * 0.2 = 10, 12 >= 10 → kept.
        let mut root = DomNode::Element {
            tag: "div".into(),
            attrs: vec![],
            children: vec![
                DomNode::Element {
                    tag: "parent".into(),
                    attrs: vec![],
                    children: vec![
                        DomNode::Element {
                            tag: "p".into(),
                            attrs: vec![],
                            children: vec![DomNode::Text("best child with some text for scoring".into())],
                            scores: [("mozilla_readability".into(), 50.0)].into(),
                            metadata: [("md_rd_subtree_acc_score".into(), "50".into())].into(),
                        },
                        DomNode::Element {
                            tag: "p".into(),
                            attrs: vec![],
                            children: vec![DomNode::Text("sibling with some text for scoring".into())],
                            scores: [("mozilla_readability".into(), 12.0)].into(),
                            metadata: [("md_rd_subtree_acc_score".into(), "12".into())].into(),
                        },
                    ],
                    scores: Default::default(),
                    metadata: [("md_rd_subtree_acc_score".into(), "60".into())].into(),
                },
            ],
            scores: Default::default(),
            metadata: [("md_rd_subtree_max_score".into(), "100".into())].into(),
        };
        pass_keep_qualifying_siblings(&mut root);
        // After fix: sibling should survive (floor = 50 * 0.2 = 10, sibling score = 12 >= 10)
        let root_children = match &root {
            DomNode::Element { children, .. } => children,
            _ => panic!("root should be Element"),
        };
        if let DomNode::Element { children, .. } = &root_children[0] {
            assert_eq!(children.len(), 2, "both children should survive with candidate-relative floor");
        } else {
            panic!("parent should be Element");
        }
    }

    #[test]
    fn test_qualifying_sibling_candidate_relative_floor_multi_branch() {
        // Multi-branch: parent1 best=50, sibling=12. parent2 other=100.
        // global_max=100. Old floor=20 → sibling(12) removed.
        // New floor=max(50*0.2,10)=10 → sibling(12) kept.
        let mut root = DomNode::Element {
            tag: "div".into(),
            attrs: vec![],
            children: vec![
                DomNode::Element {
                    tag: "parent1".into(),
                    attrs: vec![],
                    children: vec![
                        DomNode::Element {
                            tag: "p".into(),
                            attrs: vec![],
                            children: vec![DomNode::Text("best child with enough text for scoring".into())],
                            scores: [("mozilla_readability".into(), 50.0)].into(),
                            metadata: [("md_rd_subtree_acc_score".into(), "50".into())].into(),
                        },
                        DomNode::Element {
                            tag: "p".into(),
                            attrs: vec![],
                            children: vec![DomNode::Text("sibling text content".into())],
                            scores: [("mozilla_readability".into(), 12.0)].into(),
                            metadata: [("md_rd_subtree_acc_score".into(), "12".into())].into(),
                        },
                    ],
                    scores: Default::default(),
                    metadata: [("md_rd_subtree_acc_score".into(), "60".into())].into(),
                },
                DomNode::Element {
                    tag: "parent2".into(),
                    attrs: vec![],
                    children: vec![
                        DomNode::Element {
                            tag: "p".into(),
                            attrs: vec![],
                            children: vec![DomNode::Text("other branch with lots of content for scoring here".into())],
                            scores: [("mozilla_readability".into(), 100.0)].into(),
                            metadata: [("md_rd_subtree_acc_score".into(), "100".into())].into(),
                        },
                    ],
                    scores: Default::default(),
                    metadata: [("md_rd_subtree_acc_score".into(), "100".into())].into(),
                },
            ],
            scores: Default::default(),
            metadata: [("md_rd_subtree_max_score".into(), "100".into())].into(),
        };
        pass_keep_qualifying_siblings(&mut root);
        // parent1 should keep both children (sibling score 12 >= floor 10)
        let root_children = match &root {
            DomNode::Element { children, .. } => children,
            _ => panic!("root should be Element"),
        };
        if let DomNode::Element { children, .. } = &root_children[0] {
            assert_eq!(children.len(), 2, "parent1 should keep both children with candidate-relative floor");
        } else {
            panic!("parent1 should be Element");
        }
        // parent2 keeps its one child
        if let DomNode::Element { children, .. } = &root_children[1] {
            assert_eq!(children.len(), 1, "parent2 should keep its child");
        } else {
            panic!("parent2 should be Element");
        }
    }

    #[test]
    fn test_qualifying_sibling_missing_score() {
        // Sibling with missing md_rd_subtree_acc_score should survive (f64::MAX fallback).
        let mut root = DomNode::Element {
            tag: "div".into(),
            attrs: vec![],
            children: vec![
                DomNode::Element {
                    tag: "parent".into(),
                    attrs: vec![],
                    children: vec![
                        DomNode::Element {
                            tag: "p".into(),
                            attrs: vec![],
                            children: vec![DomNode::Text("best child text for scoring".into())],
                            scores: [("mozilla_readability".into(), 50.0)].into(),
                            metadata: [("md_rd_subtree_acc_score".into(), "50".into())].into(),
                        },
                        DomNode::Element {
                            tag: "span".into(),
                            attrs: vec![],
                            children: vec![DomNode::Text("sibling content".into())],
                            scores: Default::default(),
                            metadata: [("md_rd_subtree_acc_score".into(), "15".into())].into(),
                        },
                    ],
                    scores: Default::default(),
                    metadata: [("md_rd_subtree_acc_score".into(), "60".into())].into(),
                },
            ],
            scores: Default::default(),
            metadata: [("md_rd_subtree_max_score".into(), "100".into())].into(),
        };
        pass_keep_qualifying_siblings(&mut root);
        // The span sibling has no score — should be preserved via f64::MAX fallback
        let root_children = match &root {
            DomNode::Element { children, .. } => children,
            _ => panic!("root should be Element"),
        };
        if let DomNode::Element { children, .. } = &root_children[0] {
            assert_eq!(children.len(), 2, "sibling with score 15 should survive (floor 50*0.2=10)");
        } else {
            panic!("parent should be Element");
        }
    }

    #[test]
    fn test_qualifying_sibling_below_floor_removed_relative() {
        // Sibling with score below candidate-relative floor should be removed.
        let mut root = DomNode::Element {
            tag: "div".into(),
            attrs: vec![],
            children: vec![
                DomNode::Element {
                    tag: "parent".into(),
                    attrs: vec![],
                    children: vec![
                        DomNode::Element {
                            tag: "p".into(),
                            attrs: vec![],
                            children: vec![DomNode::Text("best child text for scoring".into())],
                            scores: [("mozilla_readability".into(), 100.0)].into(),
                            metadata: [("md_rd_subtree_acc_score".into(), "100".into())].into(),
                        },
                        DomNode::Element {
                            tag: "p".into(),
                            attrs: vec![],
                            children: vec![DomNode::Text("low sibling".into())],
                            scores: [("mozilla_readability".into(), 5.0)].into(),
                            metadata: [("md_rd_subtree_acc_score".into(), "5".into())].into(),
                        },
                    ],
                    scores: Default::default(),
                    metadata: [("md_rd_subtree_acc_score".into(), "100".into())].into(),
                },
            ],
            scores: Default::default(),
            metadata: [("md_rd_subtree_max_score".into(), "100".into())].into(),
        };
        pass_keep_qualifying_siblings(&mut root);
        // Sibling score 5 < floor 20 (100*0.2) → should be removed
        let root_children = match &root {
            DomNode::Element { children, .. } => children,
            _ => panic!("root should be Element"),
        };
        if let DomNode::Element { children, .. } = &root_children[0] {
            assert_eq!(children.len(), 1, "low-scoring sibling should be removed");
        } else {
            panic!("parent should be Element");
        }
    }
    fn test_qualifying_sibling_score_floor_kept() {
        // Sibling with score >= sibling_floor should be kept.
        // global_max = 100.0, sibling_floor = (100.0 * 0.2).max(10.0) = 20.0
        // best child (article) score = 100.0, sibling (section) score = 25.0 >= 20.0 → kept
        // Structure: root > parent > [article (best), section (sibling)]
        let mut root = DomNode::Element {
            tag: "div".into(),
            attrs: vec![],
            children: vec![
                DomNode::Element {
                    tag: "section".into(),
                    attrs: vec![],
                    children: vec![
                        DomNode::Element {
                            tag: "article".into(),
                            attrs: vec![],
                            children: vec![DomNode::Text("content".into())],
                            scores: Default::default(),
                            metadata: [("md_rd_subtree_acc_score".into(), "100.0".into())].into(),
                        },
                        DomNode::Element {
                            tag: "section".into(),
                            attrs: vec![],
                            children: vec![],
                            scores: Default::default(),
                            metadata: [("md_rd_subtree_acc_score".into(), "25.0".into())].into(),
                        },
                    ],
                    scores: Default::default(),
                    metadata: [("md_rd_subtree_acc_score".into(), "50.0".into())].into(),
                },
            ],
            scores: Default::default(),
            metadata: [
                ("md_rd_subtree_max_score".into(), "100.0".into()),
            ].into(),
        };
        pass_keep_qualifying_siblings(&mut root);
        if let DomNode::Element { children, .. } = &root {
            assert_eq!(children.len(), 1, "root should have 1 child (parent)");
            if let DomNode::Element { tag, children: inner, .. } = &children[0] {
                assert_eq!(tag, "section", "parent should remain");
                assert_eq!(inner.len(), 2, "both best child and floor-qualified sibling should be kept");
                let tags: Vec<&str> = inner.iter().filter_map(|c| match c {
                    DomNode::Element { tag, .. } => Some(tag.as_str()),
                    _ => None,
                }).collect();
                assert!(tags.contains(&"article"), "article (best child) should be kept");
                assert!(tags.contains(&"section"), "section (floor-qualified) should be kept");
            } else {
                panic!("root child should be Element");
            }
        } else {
            panic!("root should remain Element");
        }
    }

    #[test]
    fn test_qualifying_sibling_below_floor_removed() {
        // Sibling with score < sibling_floor should be removed.
        // global_max = 100.0, sibling_floor = 20.0
        // sibling (span) score = 5.0 < 20.0 → removed
        // Structure: root > parent > [article (best), span (sibling)]
        let mut root = DomNode::Element {
            tag: "div".into(),
            attrs: vec![],
            children: vec![
                DomNode::Element {
                    tag: "section".into(),
                    attrs: vec![],
                    children: vec![
                        DomNode::Element {
                            tag: "article".into(),
                            attrs: vec![],
                            children: vec![DomNode::Text("content".into())],
                            scores: Default::default(),
                            metadata: [("md_rd_subtree_acc_score".into(), "100.0".into())].into(),
                        },
                        DomNode::Element {
                            tag: "span".into(),
                            attrs: vec![],
                            children: vec![],
                            scores: Default::default(),
                            metadata: [("md_rd_subtree_acc_score".into(), "5.0".into())].into(),
                        },
                    ],
                    scores: Default::default(),
                    metadata: [("md_rd_subtree_acc_score".into(), "50.0".into())].into(),
                },
            ],
            scores: Default::default(),
            metadata: [
                ("md_rd_subtree_max_score".into(), "100.0".into()),
            ].into(),
        };
        pass_keep_qualifying_siblings(&mut root);
        if let DomNode::Element { children, .. } = &root {
            assert_eq!(children.len(), 1, "root should have 1 child (parent)");
            if let DomNode::Element { tag, children: inner, .. } = &children[0] {
                assert_eq!(tag, "section", "parent should remain");
                assert_eq!(inner.len(), 1, "only best child should remain");
                if let DomNode::Element { tag: ct, .. } = &inner[0] {
                    assert_eq!(ct, "article", "article (best child) should be kept");
                } else {
                    panic!("child should be Element");
                }
            } else {
                panic!("root child should be Element");
            }
        } else {
            panic!("root should remain Element");
        }
    }

    #[test]
    fn test_qualifying_sibling_same_class_bonus() {
        // Same-class bonus (+20%) keeps an otherwise low-scored sibling.
        // candidate_score = 100.0, same-class bonus = 100.0 * 0.2 = 20.0
        // sibling score = 5.0 + 20.0 bonus = 25.0 >= 20.0 floor → kept
        // Structure: root > parent > [article (best, class=content), div (sibling, class=content)]
        let mut root = DomNode::Element {
            tag: "div".into(),
            attrs: vec![],
            children: vec![
                DomNode::Element {
                    tag: "section".into(),
                    attrs: vec![],
                    children: vec![
                        DomNode::Element {
                            tag: "article".into(),
                            attrs: vec![("class".into(), "content".into())],
                            children: vec![DomNode::Text("content".into())],
                            scores: Default::default(),
                            metadata: [("md_rd_subtree_acc_score".into(), "100.0".into())].into(),
                        },
                        DomNode::Element {
                            tag: "div".into(),
                            attrs: vec![("class".into(), "content".into())],
                            children: vec![],
                            scores: Default::default(),
                            metadata: [("md_rd_subtree_acc_score".into(), "5.0".into())].into(),
                        },
                    ],
                    scores: Default::default(),
                    metadata: [("md_rd_subtree_acc_score".into(), "50.0".into())].into(),
                },
            ],
            scores: Default::default(),
            metadata: [
                ("md_rd_subtree_max_score".into(), "100.0".into()),
            ].into(),
        };
        pass_keep_qualifying_siblings(&mut root);
        if let DomNode::Element { children, .. } = &root {
            assert_eq!(children.len(), 1, "root should have 1 child (parent)");
            if let DomNode::Element { tag, children: inner, .. } = &children[0] {
                assert_eq!(tag, "section", "parent should remain");
                assert_eq!(inner.len(), 2, "both best child and same-class sibling should be kept");
                let tags: Vec<&str> = inner.iter().filter_map(|c| match c {
                    DomNode::Element { tag, .. } => Some(tag.as_str()),
                    _ => None,
                }).collect();
                assert!(tags.contains(&"article"), "article (best child) should be kept");
                assert!(tags.contains(&"div"), "div (same-class sibling) should be kept via bonus");
            } else {
                panic!("root child should be Element");
            }
        } else {
            panic!("root should remain Element");
        }
    }

    #[test]
    fn test_qualifying_sibling_p_long_text() {
        // P-sibling long-text heuristic (node_length > 80 AND link_density < 0.25).
        let long_text = "This is a long paragraph that exceeds eighty characters in total length so that it triggers the p-sibling heuristic for keeping low-scored p elements that have meaningful content.";
        let mut root = DomNode::Element {
            tag: "div".into(),
            attrs: vec![],
            children: vec![
                DomNode::Element {
                    tag: "section".into(),
                    attrs: vec![],
                    children: vec![
                        DomNode::Element {
                            tag: "article".into(),
                            attrs: vec![],
                            children: vec![DomNode::Text("content".into())],
                            scores: Default::default(),
                            metadata: [("md_rd_subtree_acc_score".into(), "100.0".into())].into(),
                        },
                        DomNode::Element {
                            tag: "p".into(),
                            attrs: vec![],
                            children: vec![DomNode::Text(long_text.into())],
                            scores: Default::default(),
                            metadata: [
                                ("md_rd_subtree_acc_score".into(), "5.0".into()),
                                ("link_density".into(), "0.1".into()),
                            ].into(),
                        },
                    ],
                    scores: Default::default(),
                    metadata: [("md_rd_subtree_acc_score".into(), "50.0".into())].into(),
                },
            ],
            scores: Default::default(),
            metadata: [
                ("md_rd_subtree_max_score".into(), "100.0".into()),
            ].into(),
        };
        pass_keep_qualifying_siblings(&mut root);
        if let DomNode::Element { children, .. } = &root {
            assert_eq!(children.len(), 1, "root should have 1 child (parent)");
            if let DomNode::Element { tag, children: inner, .. } = &children[0] {
                assert_eq!(tag, "section", "parent should remain");
                assert_eq!(inner.len(), 2, "both best child and long-text p-sibling should be kept");
                let tags: Vec<&str> = inner.iter().filter_map(|c| match c {
                    DomNode::Element { tag, .. } => Some(tag.as_str()),
                    _ => None,
                }).collect();
                assert!(tags.contains(&"article"), "article (best child) should be kept");
                assert!(tags.contains(&"p"), "p (long-text sibling) should be kept via heuristic");
            } else {
                panic!("root child should be Element");
            }
        } else {
            panic!("root should remain Element");
        }
    }

    #[test]
    fn test_qualifying_sibling_p_short_sentence() {
        // P-sibling short-sentence heuristic (length > 0, ≤ 80, link_density == 0.0, contains ". " or ends with '.').
        let mut root = DomNode::Element {
            tag: "div".into(),
            attrs: vec![],
            children: vec![
                DomNode::Element {
                    tag: "section".into(),
                    attrs: vec![],
                    children: vec![
                        DomNode::Element {
                            tag: "article".into(),
                            attrs: vec![],
                            children: vec![DomNode::Text("content".into())],
                            scores: Default::default(),
                            metadata: [("md_rd_subtree_acc_score".into(), "100.0".into())].into(),
                        },
                        DomNode::Element {
                            tag: "p".into(),
                            attrs: vec![],
                            children: vec![DomNode::Text("Short sentence.".into())],
                            scores: Default::default(),
                            metadata: [
                                ("md_rd_subtree_acc_score".into(), "5.0".into()),
                                ("link_density".into(), "0.0".into()),
                            ].into(),
                        },
                    ],
                    scores: Default::default(),
                    metadata: [("md_rd_subtree_acc_score".into(), "50.0".into())].into(),
                },
            ],
            scores: Default::default(),
            metadata: [
                ("md_rd_subtree_max_score".into(), "100.0".into()),
            ].into(),
        };
        pass_keep_qualifying_siblings(&mut root);
        if let DomNode::Element { children, .. } = &root {
            assert_eq!(children.len(), 1, "root should have 1 child (parent)");
            if let DomNode::Element { tag, children: inner, .. } = &children[0] {
                assert_eq!(tag, "section", "parent should remain");
                assert_eq!(inner.len(), 2, "both best child and short-sentence p-sibling should be kept");
                let tags: Vec<&str> = inner.iter().filter_map(|c| match c {
                    DomNode::Element { tag, .. } => Some(tag.as_str()),
                    _ => None,
                }).collect();
                assert!(tags.contains(&"article"), "article (best child) should be kept");
                assert!(tags.contains(&"p"), "p (short-sentence sibling) should be kept via heuristic");
            } else {
                panic!("root child should be Element");
            }
        } else {
            panic!("root should remain Element");
        }
    }

    #[test]
    fn test_qualifying_sibling_p_high_link_density() {
        // P-sibling with high link_density (>= 0.25) should be removed.
        let long_text = "This is a long paragraph that exceeds eighty characters in total length so that it would trigger the p-sibling heuristic but has high link density.";
        let mut root = DomNode::Element {
            tag: "div".into(),
            attrs: vec![],
            children: vec![
                DomNode::Element {
                    tag: "section".into(),
                    attrs: vec![],
                    children: vec![
                        DomNode::Element {
                            tag: "article".into(),
                            attrs: vec![],
                            children: vec![DomNode::Text("content".into())],
                            scores: Default::default(),
                            metadata: [("md_rd_subtree_acc_score".into(), "100.0".into())].into(),
                        },
                        DomNode::Element {
                            tag: "p".into(),
                            attrs: vec![],
                            children: vec![DomNode::Text(long_text.into())],
                            scores: Default::default(),
                            metadata: [
                                ("md_rd_subtree_acc_score".into(), "5.0".into()),
                                ("link_density".into(), "0.5".into()),
                            ].into(),
                        },
                    ],
                    scores: Default::default(),
                    metadata: [("md_rd_subtree_acc_score".into(), "50.0".into())].into(),
                },
            ],
            scores: Default::default(),
            metadata: [
                ("md_rd_subtree_max_score".into(), "100.0".into()),
            ].into(),
        };
        pass_keep_qualifying_siblings(&mut root);
        if let DomNode::Element { children, .. } = &root {
            assert_eq!(children.len(), 1, "root should have 1 child (parent)");
            if let DomNode::Element { tag, children: inner, .. } = &children[0] {
                assert_eq!(tag, "section", "parent should remain");
                assert_eq!(inner.len(), 1, "only best child should remain, high-LD p removed");
                if let DomNode::Element { tag: ct, .. } = &inner[0] {
                    assert_eq!(ct, "article", "article (best child) should be kept");
                } else {
                    panic!("child should be Element");
                }
            } else {
                panic!("root child should be Element");
            }
        } else {
            panic!("root should remain Element");
        }
    }

    #[test]
    fn test_qualifying_sibling_body_html_excluded() {
        // Body/html children excluded from best child selection.
        // body has score 200.0 but is excluded; article with 100.0 is selected as best.
        // Structure: root > parent > [body (score=200), article (score=100)]
        let mut root = DomNode::Element {
            tag: "div".into(),
            attrs: vec![],
            children: vec![
                DomNode::Element {
                    tag: "section".into(),
                    attrs: vec![],
                    children: vec![
                        DomNode::Element {
                            tag: "body".into(),
                            attrs: vec![],
                            children: vec![],
                            scores: Default::default(),
                            metadata: [("md_rd_subtree_acc_score".into(), "200.0".into())].into(),
                        },
                        DomNode::Element {
                            tag: "article".into(),
                            attrs: vec![],
                            children: vec![DomNode::Text("content".into())],
                            scores: Default::default(),
                            metadata: [("md_rd_subtree_acc_score".into(), "100.0".into())].into(),
                        },
                    ],
                    scores: Default::default(),
                    metadata: [("md_rd_subtree_acc_score".into(), "50.0".into())].into(),
                },
            ],
            scores: Default::default(),
            metadata: [
                ("md_rd_subtree_max_score".into(), "200.0".into()),
            ].into(),
        };
        pass_keep_qualifying_siblings(&mut root);
        if let DomNode::Element { children, .. } = &root {
            assert_eq!(children.len(), 1, "root should have 1 child (parent)");
            if let DomNode::Element { tag, children: inner, .. } = &children[0] {
                assert_eq!(tag, "section", "parent should remain");
                // body is excluded from best child selection, so article is selected.
                // body has score 200.0 which is >= sibling_floor (200.0*0.2=40.0), so body is kept as sibling.
                assert_eq!(inner.len(), 2, "both article (best) and body (qualifying sibling) should be kept");
                let tags: Vec<&str> = inner.iter().filter_map(|c| match c {
                    DomNode::Element { tag, .. } => Some(tag.as_str()),
                    _ => None,
                }).collect();
                assert!(tags.contains(&"article"), "article should be selected as best child");
                assert!(tags.contains(&"body"), "body should be kept as qualifying sibling");
            } else {
                panic!("root child should be Element");
            }
        } else {
            panic!("root should remain Element");
        }
    }

    #[test]
    fn test_qualifying_sibling_no_qualifying() {
        // No qualifying siblings → only best child kept.
        // sibling (span) score = 3.0 < 20.0 floor, no class bonus, not <p> → removed.
        // Structure: root > parent > [article (best), span (sibling)]
        let mut root = DomNode::Element {
            tag: "div".into(),
            attrs: vec![],
            children: vec![
                DomNode::Element {
                    tag: "section".into(),
                    attrs: vec![],
                    children: vec![
                        DomNode::Element {
                            tag: "article".into(),
                            attrs: vec![],
                            children: vec![DomNode::Text("content".into())],
                            scores: Default::default(),
                            metadata: [("md_rd_subtree_acc_score".into(), "100.0".into())].into(),
                        },
                        DomNode::Element {
                            tag: "span".into(),
                            attrs: vec![],
                            children: vec![],
                            scores: Default::default(),
                            metadata: [("md_rd_subtree_acc_score".into(), "3.0".into())].into(),
                        },
                    ],
                    scores: Default::default(),
                    metadata: [("md_rd_subtree_acc_score".into(), "50.0".into())].into(),
                },
            ],
            scores: Default::default(),
            metadata: [
                ("md_rd_subtree_max_score".into(), "100.0".into()),
            ].into(),
        };
        pass_keep_qualifying_siblings(&mut root);
        if let DomNode::Element { children, .. } = &root {
            assert_eq!(children.len(), 1, "root should have 1 child (parent)");
            if let DomNode::Element { tag, children: inner, .. } = &children[0] {
                assert_eq!(tag, "section", "parent should remain");
                assert_eq!(inner.len(), 1, "only best child should remain");
                if let DomNode::Element { tag: ct, .. } = &inner[0] {
                    assert_eq!(ct, "article", "article (best child) should be kept");
                } else {
                    panic!("child should be Element");
                }
            } else {
                panic!("root child should be Element");
            }
        } else {
            panic!("root should remain Element");
        }
    }

    #[test]
    fn test_qualifying_sibling_non_element_preserved() {
        // Non-Element siblings (Text nodes) should be preserved (not removed by the pass).
        // Structure: root > parent > [article (best), Text("some text")]
        let mut root = DomNode::Element {
            tag: "div".into(),
            attrs: vec![],
            children: vec![
                DomNode::Element {
                    tag: "section".into(),
                    attrs: vec![],
                    children: vec![
                        DomNode::Element {
                            tag: "article".into(),
                            attrs: vec![],
                            children: vec![DomNode::Text("content".into())],
                            scores: Default::default(),
                            metadata: [("md_rd_subtree_acc_score".into(), "100.0".into())].into(),
                        },
                        DomNode::Text("some text".into()),
                    ],
                    scores: Default::default(),
                    metadata: [("md_rd_subtree_acc_score".into(), "50.0".into())].into(),
                },
            ],
            scores: Default::default(),
            metadata: [
                ("md_rd_subtree_max_score".into(), "100.0".into()),
            ].into(),
        };
        pass_keep_qualifying_siblings(&mut root);
        if let DomNode::Element { children, .. } = &root {
            assert_eq!(children.len(), 1, "root should have 1 child (parent)");
            if let DomNode::Element { tag, children: inner, .. } = &children[0] {
                assert_eq!(tag, "section", "parent should remain");
                assert_eq!(inner.len(), 2, "best child and text node should be preserved");
                let has_text = inner.iter().any(|c| matches!(c, DomNode::Text(t) if t == "some text"));
                assert!(has_text, "text node should be preserved");
                let has_article = inner.iter().any(|c| matches!(c, DomNode::Element { tag, .. } if tag == "article"));
                assert!(has_article, "article should be kept");
            } else {
                panic!("root child should be Element");
            }
        } else {
            panic!("root should remain Element");
        }
    }

    // ── should_keep_sibling unit tests ───────────────────────────────────────

    #[test]
    fn test_should_keep_sibling_score_floor() {
        // Sibling with score >= sibling_floor → true.
        let sibling = DomNode::Element {
            tag: "div".into(),
            attrs: vec![],
            children: vec![],
            scores: Default::default(),
            metadata: [("md_rd_subtree_acc_score".into(), "30.0".into())].into(),
        };
        assert!(should_keep_sibling(&sibling, 100.0, "", 20.0));
    }

    #[test]
    fn test_should_keep_sibling_below_floor() {
        // Sibling with score < sibling_floor, no class bonus, not <p> → false.
        let sibling = DomNode::Element {
            tag: "span".into(),
            attrs: vec![],
            children: vec![],
            scores: Default::default(),
            metadata: [("md_rd_subtree_acc_score".into(), "5.0".into())].into(),
        };
        assert!(!should_keep_sibling(&sibling, 100.0, "", 20.0));
    }

    #[test]
    fn test_should_keep_sibling_class_bonus() {
        // Same-class bonus (+20%) lifts effective score above floor.
        let sibling = DomNode::Element {
            tag: "div".into(),
            attrs: vec![("class".into(), "content".into())],
            children: vec![],
            scores: Default::default(),
            metadata: [("md_rd_subtree_acc_score".into(), "5.0".into())].into(),
        };
        assert!(should_keep_sibling(&sibling, 100.0, "content", 20.0));
    }

    #[test]
    fn test_should_keep_sibling_class_bonus_different_class() {
        // Different class → no bonus.
        let sibling = DomNode::Element {
            tag: "div".into(),
            attrs: vec![("class".into(), "other".into())],
            children: vec![],
            scores: Default::default(),
            metadata: [("md_rd_subtree_acc_score".into(), "5.0".into())].into(),
        };
        assert!(!should_keep_sibling(&sibling, 100.0, "content", 20.0));
    }

    #[test]
    fn test_should_keep_sibling_p_long_text() {
        // <p> with long text (>80 chars) and low link_density (<0.25) → true.
        let long_text = "This is a long paragraph that exceeds eighty characters in total length so that it triggers the p-sibling heuristic for keeping low-scored p elements that have meaningful content.";
        let sibling = DomNode::Element {
            tag: "p".into(),
            attrs: vec![],
            children: vec![DomNode::Text(long_text.into())],
            scores: Default::default(),
            metadata: [
                ("md_rd_subtree_acc_score".into(), "5.0".into()),
                ("link_density".into(), "0.1".into()),
            ].into(),
        };
        assert!(should_keep_sibling(&sibling, 100.0, "", 20.0));
    }

    #[test]
    fn test_should_keep_sibling_p_short_sentence() {
        // <p> with short sentence (<=80 chars), link_density == 0.0, ends with '.' → true.
        let sibling = DomNode::Element {
            tag: "p".into(),
            attrs: vec![],
            children: vec![DomNode::Text("Short sentence.".into())],
            scores: Default::default(),
            metadata: [
                ("md_rd_subtree_acc_score".into(), "5.0".into()),
                ("link_density".into(), "0.0".into()),
            ].into(),
        };
        assert!(should_keep_sibling(&sibling, 100.0, "", 20.0));
    }

    #[test]
    fn test_should_keep_sibling_p_short_sentence_with_space() {
        // <p> with short sentence containing ". " → true.
        let sibling = DomNode::Element {
            tag: "p".into(),
            attrs: vec![],
            children: vec![DomNode::Text("Hi. There".into())],
            scores: Default::default(),
            metadata: [
                ("md_rd_subtree_acc_score".into(), "5.0".into()),
                ("link_density".into(), "0.0".into()),
            ].into(),
        };
        assert!(should_keep_sibling(&sibling, 100.0, "", 20.0));
    }

    #[test]
    fn test_should_keep_sibling_p_high_link_density() {
        // <p> with long text but high link_density (>= 0.25) → false.
        let long_text = "This is a long paragraph that exceeds eighty characters but has high link density.";
        let sibling = DomNode::Element {
            tag: "p".into(),
            attrs: vec![],
            children: vec![DomNode::Text(long_text.into())],
            scores: Default::default(),
            metadata: [
                ("md_rd_subtree_acc_score".into(), "5.0".into()),
                ("link_density".into(), "0.5".into()),
            ].into(),
        };
        assert!(!should_keep_sibling(&sibling, 100.0, "", 20.0));
    }

    #[test]
    fn test_should_keep_sibling_non_element() {
        // Non-Element node (Text) → false.
        let sibling = DomNode::Text("hello".into());
        assert!(!should_keep_sibling(&sibling, 100.0, "", 20.0));
    }

    #[test]
    fn test_should_keep_sibling_non_p_low_score() {
        // Non-<p> element with low score and no class bonus → false.
        let sibling = DomNode::Element {
            tag: "span".into(),
            attrs: vec![],
            children: vec![],
            scores: Default::default(),
            metadata: [("md_rd_subtree_acc_score".into(), "5.0".into())].into(),
        };
        assert!(!should_keep_sibling(&sibling, 100.0, "", 20.0));
    }

    #[test]
    fn test_should_keep_sibling_p_no_link_density() {
        // <p> without link_density metadata defaults to 0.0, long text → true.
        let long_text = "This is a long paragraph that exceeds eighty characters in total length without any link density metadata so the default kicks in and it should be kept.";
        let sibling = DomNode::Element {
            tag: "p".into(),
            attrs: vec![],
            children: vec![DomNode::Text(long_text.into())],
            scores: Default::default(),
            // NOTE: no link_density metadata
            metadata: [("md_rd_subtree_acc_score".into(), "5.0".into())].into(),
        };
        assert!(should_keep_sibling(&sibling, 100.0, "", 20.0));
    }
    // ── pass_prune_no_candidate ─────────────────────────────────────────────

    #[test]
    fn test_prune_zero_score() {
        // Scored tag (p) with md_rd_subtree_acc_score = 0.0 should be removed.
        let mut parent = DomNode::Element {
            tag: "div".into(),
            attrs: vec![],
            children: vec![
                DomNode::Element {
                    tag: "p".into(),
                    attrs: vec![],
                    children: vec![],
                    scores: Default::default(),
                    metadata: [("md_rd_subtree_acc_score".into(), "0.0".into())].into(),
                },
            ],
            scores: Default::default(),
            metadata: Default::default(),
        };
        pass_prune_no_candidate(&mut parent);
        if let DomNode::Element { children, .. } = &parent {
            assert!(children.is_empty(), "zero-score scored tag should be removed");
        } else {
            panic!("parent should remain Element");
        }
    }

    #[test]
    #[should_panic(expected = "pass_prune_no_candidate: node missing md_rd_subtree_acc_score")]
    fn test_prune_missing_score() {
        // Element with missing md_rd_subtree_acc_score should panic
        // (pipeline ordering bug: scoring must run before extraction).
        let mut parent = DomNode::Element {
            tag: "div".into(),
            attrs: vec![],
            children: vec![
                DomNode::Element {
                    tag: "article".into(),
                    attrs: vec![],
                    children: vec![],
                    scores: Default::default(),
                    metadata: Default::default(), // no md_rd_subtree_acc_score
                },
            ],
            scores: Default::default(),
            metadata: Default::default(),
        };
        pass_prune_no_candidate(&mut parent);
    }

    #[test]
    #[should_panic(expected = "pass_prune_no_candidate: unparseable md_rd_subtree_acc_score")]
    fn test_prune_nan_score() {
        // Element with NaN md_rd_subtree_acc_score should panic
        // (scoring bug: meta_parse_f64 should have rejected this).
        let mut parent = DomNode::Element {
            tag: "div".into(),
            attrs: vec![],
            children: vec![
                DomNode::Element {
                    tag: "article".into(),
                    attrs: vec![],
                    children: vec![],
                    scores: Default::default(),
                    metadata: [("md_rd_subtree_acc_score".into(), "NaN".into())].into(),
                },
            ],
            scores: Default::default(),
            metadata: Default::default(),
        };
        pass_prune_no_candidate(&mut parent);
    }

    #[test]
    fn test_prune_positive_score() {
        // Element with positive md_rd_subtree_acc_score should be kept.
        let mut parent = DomNode::Element {
            tag: "div".into(),
            attrs: vec![],
            children: vec![
                DomNode::Element {
                    tag: "article".into(),
                    attrs: vec![],
                    children: vec![],
                    scores: Default::default(),
                    metadata: [("md_rd_subtree_acc_score".into(), "42.5".into())].into(),
                },
            ],
            scores: Default::default(),
            metadata: Default::default(),
        };
        pass_prune_no_candidate(&mut parent);
        if let DomNode::Element { children, .. } = &parent {
            assert_eq!(children.len(), 1, "positive-score element should be kept");
            if let DomNode::Element { tag, .. } = &children[0] {
                assert_eq!(tag, "article", "article should remain");
            } else {
                panic!("child should be Element");
            }
        } else {
            panic!("parent should remain Element");
        }
    }

    #[test]
    fn test_prune_non_element() {
        // Non-Element nodes (Text, Comment) should pass through unchanged.
        let mut parent = DomNode::Element {
            tag: "div".into(),
            attrs: vec![],
            children: vec![
                DomNode::Text("hello".into()),
                DomNode::Comment("comment".into()),
            ],
            scores: Default::default(),
            metadata: Default::default(),
        };
        pass_prune_no_candidate(&mut parent);
        if let DomNode::Element { children, .. } = &parent {
            assert_eq!(children.len(), 2, "non-Element nodes should be preserved");
        } else {
            panic!("parent should remain Element");
        }
    }

    #[test]
    fn test_prune_mixed_siblings() {
        // Mixed siblings: some with zero score (scored tags), some with positive score.
        let mut parent = DomNode::Element {
            tag: "div".into(),
            attrs: vec![],
            children: vec![
                DomNode::Element {
                    tag: "p".into(),  // scored tag — removed on zero score
                    attrs: vec![],
                    children: vec![],
                    scores: Default::default(),
                    metadata: [("md_rd_subtree_acc_score".into(), "0.0".into())].into(),
                },
                DomNode::Element {
                    tag: "positive".into(),
                    attrs: vec![],
                    children: vec![],
                    scores: Default::default(),
                    metadata: [("md_rd_subtree_acc_score".into(), "100.0".into())].into(),
                },
                DomNode::Element {
                    tag: "p".into(),  // scored tag — removed on zero score
                    attrs: vec![],
                    children: vec![],
                    scores: Default::default(),
                    metadata: [("md_rd_subtree_acc_score".into(), "0.0".into())].into(),
                },
                DomNode::Text("text node".into()),
            ],
            scores: Default::default(),
            metadata: Default::default(),
        };
        pass_prune_no_candidate(&mut parent);
        if let DomNode::Element { children, .. } = &parent {
            assert_eq!(children.len(), 2, "should keep positive-score element + text node");
            // Verify the positive-score element survived
            let has_positive = children.iter().any(|c| matches!(c, DomNode::Element { tag, .. } if tag == "positive"));
            assert!(has_positive, "positive-score element should be kept");
            // Verify text node survived
            let has_text = children.iter().any(|c| matches!(c, DomNode::Text(t) if t == "text node"));
            assert!(has_text, "text node should be preserved");
        } else {
            panic!("parent should remain Element");
        }
    }


    #[test]
    fn test_prune_zero_score_non_scored_tag() {
        // Non-scored tag (span) with score 0.0 should be preserved (Bug A fix).
        let mut parent = DomNode::Element {
            tag: "div".into(),
            attrs: vec![],
            children: vec![
                DomNode::Element {
                    tag: "span".into(),
                    attrs: vec![],
                    children: vec![DomNode::Text("content".into())],
                    scores: Default::default(),
                    metadata: [("md_rd_subtree_acc_score".into(), "0.0".into())].into(),
                },
            ],
            scores: Default::default(),
            metadata: Default::default(),
        };
        pass_prune_no_candidate(&mut parent);
        if let DomNode::Element { children, .. } = &parent {
            assert_eq!(children.len(), 1, "non-scored tag should be preserved");
            if let DomNode::Element { tag, .. } = &children[0] {
                assert_eq!(tag, "span", "span should remain");
            } else {
                panic!("child should be Element");
            }
        } else {
            panic!("parent should remain Element");
        }
    }

    #[test]
    fn test_prune_zero_score_anchor() {
        // Anchor element with score 0.0 should be preserved (Bug A fix).
        let mut parent = DomNode::Element {
            tag: "div".into(),
            attrs: vec![],
            children: vec![
                DomNode::Element {
                    tag: "a".into(),
                    attrs: vec![],
                    children: vec![DomNode::Text("link".into())],
                    scores: Default::default(),
                    metadata: [("md_rd_subtree_acc_score".into(), "0.0".into())].into(),
                },
            ],
            scores: Default::default(),
            metadata: Default::default(),
        };
        pass_prune_no_candidate(&mut parent);
        if let DomNode::Element { children, .. } = &parent {
            assert_eq!(children.len(), 1, "anchor should be preserved");
            if let DomNode::Element { tag, .. } = &children[0] {
                assert_eq!(tag, "a", "anchor should remain");
            } else {
                panic!("child should be Element");
            }
        } else {
            panic!("parent should remain Element");
        }
    }

    #[test]
    fn test_prune_zero_score_div() {
        // Div element with score 0.0 should be preserved (Bug A fix — most common structural tag).
        let mut parent = DomNode::Element {
            tag: "div".into(),
            attrs: vec![],
            children: vec![
                DomNode::Element {
                    tag: "div".into(),
                    attrs: [("class".into(), "content".into())].into(),
                    children: vec![DomNode::Text("content".into())],
                    scores: Default::default(),
                    metadata: [("md_rd_subtree_acc_score".into(), "0.0".into())].into(),
                },
            ],
            scores: Default::default(),
            metadata: Default::default(),
        };
        pass_prune_no_candidate(&mut parent);
        if let DomNode::Element { children, .. } = &parent {
            assert_eq!(children.len(), 1, "div should be preserved");
            if let DomNode::Element { tag, .. } = &children[0] {
                assert_eq!(tag, "div", "div should remain");
            } else {
                panic!("child should be Element");
            }
        } else {
            panic!("parent should remain Element");
        }
    }



    #[test]
    fn test_prune_data_table_skip() {
        // Elements inside a data table should survive pass_prune_no_candidate.
        // The table has is_data_table=true, so SkipChildren protects its children.
        let mut root = DomNode::Element {
            tag: "div".into(),
            attrs: vec![],
            children: vec![
                DomNode::Element {
                    tag: "table".into(),
                    attrs: vec![],
                    children: vec![
                        DomNode::Element {
                            tag: "td".into(),
                            attrs: vec![],
                            children: vec![DomNode::Text("data".into())],
                            scores: Default::default(),
                            metadata: [("md_rd_subtree_acc_score".into(), "0.0".into())].into(),
                        },
                    ],
                    scores: Default::default(),
                    metadata: [("is_data_table".into(), "true".into())].into(),
                },
            ],
            scores: Default::default(),
            metadata: Default::default(),
        };
        pass_prune_no_candidate(&mut root);

        // The <td> with score 0.0 should survive inside the data table
        fn find_tag(node: &DomNode, tag: &str) -> bool {
            match node {
                DomNode::Element { tag: t, .. } if t == tag => true,
                DomNode::Element { children, .. } => children.iter().any(|c| find_tag(c, tag)),
                _ => false,
            }
        }
        assert!(
            find_tag(&root, "td"),
            "<td> inside data table should survive pass_prune_no_candidate"
        );
    }
    // ── pass_splice_cutoff ──────────────────────────────────────────

    #[test]
    fn test_splice_cutoff_low_score_spliced() {
        // Parent (child of root) has score=10, best child score=100.
        // 10 < 100/3.0 ≈ 33.33 → ReplaceWithChildren.
        let mut root = DomNode::Element {
            tag: "div".into(),
            attrs: vec![],
            children: vec![
                DomNode::Element {
                    tag: "article".into(),
                    attrs: vec![],
                    children: vec![
                        DomNode::Element {
                            tag: "p".into(),
                            attrs: vec![],
                            children: vec![DomNode::Text("content".into())],
                            scores: Default::default(),
                            metadata: [("md_rd_subtree_acc_score".into(), "100.0".into())].into(),
                        },
                    ],
                    scores: Default::default(),
                    metadata: [("md_rd_subtree_acc_score".into(), "10.0".into())].into(),
                },
            ],
            scores: Default::default(),
            metadata: [("md_rd_subtree_acc_score".into(), "0.0".into())].into(),
        };
        pass_splice_cutoff(&mut root);
        if let DomNode::Element { children, .. } = &root {
            // article wrapper should be spliced away, leaving p
            assert_eq!(children.len(), 1, "article should be spliced, leaving p");
            if let DomNode::Element { tag, children: inner, .. } = &children[0] {
                assert_eq!(tag, "p", "p should remain after article is spliced");
                assert_eq!(inner.len(), 1, "p should keep its text child");
                assert!(matches!(&inner[0], DomNode::Text(t) if t == "content"));
            } else {
                panic!("children[0] should be Element");
            }
        } else {
            panic!("root should remain Element");
        }
    }

    #[test]
    fn test_splice_cutoff_high_score_not_spliced() {
        // Parent (child of root) has score=40, best child score=100.
        // 40 >= 100/3.0 ≈ 33.33 → not spliced.
        let mut root = DomNode::Element {
            tag: "div".into(),
            attrs: vec![],
            children: vec![
                DomNode::Element {
                    tag: "article".into(),
                    attrs: vec![],
                    children: vec![
                        DomNode::Element {
                            tag: "p".into(),
                            attrs: vec![],
                            children: vec![DomNode::Text("content".into())],
                            scores: Default::default(),
                            metadata: [("md_rd_subtree_acc_score".into(), "100.0".into())].into(),
                        },
                    ],
                    scores: Default::default(),
                    metadata: [("md_rd_subtree_acc_score".into(), "40.0".into())].into(),
                },
            ],
            scores: Default::default(),
            metadata: [("md_rd_subtree_acc_score".into(), "0.0".into())].into(),
        };
        pass_splice_cutoff(&mut root);
        if let DomNode::Element { children, .. } = &root {
            // article wrapper should remain (score 40 >= 100/3.0)
            assert_eq!(children.len(), 1, "article should NOT be spliced");
            if let DomNode::Element { tag, children: inner, .. } = &children[0] {
                assert_eq!(tag, "article", "article should remain");
                assert_eq!(inner.len(), 1, "article should keep its p child");
                if let DomNode::Element { tag: pt, .. } = &inner[0] {
                    assert_eq!(pt, "p", "p should remain inside article");
                } else {
                    panic!("inner[0] should be Element");
                }
            } else {
                panic!("children[0] should be Element");
            }
        } else {
            panic!("root should remain Element");
        }
    }

    #[test]
    fn test_splice_cutoff_body_never_spliced() {
        // Body element with low score → is_body_or_html returns true → not spliced.
        let mut root = DomNode::Element {
            tag: "div".into(),
            attrs: vec![],
            children: vec![
                DomNode::Element {
                    tag: "body".into(),
                    attrs: vec![],
                    children: vec![
                        DomNode::Element {
                            tag: "p".into(),
                            attrs: vec![],
                            children: vec![DomNode::Text("content".into())],
                            scores: Default::default(),
                            metadata: [("md_rd_subtree_acc_score".into(), "100.0".into())].into(),
                        },
                    ],
                    scores: Default::default(),
                    metadata: [("md_rd_subtree_acc_score".into(), "10.0".into())].into(),
                },
            ],
            scores: Default::default(),
            metadata: [("md_rd_subtree_acc_score".into(), "0.0".into())].into(),
        };
        pass_splice_cutoff(&mut root);
        if let DomNode::Element { children, .. } = &root {
            // body should NOT be spliced despite low score
            assert_eq!(children.len(), 1, "body should NOT be spliced");
            if let DomNode::Element { tag, children: inner, .. } = &children[0] {
                assert_eq!(tag, "body", "body should remain");
                // p is still inside body
                assert_eq!(inner.len(), 1, "body should keep its p child");
            } else {
                panic!("children[0] should be Element");
            }
        } else {
            panic!("root should remain Element");
        }
    }

    #[test]
    fn test_splice_cutoff_html_never_spliced() {
        // Html element with low score → is_body_or_html returns true → not spliced.
        let mut root = DomNode::Element {
            tag: "div".into(),
            attrs: vec![],
            children: vec![
                DomNode::Element {
                    tag: "html".into(),
                    attrs: vec![],
                    children: vec![
                        DomNode::Element {
                            tag: "p".into(),
                            attrs: vec![],
                            children: vec![DomNode::Text("content".into())],
                            scores: Default::default(),
                            metadata: [("md_rd_subtree_acc_score".into(), "100.0".into())].into(),
                        },
                    ],
                    scores: Default::default(),
                    metadata: [("md_rd_subtree_acc_score".into(), "10.0".into())].into(),
                },
            ],
            scores: Default::default(),
            metadata: [("md_rd_subtree_acc_score".into(), "0.0".into())].into(),
        };
        pass_splice_cutoff(&mut root);
        if let DomNode::Element { children, .. } = &root {
            // html should NOT be spliced despite low score
            assert_eq!(children.len(), 1, "html should NOT be spliced");
            if let DomNode::Element { tag, .. } = &children[0] {
                assert_eq!(tag, "html", "html should remain");
            } else {
                panic!("children[0] should be Element");
            }
        } else {
            panic!("root should remain Element");
        }
    }

    #[test]
    fn test_splice_cutoff_no_children_not_spliced() {
        // Element with no Element children → best_child_score=0.0 → cutoff not triggered.
        let mut root = DomNode::Element {
            tag: "div".into(),
            attrs: vec![],
            children: vec![
                DomNode::Element {
                    tag: "span".into(),
                    attrs: vec![],
                    children: vec![],
                    scores: Default::default(),
                    metadata: [("md_rd_subtree_acc_score".into(), "10.0".into())].into(),
                },
            ],
            scores: Default::default(),
            metadata: [("md_rd_subtree_acc_score".into(), "0.0".into())].into(),
        };
        pass_splice_cutoff(&mut root);
        if let DomNode::Element { children, .. } = &root {
            // span has no children → best_child_score=0.0 → no cutoff
            assert_eq!(children.len(), 1, "span with no children should NOT be spliced");
            if let DomNode::Element { tag, .. } = &children[0] {
                assert_eq!(tag, "span", "span should remain");
            } else {
                panic!("children[0] should be Element");
            }
        } else {
            panic!("root should remain Element");
        }
    }

    #[test]
    fn test_splice_cutoff_all_children_zero_score() {
        // All children have score 0.0 → best_child_score=0.0 → first guard fails → not spliced.
        let mut root = DomNode::Element {
            tag: "div".into(),
            attrs: vec![],
            children: vec![
                DomNode::Element {
                    tag: "span".into(),
                    attrs: vec![],
                    children: vec![DomNode::Text("content".into())],
                    scores: Default::default(),
                    metadata: [("md_rd_subtree_acc_score".into(), "0.0".into())].into(),
                },
            ],
            scores: Default::default(),
            metadata: [("md_rd_subtree_acc_score".into(), "5.0".into())].into(),
        };
        pass_splice_cutoff(&mut root);
        if let DomNode::Element { children, .. } = &root {
            // span has score 0.0 → best_child_score=0.0 → no cutoff
            assert_eq!(children.len(), 1, "zero-score child should NOT be spliced");
            if let DomNode::Element { tag, .. } = &children[0] {
                assert_eq!(tag, "span", "span should remain");
            } else {
                panic!("children[0] should be Element");
            }
        } else {
            panic!("root should remain Element");
        }
    }

    #[test]
    fn test_splice_cutoff_single_child_not_spliced() {
        // Single child path: parent (score=50) > child (score=100).
        // 50 >= 100/3.0 ≈ 33.33 → not spliced.
        let mut root = DomNode::Element {
            tag: "div".into(),
            attrs: vec![],
            children: vec![
                DomNode::Element {
                    tag: "article".into(),
                    attrs: vec![],
                    children: vec![
                        DomNode::Element {
                            tag: "p".into(),
                            attrs: vec![],
                            children: vec![],
                            scores: Default::default(),
                            metadata: [("md_rd_subtree_acc_score".into(), "100.0".into())].into(),
                        },
                    ],
                    scores: Default::default(),
                    metadata: [("md_rd_subtree_acc_score".into(), "50.0".into())].into(),
                },
            ],
            scores: Default::default(),
            metadata: [("md_rd_subtree_acc_score".into(), "0.0".into())].into(),
        };
        pass_splice_cutoff(&mut root);
        if let DomNode::Element { children, .. } = &root {
            assert_eq!(children.len(), 1, "article with 50 >= 100/3.0 should NOT be spliced");
            if let DomNode::Element { tag, .. } = &children[0] {
                assert_eq!(tag, "article", "article should remain");
            } else {
                panic!("children[0] should be Element");
            }
        } else {
            panic!("root should remain Element");
        }
    }

    #[test]
    fn test_splice_cutoff_chain_thin_wrappers() {
        // Chain: grandparent (score=10) > parent (score=10) > child (score=100) > text
        // Both grandparent and parent have scores < 100/3.0 → both spliced.
        // Only child (section with text) should remain as child of root.
        let mut root = DomNode::Element {
            tag: "div".into(),
            attrs: vec![],
            children: vec![
                DomNode::Element {
                    tag: "article".into(),
                    attrs: vec![],
                    children: vec![
                        DomNode::Element {
                            tag: "section".into(),
                            attrs: vec![],
                            children: vec![
                                DomNode::Element {
                                    tag: "p".into(),
                                    attrs: vec![],
                                    children: vec![DomNode::Text("final content".into())],
                                    scores: Default::default(),
                                    metadata: [("md_rd_subtree_acc_score".into(), "100.0".into())].into(),
                                },
                            ],
                            scores: Default::default(),
                            metadata: [("md_rd_subtree_acc_score".into(), "10.0".into())].into(),
                        },
                    ],
                    scores: Default::default(),
                    metadata: [("md_rd_subtree_acc_score".into(), "10.0".into())].into(),
                },
            ],
            scores: Default::default(),
            metadata: [("md_rd_subtree_acc_score".into(), "0.0".into())].into(),
        };
        pass_splice_cutoff(&mut root);
        if let DomNode::Element { children, .. } = &root {
            // article (score=10, child section score=100) → 10 < 100/3.0 → ReplaceWithChildren
            // article replaced by section.
            // section (score=10, child p score=100) → 10 < 100/3.0 → ReplaceWithChildren
            // section replaced by p.
            // Result: root > p > text
            assert_eq!(children.len(), 1, "both wrappers should be spliced, leaving p");
            if let DomNode::Element { tag, children: inner, .. } = &children[0] {
                assert_eq!(tag, "p", "p should remain after both wrappers are spliced");
                assert_eq!(inner.len(), 1, "p should keep its text child");
                assert!(matches!(&inner[0], DomNode::Text(t) if t == "final content"));
            } else {
                panic!("children[0] should be Element");
            }
        } else {
            panic!("root should remain Element");
        }
    }

    // ── pass_keep_alt_cluster ─────────────────────────────────────────






    // ── pass_keep_alt_cluster ─────────────────────────────────────────

    #[test]
    fn test_alt_cluster_three_qualifying() {
        // 3+ qualifying children → alt cluster detected, non-qualifying removed.
        // Root > section (cluster candidate) > [article, div, p, span]
        let mut root = DomNode::Element {
            tag: "div".into(),
            attrs: vec![],
            children: vec![
                DomNode::Element {
                    tag: "section".into(),
                    attrs: vec![],
                    children: vec![
                        DomNode::Element { tag: "article".into(), attrs: vec![], children: vec![],
                            scores: Default::default(),
                            metadata: [("md_rd_subtree_acc_score".into(), "100.0".into())].into() },
                        DomNode::Element { tag: "div".into(), attrs: vec![], children: vec![],
                            scores: Default::default(),
                            metadata: [("md_rd_subtree_acc_score".into(), "90.0".into())].into() },
                        DomNode::Element { tag: "p".into(), attrs: vec![], children: vec![],
                            scores: Default::default(),
                            metadata: [("md_rd_subtree_acc_score".into(), "85.0".into())].into() },
                        DomNode::Element { tag: "span".into(), attrs: vec![], children: vec![],
                            scores: Default::default(),
                            metadata: [("md_rd_subtree_acc_score".into(), "10.0".into())].into() },
                    ],
                    scores: Default::default(),
                    metadata: [("md_rd_subtree_acc_score".into(), "50.0".into())].into(),
                },
            ],
            scores: Default::default(),
            metadata: [("md_rd_subtree_acc_score".into(), "10.0".into())].into(),
        };
        // top_child_score = 100.0, alt_threshold = 100.0 * 0.75 - 1e-9 ≈ 75.0
        // article(100), div(90), p(85) qualify; span(10) does not
        pass_keep_alt_cluster(&mut root);
        if let DomNode::Element { children, .. } = &root {
            assert_eq!(children.len(), 1, "root should still have 1 child (section)");
            if let DomNode::Element { tag, children: inner, .. } = &children[0] {
                assert_eq!(tag, "section", "section should remain");
                assert_eq!(inner.len(), 3, "non-qualifying span should be removed from section");
                let tags: Vec<&str> = inner.iter().filter_map(|c| match c {
                    DomNode::Element { tag, .. } => Some(tag.as_str()),
                    _ => None,
                }).collect();
                assert!(tags.contains(&"article"), "article should be kept");
                assert!(tags.contains(&"div"), "div should be kept");
                assert!(tags.contains(&"p"), "p should be kept");
                assert!(!tags.contains(&"span"), "span should be removed");
            } else {
                panic!("root child should be Element");
            }
        } else {
            panic!("root should remain Element");
        }
    }

    #[test]
    fn test_alt_cluster_two_qualifying() {
        // 2 qualifying children → no alt cluster, all kept.
        let mut root = DomNode::Element {
            tag: "div".into(),
            attrs: vec![],
            children: vec![
                DomNode::Element {
                    tag: "section".into(),
                    attrs: vec![],
                    children: vec![
                        DomNode::Element { tag: "article".into(), attrs: vec![], children: vec![],
                            scores: Default::default(),
                            metadata: [("md_rd_subtree_acc_score".into(), "100.0".into())].into() },
                        DomNode::Element { tag: "div".into(), attrs: vec![], children: vec![],
                            scores: Default::default(),
                            metadata: [("md_rd_subtree_acc_score".into(), "80.0".into())].into() },
                        DomNode::Element { tag: "span".into(), attrs: vec![], children: vec![],
                            scores: Default::default(),
                            metadata: [("md_rd_subtree_acc_score".into(), "10.0".into())].into() },
                    ],
                    scores: Default::default(),
                    metadata: [("md_rd_subtree_acc_score".into(), "40.0".into())].into(),
                },
            ],
            scores: Default::default(),
            metadata: [("md_rd_subtree_acc_score".into(), "10.0".into())].into(),
        };
        // alt_threshold = 100.0 * 0.75 - 1e-9 ≈ 75.0
        // Only article(100) and div(80) qualify → 2 < 3 → no alt cluster
        pass_keep_alt_cluster(&mut root);
        if let DomNode::Element { children, .. } = &root {
            assert_eq!(children.len(), 1, "root should still have 1 child (section)");
            if let DomNode::Element { children: inner, .. } = &children[0] {
                assert_eq!(inner.len(), 3, "all children should remain (no alt cluster)");
            } else {
                panic!("root child should be Element");
            }
        } else {
            panic!("root should remain Element");
        }
    }

    #[test]
    fn test_alt_cluster_body_html_excluded() {
        // body/html children excluded from qualifying count.
        let mut root = DomNode::Element {
            tag: "div".into(),
            attrs: vec![],
            children: vec![
                DomNode::Element {
                    tag: "section".into(),
                    attrs: vec![],
                    children: vec![
                        DomNode::Element { tag: "body".into(), attrs: vec![], children: vec![],
                            scores: Default::default(),
                            metadata: [("md_rd_subtree_acc_score".into(), "100.0".into())].into() },
                        DomNode::Element { tag: "html".into(), attrs: vec![], children: vec![],
                            scores: Default::default(),
                            metadata: [("md_rd_subtree_acc_score".into(), "95.0".into())].into() },
                        DomNode::Element { tag: "div".into(), attrs: vec![], children: vec![],
                            scores: Default::default(),
                            metadata: [("md_rd_subtree_acc_score".into(), "90.0".into())].into() },
                    ],
                    scores: Default::default(),
                    metadata: [("md_rd_subtree_acc_score".into(), "40.0".into())].into(),
                },
            ],
            scores: Default::default(),
            metadata: [("md_rd_subtree_acc_score".into(), "10.0".into())].into(),
        };
        // top non-body/html child score = 90.0, alt_threshold = 90.0 * 0.75 - 1e-9 ≈ 67.5
        // Only div qualifies (body/html excluded) → 1 < 3 → no alt cluster
        pass_keep_alt_cluster(&mut root);
        if let DomNode::Element { children, .. } = &root {
            assert_eq!(children.len(), 1, "root should still have 1 child (section)");
            if let DomNode::Element { children: inner, .. } = &children[0] {
                assert_eq!(inner.len(), 3, "all children should remain (body/html excluded from count)");
            } else {
                panic!("root child should be Element");
            }
        } else {
            panic!("root should remain Element");
        }
    }

    #[test]
    fn test_alt_cluster_no_qualifying() {
        // No qualifying children → no alt cluster.
        let mut root = DomNode::Element {
            tag: "div".into(),
            attrs: vec![],
            children: vec![
                DomNode::Element {
                    tag: "section".into(),
                    attrs: vec![],
                    children: vec![
                        DomNode::Element { tag: "article".into(), attrs: vec![], children: vec![],
                            scores: Default::default(),
                            metadata: [("md_rd_subtree_acc_score".into(), "10.0".into())].into() },
                        DomNode::Element { tag: "span".into(), attrs: vec![], children: vec![],
                            scores: Default::default(),
                            metadata: [("md_rd_subtree_acc_score".into(), "5.0".into())].into() },
                    ],
                    scores: Default::default(),
                    metadata: [("md_rd_subtree_acc_score".into(), "40.0".into())].into(),
                },
            ],
            scores: Default::default(),
            metadata: [("md_rd_subtree_acc_score".into(), "10.0".into())].into(),
        };
        // top_child_score = 10.0, alt_threshold = 10.0 * 0.75 - 1e-9 ≈ 7.5
        // Only article(10.0) qualifies → 1 < 3 → no alt cluster
        pass_keep_alt_cluster(&mut root);
        if let DomNode::Element { children, .. } = &root {
            assert_eq!(children.len(), 1, "root should still have 1 child (section)");
            if let DomNode::Element { children: inner, .. } = &children[0] {
                assert_eq!(inner.len(), 2, "all children should remain (no alt cluster)");
            } else {
                panic!("root child should be Element");
            }
        } else {
            panic!("root should remain Element");
        }
    }

    #[test]
    fn test_alt_cluster_mixed_qualifying() {
        // Alt cluster with mixed qualifying/non-qualifying, plus non-Element children.
        let mut root = DomNode::Element {
            tag: "div".into(),
            attrs: vec![],
            children: vec![
                DomNode::Element {
                    tag: "section".into(),
                    attrs: vec![],
                    children: vec![
                        DomNode::Element { tag: "article".into(), attrs: vec![], children: vec![],
                            scores: Default::default(),
                            metadata: [("md_rd_subtree_acc_score".into(), "100.0".into())].into() },
                        DomNode::Element { tag: "div".into(), attrs: vec![], children: vec![],
                            scores: Default::default(),
                            metadata: [("md_rd_subtree_acc_score".into(), "90.0".into())].into() },
                        DomNode::Element { tag: "p".into(), attrs: vec![], children: vec![],
                            scores: Default::default(),
                            metadata: [("md_rd_subtree_acc_score".into(), "85.0".into())].into() },
                        DomNode::Element { tag: "span".into(), attrs: vec![], children: vec![],
                            scores: Default::default(),
                            metadata: [("md_rd_subtree_acc_score".into(), "10.0".into())].into() },
                        DomNode::Text("some text".into()),
                    ],
                    scores: Default::default(),
                    metadata: [("md_rd_subtree_acc_score".into(), "50.0".into())].into(),
                },
            ],
            scores: Default::default(),
            metadata: [("md_rd_subtree_acc_score".into(), "10.0".into())].into(),
        };
        // alt_threshold = 100.0 * 0.75 - 1e-9 ≈ 75.0
        // article(100), div(90), p(85) qualify; span(10) does not; text node preserved
        pass_keep_alt_cluster(&mut root);
        if let DomNode::Element { children, .. } = &root {
            assert_eq!(children.len(), 1, "root should still have 1 child (section)");
            if let DomNode::Element { tag, children: inner, .. } = &children[0] {
                assert_eq!(tag, "section", "section should remain");
                assert_eq!(inner.len(), 4, "3 qualifying elements + 1 text node should remain");
                let tags: Vec<&str> = inner.iter().filter_map(|c| match c {
                    DomNode::Element { tag, .. } => Some(tag.as_str()),
                    _ => None,
                }).collect();
                assert!(tags.contains(&"article"), "article should be kept");
                assert!(tags.contains(&"div"), "div should be kept");
                assert!(tags.contains(&"p"), "p should be kept");
                assert!(!tags.contains(&"span"), "span should be removed");
                // Verify text node survived
                let has_text = inner.iter().any(|c| matches!(c, DomNode::Text(t) if t == "some text"));
                assert!(has_text, "text node should be preserved");
            } else {
                panic!("root child should be Element");
            }
        } else {
            panic!("root should remain Element");
        }
    }

    // ── pass_promote_content_child ──────────────────────────────────────────

    #[test]
    fn test_promote_content_best_child_promoted() {
        // Multiple children, best non-body/html child promoted, others removed.
        // Structure: root > parent(section) > [article(100.0), div(50.0), span(10.0)]
        // walk_pre_mut visits 'parent' which has 3 children → best (article) promoted.
        let mut root = DomNode::Element {
            tag: "div".into(),
            attrs: vec![],
            children: vec![
                DomNode::Element {
                    tag: "parent".into(),
                    attrs: vec![],
                    children: vec![
                        DomNode::Element {
                            tag: "article".into(),
                            attrs: vec![],
                            children: vec![DomNode::Text("content".into())],
                            scores: Default::default(),
                            metadata: [("md_rd_subtree_acc_score".into(), "100.0".into())].into(),
                        },
                        DomNode::Element {
                            tag: "div".into(),
                            attrs: vec![],
                            children: vec![],
                            scores: Default::default(),
                            metadata: [("md_rd_subtree_acc_score".into(), "50.0".into())].into(),
                        },
                        DomNode::Element {
                            tag: "span".into(),
                            attrs: vec![],
                            children: vec![],
                            scores: Default::default(),
                            metadata: [("md_rd_subtree_acc_score".into(), "10.0".into())].into(),
                        },
                    ],
                    scores: Default::default(),
                    metadata: Default::default(),
                },
            ],
            scores: Default::default(),
            metadata: Default::default(),
        };
        pass_promote_content_child(&mut root);
        if let DomNode::Element { children, .. } = &root {
            assert_eq!(children.len(), 1, "root should have 1 child (parent)");
            if let DomNode::Element { tag, children: inner, .. } = &children[0] {
                assert_eq!(tag, "parent", "parent should remain");
                assert_eq!(inner.len(), 1, "only best child should remain in parent");
                if let DomNode::Element { tag: ct, .. } = &inner[0] {
                    assert_eq!(ct, "article", "article with highest score should be kept");
                } else {
                    panic!("child should be Element");
                }
            } else {
                panic!("root child should be Element");
            }
        } else {
            panic!("root should remain Element");
        }
    }

    #[test]
    fn test_promote_content_single_child_unchanged() {
        // Only one child → unchanged.
        // Structure: root > parent(section) > [article(100.0)]
        let mut root = DomNode::Element {
            tag: "div".into(),
            attrs: vec![],
            children: vec![
                DomNode::Element {
                    tag: "parent".into(),
                    attrs: vec![],
                    children: vec![
                        DomNode::Element {
                            tag: "article".into(),
                            attrs: vec![],
                            children: vec![],
                            scores: Default::default(),
                            metadata: [("md_rd_subtree_acc_score".into(), "100.0".into())].into(),
                        },
                    ],
                    scores: Default::default(),
                    metadata: Default::default(),
                },
            ],
            scores: Default::default(),
            metadata: Default::default(),
        };
        pass_promote_content_child(&mut root);
        if let DomNode::Element { children, .. } = &root {
            assert_eq!(children.len(), 1, "root should have 1 child (parent)");
            if let DomNode::Element { tag, children: inner, .. } = &children[0] {
                assert_eq!(tag, "parent", "parent should remain");
                assert_eq!(inner.len(), 1, "single child in parent should remain unchanged");
            } else {
                panic!("root child should be Element");
            }
        } else {
            panic!("root should remain Element");
        }
    }

    #[test]
    fn test_promote_content_body_html_only_cleared() {
        // Body/html as only Element children → children cleared.
        // Structure: root > parent(section) > [body(200.0), html(300.0)]
        let mut root = DomNode::Element {
            tag: "div".into(),
            attrs: vec![],
            children: vec![
                DomNode::Element {
                    tag: "parent".into(),
                    attrs: vec![],
                    children: vec![
                        DomNode::Element {
                            tag: "body".into(),
                            attrs: vec![],
                            children: vec![],
                            scores: Default::default(),
                            metadata: [("md_rd_subtree_acc_score".into(), "200.0".into())].into(),
                        },
                        DomNode::Element {
                            tag: "html".into(),
                            attrs: vec![],
                            children: vec![],
                            scores: Default::default(),
                            metadata: [("md_rd_subtree_acc_score".into(), "300.0".into())].into(),
                        },
                    ],
                    scores: Default::default(),
                    metadata: Default::default(),
                },
            ],
            scores: Default::default(),
            metadata: Default::default(),
        };
        pass_promote_content_child(&mut root);
        if let DomNode::Element { children, .. } = &root {
            assert_eq!(children.len(), 1, "root should have 1 child (parent)");
            if let DomNode::Element { tag, children: inner, .. } = &children[0] {
                assert_eq!(tag, "parent", "parent should remain");
                assert!(inner.is_empty(), "body/html-only children should be cleared from parent");
            } else {
                panic!("root child should be Element");
            }
        } else {
            panic!("root should remain Element");
        }
    }

    #[test]
    fn test_promote_content_all_zero_score_cleared() {
        // All children score 0.0 → children cleared.
        // Structure: root > parent(section) > [article(0.0), div(0.0)]
        let mut root = DomNode::Element {
            tag: "div".into(),
            attrs: vec![],
            children: vec![
                DomNode::Element {
                    tag: "parent".into(),
                    attrs: vec![],
                    children: vec![
                        DomNode::Element {
                            tag: "article".into(),
                            attrs: vec![],
                            children: vec![],
                            scores: Default::default(),
                            metadata: [("md_rd_subtree_acc_score".into(), "0.0".into())].into(),
                        },
                        DomNode::Element {
                            tag: "div".into(),
                            attrs: vec![],
                            children: vec![],
                            scores: Default::default(),
                            metadata: [("md_rd_subtree_acc_score".into(), "0.0".into())].into(),
                        },
                    ],
                    scores: Default::default(),
                    metadata: Default::default(),
                },
            ],
            scores: Default::default(),
            metadata: Default::default(),
        };
        pass_promote_content_child(&mut root);
        if let DomNode::Element { children, .. } = &root {
            assert_eq!(children.len(), 1, "root should have 1 child (parent)");
            if let DomNode::Element { tag, children: inner, .. } = &children[0] {
                assert_eq!(tag, "parent", "parent should remain");
                assert!(inner.is_empty(), "all-zero-score children should be cleared from parent");
            } else {
                panic!("root child should be Element");
            }
        } else {
            panic!("root should remain Element");
        }
    }

    #[test]
    fn test_promote_content_non_element_children_graceful() {
        // Non-Element children (Text, Comment) → handled gracefully (not promoted).
        // Structure: root > parent(section) > [Text, Comment]
        let mut root = DomNode::Element {
            tag: "div".into(),
            attrs: vec![],
            children: vec![
                DomNode::Element {
                    tag: "parent".into(),
                    attrs: vec![],
                    children: vec![
                        DomNode::Text("some text".into()),
                        DomNode::Comment("a comment".into()),
                    ],
                    scores: Default::default(),
                    metadata: Default::default(),
                },
            ],
            scores: Default::default(),
            metadata: Default::default(),
        };
        pass_promote_content_child(&mut root);
        if let DomNode::Element { children, .. } = &root {
            assert_eq!(children.len(), 1, "root should have 1 child (parent)");
            if let DomNode::Element { tag, children: inner, .. } = &children[0] {
                assert_eq!(tag, "parent", "parent should remain");
                assert!(inner.is_empty(), "only non-Element children should be cleared from parent");
            } else {
                panic!("root child should be Element");
            }
        } else {
            panic!("root should remain Element");
        }
    }

    #[test]
    fn test_promote_content_best_child_is_last() {
        // Best child is the last child → others removed correctly.
        // Structure: root > parent(section) > [span(10.0), div(50.0), article(100.0)]
        let mut root = DomNode::Element {
            tag: "div".into(),
            attrs: vec![],
            children: vec![
                DomNode::Element {
                    tag: "parent".into(),
                    attrs: vec![],
                    children: vec![
                        DomNode::Element {
                            tag: "span".into(),
                            attrs: vec![],
                            children: vec![],
                            scores: Default::default(),
                            metadata: [("md_rd_subtree_acc_score".into(), "10.0".into())].into(),
                        },
                        DomNode::Element {
                            tag: "div".into(),
                            attrs: vec![],
                            children: vec![],
                            scores: Default::default(),
                            metadata: [("md_rd_subtree_acc_score".into(), "50.0".into())].into(),
                        },
                        DomNode::Element {
                            tag: "article".into(),
                            attrs: vec![],
                            children: vec![DomNode::Text("content".into())],
                            scores: Default::default(),
                            metadata: [("md_rd_subtree_acc_score".into(), "100.0".into())].into(),
                        },
                    ],
                    scores: Default::default(),
                    metadata: Default::default(),
                },
            ],
            scores: Default::default(),
            metadata: Default::default(),
        };
        pass_promote_content_child(&mut root);
        if let DomNode::Element { children, .. } = &root {
            assert_eq!(children.len(), 1, "root should have 1 child (parent)");
            if let DomNode::Element { tag, children: inner, .. } = &children[0] {
                assert_eq!(tag, "parent", "parent should remain");
                assert_eq!(inner.len(), 1, "only best child should remain in parent");
                if let DomNode::Element { tag: ct, .. } = &inner[0] {
                    assert_eq!(ct, "article", "article (last child) with highest score should be kept");
                } else {
                    panic!("child should be Element");
                }
            } else {
                panic!("root child should be Element");
            }
        } else {
            panic!("root should remain Element");
        }
    }

    #[test]
    fn test_promote_content_mixed_body_html_and_content() {
        // Body/html children exist alongside content children.
        // Body/html are excluded from selection, so content child wins.
        // Structure: root > parent(section) > [body(200.0), html(300.0), article(100.0)]
        let mut root = DomNode::Element {
            tag: "div".into(),
            attrs: vec![],
            children: vec![
                DomNode::Element {
                    tag: "parent".into(),
                    attrs: vec![],
                    children: vec![
                        DomNode::Element {
                            tag: "body".into(),
                            attrs: vec![],
                            children: vec![],
                            scores: Default::default(),
                            metadata: [("md_rd_subtree_acc_score".into(), "200.0".into())].into(),
                        },
                        DomNode::Element {
                            tag: "html".into(),
                            attrs: vec![],
                            children: vec![],
                            scores: Default::default(),
                            metadata: [("md_rd_subtree_acc_score".into(), "300.0".into())].into(),
                        },
                        DomNode::Element {
                            tag: "article".into(),
                            attrs: vec![],
                            children: vec![DomNode::Text("content".into())],
                            scores: Default::default(),
                            metadata: [("md_rd_subtree_acc_score".into(), "100.0".into())].into(),
                        },
                    ],
                    scores: Default::default(),
                    metadata: Default::default(),
                },
            ],
            scores: Default::default(),
            metadata: Default::default(),
        };
        pass_promote_content_child(&mut root);
        if let DomNode::Element { children, .. } = &root {
            assert_eq!(children.len(), 1, "root should have 1 child (parent)");
            if let DomNode::Element { tag, children: inner, .. } = &children[0] {
                assert_eq!(tag, "parent", "parent should remain");
                assert_eq!(inner.len(), 1, "only best content child should remain in parent");
                if let DomNode::Element { tag: ct, .. } = &inner[0] {
                    assert_eq!(ct, "article", "article should be selected over body/html");
                } else {
                    panic!("child should be Element");
                }
            } else {
                panic!("root child should be Element");
            }
        } else {
            panic!("root should remain Element");
        }
    }

    #[test]
    fn test_promote_content_non_element_root() {
        // Non-Element root (Text node) should be silently skipped.
        let mut node = DomNode::Text("hello".into());
        pass_promote_content_child(&mut node);
        assert!(matches!(&node, DomNode::Text(t) if t == "hello"),
            "non-Element root should be unchanged");
    }

    #[test]
    fn test_promote_content_no_children() {
        // Element with no children → unchanged.
        let mut root = DomNode::Element {
            tag: "div".into(),
            attrs: vec![],
            children: vec![],
            scores: Default::default(),
            metadata: Default::default(),
        };
        pass_promote_content_child(&mut root);
        if let DomNode::Element { children, .. } = &root {
            assert!(children.is_empty(), "empty children should remain empty");
        } else {
            panic!("root should remain Element");
        }
    }

}
