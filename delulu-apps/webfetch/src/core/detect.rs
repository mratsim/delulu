use once_cell::sync::Lazy;
use regex::Regex;

use super::types::SourceType;

// ---------------------------------------------------------------------------
// Compiled regex patterns
// ---------------------------------------------------------------------------

/// Matches Reddit thread URLs (www, old, m, np subdomains) and /s/ share links.
static REDDIT_URL_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"^https?://(?:www\.|old\.|m\.|np\.)?reddit\.com/(?:r/.*/comments/.*|s/[a-zA-Z0-9_-]+(?:/?(?:\?.*)?(?:#.*)?)?$)").unwrap()
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
pub fn detect_source_type(url: &str) -> SourceType {
    if REDDIT_URL_RE.is_match(url) {
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

/// Check whether a response body matches known bot-detection patterns.
pub(crate) fn is_bot_detected(body: &str) -> bool {
    for pattern in BOT_DETECTION_PATTERNS.iter() {
        if body.contains(pattern) {
            return true;
        }
    }
    false
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[path = "detect_test.rs"]
mod tests;
