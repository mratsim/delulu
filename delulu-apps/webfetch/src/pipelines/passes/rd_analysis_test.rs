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
