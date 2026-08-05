//!  Delulu All-MCP — static route table contract
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

//! Static drift contract for the 21-tool union route table.
//!
//! These tests are compile-time/render-time only — they exercise the static
//! `TOOL_ROUTES` slice and the `ServerId` enum; they never spawn a server or
//! touch the network. Together with the e2e cross-checks (which compare the
//! emitted names/schemas against the spawned standalone binaries) they pin the
//! route table so no tool is dropped, added, or duplicated across refactors.

use std::collections::HashSet;

use delulu_all_mcp::lib_mcp::AllServer;
use delulu_all_mcp::{ServerId, TOOL_ROUTES};

/// Compile-time check that `AllServer` is `Clone` (it must be, because the
/// rmcp framework constructs the server once and shares/branches it across
/// concurrent requests).
fn assert_clone<T: Clone>() {}

#[test]
fn all_server_is_clone() {
    assert_clone::<AllServer>();
}

/// The union has exactly 21 tools.
#[test]
fn route_table_has_exactly_21_rows() {
    assert_eq!(TOOL_ROUTES.len(), 21);
}

/// Every all-mcp tool name is unique (no duplicate emitted names).
#[test]
fn all_mcp_names_are_unique() {
    let mut seen = HashSet::new();
    for (name, _, _) in TOOL_ROUTES {
        assert!(
            seen.insert(*name),
            "duplicate all-mcp tool name in TOOL_ROUTES: {name:?}"
        );
    }
    assert_eq!(seen.len(), TOOL_ROUTES.len());
}

/// Inner tool names are unique per owning server.
///
/// The three colliding `get_paper` tools live on three *different* servers
/// (arxiv, iacr, pubmed), so the inner names are distinct within each server.
#[test]
fn inner_names_are_unique_per_server() {
    let mut groups: Vec<(ServerId, Vec<&str>)> = Vec::new();
    for (_, server, inner) in TOOL_ROUTES {
        match groups.iter_mut().find(|(sid, _)| *sid == *server) {
            Some((_, names)) => names.push(inner),
            None => groups.push((*server, vec![inner])),
        }
    }

    for (server, names) in &groups {
        let mut seen = HashSet::new();
        for inner in names {
            assert!(
                seen.insert(*inner),
                "duplicate inner name on server {server:?}: {inner:?}"
            );
        }
    }
}

/// Hardcoded per-app inner-name sets — the name contract.
///
/// Every row's inner name must belong to its owning server's hardcoded set.
/// This pins the exact inner names per app so a refactor cannot silently drop
/// or rename a tool.
#[test]
fn per_row_dispatch_inner_name_in_owner_set() {
    const WEBSET: &[&str] = &["webfetch", "webfetch_raw", "fetch_doc"];
    const WEBSEARCH_SET: &[&str] = &["web_search", "web_search_next_page"];
    const TRAVEL_SET: &[&str] = &["search_flights", "search_hotels"];
    const ARXIV_SET: &[&str] = &["search_papers", "get_papers_by_id", "get_paper"];
    const IACR_SET: &[&str] =
        &["list_recent_papers", "get_paper_details", "paper_pdf_url", "get_paper"];
    const PUBMED_SET: &[&str] = &[
        "search_pubmed",
        "get_summaries",
        "fetch_abstracts",
        "find_related",
        "get_database_info",
        "match_citation",
        "get_paper",
    ];

    let hardcoded: &[(&ServerId, &[&str])] = &[
        (&ServerId::Webfetch, WEBSET),
        (&ServerId::Websearch, WEBSEARCH_SET),
        (&ServerId::Travel, TRAVEL_SET),
        (&ServerId::Arxiv, ARXIV_SET),
        (&ServerId::Iacr, IACR_SET),
        (&ServerId::Pubmed, PUBMED_SET),
    ];

    for (name, server, inner) in TOOL_ROUTES {
        let set = hardcoded
            .iter()
            .find(|(sid, _)| *sid == server)
            .unwrap_or_else(|| panic!("row {name:?} has unknown server {server:?}"))
            .1;
        assert!(
            set.contains(inner),
            "row ({name:?}, {server:?}) inner name {inner:?} not in owning server's set"
        );
    }
}


/// The did-you-mean hint for the unprefixed `get_paper` is derived from
/// `TOOL_ROUTES`: the three colliding get_paper tools renamed to their all-mcp
/// names.
#[test]
fn did_you_mean_hint_is_derived_from_routes() {
    let mut alternatives = TOOL_ROUTES
        .iter()
        .filter(|(name, _, inner)| *inner == "get_paper" && *name != "get_paper")
        .map(|(name, _, _)| *name)
        .collect::<Vec<_>>();
    alternatives.sort_unstable();

    let mut expected = vec!["arxiv_get_paper", "iacr_get_paper", "pubmed_get_paper"];
    expected.sort_unstable();

    assert_eq!(alternatives, expected);
}

/// Sanity: every ServerId variant is exercised by at least one row, so no
/// server's tools are silently dropped from the union.
#[test]
fn every_server_id_appears() {
    let mut present: Vec<ServerId> = Vec::new();
    for (_, server, _) in TOOL_ROUTES {
        if !present.contains(server) {
            present.push(*server);
        }
    }
    let all = [
        ServerId::Webfetch,
        ServerId::Websearch,
        ServerId::Travel,
        ServerId::Arxiv,
        ServerId::Iacr,
        ServerId::Pubmed,
    ];
    for sid in all {
        assert!(present.contains(&sid), "server {sid:?} has no route rows");
    }
}