pub mod core;
pub use crate::core::page_status::{BlockedBy, PageStatus, classify_page};
pub use crate::core::response::webfetch_raw_response;
pub use crate::core::types;
pub use crate::core::types::{ExtractionResult, MarkdownDocument, RedditComment};

pub mod generators;
pub mod pipelines;
pub mod sources;

pub use crate::core::detect::detect_source_type;
pub use crate::core::types::WebfetchError;

use crate::core::types::SourceType;
use crate::pipelines::DomNode;
use crate::pipelines::PassFn;
use delulu_rate_limited_crawler::RateLimitedCrawler;
use futures_util::StreamExt;

use std::net::{IpAddr, SocketAddr};
use std::time::Duration;
use xberg::{ExtractInput, ExtractionConfig, OutputFormat, extract as xberg_extract};

/// Maximum response body size (50 MB).
pub const MAX_BODY_SIZE: usize = 50 * 1024 * 1024;

/// Neutral hard-fail sentinel reported when a page is classified `Blocked`.
///
/// Cause-neutral on purpose: a consent wall must never report a
/// bot-specific message; the structured `page_status` carries the specific
/// `BlockedBy` cause. Shared by the wrapper, the Reddit branch, and
/// `fetch_url_text` so they cannot diverge.
pub const BLOCKED_MSG: &str = "Blocked";

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
///
/// # Contract (pinned by tests)
///
/// `fetch_and_extract` maps the page's [`PageStatus`] to its return value
/// exactly as follows:
/// - `Blocked` (thin consent-walled / anti-bot pages, plus Reddit/arXiv/
///   Discourse bot hard-fails) → `Err(WebfetchError::Fetch(BLOCKED_MSG))`,
///   where the error string is EXACTLY [`BLOCKED_MSG`] (the value `"Blocked"`).
/// - `Article` (content-bearing bot pages) → `Ok(Article result)`.
/// - All other statuses → `Ok(result)`.
///
/// This contract is pinned by `test_wrap_blocked_status_contract` so it
/// cannot silently drift. Content-bearing pages always return `Ok`; only
/// content-less `Blocked` pages hard-fail.
pub async fn fetch_and_extract(
    url: &str,
    crawler: &RateLimitedCrawler,
    pipeline: &[PassFn],
) -> Result<ExtractionResult, WebfetchError> {
    let (result, status) = fetch_and_extract_inner(url, crawler, pipeline).await?;
    wrap_blocked_status(result, status)
}

/// Map a `(result, status)` pair to the `fetch_and_extract` return value.
///
/// # Contract (pinned by tests)
///
/// Returns `Err(WebfetchError::Fetch(BLOCKED_MSG))` — error string exactly
/// [`BLOCKED_MSG`] ("Blocked") — iff the status is `Blocked`; otherwise
/// returns `Ok(result)`. Content-bearing bot pages (status `Article`) and all
/// other statuses return `Ok`. Extracted so the body-injection equivalence
/// test can exercise the exact same decision without network.
pub(crate) fn wrap_blocked_status(
    result: ExtractionResult,
    status: PageStatus,
) -> Result<ExtractionResult, WebfetchError> {
    if matches!(status, PageStatus::Blocked { .. }) {
        return Err(WebfetchError::Fetch(BLOCKED_MSG.to_string()));
    }
    Ok(result)
}

