use crate::pipelines::DomNode;
use crate::pipelines::passes::rd_utils;
use crate::pipelines::walkers::{WalkerAction, WalkerFilter, walk_post_acc_mut, walk_post_mut};

struct ScoreAccumulator {
    subtree_max: f64,
    content: f64,
    text_content: f64,
    total_para: f64,
    text_total_para: f64,
    para_depth: usize,
}

// Default for non-Element nodes: no scoring data, para_depth sentinel means "no paragraph in subtree"
impl Default for ScoreAccumulator {
    fn default() -> Self {
        Self {
            subtree_max: 0.0,
            content: 0.0,
            text_content: 0.0,
            total_para: 0.0,
            text_total_para: 0.0,
            para_depth: usize::MAX,
        }
    }
}

impl ScoreAccumulator {
    /// Construct accumulator from already-processed child accumulators.
    ///
    /// # Pre
    /// - `children` accumulators are from already-processed child nodes (post-order guarantee from walker).
    ///
    /// # Post
    /// - `subtree_max` is set to 0.0 (placeholder).
    ///   Call `apply_link_density` after `add_self` to set the correct values.
    /// - All other fields are fully computed from children.
    ///
    /// # Panic-if
    /// - Never (infallible).
    fn from_children(children: &[ScoreAccumulator]) -> Self {
        let mut content = 0.0;
        let mut text_content = 0.0;
        let mut total_para = 0.0;
        let mut text_total_para = 0.0;
        let mut min_para_depth = usize::MAX;

        for child in children {
            if child.para_depth != usize::MAX {
                let divider = match child.para_depth + 1 {
                    1 => 1.0,
                    2 => 2.0,
                    n => (n - 1) as f64 * 3.0,
                };
                content += child.total_para / divider;
                text_content += child.text_total_para / divider;
                min_para_depth = min_para_depth.min(child.para_depth + 1);
            }
            total_para += child.total_para;
            text_total_para += child.text_total_para;
        }

        Self {
            subtree_max: 0.0,
            content,
            text_content,
            total_para,
            text_total_para,
            para_depth: min_para_depth,
        }
    }

    /// Add self's tag bonus, class weight, and paragraph score to accumulator.
    ///
    /// # Pre
    /// - `node` is `DomNode::Element` (guaranteed by caller's match arm).
    ///
    /// # Post
    /// - `self` now includes tag_bonus + class_weight + paragraph_score.
    /// - `para_depth` is set to 0 if this node is a scored paragraph.
    ///
    /// # Panic-if
    /// - `node` is not `DomNode::Element` (destructure panic).
    fn add_self(&mut self, node: &mut DomNode, no_class_weights: bool) {
        let DomNode::Element {
            tag,
            attrs,
            children,
            ..
        } = node
        else {
            unreachable!("add_self called on non-Element node");
        };

        // ── 1. Tag bonus and class weight ──
        let tag_bonus: f64 = match tag.as_str() {
            "div" => 5.0,
            "pre" | "td" | "blockquote" => 3.0,
            "address" | "ol" | "ul" | "dl" | "dd" | "dt" | "li" | "form" => -3.0,
            "h1" | "h2" | "h3" | "h4" | "h5" | "h6" | "th" => -5.0,
            _ => 0.0,
        };
        let class_weight = if no_class_weights {
            0.0
        } else {
            rd_utils::get_class_weight(attrs) as f64
        };
        let ancestor_bonus = tag_bonus + class_weight;

        // ── 2. Paragraph score (only for p/td/pre/blockquote) ──
        let para_score = if matches!(tag.as_str(), "p" | "td" | "pre" | "blockquote") {
            // Inlined: collect_text_nodes was called exactly once.
            fn collect_text(nodes: &[DomNode]) -> String {
                let mut buf = String::new();
                for node in nodes {
                    match node {
                        DomNode::Text(t) => buf.push_str(t),
                        DomNode::Element { children, .. } => buf.push_str(&collect_text(children)),
                        _ => {}
                    }
                }
                buf
            }
            let text = collect_text(children);
            let text_len = text.len();

            // 25-char minimum (matches JS Readability)
            if text_len < 25 {
                0.0
            } else {
                let comma_count = text.chars().filter(|&c| c == ',').count() as f64;
                // JS: split().length — N commas → N+1
                let length_bonus = ((text_len / 100) as f64).min(3.0);
                1.0 + (comma_count + 1.0) + length_bonus
            }
        } else {
            0.0
        };

        // ── 3. Accumulate our own content ──
        self.content += ancestor_bonus + para_score;
        self.text_content += ancestor_bonus + para_score;
        self.total_para += para_score;
        self.text_total_para += para_score;
        if para_score > 0.0 {
            self.para_depth = 0;
        }
    }

