//! Pure data structures and parsing logic for the arXiv API Atom XML responses.
//! No I/O — suitable for testing without network access.

use chrono::NaiveDate;
use serde::Serialize;

#[cfg_attr(feature = "mcp", derive(schemars::JsonSchema))]
/// A paper from the arXiv API.
#[derive(Debug, Clone, Serialize)]
pub struct Paper {
    pub id: String,
    pub title: String,
    pub authors: Vec<String>,
    #[serde(rename = "abstract")]
    pub abstract_text: String,
    pub comment: Option<String>,
    pub journal_ref: Option<String>,
    pub doi: Option<String>,
    pub primary_category: String,
    pub categories: Vec<String>,
    pub published: NaiveDate,
    pub updated: NaiveDate,
    /// Abstract page URL (always available).
    pub abs_url: String,
    /// Full HTML version URL (arxiv.org/html/<id>), if available.
    pub html_url: Option<String>,
    pub pdf_url: String,
}

/// Search parameters for the arXiv API.
#[derive(Debug, Clone)]
pub struct SearchQuery {
    pub query: String,
    pub max_results: Option<u32>,
    pub start: Option<u32>,
    pub sort_by: Option<String>,
    pub sort_order: Option<String>,
}

impl SearchQuery {
    pub fn new(query: impl Into<String>) -> Self {
        Self {
            query: query.into(),
            max_results: None,
            start: None,
            sort_by: None,
            sort_order: None,
        }
    }

    pub fn to_query_string(&self) -> String {
        let mut parts = vec![format!("search_query={}", urlencoding::encode(&self.query))];
        if let Some(m) = self.max_results {
            let capped = m.min(2000);
            if capped != m {
                tracing::warn!("max_results capped at 2000 (requested: {})", m);
            }
            parts.push(format!("max_results={}", capped));
        }
        if let Some(s) = self.start {
            parts.push(format!("start={}", s));
        }
        if let Some(s) = &self.sort_by {
            parts.push(format!("sortBy={}", s));
        }
        if let Some(s) = &self.sort_order {
            parts.push(format!("sortOrder={}", s));
        }
        parts.join("&")
    }
}

/// Normalize arXiv Atom XML by removing namespace declarations and renaming
/// prefixed elements (e.g. `arxiv:comment` → `arxiv_comment`).
///
/// Strips xmlns attributes from both root elements (`<feed xmlns=...>`),
/// where the preceding character is `<`, and nested elements
/// (`<entry xmlns=...>`) where the preceding character is whitespace.
/// Only matches inside XML tags to avoid false positives in text content.
fn normalize_xml(raw: &str) -> String {
    let mut result = String::with_capacity(raw.len());
    let chars: Vec<char> = raw.chars().collect();
    let len = chars.len();
    let mut i = 0;
    let mut in_tag = false;

    while i < len {
        // Track whether we're inside an XML tag
        if chars[i] == '<' {
            in_tag = true;
        } else if chars[i] == '>' {
            in_tag = false;
        }

        // Check for xmlns declarations (only inside tags)
        // Preceded by whitespace (<elem xmlns=...>) or by < (<feed xmlns=...>)
        if in_tag
            && i + 5 < len
            && (chars[i].is_ascii_whitespace() || chars[i] == '<')
            && chars[i + 1] == 'x'
            && chars[i + 2] == 'm'
            && chars[i + 3] == 'l'
            && chars[i + 4] == 'n'
            && chars[i + 5] == 's'
        {
            // Skip past the entire xmlns="..." or xmlns:prefix="..."
            // Find the end of the attribute value
            let mut attr_end = i + 6;
            let mut in_value = false;
            let mut quote_char = '"';
            while attr_end < len {
                if !in_value {
                    if chars[attr_end] == '=' {
                        in_value = true;
                    }
                } else if chars[attr_end] == '"' || chars[attr_end] == '\'' {
                    if quote_char == '"' {
                        quote_char = chars[attr_end];
                    } else if chars[attr_end] == quote_char {
                        // End of attribute value
                        i = attr_end + 1;
                        break;
                    }
                }
                attr_end += 1;
            }
            if attr_end >= len {
                result.push(chars[i]);
                i += 1;
            }
            continue;
        }

        // Rename prefixed elements: <arxiv:comment> → <arxiv_comment>
        if chars[i] == '<' {
            // Look ahead for a colon in the tag name
            let tag_start = if i + 1 < len && (chars[i + 1] == '/' || chars[i + 1] == '?') {
                i + 2
            } else {
                i + 1
            };
            let mut colon_pos = None;
            for j in tag_start..len {
                if chars[j] == '>' || chars[j] == ' ' || chars[j] == '/' || chars[j] == '?' {
                    break;
                }
                if chars[j] == ':' {
                    colon_pos = Some(j);
                    break;
                }
            }
            if let Some(col) = colon_pos {
                // Copy everything before the colon, replace colon with underscore, skip colon
                for k in i..col {
                    result.push(chars[k]);
                }
                result.push('_');
                i = col + 1;
                continue;
            }
        }

        result.push(chars[i]);
        i += 1;
    }

    result
}

