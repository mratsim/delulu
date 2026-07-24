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
    assert_eq!(
        api,
        "https://forum.example.com/t/some-topic/12345.json?raw_json=1&include_raw=1"
    );
}

#[test]
fn discourse_api_url_with_trailing_slash() {
    let url = "https://forum.example.com/t/some-topic/12345/";
    let api = discourse_url_to_api_url(url);
    assert_eq!(
        api,
        "https://forum.example.com/t/some-topic/12345.json?raw_json=1&include_raw=1"
    );
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

// -- detect_source_type: arXiv PDF/abs URLs -------------------------------

#[test]
fn detect_arxiv_pdf() {
    let url = "https://arxiv.org/pdf/1706.03762v1";
    assert_eq!(detect_source_type(url), SourceType::ArxivPdf);
}

#[test]
fn detect_arxiv_pdf_with_version() {
    let url = "https://arxiv.org/pdf/1706.03762v3";
    assert_eq!(detect_source_type(url), SourceType::ArxivPdf);
}

#[test]
fn detect_arxiv_pdf_with_dot_pdf_extension() {
    let url = "https://arxiv.org/pdf/1706.03762v1.pdf";
    assert_eq!(detect_source_type(url), SourceType::ArxivPdf);
}

#[test]
fn detect_arxiv_abs() {
    let url = "https://arxiv.org/abs/1706.03762v1";
    assert_eq!(detect_source_type(url), SourceType::ArxivPdf);
}

#[test]
fn detect_arxiv_www_subdomain() {
    let url = "https://www.arxiv.org/pdf/1706.03762";
    assert_eq!(detect_source_type(url), SourceType::ArxivPdf);
}

#[test]
fn detect_arxiv_no_version() {
    let url = "https://arxiv.org/pdf/1706.03762";
    assert_eq!(detect_source_type(url), SourceType::ArxivPdf);
}

#[test]
fn detect_non_arxiv_url_not_arxiv() {
    // arXiv-like path but wrong domain
    let url = "https://example.com/pdf/1706.03762";
    assert_eq!(detect_source_type(url), SourceType::GenericHtml);
}

// -- detect_source_type: Document file extensions -----------------------

#[test]
fn detect_document_pdf() {
    let url = "https://example.com/paper.pdf";
    assert_eq!(detect_source_type(url), SourceType::Document);
}

#[test]
fn detect_document_pdf_uppercase() {
    let url = "https://example.com/paper.PDF";
    assert_eq!(detect_source_type(url), SourceType::Document);
}

#[test]
fn detect_document_pdf_with_query() {
    let url = "https://example.com/paper.pdf?download=1";
    assert_eq!(detect_source_type(url), SourceType::Document);
}

#[test]
fn detect_document_docx() {
    let url = "https://example.com/report.docx";
    assert_eq!(detect_source_type(url), SourceType::Document);
}

#[test]
fn detect_document_pptx() {
    let url = "https://example.com/slides.pptx";
    assert_eq!(detect_source_type(url), SourceType::Document);
}

#[test]
fn detect_document_key() {
    let url = "https://example.com/presentation.key";
    assert_eq!(detect_source_type(url), SourceType::Document);
}

#[test]
fn detect_arxiv_pdf_takes_priority_over_document() {
    // arXiv PDF URLs should match ArxivPdf, not Document
    let url = "https://arxiv.org/pdf/1706.03762v1.pdf";
    assert_eq!(detect_source_type(url), SourceType::ArxivPdf);
}

// -- detect_from_mime_type ------------------------------------------------

#[test]
fn detect_mime_pdf_standard() {
    assert_eq!(
        detect_from_mime_type("application/pdf"),
        Some(SourceType::Document)
    );
}

#[test]
fn detect_mime_pdf_with_charset() {
    assert_eq!(
        detect_from_mime_type("application/pdf; charset=utf-8"),
        Some(SourceType::Document)
    );
}

#[test]
fn detect_mime_x_pdf() {
    assert_eq!(
        detect_from_mime_type("application/x-pdf"),
        Some(SourceType::Document)
    );
}

#[test]
fn detect_mime_msword() {
    assert_eq!(
        detect_from_mime_type("application/msword"),
        Some(SourceType::Document)
    );
}

#[test]
fn detect_mime_docx() {
    assert_eq!(
        detect_from_mime_type(
            "application/vnd.openxmlformats-officedocument.wordprocessingml.document"
        ),
        Some(SourceType::Document)
    );
}

#[test]
fn detect_mime_pptx() {
    assert_eq!(
        detect_from_mime_type("application/vnd.ms-powerpoint"),
        Some(SourceType::Document)
    );
}

#[test]
fn detect_mime_text_html_is_none() {
    assert_eq!(detect_from_mime_type("text/html"), None);
}

#[test]
fn detect_mime_text_plain_is_none() {
    assert_eq!(detect_from_mime_type("text/plain; charset=utf-8"), None);
}

#[test]
fn detect_mime_empty_is_none() {
    assert_eq!(detect_from_mime_type(""), None);
}

#[test]
fn detect_mime_epub_plus_zip_is_none() {
    // application/epub+zip contains "pdf" as substring of "epub" but should NOT match
    assert_eq!(detect_from_mime_type("application/epub+zip"), None);
}

#[test]
fn detect_mime_pdf_editor_is_none() {
    // application/vnd.example.pdf-editor contains "pdf" but is NOT a PDF MIME type
    assert_eq!(
        detect_from_mime_type("application/vnd.example.pdf-editor"),
        None
    );
}

// -- arxiv_url_to_html_url ------------------------------------------------

#[test]
fn arxiv_pdf_to_html_url() {
    let url = "https://arxiv.org/pdf/1706.03762v1.pdf";
    assert_eq!(
        arxiv_url_to_html_url(url).unwrap(),
        "https://arxiv.org/html/1706.03762v1"
    );
}

#[test]
fn arxiv_pdf_no_extension_to_html() {
    let url = "https://arxiv.org/pdf/1706.03762v1";
    assert_eq!(
        arxiv_url_to_html_url(url).unwrap(),
        "https://arxiv.org/html/1706.03762v1"
    );
}

#[test]
fn arxiv_abs_to_html_url() {
    let url = "https://arxiv.org/abs/1706.03762v1";
    assert_eq!(
        arxiv_url_to_html_url(url).unwrap(),
        "https://arxiv.org/html/1706.03762v1"
    );
}

#[test]
fn arxiv_http_scheme_to_html() {
    let url = "http://arxiv.org/pdf/1706.03762v1.pdf";
    assert_eq!(
        arxiv_url_to_html_url(url).unwrap(),
        "https://arxiv.org/html/1706.03762v1"
    );
}

#[test]
fn arxiv_non_arxiv_url_returns_original() {
    let url = "https://example.com/paper.pdf";
    assert_eq!(arxiv_url_to_html_url(url).unwrap(), url);
}

#[test]
fn arxiv_url_with_trailing_slash() {
    let url = "https://arxiv.org/pdf/1706.03762v1/";
    assert_eq!(
        arxiv_url_to_html_url(url).unwrap(),
        "https://arxiv.org/html/1706.03762v1"
    );
}

#[test]
fn arxiv_http_www_pdf_to_html() {
    // http://www.arxiv.org/ prefix was missing from strip_prefix chain
    let url = "http://www.arxiv.org/pdf/1706.03762v1.pdf";
    assert_eq!(
        arxiv_url_to_html_url(url).unwrap(),
        "https://arxiv.org/html/1706.03762v1"
    );
}

#[test]
fn arxiv_http_www_abs_to_html() {
    // http://www.arxiv.org/abs/ prefix was missing from strip_prefix chain
    let url = "http://www.arxiv.org/abs/1706.03762v1";
    assert_eq!(
        arxiv_url_to_html_url(url).unwrap(),
        "https://arxiv.org/html/1706.03762v1"
    );
}

#[test]
fn arxiv_https_www_pdf_to_html() {
    let url = "https://www.arxiv.org/pdf/1706.03762v1";
    assert_eq!(
        arxiv_url_to_html_url(url).unwrap(),
        "https://arxiv.org/html/1706.03762v1"
    );
}

#[test]
fn arxiv_https_www_abs_to_html() {
    let url = "https://www.arxiv.org/abs/1706.03762v1";
    assert_eq!(
        arxiv_url_to_html_url(url).unwrap(),
        "https://arxiv.org/html/1706.03762v1"
    );
}
