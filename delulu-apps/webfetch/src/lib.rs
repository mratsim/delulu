pub mod core;
pub use crate::core::types;
pub use crate::core::types::{ExtractionResult, MarkdownDocument, RedditComment};

pub mod generators;
pub mod pipelines;
pub mod sources;

pub use crate::core::detect::detect_source_type;
pub use crate::core::types::WebfetchError;

use crate::core::types::{SourceType};
use crate::pipelines::DomNode;
use crate::pipelines::PassFn;
use delulu_rate_limited_crawler::RateLimitedCrawler;
use std::io::Write;
use std::time::Duration;
use tempfile::Builder;
use xberg::{extract as xberg_extract, ExtractInput, ExtractionConfig, OutputFormat};

/// Maximum response body size (50 MB).
pub const MAX_BODY_SIZE: usize = 50 * 1024 * 1024;


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
/// - ArxivPdf: URL-based detection → rewrite to HTML URL → fetch HTML → filter_arxiv → lower to markdown
/// - Document: URL-based detection → call fetch_doc() directly (no HTTP fetch needed)
/// - GenericHtml: URL returns GenericHtml → MIME type check → content detection → pipeline → lower
pub async fn fetch_and_extract(
    url: &str,
    crawler: &RateLimitedCrawler,
    pipeline: &[PassFn],
) -> Result<ExtractionResult, WebfetchError> {
    // Step 1: URL-based detection (primary dispatch)
    let url_source_type = detect_source_type(url);

    // Step 2: Check source type BEFORE HTTP fetch
    match url_source_type {
        SourceType::ArxivPdf => {
            // Rewrite arXiv PDF/abs URL to HTML abstract page
            let html_url = crate::core::detect::arxiv_url_to_html_url(url)?;
            let (body, _) = fetch_url_text(&html_url, crawler).await?;
            let mut dom = pipelines::parse_html(&body)?;
            pipelines::dl_arxiv::filter_arxiv(&mut dom);
            let content_md = generators::gen_md::MarkdownLowerer::lower(&dom, None);
            let title = extract_title(&dom);
            return Ok(ExtractionResult::GenericHtml {
                content_md: MarkdownDocument {
                    frontmatter: format!(
                        "title: {}\nsource_type: generic_html\nsource_url: {}",
                        title, url
                    ),
                    body: content_md,
                },
            });
        }
        SourceType::Document => {
            // Direct document fetch via xberg — no HTTP fetch needed
            return fetch_doc(url, crawler).await;
        }
        _ => {
            // Reddit and GenericHtml: proceed to HTTP fetch below
        }
    }
    // Step 3: Transform Reddit URL to API URL, then fetch
    let fetch_url = if url_source_type == SourceType::Reddit {
        crate::core::detect::reddit_url_to_api_url(url)
    } else {
        url.to_string()
    };
    let (body, content_type) = fetch_url_text(&fetch_url, crawler).await?;
    // Step 4: If Reddit, dispatch immediately — no content detection needed
    if url_source_type == SourceType::Reddit {
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

    // Step 5: MIME-type based document detection (best-effort, body already read as UTF-8)
    if let Some(mime) = &content_type {
        if !mime.is_empty() && crate::core::detect::detect_from_mime_type(mime).is_some() {
            tracing::warn!(
                "Content-Type indicates document ({mime}) but body was already consumed as UTF-8; falling through to GenericHtml path"
            );
        } else if !mime.is_empty() {
            tracing::debug!(
                "Content-Type is '{}' — not a recognized document MIME type; treating as GenericHtml",
                mime
            );
        }
    } else {
        tracing::debug!("No Content-Type header; treating response as GenericHtml");
    }

    // Step 6: Detect from content (checks for Discourse markers in HTML)
    let detected = crate::core::detect::detect_from_content(&body);

    match detected {
        Some(SourceType::Discourse) => {
            // Step 6a: Second fetch — get Discourse JSON API
            let api_url = crate::core::detect::discourse_url_to_api_url(url);

            let api_body = match fetch_url_text(&api_url, crawler).await {
                Ok((body, _)) => body,
                Err(e) => {
                    tracing::warn!(
                        "Discourse JSON API fetch failed: {e}; falling back to GenericHtml"
                    );
                    return fallback_to_generic_html(url, body, pipeline);
                }
            };

            // Step 6b: Parse Discourse JSON
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
    crawler: &RateLimitedCrawler,
) -> Result<ExtractionResult, WebfetchError> {
    fetch_and_extract(
        url,
        crawler,
        &[crate::pipelines::mozilla_readability::filter_mozilla_readability],
    )
    .await
}

// ---------------------------------------------------------------------------
// xberg_html_to_markdown
// ---------------------------------------------------------------------------

/// Convert HTML (from xberg PDF/text conversion) into Markdown.
///
/// This function:
/// 1. Parses the HTML into a DOM tree via `parse_html()`
/// 2. Applies the `filter_doc` cleaning pass (removes scripts, styles, empty elements)
/// 3. Lowers the cleaned DOM to Markdown via `MarkdownLowerer::lower()`
///
/// # Arguments
/// - `xberg_html`: HTML string from xberg document conversion
/// - `base_url`: Optional base URL for resolving relative links
///
/// # Returns
/// - `Ok(markdown_string)` on success
/// - `Err(WebfetchError::Parse(msg))` if HTML parsing fails
pub fn xberg_html_to_markdown(
    xberg_html: &str,
    base_url: Option<&str>,
) -> Result<String, WebfetchError> {
    let mut dom = pipelines::parse_html(xberg_html)?;
    pipelines::dl_doc::filter_doc(&mut dom);
    let markdown = generators::gen_md::MarkdownLowerer::lower(&dom, base_url);
    Ok(markdown)
}

// ---------------------------------------------------------------------------
// fetch_doc
// ---------------------------------------------------------------------------

/// Maximum document size in bytes (50 MB).
/// PDF/document downloads exceeding this size are rejected.
const MAX_DOC_SIZE: usize = 50 * 1024 * 1024;

/// Fetch a document (PDF, DOCX, etc.) via xberg and convert to markdown.
///
/// Downloads raw bytes via `crawler`, writes to a temporary file,
/// runs xberg extraction with a 120-second timeout, then parses, filters, and
/// lowers the resulting HTML to Markdown.
///
/// # Arguments
/// - `url`: The document URL to fetch.
/// - `crawler`: A `RateLimitedCrawler` instance for HTTP fetching.
///
/// # Returns
/// - `Ok(ExtractionResult::GenericHtml { content_md })` with `source_type: "document"` in frontmatter.
/// - `Err(WebfetchError::IoError(msg))` if the document exceeds 50 MB or temp I/O fails.
/// - `Err(WebfetchError::XbergError(msg))` if xberg extraction fails or times out.
/// - `Err(WebfetchError::Fetch(msg))` if the HTTP fetch fails.
pub async fn fetch_doc(
    url: &str,
    crawler: &RateLimitedCrawler,
) -> Result<ExtractionResult, WebfetchError> {
    // 1. Download bytes via crawler
    let response = crawler
        .get(url)
        .send()
        .await
        .map_err(|e| WebfetchError::Fetch(format!("HTTP request failed: {e}")))?;

    let status = response.status().as_u16();
    if !(200..300).contains(&status) {
        return Err(WebfetchError::Fetch(format!("HTTP error {status}")));
    }

    let bytes = response
        .bytes()
        .await
        .map_err(|e| WebfetchError::Fetch(format!("Failed to read response bytes: {e}")))?;

    process_doc_bytes(bytes.to_vec(), url).await
}

/// Process raw document bytes through xberg extraction and return markdown.
///
/// Writes bytes to a temporary file, runs xberg extraction with a 120-second timeout,
/// then parses, filters, and lowers the resulting HTML to Markdown.
///
/// # Arguments
/// - `bytes`: Raw document bytes (PDF, DOCX, etc.).
/// - `url`: The original source URL (used for frontmatter `source_url`).
///
/// # Returns
/// - `Ok(ExtractionResult::GenericHtml { content_md })` with `source_type: "document"` in frontmatter.
pub async fn process_doc_bytes(
    bytes: Vec<u8>,
    url: &str,
) -> Result<ExtractionResult, WebfetchError> {
    // 1. Check size ≤ 50 MB
    if bytes.len() > MAX_DOC_SIZE {
        return Err(WebfetchError::IoError(format!(
            "Document too large: {} bytes (max {} MB)",
            bytes.len(),
            MAX_DOC_SIZE / (1024 * 1024),
        )));
    }

    // 2. Write to a NamedTempFile with extension hint from URL
    let extension = url::Url::parse(url)
        .ok()
        .and_then(|parsed| {
            std::path::Path::new(parsed.path())
                .extension()
                .map(|e| format!(".{}", e.to_string_lossy()))
        })
        .unwrap_or_default();

    let mut temp_file = Builder::new()
        .suffix(&extension)
        .tempfile()
        .map_err(|e| WebfetchError::IoError(format!("Failed to create temp file: {e}")))?;

    temp_file
        .write_all(&bytes)
        .map_err(|e| WebfetchError::IoError(format!("Failed to write temp file: {e}")))?;

    let temp_path = temp_file.path().to_path_buf();

    // 3. Run xberg with 120s timeout (hardcoded per spec requirement)
    let config = ExtractionConfig {
        output_format: OutputFormat::Html,
        use_cache: false,
        ..Default::default()
    };

    let input = ExtractInput {
        uri: Some(temp_path.to_string_lossy().to_string()),
        ..Default::default()
    };

    let result = tokio::time::timeout(
        Duration::from_secs(120),
        xberg_extract(input, &config),
    )
    .await
    .map_err(|_| {
        WebfetchError::XbergError(
            "xberg extraction timed out after 120 seconds".into(),
        )
    })?
    .map_err(|e| WebfetchError::XbergError(e.to_string()))?;

    let html = result
        .results
        .into_iter()
        .next()
        .and_then(|doc| doc.formatted_content.or(Some(doc.content)))
        .ok_or_else(|| WebfetchError::XbergError("no content produced".into()))?;

    // 4. Parse, filter, and lower to markdown
    let markdown = xberg_html_to_markdown(&html, None)?;

    // 5. Return GenericHtml with source_type: "document"
    Ok(ExtractionResult::GenericHtml {
        content_md: MarkdownDocument {
            frontmatter: format!(
                "title: {}\nsource_type: document\nsource_url: {}",
                "", url
            ),
            body: markdown,
        },
    })
}

// ---------------------------------------------------------------------------
// Helper: fetch_url_text
// ---------------------------------------------------------------------------

// Fetch a URL via RateLimitedCrawler and return the response body as text
// along with the Content-Type header value.
//
// Delegates URL validation, rate limiting, Content-Length check, and streaming
// to `RateLimitedCrawler::fetch_text()`.
//
// Only webfetch-specific checks remain here (bot detection).
//
// Does NOT perform URL transformation (Reddit API URL, etc.) — the caller
// is expected to pass the final URL to fetch.
async fn fetch_url_text(
    url: &str,
    crawler: &RateLimitedCrawler,
) -> Result<(String, Option<String>), WebfetchError> {
    let (body, content_type) = crawler
        .fetch_text(url)
        .await
        .map_err(|e| WebfetchError::Fetch(format!("HTTP request failed: {e}")))?;

    // Bot detection is webfetch-specific (content-level check)
    if crate::core::detect::is_bot_detected(&body) {
        return Err(WebfetchError::Fetch("Blocked by bot detection".to_string()));
    }

    Ok((body, content_type))
}

// ---------------------------------------------------------------------------
// fallback_to_generic_html
// ---------------------------------------------------------------------------

// Shared logic for GenericHtml extraction.
fn fallback_to_generic_html(
    url: &str,
    body: String,
    pipeline: &[PassFn],
) -> Result<ExtractionResult, WebfetchError> {
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
