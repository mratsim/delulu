//!  Delulu PubMed Paper Search — Core Unit Tests
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

//! Unit tests for the PubMed core parser module.
//!
//! Uses `#[path]` to include the core module for direct testing of

use crate::core::{
    DocSum, SearchQuery, extract_pmc_id, parse_abstract_text, parse_ecitmatch_text,
    parse_einfo_json, parse_elink_json, parse_search_json, parse_summary_json,
};

const FIXTURES_DIR: &str = "tests/fixtures";

fn read_fixture(name: &str) -> String {
    let path = format!("{}/{}", FIXTURES_DIR, name);
    let compressed =
        std::fs::read(&path).unwrap_or_else(|e| panic!("Failed to read fixture '{}': {}", path, e));
    let decompressed = zstd::decode_all(compressed.as_slice()).expect("zstd decompression failed");
    String::from_utf8(decompressed)
        .unwrap_or_else(|e| panic!("Fixture '{}' is not UTF-8: {}", path, e))
}

// ---------------------------------------------------------------------------
// SearchQuery tests
// ---------------------------------------------------------------------------

#[test]
fn test_search_query_basic() {
    let q = SearchQuery {
        query: "asthma[Title]".to_string(),
        max_results: None,
        sort: None,
    };
    let s = q.to_query_string();
    assert!(s.contains("term="));
    assert!(s.contains("asthma"));
    assert!(s.contains("%5BTitle%5D")); // URL-encoded brackets
    assert!(!s.contains("retmax"));
}

#[test]
fn test_search_query_with_max_results() {
    let q = SearchQuery {
        query: "cancer".to_string(),
        max_results: Some(100),
        sort: Some("relevance".to_string()),
    };
    let s = q.to_query_string();
    assert!(s.contains("retmax=100"));
    assert!(s.contains("sort=relevance"));
}

// ---------------------------------------------------------------------------
// Search JSON parsing tests
// ---------------------------------------------------------------------------

#[test]
fn test_parse_search_json_from_fixture() {
    let json = read_fixture("pubmed-search.json.zst");
    let result = parse_search_json(&json).unwrap();
    assert_eq!(result.total_count, 624);
    assert_eq!(result.pmids.len(), 3);
    assert_eq!(result.pmids[0], "42477534");
}

#[test]
fn test_parse_search_json_with_no_results() {
    let json = r#"{"esearchresult": {"count": "0", "idlist": []}}"#;
    let result = parse_search_json(json).unwrap();
    assert_eq!(result.total_count, 0);
    assert!(result.pmids.is_empty());
}

// ---------------------------------------------------------------------------
// Summary JSON parsing tests
// ---------------------------------------------------------------------------

#[test]
fn test_parse_summary_json_from_fixture() {
    let json = read_fixture("pubmed-summary.json.zst");
    let papers = parse_summary_json(&json).unwrap();
    assert_eq!(papers.len(), 1);
    assert_eq!(papers[0].pmid, "42477534");
    assert!(
        papers[0].title.contains("Non-pharmacological"),
        "title should match fixture"
    );
}

// ---------------------------------------------------------------------------
// Abstract text parsing tests
// ---------------------------------------------------------------------------

#[test]
fn test_parse_abstract_text_from_fixture() {
    let text = read_fixture("pubmed-abstract.txt.zst");
    let results = parse_abstract_text(&text);
    assert!(!results.is_empty(), "should parse at least one abstract");
    assert_eq!(results[0].0, "42477534", "should match fixture PMID");
    assert!(
        results[0].1.contains("Emergence delirium"),
        "abstract should contain expected text"
    );
}

// ---------------------------------------------------------------------------
// ELink JSON parsing tests
// ---------------------------------------------------------------------------

#[test]
fn test_parse_elink_json_basic() {
    let json = r#"{
        "linksets": [{
            "dbfrom": "pubmed",
            "ids": [{"id": "37994677"}],
            "linksetdbs": [{
                "dbto": "pubmed",
                "linkname": "pubmed_pubmed",
                "links": [
                    {"id": "11111111"},
                    {"id": "22222222"},
                    {"id": "33333333"}
                ]
            }]
        }]
    }"#;
    let result = parse_elink_json(json).unwrap();
    assert_eq!(result.input_pmids, vec!["37994677"]);
    assert_eq!(result.related["37994677"].len(), 3);
}