/// Parse an arXiv API Atom XML response into a list of papers.
///
/// Uses a state-machine XML parser to handle repeated elements correctly.
pub fn parse_atom_response(xml: &str) -> Result<Vec<Paper>, String> {
    let normalized = normalize_xml(xml);
    let mut papers = Vec::new();

    // Simple line-by-line and tag-based parser for the normalized XML
    // This avoids the repeated-element issues with serde XML deserializers.

    // Strategy: extract each <entry>...</entry> block and parse it separately
    let mut entries = Vec::new();
    let mut depth = 0i32;
    let mut entry_start = None;

    let chars: Vec<char> = normalized.chars().collect();
    let mut pos = 0;
    while pos < chars.len() {
        if pos + 6 < chars.len() && &chars[pos..pos+6] == ['<','e','n','t','r','y'] {
            let after_tag = pos + 6;
            if after_tag < chars.len() && (chars[after_tag] == '>' || chars[after_tag] == ' ' || chars[after_tag] == '/' || chars[after_tag] == '\n' || chars[after_tag] == '\r' || chars[after_tag] == '\t') {
                if depth == 0 {
                    entry_start = Some(pos);
                }
                depth += 1;
            }
        }
        if pos + 7 < chars.len() && &chars[pos..pos+7] == ['<','/','e','n','t','r','y'] {
            depth -= 1;
            if depth == 0 {
                if let Some(start) = entry_start {
                    let entry_xml: String = chars[start..=pos+7].iter().collect();
                    entries.push(entry_xml);
                }
                entry_start = None;
            }
        }
        pos += 1;
    }

    for entry_xml in &entries {
        match parse_single_entry(entry_xml) {
            Ok(Some(paper)) => papers.push(paper),
            Ok(None) => {} // skip empty entries
            Err(e) => return Err(format!("Entry parse error: {e}")),
        }
    }

    Ok(papers)
}

fn parse_single_entry(xml: &str) -> Result<Option<Paper>, String> {
    let id = extract_tag_content(xml, "id");
    let id = match id {
        Some(s) => extract_arxiv_id(&s),
        None => return Ok(None),
    };

    let title = extract_tag_content(xml, "title")
        .map(|s| html_unescape(&s))
        .ok_or_else(|| format!("missing title for paper {}", id))?;

    let summary = extract_tag_content(xml, "summary")
        .map(|s| html_unescape(&s))
        .ok_or_else(|| format!("missing summary for paper {}", id))?;

    let published = extract_tag_content(xml, "published")
        .unwrap_or_default();

    let updated = extract_tag_content(xml, "updated")
        .unwrap_or_default();

    let comment = extract_tag_content(xml, "arxiv_comment");
    let journal_ref = extract_tag_content(xml, "arxiv_journal_ref");
    let doi = extract_tag_content(xml, "arxiv_doi");

    // Authors: extract all <name> inside <author> blocks
    let authors: Vec<String> = extract_all_tag_contents(xml, "author", "name")
        .into_iter()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();

    // Categories: extract term attributes from <category> tags
    let categories: Vec<String> = extract_attributes(xml, "category", "term");

    // Primary category: extract term attribute from <arxiv_primary_category>
    let primary_category = extract_attributes(xml, "arxiv_primary_category", "term")
        .into_iter()
        .next()
        .ok_or_else(|| format!("missing primary_category for paper {id}"))?;

    // Links
    let abs_url = extract_link_by_rel(xml, "alternate")
        .unwrap_or_else(|| format!("https://arxiv.org/abs/{id}"));

    // Full HTML version (arxiv.org/html/<id>), may 404 for some papers
    let html_url = Some(format!("https://arxiv.org/html/{id}"));

    let pdf_url = extract_link_by_rel(xml, "related")
        .or_else(|| extract_link_by_type(xml, "application/pdf"))
        .unwrap_or_else(|| format!("https://arxiv.org/pdf/{id}"));

    if published.is_empty() {
        return Err(format!("missing published date for paper {}", id));
    }
    let published = parse_arxiv_date(&published)?;
    let updated = if updated.is_empty() {
        published
    } else {
        parse_arxiv_date(&updated)?
    };

    Ok(Some(Paper {
        id, title, authors,
        abstract_text: summary,
        comment, journal_ref, doi,
        primary_category, categories,
        published, updated,
        abs_url, html_url, pdf_url,
    }))
}

/// Extract text content of a tag (non-recursive, first occurrence).
fn extract_tag_content(xml: &str, tag: &str) -> Option<String> {
    let open = format!("<{}>", tag);
    let close = format!("</{}>", tag);
    if let Some(start) = xml.find(&open) {
        let content_start = start + open.len();
        if let Some(end) = xml[content_start..].find(&close) {
            return Some(xml[content_start..content_start + end].to_string());
        }
    }
    None
}