/// Fetch a URL and extract content, additionally returning the page status.
///
/// This is the additive variant of [`fetch_and_extract`]: it returns the
/// `ExtractionResult` together with its [`PageStatus`] and never collapses a
/// `Blocked` status into an error. Structured sources (Reddit, Discourse,
/// arXiv, Document) still hard-fail on bot detection inside the fetch path.
///
/// # Contract
///
/// `fetch_and_extract_with_status` returns `Ok((result, status))` for **every**
/// status — including `Blocked` — and never applies the `Blocked` → `Err`
/// hard-fail mapping. That mapping is exclusively [`fetch_and_extract`]'s job
/// (via [`wrap_blocked_status`]). The caller decides how to handle a `Blocked`
/// status; this function deliberately surfaces it as data rather than an error.
///
/// This asymmetry is intentional: the same body that makes [`fetch_and_extract`]
/// return `Err(BLOCKED_MSG)` (via [`wrap_blocked_status`]) is returned here as
/// `Ok((result, Blocked))`.
pub async fn fetch_and_extract_with_status(
    url: &str,
    crawler: &RateLimitedCrawler,
    pipeline: &[PassFn],
) -> Result<(ExtractionResult, PageStatus), WebfetchError> {
    fetch_and_extract_inner(url, crawler, pipeline).await
}

/// Pure status mapper for structured/extraction successes.
///
/// Reddit, Discourse, arXiv, and Document all map to `Article` on successful
/// extraction. The wrapper hardcodes `Article` for these via this single helper
/// so the mapping cannot drift.
pub(crate) fn structured_success_status() -> PageStatus {
    PageStatus::Article
}

/// Validate a webfetch URL (length + http(s) scheme) before any processing.
fn validate_webfetch_url(url: &str) -> Result<(), WebfetchError> {
    let trimmed_input = url.trim();
    if trimmed_input.len() > 2048 {
        return Err(WebfetchError::Fetch(
            "URL exceeds maximum length".to_string(),
        ));
    }
    if !trimmed_input.starts_with("http://") && !trimmed_input.starts_with("https://") {
        return Err(WebfetchError::Fetch(format!(
            "Unsupported URL scheme: '{}'",
            trimmed_input.split(':').next().unwrap_or("")
        )));
    }
    Ok(())
}