#[test]
fn test_parse_elink_json_no_related() {
    let json =
        r#"{"linksets": [{"dbfrom": "pubmed", "ids": [{"id": "37994677"}], "linksetdbs": []}]}"#;
    let result = parse_elink_json(json).unwrap();
    assert_eq!(result.input_pmids, vec!["37994677"]);
    assert!(result.related.is_empty() || result.related["37994677"].is_empty());
}

// ---------------------------------------------------------------------------
// EInfo JSON parsing tests
// ---------------------------------------------------------------------------

#[test]
fn test_parse_einfo_json_basic() {
    let json = r#"{
        "einforesult": {
            "dbinfo": [{
                "dbname": "pubmed",
                "menuname": "PubMed",
                "description": "PubMed biomedical literature database",
                "count": "35000000",
                "lastupdate": "2025-06-01",
                "fields": [
                    {"name": "ALL", "fullname": "All Fields", "description": "All terms"},
                    {"name": "TI", "fullname": "Title", "description": "Title words"},
                    {"name": "AB", "fullname": "Abstract", "description": "Abstract words"},
                    {"name": "AU", "fullname": "Author", "description": "Author names"}
                ],
                "links": []
            }]
        }
    }"#;
    let info = parse_einfo_json(json).unwrap();
    assert_eq!(info.db_name, "pubmed");
    assert_eq!(info.record_count, 35000000);
    assert_eq!(info.fields.len(), 4);
    assert_eq!(info.fields[0].name, "ALL");
    assert_eq!(info.fields[1].full_name, "Title");
}

// ---------------------------------------------------------------------------
// ECitMatch text parsing tests
// ---------------------------------------------------------------------------

#[test]
fn test_parse_ecitmatch_text_single() {
    let text = "proc+natl+acad+sci+u+s+a|1991|88|3248|mann+bj|Art1|12345678\n";
    let results = parse_ecitmatch_text(text);
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].key, "Art1");
    assert_eq!(results[0].pmid, "12345678");
}

#[test]
fn test_parse_ecitmatch_text_multiple() {
    let text = "nature|2020|578|100|author1|key1|11111111\n\
                science|2021|591|200|author2|key2|22222222\n";
    let results = parse_ecitmatch_text(text);
    assert_eq!(results.len(), 2);
    assert_eq!(results[0].key, "key1");
    assert_eq!(results[0].pmid, "11111111");
    assert_eq!(results[1].key, "key2");
    assert_eq!(results[1].pmid, "22222222");
}

#[test]
fn test_parse_ecitmatch_text_no_match() {
    let text = "journal|2020|10|100|author|key1|\n";
    let results = parse_ecitmatch_text(text);
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].key, "key1");
    assert_eq!(results[0].pmid, ""); // empty PMID means no match
}

// ---------------------------------------------------------------------------
// PMC ID extraction tests
// ---------------------------------------------------------------------------

#[test]
fn test_extract_pmc_id_from_elocationid() {
    let doc = DocSum {
        elocationid: Some("doi: 10.1234/PMC9876543".into()),
        ..Default::default()
    };
    assert_eq!(extract_pmc_id(&doc), Some("PMC9876543".into()));
}

#[test]
fn test_extract_pmc_id_no_match() {
    let doc = DocSum {
        elocationid: Some("doi: 10.1234/10.5678".into()),
        ..Default::default()
    };
    assert_eq!(extract_pmc_id(&doc), None);
}

#[test]
fn test_extract_pmc_id_none_elocationid() {
    let doc = DocSum {
        ..Default::default()
    };
    assert_eq!(extract_pmc_id(&doc), None);
}

#[test]
fn test_extract_pmc_id_pmc_in_middle() {
    let doc = DocSum {
        elocationid: Some("doi: 10.1234/PMC5555555/v2".into()),
        ..Default::default()
    };
    assert_eq!(extract_pmc_id(&doc), Some("PMC5555555".into()));
}
