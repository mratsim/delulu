//!  Delulu arXiv Paper Search — Core Unit Tests
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

//! # Core Unit Tests
//!
//! Tests for `delulu_paper_search_arxiv::core` — parsing, query building,
//! and XML normalization.
//!
//! Uses `#[path]` pattern to point to the crate root.

#[path = "../fixtures/mod.rs"]
mod fixtures;

use chrono::NaiveDate;
use crate::core::{extract_arxiv_id, parse_atom_response, SearchQuery};

/// Load the realistic fixture XML and verify parsing produces correct results.
#[test]
fn test_parse_realistic_fixture() {
    let xml = fixtures::arxiv_search_response();
    let papers = parse_atom_response(&xml).expect("should parse fixture XML");

    assert_eq!(papers.len(), 2, "fixture has 2 entries");

    // First paper
    let p0 = &papers[0];
    assert_eq!(p0.id, "cond-mat/0011267");
    assert!(
        p0.title.contains("electronic structure of cuprates"),
        "title mismatch: {}",
        p0.title
    );
    assert_eq!(p0.authors.len(), 8);
    assert_eq!(p0.authors[0], "Mark S. Golden");
    assert!(
        p0.abstract_text.contains("cuprate"),
        "abstract should contain cuprate"
    );
    assert_eq!(
        p0.comment.as_deref(),
        Some("J. Electron Spec. Relat. Phenom.: special issue on electron correlation, in press")
    );
    assert_eq!(
        p0.journal_ref.as_deref(),
        Some("J. Electron Spectr. Relat. Phenom. 117-118, 203 (2001)")
    );
    assert_eq!(p0.doi.as_deref(), None);
    assert_eq!(p0.primary_category, "cond-mat.supr-con");
    assert_eq!(p0.categories.len(), 2);
    assert!(p0.categories.contains(&"cond-mat.supr-con".to_string()));
    assert!(p0.categories.contains(&"cond-mat.str-el".to_string()));
    assert_eq!(p0.published, NaiveDate::from_ymd_opt(2000, 11, 15).unwrap());
    assert_eq!(p0.updated, NaiveDate::from_ymd_opt(2000, 11, 15).unwrap());
    assert_eq!(p0.abs_url, "https://arxiv.org/abs/cond-mat/0011267v1");
    assert_eq!(p0.pdf_url, "https://arxiv.org/pdf/cond-mat/0011267v1");

    // Second paper
    let p1 = &papers[1];
    assert_eq!(p1.id, "cond-mat/0211289");
    assert!(p1.title.contains("Surface effects on the electronic energy loss"));
    assert_eq!(p1.authors.len(), 2);
    assert_eq!(p1.authors[0], "A. Garcia-Lekue");
    assert_eq!(
        p1.comment.as_deref(),
        Some("7 pages, 3 figures, to appear in J. Electron Spectrosc")
    );
    assert_eq!(
        p1.journal_ref.as_deref(),
        Some("J. Electron Spectrosc. 129, 223 (2003)")
    );
    assert!(p1.doi.is_none(), "second paper has no DOI");
    assert_eq!(p1.primary_category, "cond-mat.mtrl-sci");
    assert_eq!(p1.categories.len(), 1);
    assert!(p1.categories.contains(&"cond-mat.mtrl-sci".to_string()));
    assert_eq!(p1.published, NaiveDate::from_ymd_opt(2002, 11, 14).unwrap());
    assert_eq!(p1.updated, NaiveDate::from_ymd_opt(2002, 11, 14).unwrap());
}

/// Parse a minimal valid entry with only required fields.
#[test]
fn test_parse_minimal_entry() {
    let xml = r#"<?xml version="1.0"?>
    <feed>
      <entry>
        <id>http://arxiv.org/abs/2301.99999</id>
        <title>Minimal Paper</title>
        <summary>minimal abstract</summary>
        <arxiv:primary_category term="cs.AI" scheme="http://arxiv.org/schemas/atom"/>
        <published>2023-01-01T00:00:00Z</published>
        <updated>2023-01-02T00:00:00Z</updated>
      </entry>
    </feed>"#;
    let papers = parse_atom_response(xml).expect("should parse minimal entry");
    assert_eq!(papers.len(), 1);
    assert_eq!(papers[0].id, "2301.99999");
    assert_eq!(papers[0].title, "Minimal Paper");
    assert!(papers[0].authors.is_empty());
    assert!(papers[0].comment.is_none());
    assert!(papers[0].journal_ref.is_none());
}

