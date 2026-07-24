use super::*;

/// Verifies that RetryExhausted with an HTTP status code includes the status
/// in the Display output and does NOT fall back to a misleading error like QpsZero.
#[test]
fn test_retry_exhausted_http_message() {
    let err = CrawlerError::RetryExhausted {
        url: "https://example.com".into(),
        retries: 4,
        last_error: None,
        last_status: Some(429),
    };
    let msg = err.to_string();
    assert!(
        msg.contains("retry exhausted after 4 attempts"),
        "expected retry count in message, got: {msg}"
    );
    assert!(
        msg.contains("HTTP 429"),
        "expected HTTP status in message, got: {msg}"
    );
    assert!(
        !msg.contains("qps"),
        "must NOT mention qps when status is set, got: {msg}"
    );
}

/// Verifies that RetryExhausted with a nested connection error includes
/// the inner error message.
#[test]
fn test_retry_exhausted_connection_message() {
    let inner = CrawlerError::MissingDomain {
        url: "http://bad".into(),
    };
    let err = CrawlerError::RetryExhausted {
        url: "https://example.com".into(),
        retries: 3,
        last_error: Some(Box::new(inner)),
        last_status: None,
    };
    let msg = err.to_string();
    assert!(
        msg.contains("retry exhausted after 3 attempts"),
        "expected retry count in message, got: {msg}"
    );
    assert!(
        msg.contains("URL has no host"),
        "expected inner error message in output, got: {msg}"
    );
}

/// Verifies that RetryExhausted with neither status nor inner error
/// produces a clean message without extraneous punctuation.
#[test]
fn test_retry_exhausted_no_status_no_error() {
    let err = CrawlerError::RetryExhausted {
        url: "https://example.com".into(),
        retries: 1,
        last_error: None,
        last_status: None,
    };
    let msg = err.to_string();
    assert!(
        msg.contains("retry exhausted after 1 attempts"),
        "expected retry count in message, got: {msg}"
    );
    // Should not end with ": " or have trailing junk
    assert!(
        !msg.contains(": "),
        "must not have trailing colon-space when no inner error, got: {msg}"
    );
}

/// Verifies that other error variants still produce correct Display output
/// after the switch to manual Display impl.
#[test]
fn test_other_variants_display() {
    assert_eq!(CrawlerError::QpsZero.to_string(), "qps must be > 0, got 0");
    assert_eq!(
        CrawlerError::BurstZero.to_string(),
        "burst must be > 0, got 0"
    );
    assert_eq!(
        CrawlerError::MaxDomainsZero.to_string(),
        "max_domains must be > 0, got 0"
    );
    assert_eq!(
        CrawlerError::InvalidConfig {
            field: "timeout",
            value: "abc".into(),
            reason: "not a number",
        }
        .to_string(),
        "invalid config: timeout=abc — not a number"
    );
    assert_eq!(
        CrawlerError::MissingDomain {
            url: "http://bad".into(),
        }
        .to_string(),
        "URL has no host: http://bad"
    );
}

/// Verifies that Debug output still works (derived).
#[test]
fn test_retry_exhausted_debug() {
    let err = CrawlerError::RetryExhausted {
        url: "https://example.com".into(),
        retries: 4,
        last_error: None,
        last_status: Some(429),
    };
    let debug = format!("{err:?}");
    assert!(
        debug.contains("RetryExhausted"),
        "Debug should show variant name"
    );
    assert!(debug.contains("429"), "Debug should show status");
}
