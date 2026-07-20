pub mod core;
pub use crate::core::types;

pub mod generators;
pub mod pipelines;
pub mod sources;

pub use crate::core::detect::detect_source_type;
pub use crate::core::http_client::WebbfetchClient;
pub use crate::core::types::{ExtractionResult, MarkdownDocument, RedditComment};

use crate::core::types::{SourceType, WebbfetchError};
use crate::pipelines::DomNode;
use crate::pipelines::PassFn;
// ---------------------------------------------------------------------------
// fetch_and_extract
// ---------------------------------------------------------------------------

/// Fetch a URL and extract content based on the detected source type.
///
/// Uses a two-step fetch for Discourse URLs:
/// 1. First fetch: raw HTML (from the original URL)
/// 2. Content detection via `detect_from_content()` checks for Discourse markers
/// 3. If Discourse detected, second fetch to `.json` API endpoint for structured data
/// 4. If JSON fetch fails, falls back to GenericHtml extraction
///
/// Dispatching:
/// - Reddit: URL-based detection → immediate dispatch (no content detection needed)
/// - Discourse: URL returns GenericHtml → content detection → second fetch → parse JSON
/// - GenericHtml: URL returns GenericHtml → content detection → pipeline → lower
pub async fn fetch_and_extract(
    url: &str,
    client: &crate::core::http_client::WebbfetchClient,
    pipeline: &[PassFn],
) -> Result<ExtractionResult, WebbfetchError> {
    // Step 1: URL-based detection (primary dispatch — only returns Reddit or GenericHtml now)
    let url_source_type = detect_source_type(url);

    // Step 2: Fetch from HTTP layer (Reddit URLs already transformed at HTTP layer)
    // Note: The HTTP layer always stores the raw response body as GenericHtml,
    // regardless of the actual source type. The body is extracted here and
    // dispatched to the appropriate parser.
    let fetch_result = client.fetch(url).await?;

    // Step 3: If Reddit, dispatch immediately — no content detection needed
    if url_source_type == SourceType::Reddit {
        match &fetch_result.content {
            ExtractionResult::GenericHtml { content_md } => {
                let body = content_md.body.clone();
                let data = sources::reddit::RedditExtractor::extract(&body)?;
                return Ok(ExtractionResult::Reddit {
                    title: data.title,
                    selftext: data.selftext,
                    author: data.author,
                    score: data.score,
                    permalink: data.permalink,
                    comments: data.comments,
                });
            }
            ExtractionResult::Reddit { .. } => return Ok(fetch_result.content),
            other => {
                tracing::warn!(
                    "fetch_and_extract: unexpected content type {:?} for Reddit URL, falling back to GenericHtml",
                    other
                );
                // Fall through to non-Reddit processing below
            }
        }
    }

    // Step 4: For non-Reddit URLs, extract body and run content detection
    let body = match &fetch_result.content {
        ExtractionResult::GenericHtml { content_md } => content_md.body.clone(),
        other => {
            tracing::warn!(
                "fetch_and_extract: unexpected content type {:?} for non-Reddit URL, falling back to GenericHtml",
                other
            );
            return Err(WebbfetchError::Pass(format!(
                "fetch_and_extract: unexpected content type {:?} for non-Reddit URL",
                other
            )));
        }
    };

    // Step 5: Detect from content (checks for Discourse markers in HTML)
    let content_type = crate::core::detect::detect_from_content(&body);

    match content_type {
        Some(SourceType::Discourse) => {
            // Step 5a: Second fetch — get Discourse JSON API
            let api_url = crate::core::detect::discourse_url_to_api_url(url);

            let api_result = match client.fetch(&api_url).await {
                Ok(r) => r,
                Err(e) => {
                    tracing::warn!(
                        "Discourse JSON API fetch failed: {e}; falling back to GenericHtml"
                    );
                    return fallback_to_generic_html(url, body, pipeline);
                }
            };

            let api_body = match &api_result.content {
                ExtractionResult::GenericHtml { content_md } => content_md.body.clone(),
                other => {
                    tracing::warn!(
                        "Discourse API returned unexpected content type: {:?}; falling back to GenericHtml",
                        other
                    );
                    return fallback_to_generic_html(url, body, pipeline);
                }
            };

            // Step 5b: Parse Discourse JSON
            let data = sources::discourse::DiscourseExtractor::extract(&api_body)?;
            Ok(ExtractionResult::Discourse {
                title: data.title,
                topic_id: data.topic_id,
                posts: data.posts,
            })
        }
        // No Discourse detected — treat as GenericHtml
        _ => fallback_to_generic_html(url, body, pipeline),
    }
}

