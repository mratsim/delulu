use once_cell::sync::Lazy;
use regex::Regex;

use super::types::SourceType;
use super::types::WebfetchError;
// ---------------------------------------------------------------------------
// Compiled regex patterns
// ---------------------------------------------------------------------------

/// Matches Reddit thread URLs (www, old, m, np subdomains) and /s/ share links.
static REDDIT_URL_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"^https?://(?:www\.|old\.|m\.|np\.)?reddit\.com/(?:r/.*/comments/.*|s/[a-zA-Z0-9_-]+(?:/?(?:\?.*)?(?:#.*)?)?$)").unwrap()
});

/// Matches arXiv PDF/abstract URLs (with optional version suffix).
/// Examples:
/// - https://arxiv.org/pdf/1706.03762v1
/// - https://arxiv.org/abs/1706.03762v1
/// - https://arxiv.org/pdf/1706.03762v1.pdf
static ARXIV_URL_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"^https?://(?:www\.)?arxiv\.org/(?:pdf|abs)/[0-9]{4}\.[0-9]+(?:v[0-9]+)?(?:\.pdf)?$")
        .unwrap()
});

/// Matches document file extensions at the end of a URL path.
static DOCUMENT_EXTENSION_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?i)\.(pdf|doc|docx|ppt|pptx|key)(?:[?#].*)?$")
        .unwrap()
});
/// Bot-detection patterns (checked against response body).
pub(crate) static BOT_DETECTION_PATTERNS: Lazy<Vec<&'static str>> = Lazy::new(|| {
    vec![
        "Just a moment...",
        "cf-browser-verification",
        "challenge-platform",
        "turnstile",
        "g-recaptcha",
        "data-sitekey",
    ]
});

// ---------------------------------------------------------------------------
// URL-based detection
// ---------------------------------------------------------------------------

/// Detect the source type from a URL based on known platform patterns.
///
/// Priority order (first match wins):
/// 1. arXiv PDF/abstract URLs
/// 2. Document file extensions (.pdf, .doc, .docx, .ppt, .pptx, .key)
/// 3. Reddit thread URLs
/// 4. Generic HTML (default)
pub fn detect_source_type(url: &str) -> SourceType {
    if ARXIV_URL_RE.is_match(url) {
        SourceType::ArxivPdf
    } else if DOCUMENT_EXTENSION_RE.is_match(url) {
        SourceType::Document
    } else if REDDIT_URL_RE.is_match(url) {
        SourceType::Reddit
    } else {
        SourceType::GenericHtml
    }
}

/// Transform a Reddit thread URL into its JSON API endpoint.
///
/// Uses the `url` crate to safely manipulate path and query parameters.
pub fn reddit_url_to_api_url(url: &str) -> String {
    if let Ok(mut parsed) = url::Url::parse(url) {
        let mut path = parsed.path().trim_end_matches('/').to_string();
        path.push_str(".json");
        parsed.set_path(&path);
        parsed.set_query(Some("raw_json=1"));
        parsed.to_string()
    } else {
        format!("{}.json?raw_json=1", url.trim_end_matches('/'))
    }
}

/// Transform a Discourse topic URL into its JSON API endpoint.
///
/// Uses the `url` crate to safely manipulate path and query parameters.
pub fn discourse_url_to_api_url(url: &str) -> String {
    if let Ok(mut parsed) = url::Url::parse(url) {
        let mut path = parsed.path().trim_end_matches('/').to_string();
        path.push_str(".json");
        parsed.set_path(&path);
        parsed.set_query(Some("raw_json=1&include_raw=1"));
        parsed.to_string()
    } else {
        format!(
            "{}.json?raw_json=1&include_raw=1",
            url.trim_end_matches('/')
        )
    }
}

// ---------------------------------------------------------------------------
// Content-based detection
// ---------------------------------------------------------------------------

/// Detect the source type from the response body (HTML content).
///
/// Checks for known CMS/forum markers:
/// - `<meta name="generator" content="Discourse">`
/// - JSON-LD `"@type": "DiscussionForumPosting"`
pub fn detect_from_content(body: &str) -> Option<SourceType> {
    if body.contains(r#"<meta name="generator" content="Discourse">"#)
        || body.contains(r##"<meta name="generator" content="Discourse "##)
    {
        return Some(SourceType::Discourse);
    }
    if body.contains(r#""@type": "DiscussionForumPosting""#) {
        return Some(SourceType::Discourse);
    }
    None
}

/// Detect the source type from a MIME type string.
///
/// Checks for common document MIME types:
/// - `application/pdf`, `application/x-pdf` (and with parameters like `; charset=utf-8`)
/// - `application/msword`
/// - `application/vnd.openxmlformats-officedocument.*`
/// - `application/vnd.ms-powerpoint.*`
///
/// Used for Content-Type dispatch after HTTP fetch — if the response
/// Content-Type header indicates a document MIME type, the caller can
/// re-fetch via `fetch_doc()` for xberg-based extraction instead of
/// treating the response as HTML.
pub(crate) fn detect_from_mime_type(mime_type: &str) -> Option<SourceType> {
    let mime = mime_type.to_lowercase();
    // Split on ';' to separate the MIME type from parameters like charset
    let type_part = mime.split(';').next().unwrap_or(&mime).trim();
    // Check for PDF MIME types with proper boundary matching
    let is_pdf = type_part == "application/pdf"
        || type_part == "application/x-pdf"
        || type_part.ends_with("/pdf")
        || type_part.starts_with("application/") && type_part.contains("+pdf");
    if is_pdf
        || mime.contains("msword")
        || mime.contains("openxmlformats")
        || mime.contains("ms-powerpoint")
    {
        return Some(SourceType::Document);
    }
    None
}

/// Transform an arXiv URL into its HTML abstract page URL.
///
/// Converts `/pdf/` to `/html/` and strips trailing `.pdf` extension.
/// Returns the URL unchanged (as `Ok`) for non-arXiv URLs or if the
/// transformation is not applicable.
pub fn arxiv_url_to_html_url(arxiv_url: &str) -> Result<String, WebfetchError> {
    let url = arxiv_url.trim_end_matches('/');
    if let Some(path) = url
        .strip_prefix("https://arxiv.org/pdf/")
        .or_else(|| url.strip_prefix("http://arxiv.org/pdf/"))
        .or_else(|| url.strip_prefix("https://www.arxiv.org/pdf/"))
        .or_else(|| url.strip_prefix("http://www.arxiv.org/pdf/"))
    {
        let id = path.strip_suffix(".pdf").unwrap_or(path);
        return Ok(format!("https://arxiv.org/html/{id}"));
    }
    if let Some(path) = url
        .strip_prefix("https://arxiv.org/abs/")
        .or_else(|| url.strip_prefix("http://arxiv.org/abs/"))
        .or_else(|| url.strip_prefix("https://www.arxiv.org/abs/"))
        .or_else(|| url.strip_prefix("http://www.arxiv.org/abs/"))
    {
        return Ok(format!("https://arxiv.org/html/{path}"));
    }
    Ok(arxiv_url.to_string())
}

/// Check whether a response body matches known bot-detection patterns.
pub(crate) fn is_bot_detected(body: &str) -> bool {
    for pattern in BOT_DETECTION_PATTERNS.iter() {
        if body.contains(pattern) {
            return true;
        }
    }
    false
}
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[path = "detect_test.rs"]
mod tests;
