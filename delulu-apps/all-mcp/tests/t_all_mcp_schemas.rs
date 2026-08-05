//!  Delulu All-MCP — e2e schema parity (the 21-tool union vs. standalone binaries)
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

//! Spawns all-mcp plus each of the 6 standalone MCP binaries, performs
//! `initialize` + `tools/list` over stdio, and asserts that every all-mcp
//! tool's `description` + `inputSchema` is **deep-equal** (via
//! `serde_json::Value` equality) to the owning standalone binary's entry.
//!
//! For the 3 colliding `get_paper` tools (renamed to `arxiv_get_paper`,
//! `iacr_get_paper`, `pubmed_get_paper`) the `name` is allowed to differ (a
//! 3-name allowlist); `description` + `inputSchema` must still be deep-equal.
//!
//! Offline: pipes only to local binaries; no network traffic.

mod mcp_helpers;

use delulu_all_mcp::{ServerId, TOOL_ROUTES};
use mcp_helpers::{find_binary, list_tools_entries, spawn_stdio_server};
use serde_json::Value;

// A tool is "renamed" when its all-mcp name differs from its inner tool
// name (only the 3 colliding `get_paper` renames). Everything else must
// match by name AND by description + inputSchema.

/// Release binary stem for each owning inner server.
fn server_bin(server_id: ServerId) -> &'static str {
    match server_id {
        ServerId::Webfetch => "delulu-webfetch-mcp",
        ServerId::Websearch => "delulu-websearch-mcp",
        ServerId::Travel => "delulu-travel-mcp",
        ServerId::Arxiv => "delulu-arxiv-mcp",
        ServerId::Iacr => "delulu-iacr-mcp",
        ServerId::Pubmed => "delulu-pubmed-mcp",
    }
}

/// The distinct set of standalone binaries referenced by `TOOL_ROUTES`.
fn owned_standalone_bins() -> Vec<&'static str> {
    let mut bins: Vec<&'static str> = TOOL_ROUTES
        .iter()
        .map(|(_, sid, _)| server_bin(*sid))
        .collect();
    bins.sort();
    bins.dedup();
    bins
}

/// Spawn a binary, initialize + list tools, and return its full tool entries.
async fn spawn_and_list_entries(bin: &str) -> Vec<Value> {
    let path = find_binary(bin).unwrap_or_else(|e| panic!("find_binary({bin}): {e}"));
    let (mut child, mut stdin, mut stdout) = spawn_stdio_server(&path)
        .await
        .unwrap_or_else(|e| panic!("failed to spawn {bin}: {e}"));
    let mut initialized = false;
    let entries = list_tools_entries(&mut stdin, &mut stdout, &mut initialized)
        .await
        .unwrap_or_else(|e| panic!("failed to list tools for {bin}: {e}"));
    let _ = child.kill().await;
    entries
}

/// Build a map `inner_name -> tool entry` for a list of tool entries.
fn index_by_name(entries: &[Value]) -> std::collections::HashMap<String, Value> {
    entries
        .iter()
        .map(|e| {
            let name = e["name"].as_str().expect("tool entry has a name").to_string();
            (name, e.clone())
        })
        .collect()
}

/// Extract `description` from a tool entry (may be absent/null).
fn description_of(entry: &Value) -> Value {
    entry.get("description").cloned().unwrap_or(Value::Null)
}

/// Extract `inputSchema` from a tool entry (must exist per MCP).
fn input_schema_of(entry: &Value) -> Value {
    entry["inputSchema"].clone()
}

/// For each `TOOL_ROUTES` row assert the all-mcp entry deep-equals the owning
/// standalone entry (modulo `name` for the 3 renames).
#[tokio::test(flavor = "multi_thread")]
async fn all_mcp_schemas_deep_equal_standalone() {
    // Spawn all-mcp and read its 21 tool entries.
    let all_entries = spawn_and_list_entries("delulu-all-mcp").await;
    assert_eq!(all_entries.len(), TOOL_ROUTES.len(), "all-mcp must expose one tool per TOOL_ROUTES row");
    let all_by_name = index_by_name(&all_entries);

    // Spawn each standalone binary once and index its entries by inner name.
    let mut standalone_by_bin: std::collections::HashMap<&str, std::collections::HashMap<String, Value>> =
        std::collections::HashMap::new();
    for bin in owned_standalone_bins() {
        let entries = spawn_and_list_entries(bin).await;
        standalone_by_bin.insert(bin, index_by_name(&entries));
    }

    for (route_name, server_id, inner_name) in TOOL_ROUTES {
        let bin = server_bin(*server_id);
        let standalone_map = &standalone_by_bin[bin];
        let standalone_entry = standalone_map
            .get(*inner_name)
            .unwrap_or_else(|| panic!("{bin} missing inner tool '{inner_name}'"));

        let all_entry = all_by_name
            .get(*route_name)
            .unwrap_or_else(|| panic!("all-mcp missing tool '{route_name}'"));

        // Description and inputSchema must be deep-equal in ALL cases.
        assert_eq!(
            description_of(all_entry),
            description_of(standalone_entry),
            "description mismatch for tool '{route_name}' vs standalone '{inner_name}' ({bin})"
        );
        assert_eq!(
            input_schema_of(all_entry),
            input_schema_of(standalone_entry),
            "inputSchema mismatch for tool '{route_name}' vs standalone '{inner_name}' ({bin})"
        );

        // Name: identical for the 18 non-colliding tools; allowed to differ
        // (all-mcp name != inner name) for the 3 renames.
        let all_name = all_entry["name"].as_str().unwrap();
        if *route_name != *inner_name {
            assert_eq!(
                all_name, *route_name,
                "all-mcp '{route_name}' must keep its prefixed name"
            );
        } else {
            assert_eq!(
                all_name, *inner_name,
                "all-mcp tool '{route_name}' must keep the inner name '{inner_name}'"
            );
        }
    }
}