async fn fetch_and_extract_inner(
    url: &str,
    crawler: &RateLimitedCrawler,
    pipeline: &[PassFn],
) -> Result<(ExtractionResult, PageStatus), WebfetchError> {
    // Step 1: URL-based detection (primary dispatch)
    let url_source_type = detect_source_type(url);

    // Validate URL before any processing
    validate_webfetch_url(url)?;

    // Step 2: Check source type BEFORE HTTP fetch
    match url_source_type {
        SourceType::ArxivPdf => {
            // Rewrite arXiv PDF/abs URL to HTML abstract page
            let html_url = crate::core::detect::arxiv_url_to_html_url(url)?;
            let (body, _) = fetch_url_text(&html_url, crawler).await?;
            let mut dom = pipelines::parse_html(&body)?;
            let title = extract_title(&dom);
            pipelines::dl_arxiv::filter_arxiv(&mut dom);
            let content_md = generators::gen_md::MarkdownLowerer::lower(&dom, None);
            let raw_html_len = body.len();
            let filtered_html_len = content_md.len();
            return Ok((
                ExtractionResult::GenericHtml {
                    content_md: MarkdownDocument {
                        frontmatter: format!(
                            "title: {}\nsource_type: generic_html\nsource_url: {}\ndate_of_publication: N/A\ndate_of_retrieval: N/A",
                            title, url
                        ),
                        body: content_md,
                    },
                    raw_html_len,
                    filtered_html_len,
                },
                structured_success_status(),
            ));
        }
        SourceType::Document => {
            // Direct document fetch via xberg — no HTTP fetch needed
            let r = fetch_doc(url, crawler).await?;
            return Ok((r, structured_success_status()));
        }
        _ => {
            // Reddit and GenericHtml: proceed to HTTP fetch below
        }
    }

    // Step 3: Check Content-Type BEFORE consuming the body as text.
    // This prevents String::from_utf8_lossy from corrupting binary payloads
    // (PDFs, DOCX, etc.) served from URLs without a recognized file extension.
    let fetch_url = if url_source_type == SourceType::Reddit {
        crate::core::detect::reddit_url_to_api_url(url)
    } else {
        url.to_string()
    };

    let response = crawler
        .get(&fetch_url)
        .send()
        .await
        .map_err(|e| WebfetchError::Fetch(format!("HTTP request failed: {e}")))?;

    let status = response.status();
    if !status.is_success() {
        return Err(WebfetchError::Fetch(format!(
            "HTTP {}: {}",
            status.as_u16(),
            url
        )));
    }

    let content_type = response
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok().map(String::from));

    // If the response is a document (PDF, DOCX, etc.), consume as bytes
    // before the body is corrupted by text conversion.
    if let Some(ref mime) = content_type
        && !mime.is_empty()
        && crate::core::detect::detect_from_mime_type(mime).is_some()
    {
        // Reject oversized responses before consuming
        if let Some(len) = response.content_length()
            && len as usize > MAX_BODY_SIZE
        {
            return Err(WebfetchError::IoError(format!(
                "Document too large: Content-Length {len} bytes (max {} MB)",
                MAX_BODY_SIZE / (1024 * 1024),
            )));
        }
        // Stream chunks with size limit enforcement
        let mut body = Vec::new();
        let mut stream = response.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk
                .map_err(|e| WebfetchError::Fetch(format!("Failed to read response chunk: {e}")))?;
            if body.len() + chunk.len() > MAX_BODY_SIZE {
                return Err(WebfetchError::IoError(format!(
                    "Document exceeded size limit while streaming (max {} MB)",
                    MAX_BODY_SIZE / (1024 * 1024),
                )));
            }
            body.extend_from_slice(&chunk);
        }
        let raw_bytes_len = body.len();
        let html = doc_to_html(body, url).await?;
        let markdown = doc_html_to_markdown(&html, None)?;
        let filtered_html_len = markdown.len();
        return Ok((
            ExtractionResult::GenericHtml {
                content_md: MarkdownDocument {
                    frontmatter: format!(
                        "title: {}\nsource_type: document\nsource_url: {}\ndate_of_publication: N/A\ndate_of_retrieval: N/A",
                        "", url
                    ),
                    body: markdown,
                },
                raw_html_len: raw_bytes_len,
                filtered_html_len,
            },
            structured_success_status(),
        ));
    }

    // Step 4: Consume body as text (safe now — confirmed not a document MIME)
    let body = response
        .text()
        .await
        .map_err(|e| WebfetchError::Fetch(format!("Failed to read response: {e}")))?;

    process_text_body(url, url_source_type, body, pipeline, crawler).await
}

/// Process a raw text (HTML/JSON) response body past the fetch step.
///
/// Handles Reddit dispatch (with its bot hard-fail), Discourse dispatch (with
/// the pre-JSON-fetch bot check on the HTML body), and the GenericHtml fallback
/// (whose status comes from `classify_page` — `Blocked` is reported as a status,
/// not a hard-fail).
async fn process_text_body(
    url: &str,
    url_source_type: SourceType,
    body: String,
    pipeline: &[PassFn],
    crawler: &RateLimitedCrawler,
) -> Result<(ExtractionResult, PageStatus), WebfetchError> {
    // Step 5: If Reddit, dispatch immediately — no content detection needed
    if url_source_type == SourceType::Reddit {
        // Reddit keeps its hard-fail on bot-blocked bodies.
        if crate::core::detect::is_bot_detected(&body) {
            return Err(WebfetchError::Fetch(BLOCKED_MSG.to_string()));
        }
        let data = sources::reddit::RedditExtractor::extract(&body)?;
        // Build a well-formed source_url from the permalink. A missing or
        // non-`/`-prefixed permalink would otherwise yield a malformed URL
        // (`https://reddit.com` bare, or `https://reddit.comnonslash`).
        // Normalize: an empty permalink falls back to the original full `url`
        // the caller fetched (already well-formed); a non-`/`-prefixed
        // permalink gets `/` prepended so the separator is never lost.
        let source_url = if data.permalink.trim().is_empty() {
            url.trim().to_string()
        } else if data.permalink.starts_with('/') {
            format!("https://reddit.com{}", data.permalink)
        } else {
            format!("https://reddit.com/{}", data.permalink)
        };
        let comment_count = data.comments.len();
        return Ok((
            ExtractionResult::Reddit {
                title: data.title,
                selftext: data.selftext,
                author: data.author,
                score: data.score,
                permalink: data.permalink,
                source_url,
                comments: data.comments,
                comment_count,
                comments_truncated: data.comments_truncated,
            },
            structured_success_status(),
        ));
    }

    // Step 6: Detect from content (checks for Discourse markers in HTML)
    let detected = crate::core::detect::detect_from_content(&body);

    match detected {
        Some(SourceType::Discourse) => {
            // Discourse-path bot check: a bot-marked HTML body must
            // hard-fail even with a clean JSON API.
            if crate::core::detect::is_bot_detected(&body) {
                return Err(WebfetchError::Fetch(BLOCKED_MSG.to_string()));
            }
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
            let posts_returned = data.posts.len();
            Ok((
                ExtractionResult::Discourse {
                    title: data.title,
                    topic_id: data.topic_id,
                    posts: data.posts,
                    post_count: data.post_count,
                    posts_returned,
                },
                structured_success_status(),
            ))
        }
        // No Discourse detected — treat as GenericHtml
        _ => fallback_to_generic_html(url, body, pipeline),
    }
}