// ---------------------------------------------------------------------------
// extract
// ---------------------------------------------------------------------------

/// Extract content from a URL using the default Mozilla Readability pipeline.
pub async fn extract(
    url: &str,
    client: &crate::core::http_client::WebbfetchClient,
) -> Result<ExtractionResult, WebbfetchError> {
    fetch_and_extract(
        url,
        client,
        &[crate::pipelines::mozilla_readability::filter_mozilla_readability],
    )
    .await
}

// ---------------------------------------------------------------------------
// fallback_to_generic_html
// ---------------------------------------------------------------------------

/// Shared logic for GenericHtml extraction.
///
/// # Precondition
/// - `url` is the original source URL (used for frontmatter `source_url`).
/// - `body` is the raw HTML string.
/// - `pipeline` is a slice of `PassFn` passes.
///
/// # Postcondition
/// - Returns `Ok(ExtractionResult::GenericHtml { content_md })` with frontmatter.
/// - Pipeline passes may panic on logic bugs (intentional pre-alpha behavior).
fn fallback_to_generic_html(
    url: &str,
    body: String,
    pipeline: &[PassFn],
) -> Result<ExtractionResult, WebbfetchError> {
    let mut dom = pipelines::parse_html(&body)?;
    for pass in pipeline {
        pass(&mut dom);
    }
    let content_md = generators::gen_md::MarkdownLowerer::lower(&dom, None);
    let title = extract_title(&dom);

    Ok(ExtractionResult::GenericHtml {
        content_md: MarkdownDocument {
            frontmatter: format!(
                "title: {}\nsource_type: generic_html\nsource_url: {}",
                title, url
            ),
            body: content_md,
        },
    })
}

// ---------------------------------------------------------------------------
// extract_title
// ---------------------------------------------------------------------------

/// Extract the page title from a DOM tree.
///
/// Searches for the first `<h1>` element and returns its text content.
/// Falls back to the first `<title>` element if no `<h1>` is found.
/// Returns an empty string if neither is present.
pub fn extract_title(dom: &DomNode) -> String {
    // First pass: search for <h1>
    if let Some(title) = find_first_heading(dom, "h1") {
        return title;
    }
    // Fallback: search for <title>
    find_first_heading(dom, "title").unwrap_or_default()
}

/// Recursively find the first element with the given tag and return its text content.
fn find_first_heading(node: &DomNode, tag: &str) -> Option<String> {
    match node {
        DomNode::Element {
            tag: t, children, ..
        } if t == tag => {
            let text = collect_text_from_nodes(children);
            let trimmed = text.trim().to_string();
            if !trimmed.is_empty() {
                return Some(trimmed);
            }
        }
        DomNode::Element { children, .. } => {
            for child in children {
                if let found @ Some(_) = find_first_heading(child, tag) {
                    return found;
                }
            }
        }
        _ => {}
    }
    None
}

/// Collect all descendant text nodes into a single string.
fn collect_text_from_nodes(nodes: &[DomNode]) -> String {
    let mut buf = String::new();
    for node in nodes {
        match node {
            DomNode::Text(t) => buf.push_str(t),
            DomNode::Element { children, .. } => {
                buf.push_str(&collect_text_from_nodes(children));
            }
            _ => {}
        }
    }
    buf
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[path = "lib_test.rs"]
mod tests;
