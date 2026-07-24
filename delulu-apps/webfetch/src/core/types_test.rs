use super::*;

// -- WebfetchError construction ---------------------------------------

#[test]
fn test_error_fetch() {
    let e = WebfetchError::Fetch("connection refused".into());
    assert_eq!(e.to_string(), "HTTP fetch error: connection refused");
}

#[test]
fn test_error_parse() {
    let e = WebfetchError::Parse("invalid JSON".into());
    assert_eq!(e.to_string(), "Parse error: invalid JSON");
}

#[test]
fn test_error_pass() {
    let e = WebfetchError::Pass("DOM walk failed".into());
    assert_eq!(e.to_string(), "DOM pass error: DOM walk failed");
}

#[test]
fn test_error_invalid_url() {
    let e = WebfetchError::InvalidUrl("bad scheme".into());
    assert_eq!(e.to_string(), "Invalid URL: bad scheme");
}

#[test]
fn test_error_timeout() {
    let e = WebfetchError::Timeout("timed out after 30s".into());
    assert_eq!(e.to_string(), "Request timed out: timed out after 30s");
}

#[test]
fn test_error_retry_exhausted() {
    let e = WebfetchError::RetryExhausted(3);
    assert_eq!(e.to_string(), "Retry exhausted after 3 attempts");
}

#[test]
fn test_error_auth_required() {
    let e = WebfetchError::AuthRequired("API key missing".into());
    assert_eq!(e.to_string(), "Authentication required: API key missing");
}

// -- SourceType Display / FromStr round-trip ----------------------------

#[test]
fn test_source_type_display_reddit() {
    assert_eq!(SourceType::Reddit.to_string(), "reddit");
}

#[test]
fn test_source_type_display_discourse() {
    assert_eq!(SourceType::Discourse.to_string(), "discourse");
}

#[test]
fn test_source_type_display_generic_html() {
    assert_eq!(SourceType::GenericHtml.to_string(), "generic_html");
}

#[test]
fn test_source_type_from_str_reddit() {
    assert_eq!("reddit".parse::<SourceType>().unwrap(), SourceType::Reddit);
}

#[test]
fn test_source_type_from_str_discourse() {
    assert_eq!(
        "discourse".parse::<SourceType>().unwrap(),
        SourceType::Discourse
    );
}

#[test]
fn test_source_type_from_str_generic() {
    assert_eq!(
        "generic".parse::<SourceType>().unwrap(),
        SourceType::GenericHtml
    );
}

#[test]
fn test_source_type_from_str_html() {
    assert_eq!(
        "html".parse::<SourceType>().unwrap(),
        SourceType::GenericHtml
    );
}

#[test]
fn test_source_type_from_str_generic_html() {
    assert_eq!(
        "generic_html".parse::<SourceType>().unwrap(),
        SourceType::GenericHtml
    );
}

#[test]
fn test_source_type_from_str_case_insensitive() {
    assert_eq!("Reddit".parse::<SourceType>().unwrap(), SourceType::Reddit);
    assert_eq!(
        "DISCOURSE".parse::<SourceType>().unwrap(),
        SourceType::Discourse
    );
    assert_eq!(
        "GENERIC_HTML".parse::<SourceType>().unwrap(),
        SourceType::GenericHtml
    );
}

#[test]
fn test_source_type_from_str_unknown() {
    let err = "unknown".parse::<SourceType>().unwrap_err();
    assert!(err.contains("unknown source type"));
}

// -- SourceType round-trip ----------------------------------------------

#[test]
fn test_source_type_round_trip() {
    for variant in [
        SourceType::Reddit,
        SourceType::Discourse,
        SourceType::GenericHtml,
    ] {
        let display = variant.to_string();
        let parsed: SourceType = display.parse().unwrap();
        assert_eq!(parsed, variant);
    }
}

// -- New SourceType variants --------------------------------------------------

#[test]
fn test_source_type_display_arxiv_pdf() {
    assert_eq!(SourceType::ArxivPdf.to_string(), "arxiv_pdf");
}

#[test]
fn test_source_type_display_document() {
    assert_eq!(SourceType::Document.to_string(), "document");
}

#[test]
fn test_source_type_from_str_arxiv_pdf() {
    assert_eq!(
        "arxiv_pdf".parse::<SourceType>().unwrap(),
        SourceType::ArxivPdf
    );
}

#[test]
fn test_source_type_from_str_arxiv() {
    assert_eq!("arxiv".parse::<SourceType>().unwrap(), SourceType::ArxivPdf);
}

#[test]
fn test_source_type_from_str_document() {
    assert_eq!(
        "document".parse::<SourceType>().unwrap(),
        SourceType::Document
    );
}

#[test]
fn test_source_type_from_str_doc() {
    assert_eq!("doc".parse::<SourceType>().unwrap(), SourceType::Document);
}

#[test]
fn test_source_type_from_str_arxiv_pdf_case_insensitive() {
    assert_eq!(
        "ArXiv_PDF".parse::<SourceType>().unwrap(),
        SourceType::ArxivPdf
    );
    assert_eq!("ARXIV".parse::<SourceType>().unwrap(), SourceType::ArxivPdf);
}

#[test]
fn test_source_type_from_str_document_case_insensitive() {
    assert_eq!(
        "DOCUMENT".parse::<SourceType>().unwrap(),
        SourceType::Document
    );
    assert_eq!("Doc".parse::<SourceType>().unwrap(), SourceType::Document);
}

#[test]
fn test_source_type_round_trip_includes_new() {
    for variant in [SourceType::ArxivPdf, SourceType::Document] {
        let display = variant.to_string();
        let parsed: SourceType = display.parse().unwrap();
        assert_eq!(parsed, variant);
    }
}

// -- New WebfetchError variants ----------------------------------------------

#[test]
fn test_error_io() {
    let e = WebfetchError::IoError("file not found".into());
    assert_eq!(e.to_string(), "I/O error: file not found");
}

#[test]
fn test_error_xberg() {
    let e = WebfetchError::XbergError("xberg service unavailable".into());
    assert_eq!(e.to_string(), "xberg error: xberg service unavailable");
}
