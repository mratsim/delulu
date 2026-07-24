//!  Delulu IACR Paper Search — Core Unit Tests
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
//! Tests for `delulu_paper_search_iacr::core` — RSS parsing and HTML scraping.
//!
//! Uses `#[path]` pattern to point to the crate root.

#[path = "../fixtures/mod.rs"]
mod fixtures;

use crate::core::{parse_paper_html, parse_rss_response};

/// Load the realistic RSS fixture and verify parsing produces correct results.
#[test]
fn test_parse_rss_realistic_fixture() {
    let xml = fixtures::iacr_rss_feed();
    let papers = parse_rss_response(&xml).expect("should parse fixture RSS XML");

    assert_eq!(papers.len(), 100, "fixture has 100 entries");

    // First paper
    let p0 = &papers[0];
    assert_eq!(p0.id, "2026/1459");
    assert_eq!(p0.year, 2026);
    assert_eq!(p0.number, 1459);
    assert!(
        p0.title.contains("Hybrid hash function based on the DLP"),
        "title mismatch: {}",
        p0.title
    );
    assert_eq!(p0.authors.len(), 2);
    assert_eq!(p0.authors[0], "Dimitri Koshelev");
    assert_eq!(p0.authors[1], "Francesc Sebé");
    assert!(
        p0.abstract_text.contains("hybrid hash function"),
        "abstract should contain 'hybrid hash function'",
    );
    assert_eq!(p0.html_url, "https://eprint.iacr.org/2026/1459");
    assert_eq!(p0.pdf_url, "https://eprint.iacr.org/2026/1459.pdf");

    // Second paper
    let p1 = &papers[1];
    assert_eq!(p1.id, "2024/1008");
    assert_eq!(p1.year, 2024);
    assert_eq!(p1.number, 1008);
    assert!(p1.title.contains("Multi-round Dependency Identification"));
    assert_eq!(p1.authors.len(), 6);
    assert_eq!(p1.authors[0], "Xichao Hu");
    assert!(p1.abstract_text.contains("impossible boomerang"));

    // Third paper
    let p2 = &papers[2];
    assert_eq!(p2.id, "2023/1258");
    assert_eq!(p2.year, 2023);
    assert_eq!(p2.number, 1258);
    assert!(p2.title.contains("Flexway O-Sort"));
    assert_eq!(p2.authors.len(), 6);
    assert_eq!(p2.authors[0], "Tianyao Gu");
    assert!(p2.abstract_text.contains("Oblivious algorithms"));
}

/// Parse a minimal RSS feed with one item.
#[test]
fn test_parse_rss_minimal() {
    let xml = r#"<?xml version="1.0"?>
    <rss version="2.0">
      <channel>
        <item>
          <title>Minimal Paper</title>
          <link>https://eprint.iacr.org/2024/1</link>
          <description>Minimal abstract</description>
          <pubDate>Mon, 01 Jan 2024 00:00:00 +0000</pubDate>
        </item>
      </channel>
    </rss>"#;
    let papers = parse_rss_response(xml).expect("should parse minimal RSS");
    assert_eq!(papers.len(), 1);
    assert_eq!(papers[0].id, "2024/1");
    assert_eq!(papers[0].title, "Minimal Paper");
    assert!(papers[0].authors.is_empty());
    assert_eq!(papers[0].abstract_text, "Minimal abstract");
    assert_eq!(papers[0].html_url, "https://eprint.iacr.org/2024/1");
    assert_eq!(papers[0].pdf_url, "https://eprint.iacr.org/2024/1.pdf");
}

/// Parse an RSS feed with no items.
#[test]
fn test_parse_rss_empty() {
    let xml = r#"<?xml version="1.0"?>
    <rss version="2.0">
      <channel>
        <title>Empty Feed</title>
      </channel>
    </rss>"#;
    let papers = parse_rss_response(xml).expect("should parse empty RSS");
    assert!(papers.is_empty());
}

/// Verify that the RSS parser handles HTML entities in titles.
#[test]
fn test_parse_rss_html_entities() {
    let xml = r#"<?xml version="1.0"?>
    <rss version="2.0">
      <channel>
        <item>
          <title>A &amp; B: &lt;test&gt;</title>
          <link>https://eprint.iacr.org/2024/2</link>
          <description>abstract with &quot;quotes&quot;</description>
          <pubDate>Mon, 01 Jan 2024 00:00:00 +0000</pubDate>
        </item>
      </channel>
    </rss>"#;
    let papers = parse_rss_response(xml).expect("should parse");
    assert_eq!(papers.len(), 1);
    assert_eq!(papers[0].title, "A & B: <test>");
    assert_eq!(papers[0].abstract_text, "abstract with \"quotes\"");
}

/// Load the realistic HTML fixture and verify parsing produces correct results.
#[test]
fn test_parse_html_realistic_fixture() {
    let html = fixtures::iacr_paper_html();
    let paper = parse_paper_html(&html).expect("should parse fixture HTML");

    assert_eq!(paper.id, "2025/1");
    assert_eq!(paper.year, 2025);
    assert_eq!(paper.number, 1);
    assert_eq!(
        paper.title,
        "Attribute Based Encryption for Turing Machines from Lattices"
    );
    assert_eq!(paper.authors.len(), 3);
    assert_eq!(paper.authors[0], "Shweta Agrawal");
    assert_eq!(paper.authors[1], "Simran Kumari");
    assert_eq!(paper.authors[2], "Shota Yamada");
    assert!(
        paper.abstract_text.contains("attribute based encryption"),
        "abstract should contain 'attribute based encryption'",
    );
    assert!(paper.abstract_text.contains("lattice assumptions"));
    assert_eq!(paper.html_url, "https://eprint.iacr.org/2025/1");
    assert_eq!(paper.pdf_url, "https://eprint.iacr.org/2025/1.pdf");
}

/// Test that parse_paper_html returns an error for invalid HTML.
#[test]
fn test_parse_html_invalid() {
    let html = "<html><body>no paper content</body></html>";
    let result = parse_paper_html(html);
    assert!(result.is_err(), "should fail on invalid HTML");
}

/// Test that parse_paper_html returns an error for empty HTML.
#[test]
fn test_parse_html_empty() {
    let result = parse_paper_html("");
    assert!(result.is_err(), "should fail on empty HTML");
}