/// Body-injection seam for tests: process a raw body past the fetch step.
///
/// `#[cfg(test)]`-gated internal helper that lets the blocked hard-fail
/// equivalence be proven without network or host-override. The `crawler` is
/// passed only so the Discourse second-fetch branch compiles; the injected-body
/// tests target GenericHtml and Reddit bodies, which never need it.
#[cfg(test)]
async fn fetch_and_extract_inner_with_body(
    url: &str,
    crawler: &RateLimitedCrawler,
    pipeline: &[PassFn],
    body: String,
) -> Result<(ExtractionResult, PageStatus), WebfetchError> {
    let url_source_type = detect_source_type(url);
    validate_webfetch_url(url)?;
    process_text_body(url, url_source_type, body, pipeline, crawler).await
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
        &[crate::pipelines::trafilatura::filter_trafilatura],
    )
    .await
}

// ---------------------------------------------------------------------------
// doc_html_to_markdown
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
pub fn doc_html_to_markdown(
    xberg_html: &str,
    base_url: Option<&str>,
) -> Result<String, WebfetchError> {
    let mut dom = pipelines::parse_html(xberg_html)?;
    pipelines::dl_doc::filter_doc(&mut dom);
    let markdown = generators::gen_md::MarkdownLowerer::lower(&dom, base_url);
    Ok(markdown)
}

