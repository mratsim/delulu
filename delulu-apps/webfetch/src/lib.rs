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
use std::io::Write;
use std::time::Duration;
use tempfile::Builder;
use xberg::{extract as xberg_extract, ExtractInput, ExtractionConfig, OutputFormat};
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
    client: &crate::core::http_client::WebbfetchClient,
    pipeline: &[PassFn],
) -> Result<ExtractionResult, WebbfetchError> {
    // Step 1: URL-based detection (primary dispatch)
    let url_source_type = detect_source_type(url);

    // Step 2: Check source type BEFORE HTTP fetch
    match url_source_type {
        SourceType::ArxivPdf => {
            // Rewrite arXiv PDF/abs URL to HTML abstract page
            let html_url = crate::core::detect::arxiv_url_to_html_url(url)?;
            let fetch_result = client.fetch(&html_url).await?;
            let body = match &fetch_result.content {
                ExtractionResult::GenericHtml { content_md } => content_md.body.clone(),
                other => {
                    tracing::warn!(
                        "fetch_and_extract: unexpected content type {:?} for arXiv HTML URL, falling back to GenericHtml",
                        other
                    );
                    return Err(WebbfetchError::Pass(format!(
                        "fetch_and_extract: arXiv HTML fetch returned unexpected content type {:?}",
                        other
                    )));
                }
            };
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
            return fetch_doc(url, client).await;
        }
        _ => {
            // Reddit and GenericHtml: proceed to HTTP fetch below
        }
    }

    // Step 3: Fetch from HTTP layer (Reddit URLs already transformed at HTTP layer)
    // Note: The HTTP layer always stores the raw response body as GenericHtml,
    // regardless of the actual source type. The body is extracted here and
    // dispatched to the appropriate parser.
    let fetch_result = client.fetch(url).await?;

    // Step 4: If Reddit, dispatch immediately — no content detection needed
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

    // Step 5: For non-Reddit URLs, extract body and run MIME/content detection
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

    // Step 6: MIME-type based document detection (best-effort, body already read as UTF-8)
    //
    // If the Content-Type header indicates a document MIME type (e.g. application/pdf),
    // we would ideally re-fetch via fetch_doc() for proper xberg-based extraction.
    // However, the HTTP response body was already consumed as UTF-8 text above, so
    // calling fetch_doc() would require a second HTTP request. The spec intentionally
    // treats this as best-effort — we log a warning and fall through to GenericHtml.
    //
    // This is a deliberate trade-off: the common case (file extension in URL) is
    // handled by URL-based detection in Step 1 (SourceType::Document dispatch).
    // The MIME-based path only matters for servers that serve documents without
    // a file extension in the URL path.
    if let Some(mime) = &fetch_result.content_type {
        if !mime.is_empty() && crate::core::detect::detect_from_mime_type(mime).is_some() {
            tracing::warn!(
                "Content-Type indicates document ({mime}) but body was already consumed as UTF-8; falling through to GenericHtml path"
            );
            // Best-effort: body already consumed as UTF-8, fall through to GenericHtml
        } else if !mime.is_empty() {
            tracing::debug!(
                "Content-Type is '{}' — not a recognized document MIME type; treating as GenericHtml",
                mime
            );
        }
    } else {
        tracing::debug!("No Content-Type header; treating response as GenericHtml");
    }

    // Step 7: Detect from content (checks for Discourse markers in HTML)
    let detected = crate::core::detect::detect_from_content(&body);

    match detected {
        Some(SourceType::Discourse) => {
            // Step 7a: Second fetch — get Discourse JSON API
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

            // Step 7b: Parse Discourse JSON
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
/// - `Err(WebbfetchError::Parse(msg))` if HTML parsing fails
pub fn xberg_html_to_markdown(
    xberg_html: &str,
    base_url: Option<&str>,
) -> Result<String, WebbfetchError> {
    let mut dom = pipelines::parse_html(xberg_html)?;
    pipelines::dl_doc::filter_doc(&mut dom);
    let markdown = generators::gen_md::MarkdownLowerer::lower(&dom, base_url);
    Ok(markdown)
}

// ---------------------------------------------------------------------------
// fetch_doc
// ---------------------------------------------------------------------------

/// Maximum document size in bytes (50 MB).
/// Per spec: raised from 10 MB to 50 MB for PDF/document downloads.
const MAX_DOC_SIZE: usize = 50 * 1024 * 1024;

/// Fetch a document (PDF, DOCX, etc.) via xberg and convert to markdown.
///
/// Downloads raw bytes via `client.get_bytes()`, writes to a temporary file,
/// runs xberg extraction with a 120-second timeout, then parses, filters, and
/// lowers the resulting HTML to Markdown.
///
/// # Arguments
/// - `url`: The document URL to fetch.
/// - `client`: A `WebbfetchClient` instance for HTTP fetching.
///
/// # Returns
/// - `Ok(ExtractionResult::GenericHtml { content_md })` with `source_type: "document"` in frontmatter.
/// - `Err(WebbfetchError::IoError(msg))` if the document exceeds 50 MB or temp I/O fails.
/// - `Err(WebbfetchError::XbergError(msg))` if xberg extraction fails or times out.
/// - `Err(WebbfetchError::Fetch(msg))` if the HTTP fetch fails.
pub async fn fetch_doc(
    url: &str,
    client: &crate::core::http_client::WebbfetchClient,
) -> Result<ExtractionResult, WebbfetchError> {
    // 1. Download bytes via client.get_bytes()
    let bytes = client.get_bytes(url).await?;

    // 2. Check size ≤ 50 MB
    if bytes.len() > MAX_DOC_SIZE {
        return Err(WebbfetchError::IoError(format!(
            "Document too large: {} bytes (max {} MB)",
            bytes.len(),
            MAX_DOC_SIZE / (1024 * 1024),
        )));
    }

    // 3. Write to a NamedTempFile with extension hint from URL
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
        .map_err(|e| WebbfetchError::IoError(format!("Failed to create temp file: {e}")))?;

    temp_file
        .write_all(&bytes)
        .map_err(|e| WebbfetchError::IoError(format!("Failed to write temp file: {e}")))?;

    let temp_path = temp_file.path().to_path_buf();

    // 4. Run xberg with 120s timeout (hardcoded per spec requirement)
    // The spec requires a 120-second timeout for xberg extraction.
    // This is intentionally hardcoded — the CLI --timeout flag controls
    // only the HTTP fetch timeout, not the extraction timeout.
    let config = ExtractionConfig {
        output_format: OutputFormat::Html,
        use_cache: false,
        ..Default::default()
    };

    // Provide a filename hint for MIME detection (xberg uses file extension)
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
        WebbfetchError::XbergError(
            "xberg extraction timed out after 120 seconds".into(),
        )
    })?
    .map_err(|e| WebbfetchError::XbergError(e.to_string()))?;

    let html = result
        .results
        .into_iter()
        .next()
        .and_then(|doc| doc.formatted_content.or(Some(doc.content)))
        .ok_or_else(|| WebbfetchError::XbergError("no content produced".into()))?;

    // 5. Parse, filter, and lower to markdown
    let markdown = xberg_html_to_markdown(&html, None)?;

    // 6. Return GenericHtml with source_type: "document"
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