    /// Apply link density penalty and write final scores to node metadata.
    ///
    /// # Pre
    /// - `node` is `DomNode::Element` (guaranteed by caller).
    /// - `children_subtree_max` is the max of child accumulators' `subtree_max` values.
    /// - `metadata["link_density"]` should exist (if missing, silently uses 0.0).
    ///
    /// # Post
    /// - `node.scores["mozilla_readability"]` is set to `final_score`.
    /// - `node.metadata["md_rd_subtree_acc_score"]` is set to `final_score.to_string()`.
    /// - `node.metadata["md_rd_subtree_max_score"]` is set to `subtree_max.to_string()`.
    /// - `self.subtree_max` is finalized.
    ///
    /// # Panic-if
    /// - Never (infallible). Missing/unparsable link_density silently defaults to 0.0.
    fn apply_link_density(&mut self, node: &mut DomNode, children_subtree_max: f64) {
        let DomNode::Element {
            metadata, scores, ..
        } = node
        else {
            unreachable!("apply_link_density called on non-Element node");
        };

        let link_density = metadata
            .get("link_density")
            .and_then(|s| rd_utils::meta_parse_f64(s))
            .unwrap_or(0.0);

        let final_score = (self.content * (1.0 - link_density)).max(0.0);

        self.subtree_max = final_score.max(children_subtree_max);

        scores.insert("mozilla_readability".to_string(), final_score);
        metadata.insert(
            "md_rd_subtree_acc_score".to_string(),
            final_score.to_string(),
        );
        metadata.insert(
            "md_rd_subtree_max_score".to_string(),
            self.subtree_max.to_string(),
        );
    }
}

// ---------------------------------------------------------------------------
// 3. analyze_is_data_table
// ---------------------------------------------------------------------------

/// Walk the DOM tree in post-order and mark data tables by structural heuristics.
///
/// Sets `metadata["is_data_table"] = "true"` on `<table>` elements that match
/// any of the 7 data-table criteria. Layout indicators (role=presentation,
/// datatable=0) are checked first; data indicators (summary, caption, colgroup/col,
/// thead/tfoot, >2 rows + >1 col) are checked second.
///
/// Uses `walk_post_mut` for O(n) post-order traversal.
///
/// # Pre
/// `node` is any valid `DomNode`. No prior analysis passes are required.
///
/// # Post
/// - Data table `<table>` elements have `metadata["is_data_table"] = "true"`.
/// - Layout `<table>` elements and non-`<table>` elements have no `is_data_table` metadata set.
/// - The tree structure, all other metadata, and all scores are unchanged.
///
/// # Panic-if
/// Never. All operations are infallible.
pub fn mark_data_tables_by_structure(node: &mut DomNode) {
    let mut filter = |n: &mut DomNode| -> WalkerAction {
        if let DomNode::Element {
            tag,
            attrs,
            children,
            metadata,
            ..
        } = n
            && tag == "table"
            && !is_layout_table(attrs)
            && is_data_table_by_structure(attrs, children)
        {
            metadata.insert("is_data_table".into(), "true".into());
        }
        WalkerAction::Continue
    };
    let mut filters: Vec<&mut WalkerFilter> = vec![&mut filter];
    walk_post_mut(node, &mut filters, None);
    // walk_post_mut processes children only — apply filter to root node as well.
    filter(node);
}

/// Check if a `<table>` element is a layout table based on its attributes.
///
/// A table is layout if it has `role="presentation"` or `datatable="0"`.
/// These are the ONLY two layout checks. No nested-`<table>` check.
///
/// # Pre
///
/// `attrs` is the `attrs` field of a `<table>` `DomNode::Element`.
///
/// # Post
///
/// Returns `true` if the table is a layout table (no `is_data_table` metadata
/// should be set). Returns `false` otherwise.
///
/// # Panic-if
///
/// Never.
fn is_layout_table(attrs: &[(String, String)]) -> bool {
    // 1. role=presentation → layout table
    if attrs
        .iter()
        .any(|(k, v)| k == "role" && v == "presentation")
    {
        return true;
    }
    // 2. datatable=0 → layout table
    if attrs.iter().any(|(k, v)| k == "datatable" && v == "0") {
        return true;
    }
    false
}

