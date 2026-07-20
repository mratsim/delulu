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
        {
            if tag == "table"
                && !is_layout_table(attrs)
                && is_data_table_by_structure(attrs, children)
            {
                metadata.insert("is_data_table".into(), "true".into());
            }
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
mod tests {
    use super::*;
    use crate::pipelines::parse_html;

    /// Weight multiplier for hash links (fragment identifiers like `#section`).
    /// Used in `total_link_len` test helper.
    const HASH_LINK_WEIGHT: f64 = 0.3;

    /// Helper: extract md_rd_subtree_acc_score for a specific tag from the DOM tree.
    fn get_acc(node: &DomNode, target_tag: &str) -> Option<f64> {
        match node {
            DomNode::Element {
                tag,
                metadata,
                children,
                ..
            } => {
                if tag == target_tag
                    && let Some(val) = metadata.get("md_rd_subtree_acc_score")
                    && let Ok(v) = val.parse::<f64>()
                {
                    return Some(v);
                }
                for child in children {
                    if let Some(v) = get_acc(child, target_tag) {
                        return Some(v);
                    }
                }
                None
            }
            _ => None,
        }
    }

    #[test]
    fn test_distance_division_simple_three_level() {
        let html =
            "<body><div><p>a paragraph with at least twenty five characters here</p></div></body>";
        let mut root = parse_html(html).expect("valid HTML");
        rd_score_mozilla_readability(&mut root);

        let p_acc = get_acc(&root, "p").expect("p should have acc");
        let div_acc = get_acc(&root, "div").expect("div should have acc");
        let body_acc = get_acc(&root, "body").expect("body should have acc");

        // With the new scoring (REQ-P0-001/002/003):
        // div receives: ancestor_bonus(5.0) + p.para_score(2.0) / 1.0 = 7.0
        // body receives: p.para_score(2.0) / 2.0 = 1.0
        // div/p = 7.0 / 2.0 = 3.5
        // body/p = 1.0 / 2.0 = 0.5
        assert!(
            (div_acc / p_acc - 3.5).abs() < 1e-6,
            "div/p ratio should be ≈3.5, got {}",
            div_acc / p_acc
        );
        assert!(
            (body_acc / p_acc - 0.5).abs() < 1e-6,
            "body/p ratio should be ≈0.5, got {}",
            body_acc / p_acc
        );
    }

    #[test]
    fn test_distance_division_four_level() {
        let html = "<body><div><section><p>a paragraph with at least twenty five characters here</p></section></div></body>";
        let mut root = parse_html(html).expect("valid HTML");
        rd_score_mozilla_readability(&mut root);

        let p_acc = get_acc(&root, "p").expect("p should have acc");
        let section_acc = get_acc(&root, "section").expect("section should have acc");
        let div_acc = get_acc(&root, "div").expect("div should have acc");
        let body_acc = get_acc(&root, "body").expect("body should have acc");

        // With the new scoring (REQ-P0-001/002/003):
        // section (parent of p, level 0): gets p.para_score/1 = 2.0/1 = 2.0
        // div (grandparent of p, level 1): gets ancestor_bonus(5) + p.para_score/2
        //   = 5.0 + 2.0/2.0 = 6.0
        // body (great-grandparent of p, level 2): gets p.para_score/6 = 2.0/6.0 = 0.333
        // section/p = 1.0
        // div/p = 3.0
        // body/p = 1/6 ≈ 0.1667
        assert!(
            (section_acc / p_acc - 1.0).abs() < 1e-6,
            "section/p ratio should be ≈1.0, got {}",
            section_acc / p_acc
        );
        assert!(
            (div_acc / p_acc - 3.0).abs() < 1e-6,
            "div/p ratio should be ≈3.0, got {}",
            div_acc / p_acc
        );
        assert!(
            (body_acc / p_acc - 1.0 / 6.0).abs() < 1e-6,
            "body/p ratio should be ≈0.1667, got {}",
            body_acc / p_acc
        );
    }

    /// Compute link density by traversing the DOM tree directly (no pre-computed metadata).
    fn compute_link_density_for_test(node: &DomNode) -> String {
        fn total_text_len(nodes: &[DomNode]) -> usize {
            nodes
                .iter()
                .map(|c| match c {
                    DomNode::Text(t) => t.len(),
                    DomNode::Element { children, .. } => total_text_len(children),
                    _ => 0,
                })
                .sum()
        }
        fn total_link_len(nodes: &[DomNode]) -> f64 {
            nodes
                .iter()
                .map(|c| match c {
                    DomNode::Element {
                        tag,
                        attrs,
                        children,
                        ..
                    } if tag == "a" => {
                        let raw_len = total_text_len(children) as f64;
                        let is_hash = attrs.iter().any(|(k, v)| k == "href" && v.starts_with('#'));
                        if is_hash {
                            raw_len * HASH_LINK_WEIGHT
                        } else {
                            raw_len
                        }
                    }
                    DomNode::Element { children, .. } => total_link_len(children),
                    _ => 0.0,
                })
                .sum()
        }
        match node {
            DomNode::Element { children, .. } => {
                let total_len: usize = total_text_len(children);
                if total_len == 0 {
                    return "0.0".into();
                }
                let link_len = total_link_len(children);
                let density = link_len / total_len as f64;
                format!("{:.6}", density)
            }
            _ => "0.0".into(),
        }
    }

    #[test]
    fn test_analyze_link_density_hash_link_coefficient() {
        // A div with one normal link and one hash link
        let div = DomNode::Element {
            tag: "div".into(),
            attrs: vec![],
            children: vec![
                DomNode::Text("click ".into()),
                DomNode::Element {
                    tag: "a".into(),
                    attrs: vec![("href".into(), "/real".into())],
                    children: vec![DomNode::Text("here".into())],
                    scores: Default::default(),
                    metadata: Default::default(),
                },
                DomNode::Text(" ".into()),
                DomNode::Element {
                    tag: "a".into(),
                    attrs: vec![("href".into(), "#section".into())],
                    children: vec![DomNode::Text("nav".into())],
                    scores: Default::default(),
                    metadata: Default::default(),
                },
                DomNode::Text(" link".into()),
            ],
            scores: Default::default(),
            metadata: Default::default(),
        };
        let result = compute_link_density_for_test(&div);
        let density: f64 = result.parse().unwrap();
        // Expected: (4 + 3 * HASH_LINK_WEIGHT) / 19 = 4.9 / 19 ≈ 0.257895
        let expected = (4.0 + 3.0 * HASH_LINK_WEIGHT) / 19.0;
        assert!(
            (density - expected).abs() < 1e-6,
            "hash-link coefficient: {density} vs {expected}"
        );
    }

    #[test]
    fn test_analyze_link_density_no_hash_link() {
        // Without hash links, the coefficient should not affect normal links
        let div = DomNode::Element {
            tag: "div".into(),
            attrs: vec![],
            children: vec![
                DomNode::Text("before ".into()),
                DomNode::Element {
                    tag: "a".into(),
                    attrs: vec![("href".into(), "/real".into())],
                    children: vec![DomNode::Text("click".into())],
                    scores: Default::default(),
                    metadata: Default::default(),
                },
                DomNode::Text(" after".into()),
            ],
            scores: Default::default(),
            metadata: Default::default(),
        };
        let result = compute_link_density_for_test(&div);
        let density: f64 = result.parse().unwrap();
        // Expected: 5 / 18 = 0.277778 (total text is "before click after" = 18 chars)
        let expected = 5.0_f64 / 18.0_f64;
        assert!(
            (density - expected).abs() < 1e-6,
            "normal link density: {density} vs {expected}"
        );
    }
    // ===== Data table detection tests =====

    /// Helper: create a simple table element with given attrs and children.
    fn make_table(attrs: Vec<(&str, &str)>, children: Vec<DomNode>) -> DomNode {
        DomNode::Element {
            tag: "table".into(),
            attrs: attrs
                .into_iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
            children,
            scores: Default::default(),
            metadata: Default::default(),
        }
    }

    /// Helper: create a simple non-table element.
    fn make_div(children: Vec<DomNode>) -> DomNode {
        DomNode::Element {
            tag: "div".into(),
            attrs: vec![],
            children,
            scores: Default::default(),
            metadata: Default::default(),
        }
    }

    /// Helper: create a <tr> with given cell tags.
    fn make_row(cell_tags: &[&str]) -> DomNode {
        let cells: Vec<DomNode> = cell_tags
            .iter()
            .map(|&tag| DomNode::Element {
                tag: tag.into(),
                attrs: vec![],
                children: vec![DomNode::Text("cell".into())],
                scores: Default::default(),
                metadata: Default::default(),
            })
            .collect();
        DomNode::Element {
            tag: "tr".into(),
            attrs: vec![],
            children: cells,
            scores: Default::default(),
            metadata: Default::default(),
        }
    }

    /// Helper: check if a table node has is_data_table metadata.
    fn has_is_data_table(node: &DomNode) -> bool {
        match node {
            DomNode::Element { metadata, .. } => {
                metadata.get("is_data_table").map(|s| s.as_str()) == Some("true")
            }
            _ => false,
        }
    }

    #[test]
    fn test_dt_rule1_role_presentation_is_layout() {
        // role=presentation → layout, no is_data_table set
        let mut table = make_table(vec![("role", "presentation")], vec![]);
        mark_data_tables_by_structure(&mut table);
        assert!(
            !has_is_data_table(&table),
            "role=presentation should NOT be marked as data table"
        );
    }

    #[test]
    fn test_dt_rule2_datatable_0_is_layout() {
        // datatable=0 → layout, no is_data_table set
        let mut table = make_table(vec![("datatable", "0")], vec![]);
        mark_data_tables_by_structure(&mut table);
        assert!(
            !has_is_data_table(&table),
            "datatable=0 should NOT be marked as data table"
        );
    }

    #[test]
    fn test_dt_rule3_summary_is_data_table() {
        // Has summary attr → data table
        let mut table = make_table(vec![("summary", "prices")], vec![]);
        mark_data_tables_by_structure(&mut table);
        assert!(
            has_is_data_table(&table),
            "summary attr should be marked as data table"
        );
    }

    #[test]
    fn test_dt_rule4_caption_is_data_table() {
        // Has <caption> child → data table
        let caption = DomNode::Element {
            tag: "caption".into(),
            attrs: vec![],
            children: vec![DomNode::Text("Prices".into())],
            scores: Default::default(),
            metadata: Default::default(),
        };
        let mut table = make_table(vec![], vec![caption]);
        mark_data_tables_by_structure(&mut table);
        assert!(
            has_is_data_table(&table),
            "<caption> should be marked as data table"
        );
    }

    #[test]
    fn test_dt_rule5_colgroup_is_data_table() {
        // Has <colgroup> child → data table
        let colgroup = DomNode::Element {
            tag: "colgroup".into(),
            attrs: vec![],
            children: vec![],
            scores: Default::default(),
            metadata: Default::default(),
        };
        let mut table = make_table(vec![], vec![colgroup]);
        mark_data_tables_by_structure(&mut table);
        assert!(
            has_is_data_table(&table),
            "<colgroup> should be marked as data table"
        );
    }

    #[test]
    fn test_dt_rule5_col_is_data_table() {
        // Has <col> child → data table
        let col = DomNode::Element {
            tag: "col".into(),
            attrs: vec![],
            children: vec![],
            scores: Default::default(),
            metadata: Default::default(),
        };
        let mut table = make_table(vec![], vec![col]);
        mark_data_tables_by_structure(&mut table);
        assert!(
            has_is_data_table(&table),
            "<col> should be marked as data table"
        );
    }

    #[test]
    fn test_dt_rule6_thead_is_data_table() {
        // Has <thead> child → data table
        let thead = DomNode::Element {
            tag: "thead".into(),
            attrs: vec![],
            children: vec![],
            scores: Default::default(),
            metadata: Default::default(),
        };
        let mut table = make_table(vec![], vec![thead]);
        mark_data_tables_by_structure(&mut table);
        assert!(
            has_is_data_table(&table),
            "<thead> should be marked as data table"
        );
    }

    #[test]
    fn test_dt_rule6_tfoot_is_data_table() {
        // Has <tfoot> child → data table
        let tfoot = DomNode::Element {
            tag: "tfoot".into(),
            attrs: vec![],
            children: vec![],
            scores: Default::default(),
            metadata: Default::default(),
        };
        let mut table = make_table(vec![], vec![tfoot]);
        mark_data_tables_by_structure(&mut table);
        assert!(
            has_is_data_table(&table),
            "<tfoot> should be marked as data table"
        );
    }

    #[test]
    fn test_dt_rule7_three_rows_two_cols_is_data_table() {
        // >2 rows AND >1 col → data table
        let rows = vec![
            make_row(&["td", "td"]),
            make_row(&["td", "td"]),
            make_row(&["td", "td"]),
        ];
        let mut table = make_table(vec![], rows);
        mark_data_tables_by_structure(&mut table);
        assert!(
            has_is_data_table(&table),
            ">2 rows with >1 col should be marked as data table"
        );
    }

    #[test]
    fn test_dt_rule7_three_rows_one_col_is_not_data_table() {
        // >2 rows but only 1 col → NOT data table
        let rows = vec![make_row(&["td"]), make_row(&["td"]), make_row(&["td"])];
        let mut table = make_table(vec![], rows);
        mark_data_tables_by_structure(&mut table);
        assert!(
            !has_is_data_table(&table),
            ">2 rows with 1 col should NOT be marked as data table"
        );
    }

    #[test]
    fn test_dt_rule7_two_rows_two_cols_is_not_data_table() {
        // 2 rows with 2 cols → NOT data table (needs >2 rows)
        let rows = vec![make_row(&["td", "td"]), make_row(&["td", "td"])];
        let mut table = make_table(vec![], rows);
        mark_data_tables_by_structure(&mut table);
        assert!(
            !has_is_data_table(&table),
            "2 rows with 2 cols should NOT be marked as data table"
        );
    }
    #[test]
    fn test_dt_rule7_tbody_wrapped_rows() {
        // 3 rows, 2 cols wrapped in <tbody> → should be marked as data table
        let rows = vec![
            make_row(&["td", "td"]),
            make_row(&["td", "td"]),
            make_row(&["td", "td"]),
        ];
        let tbody = DomNode::Element {
            tag: "tbody".into(),
            attrs: vec![],
            children: rows,
            scores: Default::default(),
            metadata: Default::default(),
        };
        let mut table = make_table(vec![], vec![tbody]);
        mark_data_tables_by_structure(&mut table);
        assert!(
            has_is_data_table(&table),
            "3 tbody-wrapped rows with 2 cols should be marked as data table"
        );
    }

    #[test]
    fn test_dt_rule7_tbody_three_rows_one_col_is_not_data_table() {
        // 3 rows, 1 col wrapped in <tbody> → NOT data table
        let rows = vec![make_row(&["td"]), make_row(&["td"]), make_row(&["td"])];
        let tbody = DomNode::Element {
            tag: "tbody".into(),
            attrs: vec![],
            children: rows,
            scores: Default::default(),
            metadata: Default::default(),
        };
        let mut table = make_table(vec![], vec![tbody]);
        mark_data_tables_by_structure(&mut table);
        assert!(
            !has_is_data_table(&table),
            "3 tbody-wrapped rows with 1 col should NOT be marked as data table"
        );
    }

    #[test]
    fn test_dt_rule7_tbody_two_rows_two_cols_is_not_data_table() {
        // 2 rows, 2 cols wrapped in <tbody> → NOT data table (needs >2 rows)
        let rows = vec![make_row(&["td", "td"]), make_row(&["td", "td"])];
        let tbody = DomNode::Element {
            tag: "tbody".into(),
            attrs: vec![],
            children: rows,
            scores: Default::default(),
            metadata: Default::default(),
        };
        let mut table = make_table(vec![], vec![tbody]);
        mark_data_tables_by_structure(&mut table);
        assert!(
            !has_is_data_table(&table),
            "2 tbody-wrapped rows with 2 cols should NOT be marked as data table"
        );
    }

    #[test]
    fn test_dt_layout_wins_over_data() {
        // role=presentation + summary → layout wins (no is_data_table)
        let mut table = make_table(
            vec![("role", "presentation"), ("summary", "prices")],
            vec![],
        );
        mark_data_tables_by_structure(&mut table);
        assert!(
            !has_is_data_table(&table),
            "layout check should win over data check"
        );
    }

    #[test]
    fn test_dt_empty_table_not_data_table() {
        // Empty table → no is_data_table set
        let mut table = make_table(vec![], vec![]);
        mark_data_tables_by_structure(&mut table);
        assert!(
            !has_is_data_table(&table),
            "empty table should NOT be marked as data table"
        );
    }

    #[test]
    fn test_dt_non_table_element_untouched() {
        // Non-table element → no is_data_table set
        let mut div = make_div(vec![]);
        mark_data_tables_by_structure(&mut div);
        assert!(
            !has_is_data_table(&div),
            "non-table element should NOT have is_data_table set"
        );
    }

    #[test]
    fn test_dt_neither_layout_nor_data() {
        // Table with 2 rows, 1 col each → neither layout nor data → no is_data_table
        let rows = vec![make_row(&["td"]), make_row(&["td"])];
        let mut table = make_table(vec![], rows);
        mark_data_tables_by_structure(&mut table);
        assert!(
            !has_is_data_table(&table),
            "neither layout nor data table should NOT have is_data_table set"
        );
    }

    #[test]
    fn test_dt_post_order_children_first() {
        // Post-order: nested table in parent non-table should not interfere
        // A <div> containing a data <table> — the table should be marked, the div should not
        let inner_table = make_table(vec![("summary", "data")], vec![]);
        let mut div = make_div(vec![inner_table]);
        mark_data_tables_by_structure(&mut div);
        // The inner table should be marked
        if let DomNode::Element { children, .. } = &div {
            assert!(
                has_is_data_table(&children[0]),
                "nested data table should be marked"
            );
        }
        // The div should NOT be marked
        assert!(
            !has_is_data_table(&div),
            "parent div should NOT have is_data_table set"
        );
    }

    #[test]
    fn test_dt_direct_children_only() {
        // <caption> as nested (not direct) child → should NOT trigger rule 4
        let wrapper = DomNode::Element {
            tag: "div".into(),
            attrs: vec![],
            children: vec![DomNode::Element {
                tag: "caption".into(),
                attrs: vec![],
                children: vec![],
                scores: Default::default(),
                metadata: Default::default(),
            }],
            scores: Default::default(),
            metadata: Default::default(),
        };
        let mut table = make_table(vec![], vec![wrapper]);
        mark_data_tables_by_structure(&mut table);
        // <caption> is wrapped in a <div>, not a direct child → should NOT match
        assert!(
            !has_is_data_table(&table),
            "nested caption should NOT trigger data table detection"
        );
    }

    /// Differential test: compare merged function against a reference implementation
    /// of the original two-phase logic (pure fn + tree walker).
    /// Runs on a representative corpus covering all 7 rules + edge cases.
    #[test]
    fn test_dt_differential_vs_original_logic() {
        // Reference implementation of the original two-phase logic:
        // Phase 1: pure function returning Option<String>
        fn old_pure(node: &DomNode) -> Option<String> {
            match node {
                DomNode::Element {
                    tag,
                    attrs,
                    children,
                    ..
                } if tag == "table" => {
                    // 1. role=presentation → layout
                    if attrs
                        .iter()
                        .any(|(k, v)| k == "role" && v == "presentation")
                    {
                        return None;
                    }
                    // 2. datatable=0 → layout
                    if attrs.iter().any(|(k, v)| k == "datatable" && v == "0") {
                        return None;
                    }
                    // 3. Has summary → data table
                    if attrs.iter().any(|(k, _)| k == "summary") {
                        return Some("true".into());
                    }
                    // 4. Has caption → data table
                    if children
                        .iter()
                        .any(|c| matches!(c, DomNode::Element { tag, .. } if tag == "caption"))
                    {
                        return Some("true".into());
                    }
                    // 5. Has colgroup/col → data table
                    if children.iter().any(|c| matches!(c, DomNode::Element { tag, .. } if tag == "colgroup" || tag == "col")) {
                        return Some("true".into());
                    }
                    // 6. Has thead/tfoot → data table
                    if children.iter().any(|c| matches!(c, DomNode::Element { tag, .. } if tag == "thead" || tag == "tfoot")) {
                        return Some("true".into());
                    }
                    // 7. >2 rows AND >1 col → data table
                    let rows: Vec<&DomNode> = children
                        .iter()
                        .filter(|c| matches!(c, DomNode::Element { tag, .. } if tag == "tr"))
                        .collect();
                    if rows.len() > 2 {
                        let max_cols = rows
                            .iter()
                            .map(|row| match row {
                                DomNode::Element { children, .. } => {
                                    children.iter()
                                        .filter(|c| matches!(c, DomNode::Element { tag, .. } if tag == "td" || tag == "th"))
                                        .count()
                                }
                                _ => 0,
                            })
                            .max()
                            .unwrap_or(0);
                        if max_cols > 1 {
                            return Some("true".into());
                        }
                    }
                    None
                }
                _ => None,
            }
        }
        // Phase 2: tree walker that sets metadata
        fn old_tree_walker(node: &mut DomNode) {
            match node {
                DomNode::Element { children, .. } => {
                    for child in children.iter_mut() {
                        old_tree_walker(child);
                    }
                    let result = old_pure(node);
                    if let DomNode::Element { metadata, .. } = node {
                        if let Some(val) = result {
                            metadata.insert("is_data_table".to_string(), val);
                        }
                    }
                }
                _ => {}
            }
        }

        // Test cases covering all 7 rules + edge cases
        let test_cases: Vec<DomNode> = vec![
            // Rule 1: role=presentation
            make_table(vec![("role", "presentation")], vec![]),
            // Rule 2: datatable=0
            make_table(vec![("datatable", "0")], vec![]),
            // Rule 3: summary
            make_table(vec![("summary", "prices")], vec![]),
            // Rule 4: caption
            make_table(
                vec![],
                vec![DomNode::Element {
                    tag: "caption".into(),
                    attrs: vec![],
                    children: vec![DomNode::Text("T".into())],
                    scores: Default::default(),
                    metadata: Default::default(),
                }],
            ),
            // Rule 5: colgroup
            make_table(
                vec![],
                vec![DomNode::Element {
                    tag: "colgroup".into(),
                    attrs: vec![],
                    children: vec![],
                    scores: Default::default(),
                    metadata: Default::default(),
                }],
            ),
            // Rule 5: col
            make_table(
                vec![],
                vec![DomNode::Element {
                    tag: "col".into(),
                    attrs: vec![],
                    children: vec![],
                    scores: Default::default(),
                    metadata: Default::default(),
                }],
            ),
            // Rule 6: thead
            make_table(
                vec![],
                vec![DomNode::Element {
                    tag: "thead".into(),
                    attrs: vec![],
                    children: vec![],
                    scores: Default::default(),
                    metadata: Default::default(),
                }],
            ),
            // Rule 6: tfoot
            make_table(
                vec![],
                vec![DomNode::Element {
                    tag: "tfoot".into(),
                    attrs: vec![],
                    children: vec![],
                    scores: Default::default(),
                    metadata: Default::default(),
                }],
            ),
            // Rule 7: 3 rows, 2 cols
            make_table(
                vec![],
                vec![
                    make_row(&["td", "td"]),
                    make_row(&["td", "td"]),
                    make_row(&["td", "td"]),
                ],
            ),
            // Rule 7: 3 rows, 1 col (should NOT match)
            make_table(
                vec![],
                vec![make_row(&["td"]), make_row(&["td"]), make_row(&["td"])],
            ),
            // Rule 7: 2 rows, 2 cols (should NOT match)
            make_table(
                vec![],
                vec![make_row(&["td", "td"]), make_row(&["td", "td"])],
            ),
            // Layout wins over data
            make_table(
                vec![("role", "presentation"), ("summary", "prices")],
                vec![],
            ),
            // Empty table
            make_table(vec![], vec![]),
            // Non-table element
            make_div(vec![]),
            // Neither layout nor data
            make_table(vec![], vec![make_row(&["td"]), make_row(&["td"])]),
        ];

        for (i, case) in test_cases.into_iter().enumerate() {
            // Clone for old logic
            let mut case_old = case.clone();
            let mut case_new = case;
            // Apply old logic
            old_tree_walker(&mut case_old);
            // Apply new merged logic
            mark_data_tables_by_structure(&mut case_new);
            // Compare results
            let old_val = match &case_old {
                DomNode::Element { metadata, .. } => metadata.get("is_data_table").cloned(),
                _ => None,
            };
            let new_val = match &case_new {
                DomNode::Element { metadata, .. } => metadata.get("is_data_table").cloned(),
                _ => None,
            };
            assert_eq!(
                old_val, new_val,
                "Differential test failed for case {}: old={:?} new={:?}",
                i, old_val, new_val
            );
        }
    }
}