// ---------------------------------------------------------------------------
// fetch_doc_as_html
// ---------------------------------------------------------------------------
/// Maximum document size in bytes (50 MB).
/// PDF/document downloads exceeding this size are rejected.
const MAX_DOC_SIZE: usize = 50 * 1024 * 1024;
/// Fetch a document (PDF, DOCX, etc.) and return raw HTML from xberg.
///
/// Downloads raw bytes via `crawler`, writes to a temporary file,
/// runs xberg extraction with a 10-second timeout, and returns the
/// raw HTML output from xberg without converting to Markdown.
///
/// # Arguments
/// - `url`: The document URL to fetch.
/// - `crawler`: A `RateLimitedCrawler` instance for HTTP fetching.
///
/// # Returns
/// - `Ok(html_string)` — raw HTML from xberg.
/// - `Err(WebfetchError::IoError(msg))` if the document exceeds 50 MB or temp I/O fails.
/// - `Err(WebfetchError::XbergError(msg))` if xberg extraction fails or times out.
/// - `Err(WebfetchError::Fetch(msg))` if the HTTP fetch fails.
pub async fn fetch_doc_as_html(
    url: &str,
    crawler: &RateLimitedCrawler,
) -> Result<String, WebfetchError> {
    // 1. Stream response body with size limit check
    let response = crawler
        .get(url)
        .send()
        .await
        .map_err(|e| WebfetchError::Fetch(format!("HTTP request failed: {e}")))?;

    let status = response.status().as_u16();
    if !(200..300).contains(&status) {
        return Err(WebfetchError::Fetch(format!("HTTP error {status}")));
    }

    // Reject oversized responses before streaming
    if let Some(len) = response.content_length()
        && len as usize > MAX_DOC_SIZE
    {
        return Err(WebfetchError::IoError(format!(
            "Document too large: Content-Length {len} bytes (max {} MB)",
            MAX_DOC_SIZE / (1024 * 1024),
        )));
    }

    // Stream chunks with size limit enforcement
    let mut body = Vec::new();
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk
            .map_err(|e| WebfetchError::Fetch(format!("Failed to read response chunk: {e}")))?;
        if body.len() + chunk.len() > MAX_DOC_SIZE {
            return Err(WebfetchError::IoError(format!(
                "Document exceeded size limit while streaming (max {} MB)",
                MAX_DOC_SIZE / (1024 * 1024),
            )));
        }
        body.extend_from_slice(&chunk);
    }

    // 2. Convert bytes to HTML via xberg
    doc_to_html(body, url).await
}

/// Convert raw document bytes to HTML via xberg extraction.
///
/// Writes bytes to a temporary file with an extension hint from the URL,
/// runs xberg extraction with a 10-second timeout, and returns the raw HTML.
pub async fn doc_to_html(bytes: Vec<u8>, url: &str) -> Result<String, WebfetchError> {
    // 1. Check size <= 50 MB
    if bytes.len() > MAX_DOC_SIZE {
        return Err(WebfetchError::IoError(format!(
            "Document too large: {} bytes (max {} MB)",
            bytes.len(),
            MAX_DOC_SIZE / (1024 * 1024),
        )));
    }

    // 2. Extension hint from URL
    let extension = url::Url::parse(url)
        .ok()
        .and_then(|parsed| {
            std::path::Path::new(parsed.path())
                .extension()
                .map(|e| format!(".{}", e.to_string_lossy()))
        })
        .unwrap_or_default();

    // 3. Write to temp file (blocking I/O, run on blocking thread pool)
    let temp_file = tokio::task::spawn_blocking({
        let extension = extension.clone();
        let bytes = bytes.clone();
        move || -> Result<_, WebfetchError> {
            let mut temp_file = tempfile::Builder::new()
                .suffix(&extension)
                .tempfile()
                .map_err(|e| WebfetchError::IoError(format!("Failed to create temp file: {e}")))?;
            use std::io::Write;
            temp_file
                .write_all(&bytes)
                .map_err(|e| WebfetchError::IoError(format!("Failed to write temp file: {e}")))?;
            Ok(temp_file)
        }
    })
    .await
    .map_err(|e| WebfetchError::IoError(format!("Temp file task panicked: {e}")))?;
    let temp_file = temp_file?;
    let temp_path = temp_file.path().to_path_buf();

    // 4. Run xberg with 10s timeout
    let config = ExtractionConfig {
        output_format: OutputFormat::Html,
        use_cache: false,
        ..Default::default()
    };
    let input = ExtractInput {
        uri: Some(temp_path.to_string_lossy().to_string()),
        ..Default::default()
    };
    let result = tokio::time::timeout(Duration::from_secs(10), xberg_extract(input, &config))
        .await
        .map_err(|_| {
            WebfetchError::XbergError("xberg extraction timed out after 10 seconds".into())
        })?
        .map_err(|e| WebfetchError::XbergError(e.to_string()))?;

    let html = result
        .results
        .into_iter()
        .next()
        .and_then(|doc| doc.formatted_content.or(Some(doc.content)))
        .ok_or_else(|| WebfetchError::XbergError("no content produced".into()))?;

    Ok(html)
}