/// Extract text content of a child tag within all parent blocks.
fn extract_all_tag_contents(xml: &str, parent: &str, child: &str) -> Vec<String> {
    let mut results = Vec::new();
    let parent_open = format!("<{}>", parent);
    let parent_close = format!("</{}>", parent);
    let child_open = format!("<{}>", child);
    let child_close = format!("</{}>", child);

    let mut search_start = 0;
    while let Some(ps) = xml[search_start..].find(&parent_open) {
        let abs_ps = search_start + ps;
        if let Some(pc) = xml[abs_ps..].find(&parent_close) {
            let parent_content = &xml[abs_ps..abs_ps + pc];
            if let Some(cs) = parent_content.find(&child_open) {
                let cc_start = cs + child_open.len();
                if let Some(ce) = parent_content[cc_start..].find(&child_close) {
                    results.push(parent_content[cc_start..cc_start + ce].to_string());
                }
            }
            search_start = abs_ps + pc + parent_close.len();
        } else {
            break;
        }
    }
    results
}

/// Extract attribute values from all tags with the given name.
fn extract_attributes(xml: &str, tag: &str, attr: &str) -> Vec<String> {
    let mut results = Vec::new();
    let mut search_start = 0;
    let tag_open = format!("<{}", tag);

    while let Some(pos) = xml[search_start..].find(&tag_open) {
        let abs_pos = search_start + pos;
        // Find the end of this tag
        if let Some(end) = xml[abs_pos..].find('>') {
            let tag_content = &xml[abs_pos..abs_pos + end];
            // Look for attr="value"
            let search_attr = format!("{}=\"", attr);
            if let Some(ap) = tag_content.find(&search_attr) {
                let val_start = ap + search_attr.len();
                if let Some(quote_end) = tag_content[val_start..].find('"') {
                    results.push(tag_content[val_start..val_start + quote_end].to_string());
                }
            }
            search_start = abs_pos + end + 1;
        } else {
            break;
        }
    }
    results
}

/// Extract href from a <link> tag with the given rel attribute.
fn extract_link_by_rel(xml: &str, rel: &str) -> Option<String> {
    let mut search_start = 0;
    while let Some(pos) = xml[search_start..].find("<link") {
        let abs_pos = search_start + pos;
        if let Some(end) = xml[abs_pos..].find('>') {
            let tag = &xml[abs_pos..abs_pos + end];
            let rel_search = format!("rel=\"{}\"", rel);
            if tag.contains(&rel_search) {
                if let Some(hp) = tag.find("href=\"") {
                    let val_start = hp + 6;
                    if let Some(qe) = tag[val_start..].find('"') {
                        return Some(tag[val_start..val_start + qe].to_string());
                    }
                }
            }
            search_start = abs_pos + end + 1;
        } else {
            break;
        }
    }
    None
}

/// Extract href from a <link> tag with the given type attribute.
fn extract_link_by_type(xml: &str, type_: &str) -> Option<String> {
    let mut search_start = 0;
    while let Some(pos) = xml[search_start..].find("<link") {
        let abs_pos = search_start + pos;
        if let Some(end) = xml[abs_pos..].find('>') {
            let tag = &xml[abs_pos..abs_pos + end];
            let type_search = format!("type=\"{}\"", type_);
            if tag.contains(&type_search) {
                if let Some(hp) = tag.find("href=\"") {
                    let val_start = hp + 6;
                    if let Some(qe) = tag[val_start..].find('"') {
                        return Some(tag[val_start..val_start + qe].to_string());
                    }
                }
            }
            search_start = abs_pos + end + 1;
        } else {
            break;
        }
    }
    None
}

/// Extract arXiv ID from a URL like `http://arxiv.org/abs/2301.12345v2`.
fn extract_arxiv_id(url: &str) -> String {
    let id = url.trim_start_matches("http://arxiv.org/abs/")
        .trim_start_matches("https://arxiv.org/abs/")
        .split('?').next().unwrap_or("")
        .split('#').next().unwrap_or("");
    // Strip version suffix (e.g. "2301.12345v2" → "2301.12345")
    if let Some(v_pos) = id.rfind('v') {
        if v_pos > 0 && id[v_pos..].len() > 1 && id[v_pos+1..].chars().all(|c| c.is_ascii_digit()) {
            return id[..v_pos].to_string();
        }
    }
    id.to_string()
}

/// Parse an arXiv date string (ISO 8601) into a NaiveDate.
fn parse_arxiv_date(s: &str) -> Result<NaiveDate, String> {
    // Handle ISO 8601 format: "2023-01-20T18:30:00Z" or "2023-01-20"
    let date_part = s.split('T').next().unwrap_or(s);
    NaiveDate::parse_from_str(date_part, "%Y-%m-%d")
        .map_err(|e| format!("invalid date '{s}': {e}"))
}

/// Basic HTML entity unescaping for arXiv titles/abstracts.
fn html_unescape(s: &str) -> String {
    s.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
}

#[cfg(test)]
#[path = "../tests/unit/core_test.rs"]  // auto-pulled by paper-search-arxiv/src/core.rs
mod tests;