/// Parse a feed with no entries.
#[test]
fn test_parse_no_entries() {
    let xml = r#"<?xml version="1.0"?><feed></feed>"#;
    let papers = parse_atom_response(xml).expect("should parse empty feed");
    assert!(papers.is_empty());
}

/// Verify that the parser handles HTML entities in title/abstract.
#[test]
fn test_parse_with_html_entities() {
    let xml = r#"<?xml version="1.0"?>
    <feed>
      <entry>
        <id>http://arxiv.org/abs/2301.99999</id>
        <title>A &amp; B: &lt;test&gt;</title>
        <summary>abstract with &quot;quotes&quot; &amp; &lt;tags&gt;</summary>
        <published>2023-01-01T00:00:00Z</published>
        <updated>2023-01-02T00:00:00Z</updated>
        <arxiv:primary_category term="cs.IR" scheme="http://arxiv.org/schemas/atom"/>
      </entry>
    </feed>"#;
    let papers = parse_atom_response(xml).expect("should parse");
    assert_eq!(papers.len(), 1);
    assert_eq!(papers[0].title, "A & B: <test>");
    assert_eq!(papers[0].abstract_text, r#"abstract with "quotes" & <tags>"#);
}

/// Verify that the parser falls back to constructing URLs from the arXiv ID
/// when no links are present in the XML.
#[test]
fn test_parse_missing_links() {
    let xml = r#"<?xml version="1.0"?>
    <feed>
      <entry>
        <id>http://arxiv.org/abs/2301.99999</id>
        <title>No Links Paper</title>
        <summary>abstract text</summary>
        <arxiv:primary_category term="cs.LG" scheme="http://arxiv.org/schemas/atom"/>
        <published>2023-01-01T00:00:00Z</published>
        <updated>2023-01-02T00:00:00Z</updated>
      </entry>
    </feed>"#;
    let papers = parse_atom_response(xml).expect("should parse");
    assert_eq!(papers[0].abs_url, "https://arxiv.org/abs/2301.99999");
    assert_eq!(papers[0].pdf_url, "https://arxiv.org/pdf/2301.99999");
}

/// Verify that the SearchQuery builds correct URL query strings.
#[test]
fn test_search_query_url_encoding() {
    let q = SearchQuery {
        query: "au:smith AND ti:learning".to_string(),
        max_results: Some(5),
        start: None,
        sort_by: None,
        sort_order: None,
    };
    let s = q.to_query_string();
    assert!(s.contains("search_query=au%3Asmith%20AND%20ti%3Alearning"));
    assert!(s.contains("max_results=5"));
    assert!(!s.contains("sortBy"));
}

// ---------------------------------------------------------------------------
// arXiv ID extraction tests
// ---------------------------------------------------------------------------

#[test]
fn test_extract_arxiv_id_standard() {
    assert_eq!(extract_arxiv_id("http://arxiv.org/abs/2301.12345"), "2301.12345");
}

#[test]
fn test_extract_arxiv_id_https() {
    assert_eq!(extract_arxiv_id("https://arxiv.org/abs/2301.12345"), "2301.12345");
}

#[test]
fn test_extract_arxiv_id_with_version() {
    assert_eq!(extract_arxiv_id("http://arxiv.org/abs/2301.12345v2"), "2301.12345");
}

#[test]
fn test_extract_arxiv_id_old_format() {
    assert_eq!(extract_arxiv_id("http://arxiv.org/abs/cond-mat/0011267"), "cond-mat/0011267");
}

#[test]
fn test_extract_arxiv_id_old_format_with_version() {
    assert_eq!(extract_arxiv_id("http://arxiv.org/abs/cond-mat/0011267v1"), "cond-mat/0011267");
}

#[test]
fn test_extract_arxiv_id_v_in_body() {
    // 'v' character in the paper ID itself, not a version suffix
    assert_eq!(extract_arxiv_id("http://arxiv.org/abs/2301.12345v"), "2301.12345v");
}


#[test]
fn test_extract_arxiv_id_strips_query_params() {
    assert_eq!(extract_arxiv_id("http://arxiv.org/abs/2301.12345?format=pdf"), "2301.12345");
}

#[test]
fn test_extract_arxiv_id_strips_fragment() {
    assert_eq!(extract_arxiv_id("http://arxiv.org/abs/2301.12345#section2"), "2301.12345");
}
