//!  Delulu IACR Paper Search — Core
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

//! # Core — IACR ePrint RSS & HTML Parser
//!
//! Pure data structures and parsing logic for the IACR ePrint Archive.
//! No I/O — suitable for testing without network access.
//!
//! Two data sources:
//! - RSS feed (`/rss/rss.xml`) — lists recent papers
//! - HTML page (`/{year}/{number}`) — full metadata for a single paper

use regex::Regex;
use scraper::{Html, Selector};
use serde::Deserialize;
use std::sync::LazyLock;

// ---------------------------------------------------------------------------
// Paper data structure
// ---------------------------------------------------------------------------

/// A parsed IACR ePrint paper.
#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(feature = "mcp", derive(schemars::JsonSchema))]
pub struct Paper {
    /// Paper ID (e.g. "2024/123")
    pub id: String,
    /// Publication year
    pub year: u32,
    /// Paper number within the year
    pub number: u32,
    /// Paper title
    pub title: String,
    /// List of author names
    pub authors: Vec<String>,
    /// Abstract text
    #[serde(rename = "abstract")]
    pub abstract_text: String,
    /// HTML URL on eprint.iacr.org
    pub html_url: String,
    /// PDF URL on eprint.iacr.org
    pub pdf_url: String,
}

// ---------------------------------------------------------------------------
// RSS 2.0 intermediate deserialization structs
// ---------------------------------------------------------------------------

/// Top-level RSS envelope.
#[derive(Debug, Deserialize)]
struct Rss {
    #[serde(rename = "channel")]
    channel: RssChannel,
}

#[derive(Debug, Deserialize)]
struct RssChannel {
    #[serde(rename = "item", default)]
    items: Vec<RssItem>,
}

