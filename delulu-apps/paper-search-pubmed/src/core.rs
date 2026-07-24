//!  Delulu PubMed Paper Search — Core
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

//! # Core — PubMed E-utilities Response Parsers
//!
//! Pure data structures and parsing logic for NCBI E-utilities JSON and text responses.
//! Pure data structures and parsing logic for NCBI E-utilities JSON and text responses.

use serde::Deserialize;
use serde_json::Value;

// ---------------------------------------------------------------------------
// Search query
// ---------------------------------------------------------------------------

/// Search query parameters for the PubMed ESearch API.
#[derive(Debug, Clone)]
pub struct SearchQuery {
    /// Raw query string using PubMed search syntax
    /// (e.g. `"asthma[Title] AND 2023[pdat]"`)
    pub query: String,
    /// Maximum results to return (default: 20)
    pub max_results: Option<u32>,
    /// Sort order: "relevance", "pub_date", "author", "journal"
    pub sort: Option<String>,
}

impl SearchQuery {
    /// Build the query string for the ESearch URL.
    pub fn to_query_string(&self) -> String {
        let mut parts: Vec<String> = Vec::new();

        let encoded_query = urlencoding::encode(&self.query);
        parts.push(format!("term={}", encoded_query));

        if let Some(max) = self.max_results {
            parts.push(format!("retmax={}", max));
        }
        if let Some(sort) = &self.sort {
            parts.push(format!("sort={}", sort));
        }

        parts.join("&")
    }
}

// ---------------------------------------------------------------------------
// Paper data structure
// ---------------------------------------------------------------------------

/// A parsed PubMed paper.
#[derive(Debug, Clone, serde::Serialize)]
pub struct Paper {
    /// PubMed ID
    pub pmid: String,
    /// Paper title
    pub title: String,
    /// List of author names
    pub authors: Vec<String>,
    /// Abstract text (optional — only present when fetched)
    #[serde(rename = "abstract")]
    pub abstract_text: Option<String>,
    /// Journal name
    pub journal: Option<String>,
    /// Publication date string (e.g. "2023 Jan 15")
    pub publication_date: Option<String>,
    /// DOI identifier
    pub doi: Option<String>,
    /// PubMed Central ID (e.g. "PMC1234567")
    pub pmc_id: Option<String>,
}

// ---------------------------------------------------------------------------
// ESearch JSON intermediate deserialization structs
// ---------------------------------------------------------------------------

/// Top-level ESearch JSON response.
#[derive(Debug, Deserialize)]
pub(crate) struct ESearchResponse {
    #[serde(rename = "esearchresult")]
    pub result: ESearchResult,
}

#[derive(Debug, Deserialize)]
pub(crate) struct ESearchResult {
    pub count: String,
    #[serde(rename = "idlist", default)]
    pub id_list: Vec<String>,
}

/// Parsed search result with total count and PMID list.
#[derive(Debug, Clone, serde::Serialize)]
pub struct SearchResult {
    /// Total number of results matching the query
    pub total_count: u64,
    /// List of PMIDs for this page
    pub pmids: Vec<String>,
}

// ---------------------------------------------------------------------------
// ESummary JSON intermediate deserialization structs
// ---------------------------------------------------------------------------

/// Top-level ESummary JSON response.
#[derive(Debug, Deserialize)]
pub(crate) struct ESummaryResponse {
    pub result: ESummaryResult,
}

#[derive(Debug, Deserialize)]
pub(crate) struct ESummaryResult {
    /// Map from PMID -> DocSum, plus a "uids" array
    #[serde(flatten)]
    pub docs: std::collections::HashMap<String, serde_json::Value>,
}

/// A single document summary from ESummary.
#[derive(Debug, Deserialize, Clone, Default)]
pub(crate) struct DocSum {
    pub uid: String,
    pub title: Option<String>,
    pub source: Option<String>,
    pub pubdate: Option<String>,
    pub authors: Option<Vec<DocSumAuthor>>,
    pub elocationid: Option<String>,
    pub history: Option<Vec<DocSumHistory>>,
    #[serde(default, deserialize_with = "deserialize_string_or_int")]
    pub pmcrefcount: Option<String>,
}

#[derive(Debug, Deserialize, Clone)]
pub(crate) struct DocSumAuthor {
    pub name: Option<String>,
    pub authtype: Option<String>,
    pub clusterid: Option<String>,
}

