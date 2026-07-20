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
        format!("{}.json?raw_json=1&include_raw=1", url.trim_end_matches('/'))
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
mod tests {
    use super::*;

    // -- detect_source_type -------------------------------------------------

    #[test]
    fn detect_reddit_www() {
        let url = "https://www.reddit.com/r/rust/comments/abc123/hello_world/";
        assert_eq!(detect_source_type(url), SourceType::Reddit);
    }

    #[test]
    fn detect_reddit_old() {
        let url = "https://old.reddit.com/r/rust/comments/abc123/hello_world/";
        assert_eq!(detect_source_type(url), SourceType::Reddit);
    }

    #[test]
    fn detect_reddit_np() {
        let url = "https://np.reddit.com/r/rust/comments/abc123/hello_world/";
        assert_eq!(detect_source_type(url), SourceType::Reddit);
    }

    #[test]
    fn detect_reddit_share_link() {
        let url = "https://www.reddit.com/s/abc123xyz";
        assert_eq!(detect_source_type(url), SourceType::Reddit);
    }

    #[test]
    fn detect_reddit_np_share_link() {
        let url = "https://np.reddit.com/s/abc123xyz";
        assert_eq!(detect_source_type(url), SourceType::Reddit);
    }

    #[test]
    fn detect_reddit_no_subdomain() {
        let url = "https://reddit.com/r/rust/comments/abc123/";
        assert_eq!(detect_source_type(url), SourceType::Reddit);
    }

    #[test]
    fn detect_generic_html() {
        let url = "https://example.com/page";
        assert_eq!(detect_source_type(url), SourceType::GenericHtml);
    }

    #[test]
    fn detect_reddit_user_page_not_thread() {
        // User pages should not match Reddit (no /comments/ in path)
        let url = "https://www.reddit.com/user/someuser/";
        assert_eq!(detect_source_type(url), SourceType::GenericHtml);
    }
    #[test]
    fn detect_non_discourse_t_url_is_generic_html() {
        let url = "https://forum.example.com/t/some-topic/12345";
        assert_eq!(detect_source_type(url), SourceType::GenericHtml);
    }

    // -- reddit_url_to_api_url ----------------------------------------------

    #[test]
    fn reddit_api_url_no_trailing_slash() {
        let url = "https://www.reddit.com/r/rust/comments/abc123/hello_world";
        let api = reddit_url_to_api_url(url);
        assert_eq!(
            api,
            "https://www.reddit.com/r/rust/comments/abc123/hello_world.json?raw_json=1"
        );
    }

    #[test]
    fn reddit_api_url_with_trailing_slash() {
        let url = "https://www.reddit.com/r/rust/comments/abc123/hello_world/";
        let api = reddit_url_to_api_url(url);
        assert_eq!(
            api,
            "https://www.reddit.com/r/rust/comments/abc123/hello_world.json?raw_json=1"
        );
    }

    #[test]
    fn reddit_api_url_share_link() {
        // Share links get the same treatment: strip trailing slash, append .json?raw_json=1
        // Note: Reddit's /s/ endpoint may not serve JSON; this documents current behavior.
        let url = "https://www.reddit.com/s/abc123xyz";
        let api = reddit_url_to_api_url(url);
        assert_eq!(api, "https://www.reddit.com/s/abc123xyz.json?raw_json=1");
    }

    // -- discourse_url_to_api_url -------------------------------------------

    #[test]
    fn discourse_api_url_no_trailing_slash() {
        let url = "https://forum.example.com/t/some-topic/12345";
        let api = discourse_url_to_api_url(url);
        assert_eq!(api, "https://forum.example.com/t/some-topic/12345.json?raw_json=1&include_raw=1");
    }

    #[test]
    fn discourse_api_url_with_trailing_slash() {
        let url = "https://forum.example.com/t/some-topic/12345/";
        let api = discourse_url_to_api_url(url);
        assert_eq!(api, "https://forum.example.com/t/some-topic/12345.json?raw_json=1&include_raw=1");
    }

    // -- detect_from_content ------------------------------------------------

    #[test]
    fn detect_discourse_from_meta() {
        let body = r#"<html><head><meta name="generator" content="Discourse"></head></html>"#;
        assert_eq!(detect_from_content(body), Some(SourceType::Discourse));
    }

    #[test]
    fn detect_discourse_from_versioned_meta() {
        let body = r##"<html><head><meta name="generator" content="Discourse 3.5.2 - https://discourse.org"></head></html>"##;
        assert_eq!(detect_from_content(body), Some(SourceType::Discourse));
    }

    #[test]
    fn detect_discourse_from_json_ld() {
        let body = r#"{"@type": "DiscussionForumPosting", "name": "Test"}"#;
        assert_eq!(detect_from_content(body), Some(SourceType::Discourse));
    }

    #[test]
    fn detect_from_content_no_match() {
        let body = "<html><head><title>Hello</title></head><body>Plain page</body></html>";
        assert_eq!(detect_from_content(body), None);
    }

    // -- is_bot_detected ----------------------------------------------------

    #[test]
    fn bot_detected_cloudflare() {
        let body = "<html>Just a moment... <div>cf-browser-verification</div></html>";
        assert!(is_bot_detected(body));
    }

    #[test]
    fn bot_detected_challenge() {
        let body = "challenge-platform turnstile";
        assert!(is_bot_detected(body));
    }

    #[test]
    fn bot_detected_recaptcha() {
        let body = r#"<div class="g-recaptcha" data-sitekey="abc123"></div>"#;
        assert!(is_bot_detected(body));
    }

    #[test]
    fn bot_detected_no_match() {
        let body = "<html>Normal page content here</html>";
        assert!(!is_bot_detected(body));
    }
    // -- URL edge cases ----------------------------------------------------

    #[test]
    fn detect_reddit_trailing_slash_variants() {
        let url = "https://www.reddit.com/r/rust/comments/abc123/hello_world/";
        assert_eq!(detect_source_type(url), SourceType::Reddit);
        let url = "https://www.reddit.com/r/rust/comments/abc123/hello_world";
        assert_eq!(detect_source_type(url), SourceType::Reddit);
    }

    #[test]
    fn detect_reddit_with_query_params() {
        let url = "https://www.reddit.com/r/rust/comments/abc123/hello_world/?sort=new&limit=50";
        assert_eq!(detect_source_type(url), SourceType::Reddit);
    }

    #[test]
    fn detect_reddit_with_fragment() {
        let url = "https://www.reddit.com/r/rust/comments/abc123/hello_world/#section";
        assert_eq!(detect_source_type(url), SourceType::Reddit);
    }

    #[test]
    fn detect_reddit_with_query_and_fragment() {
        let url = "https://www.reddit.com/r/rust/comments/abc123/hello_world?sort=top#comments";
        assert_eq!(detect_source_type(url), SourceType::Reddit);
    }

    #[test]
    fn detect_generic_html_with_fragment() {
        let url = "https://example.com/page#section";
        assert_eq!(detect_source_type(url), SourceType::GenericHtml);
    }

    #[test]
    fn detect_generic_html_with_query_params() {
        let url = "https://example.com/page?foo=bar&baz=qux";
        assert_eq!(detect_source_type(url), SourceType::GenericHtml);
    }

    #[test]
    fn detect_generic_html_with_trailing_slash() {
        let url = "https://example.com/page/";
        assert_eq!(detect_source_type(url), SourceType::GenericHtml);
    }
}