// ---------------------------------------------------------------------------
// fetch_doc
// ---------------------------------------------------------------------------
/// Fetch a document (PDF, DOCX, etc.) via xberg and convert to markdown.
///
/// Downloads raw bytes via `crawler`, runs xberg extraction, then converts
/// the resulting HTML to Markdown.
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
    let html = fetch_doc_as_html(url, crawler).await?;
    let markdown = doc_html_to_markdown(&html, None)?;
    let filtered_html_len = markdown.len();
    Ok(ExtractionResult::GenericHtml {
        content_md: MarkdownDocument {
            frontmatter: format!(
                "title: {}\nsource_type: document\nsource_url: {}\ndate_of_publication: N/A\ndate_of_retrieval: N/A",
                "", url
            ),
            body: markdown,
        },
        raw_html_len: html.len(),
        filtered_html_len,
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
//
// NOTE: Only use this for endpoints known to return text (arXiv HTML,
// Discourse JSON). For arbitrary URLs, use fetch_and_extract which checks
// Content-Type before consuming the body.
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
        return Err(WebfetchError::Fetch(BLOCKED_MSG.to_string()));
    }

    Ok((body, content_type))
}

// ---------------------------------------------------------------------------
// fetch_raw_html
// ---------------------------------------------------------------------------

/// Fetch a URL and return the raw HTTP response body as a string.
/// Skips the extraction pipeline entirely — just returns the raw HTML.
pub async fn fetch_raw_html(
    url: &str,
    crawler: &RateLimitedCrawler,
) -> Result<String, WebfetchError> {
    let response = crawler
        .get(url)
        .send()
        .await
        .map_err(|e| WebfetchError::Fetch(format!("HTTP request failed: {e}")))?;
    let status = response.status().as_u16();
    if !(200..300).contains(&status) {
        return Err(WebfetchError::Fetch(format!("HTTP error {status}")));
    }
    // Reject oversized responses before streaming
    if let Some(len) = response.content_length()
        && len as usize > MAX_BODY_SIZE
    {
        return Err(WebfetchError::IoError(format!(
            "Response too large: Content-Length {len} bytes (max {} MB)",
            MAX_BODY_SIZE / (1024 * 1024),
        )));
    }
    // Stream chunks with size limit enforcement
    let mut body = Vec::new();
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk
            .map_err(|e| WebfetchError::Fetch(format!("Failed to read response chunk: {e}")))?;
        if body.len() + chunk.len() > MAX_BODY_SIZE {
            return Err(WebfetchError::IoError(format!(
                "Response exceeded size limit while streaming (max {} MB)",
                MAX_BODY_SIZE / (1024 * 1024),
            )));
        }
        body.extend_from_slice(&chunk);
    }
    let body_str = String::from_utf8(body)
        .map_err(|e| WebfetchError::Fetch(format!("Response is not valid UTF-8: {e}")))?;
    Ok(body_str)
}

// ---------------------------------------------------------------------------
// fallback_to_generic_html
// ---------------------------------------------------------------------------

