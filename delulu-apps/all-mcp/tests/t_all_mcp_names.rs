//!  Delulu All-MCP — e2e tool-name contract (the 21-tool union)
//!
//!  Copyright (C) 2026  Mamy Ratsimbazafy
//!
//!  This program is free software: you can redistribute it and/or modify
//!  it under the terms of the GNU Affero General Public License as published by
//!  the Free Software Foundation, either version 3 of the License, or
//!  (at your option) any later version.
//!
//!  This program is distributed in the hope that it will be useful,
//!  but WITHOUT ANY WARRANTY; without even the implied warranty of
//!  MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
//!  GNU Affero General Public License for more details.
//!
//!  You should have received a copy of the GNU Affero General Public License
//!  along with this program.  If not, see <http://www.gnu.org/licenses/>.
//!

//! Spawns each of the 7 MCP binaries, performs `initialize` + `tools/list` over
//! stdio, and asserts the **exact** per-app tool-name sets (the hardcoded name
//! contract) plus the all-mcp 21-name union with prefixed `get_paper` names.
//!
//! Offline: pipes only to the local binary; no network traffic.
//!
//! The heavy lifting (find_binary, spawn, initialize, list_tools) lives in
//! `mcp_helpers`, which ships with this crate.

mod mcp_helpers;

use mcp_helpers::{find_binary, list_tools, spawn_stdio_server};
use std::collections::BTreeSet;

/// Per-app tool-name sets from the 21-tool name contract.
struct AppContract {
    /// Release binary stem, e.g. `"delulu-webfetch-mcp"`.
    bin: &'static str,
    /// Exact tool-name set the standalone binary must expose.
    names: &'static [&'static str],
}

const APPS: &[AppContract] = &[
    AppContract {
        bin: "delulu-webfetch-mcp",
        names: &["webfetch", "webfetch_raw", "fetch_doc"],
    },
    AppContract {
        bin: "delulu-websearch-mcp",
        names: &["web_search", "web_search_next_page"],
    },
    AppContract {
        bin: "delulu-travel-mcp",
        names: &["search_flights", "search_hotels"],
    },
    AppContract {
        bin: "delulu-arxiv-mcp",
        names: &["search_papers", "get_papers_by_id", "get_paper"],
    },
    AppContract {
        bin: "delulu-iacr-mcp",
        names: &[
            "list_recent_papers",
            "get_paper_details",
            "paper_pdf_url",
            "get_paper",
        ],
    },
    AppContract {
        bin: "delulu-pubmed-mcp",
        names: &[
            "search_pubmed",
            "get_summaries",
            "fetch_abstracts",
            "find_related",
            "get_database_info",
            "match_citation",
            "get_paper",
        ],
    },
    AppContract {
        bin: "delulu-all-mcp",
        names: &[
            // webfetch (3)
            "webfetch",
            "webfetch_raw",
            "fetch_doc",
            // websearch (2)
            "web_search",
            "web_search_next_page",
            // travel (2)
            "search_flights",
            "search_hotels",
            // arxiv (3; get_paper -> arxiv_get_paper)
            "search_papers",
            "get_papers_by_id",
            "arxiv_get_paper",
            // iacr (4; get_paper -> iacr_get_paper)
            "list_recent_papers",
            "get_paper_details",
            "paper_pdf_url",
            "iacr_get_paper",
            // pubmed (7; get_paper -> pubmed_get_paper)
            "search_pubmed",
            "get_summaries",
            "fetch_abstracts",
            "find_related",
            "get_database_info",
            "match_citation",
            "pubmed_get_paper",
        ],
    },
];

/// Spawn a binary, initialize + list_tools, and return its sorted tool names.
async fn spawn_and_list_names(bin: &str) -> Vec<String> {
    let path = find_binary(bin).unwrap_or_else(|e| panic!("find_binary({bin}): {e}"));
    let (mut child, mut stdin, mut stdout) = spawn_stdio_server(&path)
        .await
        .unwrap_or_else(|e| panic!("failed to spawn {bin}: {e}"));
    let mut initialized = false;
    let names = list_tools(&mut stdin, &mut stdout, &mut initialized)
        .await
        .unwrap_or_else(|e| panic!("failed to list tools for {bin}: {e}"));
    let _ = child.kill().await;
    names
}

/// The app's tool-name set with `get_paper` renamed to its all-mcp prefixed form.
fn app_names_with_prefix(app: &AppContract) -> Vec<String> {
    let prefix = app
        .bin
        .trim_start_matches("delulu-")
        .trim_end_matches("-mcp");
    app.names
        .iter()
        .map(|n| {
            if *n == "get_paper" {
                format!("{prefix}_get_paper")
            } else {
                (*n).to_string()
            }
        })
        .collect()
}

/// The exact expected all-mcp 21-name union (sorted).
fn expected_all_mcp() -> Vec<String> {
    let mut set: BTreeSet<String> = BTreeSet::new();
    for app in APPS.iter().filter(|a| a.bin != "delulu-all-mcp") {
        set.extend(app_names_with_prefix(app));
    }
    assert_eq!(set.len(), 21, "expected all-mcp set must contain 21 names");
    set.into_iter().collect()
}

/// Spawn each of the 7 binaries and assert the exact per-app tool-name set.
#[tokio::test(flavor = "multi_thread")]
async fn each_app_exposes_exact_tool_names() {
    for app in APPS {
        let names = spawn_and_list_names(app.bin).await;
        assert_unique(&names, app.bin);
        let mut expected = app.names.to_vec();
        expected.sort();
        assert_eq!(
            names, expected,
            "{} tool-name set deviates from the 21-tool contract",
            app.bin
        );
    }
}

/// all-mcp must expose the 21-name union with prefixed get_paper names, must
/// be distinct, and must NOT contain a bare `get_paper`.
#[tokio::test(flavor = "multi_thread")]
async fn all_mcp_is_21_distinct_union_with_prefixed_get_paper() {
    let names = spawn_and_list_names("delulu-all-mcp").await;

    assert_eq!(names.len(), 21, "all-mcp must expose exactly 21 tools");
    assert_unique(&names, "delulu-all-mcp");
    assert!(
        !names.iter().any(|n| n == "get_paper"),
        "all-mcp must NOT expose a bare get_paper; got {names:?}"
    );

    let expected = expected_all_mcp();
    assert_eq!(
        names, expected,
        "all-mcp union must be exactly the prefixed 21-name set"
    );

    // Every standalone (un-prefixed) tool name must be present, mapped through
    // the collision-prefix rule.
    for app in APPS.iter().filter(|a| a.bin != "delulu-all-mcp") {
        for expected_name in app_names_with_prefix(app) {
            assert!(
                names.contains(&expected_name),
                "all-mcp missing {expected_name} (from {})",
                app.bin
            );
        }
    }
}

fn assert_unique(sorted: &[String], label: &str) {
    let mut prev: Option<&String> = None;
    for n in sorted {
        if let Some(p) = prev
            && p == n
        {
            panic!("duplicate tool name '{n}' in {label}");
        }
        prev = Some(n);
    }
}