/// Collect `<tr>` elements from direct children and from inside
/// `<tbody>`, `<thead>`, `<tfoot>` wrappers (one level deep only).
fn collect_tr_nodes(nodes: &[DomNode]) -> Vec<&DomNode> {
    let mut result = Vec::new();
    for node in nodes {
        match node {
            DomNode::Element { tag, children, .. } if tag == "tr" => {
                result.push(node);
            }
            DomNode::Element { tag, children, .. }
                if tag == "tbody" || tag == "thead" || tag == "tfoot" =>
            {
                for child in children {
                    if let DomNode::Element { tag: ct, .. } = child
                        && ct == "tr"
                    {
                        result.push(child);
                    }
                }
            }
            _ => {}
        }
    }
    result
}

/// Check if a `<table>` element is a data table based on its structure.
///
/// A table is a data table if it has any of: `summary` attribute, `<caption>`
/// direct child, `<colgroup>/<col>` direct child, `<thead>/<tfoot>` direct child,
/// or >2 `<tr>` direct children with >1 `<td>/<th>` max per row.
///
/// # Pre
///
/// `attrs` is the `attrs` field of a `<table>` `DomNode::Element`.
/// `children` is the `children` field of the same `<table>` `DomNode::Element`.
/// Only direct children are inspected — no recursive descendant search.
///
/// # Post
///
/// Returns `true` if the table matches any data-table criterion.
/// Returns `false` if it matches no data-table criterion.
///
/// # Panic-if
///
/// Never.
///
/// # Notes
///
/// - Only `<td>` and `<th>` are counted as cells. Non-standard cell tags
///   (e.g., `<custom-cell>`) are NOT counted.
/// - Only direct children of `<tr>` are counted as columns. Nested `<table>`
///   elements inside a `<tr>` may contribute their `<td>`/`<th>` children to
///   the column count. This matches the original behavior.
/// - The function is O(n) on the whole tree. No subtree re-traversal.
fn is_data_table_by_structure(attrs: &[(String, String)], children: &[DomNode]) -> bool {
    // 3. Has summary → data table
    if attrs.iter().any(|(k, _)| k == "summary") {
        return true;
    }
    // 4. Has caption → data table
    if children
        .iter()
        .any(|c| matches!(c, DomNode::Element { tag, .. } if tag == "caption"))
    {
        return true;
    }
    // 5. Has colgroup/col → data table
    if children
        .iter()
        .any(|c| matches!(c, DomNode::Element { tag, .. } if tag == "colgroup" || tag == "col"))
    {
        return true;
    }
    // 6. Has thead/tfoot → data table
    if children
        .iter()
        .any(|c| matches!(c, DomNode::Element { tag, .. } if tag == "thead" || tag == "tfoot"))
    {
        return true;
    }
    // 7. >2 rows AND >1 col → data table (heuristic)
    // Collect <tr> from direct children AND from inside <tbody>/<thead>/<tfoot> wrappers
    let rows = collect_tr_nodes(children);
    if rows.len() > 2 {
        let max_cols = rows
            .iter()
            .map(|row| match row {
                DomNode::Element { children, .. } => children
                    .iter()
                    .filter(
                        |c| matches!(c, DomNode::Element { tag, .. } if tag == "td" || tag == "th"),
                    )
                    .count(),
                _ => 0,
            })
            .max()
            // rows.len() > 2 (enclosing check), so max() is always Some.
            // unwrap_or(0) is a defensive fallback that matches the original
            // code's pattern and handles the edge case of rows with zero cells.
            .unwrap_or(0);
        if max_cols > 1 {
            return true;
        }
    }
    false
}

// =============================================================
// Mozilla Readability Scoring Functions (walker-based post-order accumulation)
// =============================================================