#[derive(Debug, Deserialize, Clone)]
pub(crate) struct DocSumHistory {
    pub pubstatus: Option<String>,
    pub date: Option<String>,
}

// ---------------------------------------------------------------------------
// ELink JSON intermediate deserialization structs
// ---------------------------------------------------------------------------

/// Top-level ELink JSON response.
#[derive(Debug, Deserialize)]
pub(crate) struct ELinkResponse {
    pub linksets: Vec<LinkSet>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct LinkSet {
    pub dbfrom: Option<String>,
    pub ids: Option<Vec<LinkSetId>>,
    pub linksetdbs: Option<Vec<LinkSetDb>>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct LinkSetId {
    pub id: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct LinkSetDb {
    pub dbto: Option<String>,
    pub linkname: Option<String>,
    pub links: Option<Vec<LinkSetLink>>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct LinkSetLink {
    pub id: Option<String>,
}

/// Parsed related articles result.
#[derive(Debug, Clone, serde::Serialize)]
pub struct RelatedArticles {
    /// The input PMIDs
    pub input_pmids: Vec<String>,
    /// Map from input PMID to list of related PMIDs
    pub related: std::collections::HashMap<String, Vec<String>>,
}

// ---------------------------------------------------------------------------
// EInfo JSON intermediate deserialization structs
// ---------------------------------------------------------------------------

/// Top-level EInfo JSON response.
#[derive(Debug, Deserialize)]
pub(crate) struct EInfoResponse {
    #[serde(rename = "einforesult")]
    pub einforesult: EInfoResult,
}

#[derive(Debug, Deserialize)]
pub(crate) struct EInfoResult {
    #[serde(rename = "dbinfo", default)]
    pub dbinfo: Vec<DbInfo>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct DbInfo {
    pub dbname: Option<String>,
    pub menuname: Option<String>,
    pub description: Option<String>,
    pub count: Option<String>,
    pub lastupdate: Option<String>,
    pub fields: Option<Vec<FieldInfo>>,
    pub links: Option<Vec<LinkInfo>>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct FieldInfo {
    pub name: Option<String>,
    pub fullname: Option<String>,
    pub description: Option<String>,
    pub termcount: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct LinkInfo {
    pub name: Option<String>,
    pub menuname: Option<String>,
    pub description: Option<String>,
    pub targetdb: Option<String>,
}

/// Parsed database info result.
#[derive(Debug, Clone, serde::Serialize)]
pub struct DatabaseInfo {
    /// Database name
    pub db_name: String,
    /// Menu name
    pub menu_name: String,
    /// Description
    pub description: String,
    /// Total record count
    pub record_count: u64,
    /// Last update date
    pub last_update: String,
    /// Available search fields
    pub fields: Vec<FieldDef>,
}

/// A search field definition.
#[derive(Debug, Clone, serde::Serialize)]
pub struct FieldDef {
    pub name: String,
    pub full_name: String,
    pub description: String,
}

// ---------------------------------------------------------------------------
// ECitMatch response parsing
// ---------------------------------------------------------------------------

/// Parsed citation match result.
#[derive(Debug, Clone, serde::Serialize)]
pub struct CitationMatch {
    /// The citation key provided in the input
    pub key: String,
    /// The matched PMID (or empty if no match)
    pub pmid: String,
}

// ---------------------------------------------------------------------------
// Public parsers
// ---------------------------------------------------------------------------

/// Deserialize a JSON value that may be either a string or an integer into Option<String>.
fn deserialize_string_or_int<'de, D: serde::Deserializer<'de>>(
    d: D,
) -> Result<Option<String>, D::Error> {
    match Value::deserialize(d) {
        Ok(Value::String(s)) => Ok(Some(s)),
        Ok(Value::Number(n)) => Ok(Some(n.to_string())),
        Ok(Value::Null) => Ok(None),
        _ => Ok(None),
    }
}

/// Parse an ESearch JSON response into a `SearchResult`.
///
/// # Errors
///
/// Returns an error if the JSON is malformed or required fields are missing.
pub fn parse_search_json(json: &str) -> Result<SearchResult, String> {
    let resp: ESearchResponse =
        serde_json::from_str(json).map_err(|e| format!("JSON parse error: {}", e))?;

    let count: u64 = resp.result.count.parse().unwrap_or_else(|_| {
        tracing::warn!(
            "Failed to parse PubMed count '{}', falling back to id_list.len() ({})",
            resp.result.count,
            resp.result.id_list.len(),
        );
        resp.result.id_list.len() as u64
    });

    Ok(SearchResult {
        total_count: count,
        pmids: resp.result.id_list,
    })
}

/// Parse an ESummary JSON response into a list of `Paper`s.
///
/// # Errors
///
/// Returns an error if the JSON is malformed.
pub fn parse_summary_json(json: &str) -> Result<Vec<Paper>, String> {
    let resp: ESummaryResponse =
        serde_json::from_str(json).map_err(|e| format!("JSON parse error: {}", e))?;

    let uids: Vec<String> = resp
        .result
        .docs
        .get("uids")
        .and_then(|v| serde_json::from_value::<Vec<String>>(v.clone()).ok())
        .unwrap_or_default();

    let mut papers = Vec::with_capacity(uids.len());

    for uid in &uids {
        let doc_value = match resp.result.docs.get(uid) {
            Some(v) => v,
            None => continue,
        };

        let doc: DocSum = match serde_json::from_value(doc_value.clone()) {
            Ok(d) => d,
            Err(_) => continue,
        };

        let title = doc.title.clone().unwrap_or_default();
        let authors: Vec<String> = doc
            .authors
            .clone()
            .unwrap_or_default()
            .iter()
            .filter_map(|a| a.name.clone())
            .collect();

        // Extract DOI from elocationid
        let doi = doc.elocationid.as_ref().and_then(|eid| {
            if eid.starts_with("doi: ") || eid.starts_with("doi:") {
                Some(
                    eid.trim_start_matches("doi: ")
                        .trim_start_matches("doi:")
                        .to_string(),
                )
            } else {
                None
            }
        });

        // Extract PMC ID from history or other fields
        let pmc_id = extract_pmc_id(&doc);

        papers.push(Paper {
            pmid: doc.uid,
            title,
            authors,
            abstract_text: None,
            journal: doc.source,
            publication_date: doc.pubdate,
            doi,
            pmc_id,
        });
    }

    Ok(papers)
}

/// Extract PMC ID from a DocSum.
fn extract_pmc_id(doc: &DocSum) -> Option<String> {
    // PMC ID might be in the elocationid as "doi: 10.xxxx/PMC1234567"
    if let Some(ref eid) = doc.elocationid
        && eid.to_lowercase().contains("pmc")
    {
        let parts: Vec<&str> = eid.split('/').collect();
        for part in parts {
            let trimmed = part.trim();
            if trimmed.to_uppercase().starts_with("PMC") {
                return Some(trimmed.to_string());
            }
        }
    }
    None
}

/// Parse abstract text from NCBI EFetch medline format.
///
/// The medline format from EFetch with rettype=medline:
/// ```text
/// PMID- 38742940
/// AB  - BACKGROUND: Biofeedback-based virtual reality...
///      More abstract text...
/// ```
///
/// Returns a list of (pmid, abstract_text) tuples.
pub fn parse_abstract_text(text: &str) -> Vec<(String, String)> {
    let mut results: Vec<(String, String)> = Vec::new();
    let mut current_pmid: Option<String> = None;
    let mut current_abstract: Vec<String> = Vec::new();
    let mut collecting = false;

    for line in text.lines() {
        if line.starts_with("PMID- ") {
            if let Some(pmid) = current_pmid.take() {
                let abstract_text = current_abstract.join(" ").trim().to_string();
                if !abstract_text.is_empty() {
                    results.push((pmid, abstract_text));
                }
                current_abstract.clear();
            }
            current_pmid = Some(
                line.strip_prefix("PM  -")
                    .unwrap_or(line)
                    .trim()
                    .to_string(),
            );
            collecting = false;
            continue;
        }

        if line.starts_with("AB  -") {
            collecting = true;
            let rest = line
                .strip_prefix("AB  -")
                .unwrap_or(line)
                .trim()
                .to_string();
            if !rest.is_empty() {
                current_abstract.push(rest);
            }
            continue;
        }

        if collecting && line.starts_with("      ") {
            let text = line.trim().to_string();
            if !text.is_empty() {
                current_abstract.push(text);
            }
            continue;
        }

        if collecting && !line.is_empty() {
            collecting = false;
        }
    }

    if let Some(pmid) = current_pmid {
        let abstract_text = current_abstract.join(" ").trim().to_string();
        if !abstract_text.is_empty() {
            results.push((pmid, abstract_text));
        }
    }

    results
}

/// Parse an ELink JSON response into a `RelatedArticles`.
///
/// # Errors
///
/// Returns an error if the JSON is malformed.
pub fn parse_elink_json(json: &str) -> Result<RelatedArticles, String> {
    let resp: ELinkResponse =
        serde_json::from_str(json).map_err(|e| format!("JSON parse error: {}", e))?;

    let mut input_pmids: Vec<String> = Vec::new();
    let mut seen_input: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut related: std::collections::HashMap<String, Vec<String>> =
        std::collections::HashMap::new();

    for linkset in &resp.linksets {
        // Collect input PMIDs (global for output)
        if let Some(ref ids) = linkset.ids {
            for id_entry in ids {
                if let Some(ref id) = id_entry.id
                    && seen_input.insert(id.clone())
                {
                    input_pmids.push(id.clone());
                }
            }
        }

        // Collect this LinkSet's input PMIDs for association (local, not accumulated)
        let current_ids: Vec<String> = linkset
            .ids
            .as_ref()
            .map(|ids| {
                ids.iter()
                    .filter_map(|id_entry| id_entry.id.clone())
                    .collect()
            })
            .unwrap_or_default();

        // Collect related PMIDs
        if let Some(ref linksetdbs) = linkset.linksetdbs {
            for lsdb in linksetdbs {
                if let Some(ref links) = lsdb.links {
                    let pmids: Vec<String> = links.iter().filter_map(|l| l.id.clone()).collect();

                    // Associate with each input PMID from THIS LinkSet only
                    for input_id in &current_ids {
                        let entry = related.entry(input_id.clone()).or_default();
                        let mut seen_related: std::collections::HashSet<String> =
                            entry.iter().cloned().collect();
                        for pmid in &pmids {
                            if seen_related.insert(pmid.clone()) {
                                entry.push(pmid.clone());
                            }
                        }
                    }
                }
            }
        }
    }

    Ok(RelatedArticles {
        input_pmids,
        related,
    })
}

/// Parse an EInfo JSON response into a `DatabaseInfo`.
///
/// # Errors
///
/// Returns an error if the JSON is malformed.
pub fn parse_einfo_json(json: &str) -> Result<DatabaseInfo, String> {
    let resp: EInfoResponse =
        serde_json::from_str(json).map_err(|e| format!("JSON parse error: {e}"))?;

    let dinfo = resp
        .einforesult
        .dbinfo
        .into_iter()
        .next()
        .ok_or_else(|| "Missing dbinfo in EInfo response".to_string())?;

    let db_name = dinfo.dbname.unwrap_or_default();
    let menu_name = dinfo.menuname.unwrap_or_default();
    let description = dinfo.description.unwrap_or_default();
    let count: u64 = dinfo.count.as_deref().unwrap_or("0").parse().unwrap_or(0);
    let last_update = dinfo.lastupdate.unwrap_or_default();

    let fields = dinfo
        .fields
        .unwrap_or_default()
        .iter()
        .map(|f| FieldDef {
            name: f.name.clone().unwrap_or_default(),
            full_name: f.fullname.clone().unwrap_or_default(),
            description: f.description.clone().unwrap_or_default(),
        })
        .collect();

    Ok(DatabaseInfo {
        db_name,
        menu_name,
        description,
        record_count: count,
        last_update,
        fields,
    })
}

/// Parse an ECitMatch plain-text response.
///
/// The response format is:
/// ```text
/// journal|year|volume|first_page|author|key|pmid
/// ```
pub fn parse_ecitmatch_text(text: &str) -> Vec<CitationMatch> {
    let mut results = Vec::new();

    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        let parts: Vec<&str> = trimmed.split('|').collect();
        if parts.len() >= 7 {
            let key = parts[5].trim().to_string();
            let pmid = parts[6].trim().to_string();
            results.push(CitationMatch { key, pmid });
        }
    }

    results
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

// Tests — auto-pulled by paper-search-pubmed/src/core.rs
// ---------------------------------------------------------------------------
#[cfg(test)]
#[path = "../tests/unit/core_test.rs"]
mod tests;