/// A single RSS item representing a paper.
#[derive(Debug, Deserialize)]
struct RssItem {
    /// Paper title
    title: String,
    /// Link to the paper HTML page
    link: String,
    /// Description (abstract), may contain CDATA
    #[serde(default)]
    description: Option<String>,
    /// Authors via dc:creator (Dublin Core namespace) — normalized to dc_creator
    #[serde(rename = "dc_creator", default)]
    dc_creator: Vec<String>,
    /// Publication date (RFC 2822) — kept for serde deserialization completeness
    #[serde(default)]
    #[allow(dead_code)]
    pub_date: Option<String>,
    /// Enclosure (PDF URL)
    #[serde(default)]
    enclosure: Option<RssEnclosure>,
    /// GUID — kept for serde deserialization completeness
    #[serde(default)]
    #[allow(dead_code)]
    guid: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RssEnclosure {
    #[serde(rename = "@url")]
    url: String,
}

// ---------------------------------------------------------------------------
// RSS normalization (namespace handling)
// ---------------------------------------------------------------------------

/// Normalize IACR RSS XML by stripping XML declarations and xmlns declarations
/// so that serde_xml_rs can deserialize without issues.
///
/// IACR returns RSS like:
/// ```xml
/// <?xml version='1.0' encoding='UTF-8'?>
/// <rss xmlns:dc="http://purl.org/dc/elements/1.1/" ...>
///   <channel>
///     <item>
///       <dc:creator>Author Name</dc:creator>
///     </item>
///   </channel>
/// </rss>
/// ```
/// After normalization the `dc:creator` elements are rewritten as `dc_creator`
/// and xmlns declarations are removed.
fn normalize_rss_xml(raw: &str) -> String {
    let mut result = String::with_capacity(raw.len());
    let bytes = raw.as_bytes();
    let len = bytes.len();
    let mut i = 0;

    // Strip XML declaration <?xml ...?>
    if raw.trim_start().starts_with("<?xml") && let Some(end) = raw.find("?>") {
        i = end + 2;
    }

    // Strip <!DOCTYPE ...> if present
    while i < len {
        if bytes[i] == b'<' && i + 8 < len {
            let rest = &raw[i..];
            if (rest.starts_with("<!DOCTYPE") || rest.starts_with("<!doctype")) && let Some(end) = rest.find('>') {
                i += end + 1;
                continue;
            }
        }
        break;
    }

    while i < len {
        // Remove xmlns declarations
        if i + 5 < len
            && bytes[i].is_ascii_whitespace()
            && bytes[i + 1] == b'x'
            && bytes[i + 2] == b'm'
            && bytes[i + 3] == b'l'
            && bytes[i + 4] == b'n'
            && bytes[i + 5] == b's'
        {
            // Find the '='
            let mut eq_pos = i + 6;
            while eq_pos < len && bytes[eq_pos] != b'=' {
                eq_pos += 1;
            }
            if eq_pos < len && eq_pos + 1 < len {
                let quote = bytes[eq_pos + 1];
                let mut end = eq_pos + 2;
                while end < len && bytes[end] != quote {
                    end += 1;
                }
                if end < len {
                    i = end + 1;
                    continue;
                }
            }
        }

        // Check for namespace-prefixed element names like <dc:creator>
        if bytes[i] == b'<' {
            let tag_start = i + 1;
            let is_closing = tag_start < len && bytes[tag_start] == b'/';
            let name_start = if is_closing { tag_start + 1 } else { tag_start };

            // Find colon in tag name
            let mut colon_pos = name_start;
            while colon_pos < len
                && bytes[colon_pos] != b'>'
                && bytes[colon_pos] != b' '
                && bytes[colon_pos] != b'/'
                && bytes[colon_pos] != b':'
            {
                colon_pos += 1;
            }

            if colon_pos < len && bytes[colon_pos] == b':' {
                // Found prefixed element — rewrite with underscore
                result.push('<');
                if is_closing {
                    result.push('/');
                }
                // Copy prefix
                result.push_str(&raw[name_start..colon_pos]);
                result.push('_');
                // Find end of local name
                let mut local_end = colon_pos + 1;
                while local_end < len
                    && bytes[local_end] != b'>'
                    && bytes[local_end] != b' '
                    && bytes[local_end] != b'/'
                {
                    local_end += 1;
                }
                result.push_str(&raw[colon_pos + 1..local_end]);
                i = local_end;
                continue;
            }
        }

        result.push(raw[i..].chars().next().unwrap());
        i += raw[i..].chars().next().unwrap().len_utf8();
    }

    result
}

/// Strip CDATA sections from a string.
fn strip_cdata(s: &str) -> String {
    let s = s.trim();
    if s.starts_with("<![CDATA[") && s.ends_with("]]>") {
        s[9..s.len() - 3].to_string()
    } else {
        s.to_string()
    }
}

/// Simple HTML entity unescape for common entities.
fn html_unescape(s: &str) -> String {
    s.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
}

// ---------------------------------------------------------------------------
// Public RSS parser
// ---------------------------------------------------------------------------

/// Parse an IACR RSS feed response into a list of papers.
///
/// # Errors
///
/// Returns an error if the XML is malformed or required fields are missing.
pub fn parse_rss_response(xml: &str) -> Result<Vec<Paper>, String> {
    let normalized = normalize_rss_xml(xml);

    let rss: Rss =
        serde_xml_rs::from_str(&normalized).map_err(|e| format!("RSS XML parse error: {}", e))?;

    let mut papers = Vec::with_capacity(rss.channel.items.len());

    for item in rss.channel.items {
        let (year, number) = extract_year_number_from_url(&item.link)?;
        let id = format!("{}/{}", year, number);
        let title = html_unescape(&item.title);
        let authors = if !item.dc_creator.is_empty() {
            item.dc_creator
        } else {
            Vec::new()
        };
        let abstract_text = item
            .description
            .as_ref()
            .map(|d| {
                let cleaned = strip_cdata(d);
                html_unescape(&cleaned)
            })
            .unwrap_or_default();
        let html_url = item.link.clone();
        let pdf_url = item
            .enclosure
            .as_ref()
            .map(|e| e.url.clone())
            .unwrap_or_else(|| format!("https://eprint.iacr.org/{}/{}.pdf", year, number));

        papers.push(Paper {
            id,
            year,
            number,
            title,
            authors,
            abstract_text,
            html_url,
            pdf_url,
        });
    }

    Ok(papers)
}

// ---------------------------------------------------------------------------
// Public HTML parser
// ---------------------------------------------------------------------------

/// Parse an IACR paper HTML page into a Paper struct.
///
/// Scrapes the HTML page at `https://eprint.iacr.org/{year}/{number}`
/// to extract title, authors, abstract, and metadata.
///
/// # Errors
///
/// Returns an error if the HTML is malformed or required fields are missing.
pub fn parse_paper_html(html: &str) -> Result<Paper, String> {
    let document = Html::parse_document(html);

    // Extract paper ID from <h4>Paper YYYY/NNN</h4>
    let h4_selector =
        Selector::parse("h4").map_err(|e| format!("Failed to parse h4 selector: {}", e))?;
    let h4_text = document
        .select(&h4_selector)
        .next()
        .map(|el| el.text().collect::<String>())
        .ok_or_else(|| "Missing <h4> element for paper ID".to_string())?;

    let paper_id = h4_text.trim();
    let (year, number) = if let Some(id_part) = paper_id.strip_prefix("Paper ") {
        parse_year_number(id_part)?
    } else {
        return Err(format!("Unexpected <h4> format: '{}'", paper_id));
    };

    let id = format!("{}/{}", year, number);

    // Extract title from <h3 class="mb-3"><a href="...pdf">Title</a></h3>
    let title_selector =
        Selector::parse("h3.mb-3 a").map_err(|e| format!("Failed to parse title selector: {}", e))?;
    let title = document
        .select(&title_selector)
        .next()
        .map(|el| el.text().collect::<String>())
        .ok_or_else(|| "Missing title element".to_string())?;
    let title = title.trim().to_string();

    // Extract authors from <span class="authorName">
    let author_selector = Selector::parse("span.authorName")
        .map_err(|e| format!("Failed to parse author selector: {}", e))?;
    let authors: Vec<String> = document
        .select(&author_selector)
        .map(|el| el.text().collect::<String>().trim().to_string())
        .collect();

    // Extract abstract from <p style="white-space: pre-wrap;">
    let abstract_selector = Selector::parse("p[style=\"white-space: pre-wrap;\"]")
        .map_err(|e| format!("Failed to parse abstract selector: {}", e))?;
    let abstract_text = document
        .select(&abstract_selector)
        .next()
        .map(|el| el.text().collect::<String>())
        .ok_or_else(|| "Missing abstract element".to_string())?;
    let abstract_text = abstract_text.trim().to_string();

    // Build URLs
    let html_url = format!("https://eprint.iacr.org/{}", id);
    let pdf_url = format!("https://eprint.iacr.org/{}.pdf", id);

    Ok(Paper {
        id,
        year,
        number,
        title,
        authors,
        abstract_text,
        html_url,
        pdf_url,
    })
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Extract year and number from a URL like `https://eprint.iacr.org/2024/123`
/// or `https://eprint.iacr.org/2024/123.pdf`.
fn extract_year_number_from_url(url: &str) -> Result<(u32, u32), String> {
    // Strip .pdf suffix if present
    let url = url.strip_suffix(".pdf").unwrap_or(url);
    // Strip trailing slash
    let url = url.strip_suffix('/').unwrap_or(url);

    static RE: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"/(\d{4})/(\d+)$").expect("Invalid year/url regex")
    });

    if let Some(caps) = RE.captures(url) {
        let year: u32 = caps[1]
            .parse()
            .map_err(|_| format!("Invalid year in URL: {}", url))?;
        let number: u32 = caps[2]
            .parse()
            .map_err(|_| format!("Invalid number in URL: {}", url))?;
        Ok((year, number))
    } else {
        Err(format!(
            "Could not extract year/number from URL: {}",
            url
        ))
    }
}

/// Parse a string like "2024/123" into (year, number).
fn parse_year_number(s: &str) -> Result<(u32, u32), String> {
    let s = s.trim();

    static RE: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"^(\d{4})/(\d+)$").expect("Invalid year/number regex")
    });

    if let Some(caps) = RE.captures(s) {
        let year: u32 = caps[1]
            .parse()
            .map_err(|_| format!("Invalid year: {}", s))?;
        let number: u32 = caps[2]
            .parse()
            .map_err(|_| format!("Invalid number: {}", s))?;
        Ok((year, number))
    } else {
        Err(format!("Invalid year/number format: '{}'", s))
    }
}

// ---------------------------------------------------------------------------
// Tests — moved to tests/unit/core_test.rs via #[path] pattern
// ---------------------------------------------------------------------------

// Tests — auto-pulled by paper-search-iacr/src/core.rs
// ---------------------------------------------------------------------------
#[cfg(test)]
#[path = "../tests/unit/core_test.rs"]
mod tests;