// Shared logic for GenericHtml extraction.
//
// Returns the `GenericHtml` result together with its `PageStatus` computed via
// `classify_page(&body, visible_len, script_len)` from PRE-pipeline
// measurements (while `<script>` is still present). A bot-blocked body →
// `Blocked{by}`; a consent-walled body → `Blocked{by: CookieConsent}` — both
// only when visible content is below the threshold. Note that classification
// uses pre-pipeline `visible_len`/`script_len`, while `filtered_html_len`
// below remains markdown bytes (post-pipeline) — intentionally different.
fn fallback_to_generic_html(
    url: &str,
    body: String,
    pipeline: &[PassFn],
) -> Result<(ExtractionResult, PageStatus), WebfetchError> {
    let raw_html_len = body.len();
    let mut dom = pipelines::parse_html(&body)?;
    // Classification measurements are taken PRE-pipeline, while `<script>` is
    // still present. Post-pipeline passes strip `<script>`, so script content
    // would be gone.
    let visible_len = dom.visible_text_len();
    let script_len = dom.script_len();
    for pass in pipeline {
        pass(&mut dom);
    }
    let content_md = generators::gen_md::MarkdownLowerer::lower(&dom, None);
    let title = extract_title(&dom);
    let filtered_html_len = content_md.len();
    let status = classify_page(&body, visible_len, script_len);

    Ok((
        ExtractionResult::GenericHtml {
            content_md: MarkdownDocument {
                frontmatter: format!(
                    "title: {}\nsource_type: generic_html\nsource_url: {}\ndate_of_publication: N/A\ndate_of_retrieval: N/A",
                    title, url
                ),
                body: content_md,
            },
            raw_html_len,
            filtered_html_len,
        },
        status,
    ))
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
            let text = children
                .iter()
                .map(|c| c.text_content())
                .collect::<String>();
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

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

/// Check if an IP address is in a private/internal range.
/// Used for SSRF protection in the MCP server.
pub fn is_private_ip(ip: &IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            v4.is_loopback()         // 127.0.0.0/8
                || v4.is_private()    // 10.0.0.0/8, 172.16.0.0/12, 192.168.0.0/16
                || v4.is_link_local() // 169.254.0.0/16 (includes cloud metadata 169.254.169.254)
        }
        IpAddr::V6(v6) => {
            if is_ipv4_mapped(v6) {
                // Extract embedded IPv4 and check if IT is private
                let ipv4 = std::net::Ipv4Addr::from(
                    (u128::from(*v6) & 0x0000_0000_0000_0000_0000_0000_FFFF_FFFF) as u32,
                );
                return ipv4.is_loopback() || ipv4.is_private() || ipv4.is_link_local();
            }
            v6.is_loopback()
                || v6.is_unspecified()
                || is_ula(v6)
                || is_link_local(v6)
                || is_ipv4_compatible(v6)
                || is_documentation(v6)
        }
    }
}

/// Check if an IPv6 address is a Unique Local Address (fc00::/7).
pub fn is_ula(v6: &std::net::Ipv6Addr) -> bool {
    v6.octets()[0] & 0xfe == 0xfc
}

/// Check if an IPv6 address is a link-local address (fe80::/10).
pub fn is_link_local(v6: &std::net::Ipv6Addr) -> bool {
    v6.octets()[0] == 0xfe && v6.octets()[1] & 0xc0 == 0x80
}

/// Check if an IPv6 address is an IPv4-mapped address (::ffff:0:0/96).
pub fn is_ipv4_mapped(v6: &std::net::Ipv6Addr) -> bool {
    (u128::from(*v6) >> 32) == 0xFFFF
}

/// Check if an IPv6 address is an IPv4-compatible address (::ffff:0:0:0/96).
pub fn is_ipv4_compatible(v6: &std::net::Ipv6Addr) -> bool {
    (u128::from(*v6) >> 32) == 0
}

/// Check if an IPv6 address is a documentation address (2001:db8::/32).
pub fn is_documentation(v6: &std::net::Ipv6Addr) -> bool {
    (u128::from(*v6) >> 96) == 0x2001_0DB8
}

/// Check if two socket addresses share the same subnet.
/// Uses /16 for IPv4, /64 for IPv6.
pub fn same_subnet_16(a: SocketAddr, b: SocketAddr) -> bool {
    match (a.ip(), b.ip()) {
        (IpAddr::V4(a), IpAddr::V4(b)) => (u32::from(a) >> 16) == (u32::from(b) >> 16),
        (IpAddr::V6(a), IpAddr::V6(b)) => (u128::from(a) >> 64) == (u128::from(b) >> 64),
        _ => false,
    }
}

#[cfg(test)]
#[path = "../tests/unit/lib_test.rs"]
mod tests;