/// Score a DOM tree using the Mozilla Readability scoring algorithm.
///
/// Uses `walk_post_acc_mut` for post-order traversal with `ScoreAccumulator`.
/// Each element node is scored by: (1) accumulating child scores via `from_children`,
/// (2) adding self's tag bonus, class weight, and paragraph score via `add_self`,
/// (3) applying link density penalty via `apply_link_density`.
/// The root node (not part of children) is scored as the final step.
///
/// Pre: `PassRegistry::run_all()` has been called (metadata["link_density"]
///      is populated on each node). DOM tree is fully parsed and normalized.
/// Post: Every Element node has scores["mozilla_readability"] and
///       metadata["md_rd_subtree_acc_score"] set (accumulated content score).
///       Also sets metadata["md_rd_subtree_max_score"] (single-element peak).
/// Panic-if: Never panics. Missing or unparsable `link_density` metadata
///            silently defaults to 0.0 (defensive fallback).
pub fn rd_score_mozilla_readability(node: &mut DomNode) {
    let DomNode::Element { children, .. } = node else {
        return;
    };
    let child_accs = walk_post_acc_mut::<ScoreAccumulator>(
        children,
        None,
        &mut |n: &mut DomNode, child_accs: &[ScoreAccumulator]| {
            if !matches!(n, DomNode::Element { .. }) {
                return (WalkerAction::Continue, ScoreAccumulator::default());
            }
            let mut acc = ScoreAccumulator::from_children(child_accs);
            acc.add_self(n, false);
            let children_subtree_max = child_accs.iter().map(|c| c.subtree_max).fold(0.0, f64::max);
            acc.apply_link_density(n, children_subtree_max);
            (WalkerAction::Continue, acc)
        },
    );
    // Score the root node itself (it's not in children)
    let mut root_acc = ScoreAccumulator::from_children(&child_accs);
    root_acc.add_self(node, false);
    let children_subtree_max = child_accs.iter().map(|c| c.subtree_max).fold(0.0, f64::max);
    root_acc.apply_link_density(node, children_subtree_max);
}

/// Score using tag bonus only (no class/ID weights).
///
/// Uses `walk_post_acc_mut` for post-order traversal with `ScoreAccumulator`.
/// Identical to `rd_score_mozilla_readability` but passes `no_class_weights: true`
/// to `add_self` so class/ID weight bonuses are ignored.
///
/// Pre: `PassRegistry::run_all()` has been called (metadata["link_density"]
///      is populated on each node). DOM tree is fully parsed and normalized.
/// Post: Every Element node has scores["mozilla_readability"] and
///       metadata["md_rd_subtree_acc_score"] set.
///       Also sets metadata["md_rd_subtree_max_score"].
/// Panic-if: Never panics. Missing or unparsable `link_density` metadata
///            silently defaults to 0.0 (defensive fallback).
pub fn rd_score_mozilla_readability_no_class_weights(node: &mut DomNode) {
    let DomNode::Element { children, .. } = node else {
        return;
    };
    let child_accs = walk_post_acc_mut::<ScoreAccumulator>(
        children,
        None,
        &mut |n: &mut DomNode, child_accs: &[ScoreAccumulator]| {
            if !matches!(n, DomNode::Element { .. }) {
                return (WalkerAction::Continue, ScoreAccumulator::default());
            }
            let mut acc = ScoreAccumulator::from_children(child_accs);
            acc.add_self(n, true);
            let children_subtree_max = child_accs.iter().map(|c| c.subtree_max).fold(0.0, f64::max);
            acc.apply_link_density(n, children_subtree_max);
            (WalkerAction::Continue, acc)
        },
    );
    // Score the root node itself (it's not in children)
    let mut root_acc = ScoreAccumulator::from_children(&child_accs);
    root_acc.add_self(node, true);
    let children_subtree_max = child_accs.iter().map(|c| c.subtree_max).fold(0.0, f64::max);
    root_acc.apply_link_density(node, children_subtree_max);
}

/// Mark tables inside <figure> elements as data tables.
/// (a `<table>` inside a `<figure>` is almost always a data table,
/// not a layout table).
pub(crate) fn mark_data_tables_inside_figures(node: &mut DomNode) {
    fn scan(node: &mut DomNode, parent_is_figure: bool) {
        if let DomNode::Element {
            tag,
            children,
            metadata,
            ..
        } = node
        {
            let is_figure = tag == "figure";
            if parent_is_figure && tag == "table" {
                metadata.insert("is_data_table".to_string(), "true".to_string());
            }
            for child in children.iter_mut() {
                scan(child, is_figure);
            }
        }
    }
    scan(node, false);
}

#[cfg(test)]
#[path = "rd_analysis_test.rs"]
mod tests;
