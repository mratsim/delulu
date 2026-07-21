//! Error types for the rate-limited crawler.

use thiserror::Error;

/// Errors that can occur during crawling operations.
#[derive(Debug, Error)]
pub enum CrawlerError {
    /// An HTTP request failed (network error, timeout, etc.).
    #[error("HTTP request failed: {0}")]
    Http(#[source] wreq::Error),

    /// Builder was called with qps=0.
    #[error("qps must be > 0, got 0")]
    QpsZero,

    /// Builder was called with burst=0.
    #[error("burst must be > 0, got 0")]
    BurstZero,

    /// Builder was called with max_domains=0.
    #[error("max_domains must be > 0, got 0")]
    MaxDomainsZero,

    /// Other configuration validation failure.
    #[error("invalid config: {field}={value} — {reason}")]
    InvalidConfig {
        /// The config field name.
        field: &'static str,
        /// The invalid value.
        value: String,
        /// Why it's invalid.
        reason: &'static str,
    },

    /// The retry loop was exhausted.
    #[error("retry exhausted after {retries} attempts: {last_error}")]
    RetryExhausted {
        /// The URL that was being fetched.
        url: String,
        /// Number of retry attempts made.
        retries: u32,
        /// The error from the last retry attempt.
        last_error: Box<CrawlerError>,
        /// HTTP status code from the last attempt, if it was an HTTP response.
        last_status: Option<u16>,
    },

    /// URL parsing failed.
    #[error("URL parse error: {0}")]
    UrlParse(#[from] url::ParseError),

    /// The URL has no extractable host (e.g., IP address or file URL).
    #[error("URL has no host: {url}")]
    MissingDomain {
        /// The URL that caused the error.
        url: String,
    },
}

impl CrawlerError {
    /// Returns `true` if this error is retryable.
    ///
    /// Retryable errors: connection errors, timeouts, DNS failures.
    /// Non-retryable: TLS errors, protocol errors, request construction errors.
    pub fn is_retryable(&self) -> bool {
        match self {
            CrawlerError::Http(e) => e.is_timeout() || e.is_connect(),
            CrawlerError::RetryExhausted { .. }
            | CrawlerError::UrlParse(_)
            | CrawlerError::MissingDomain { .. }
            | CrawlerError::QpsZero
            | CrawlerError::BurstZero
            | CrawlerError::MaxDomainsZero
            | CrawlerError::InvalidConfig { .. } => false,
        }
    }
}

impl From<wreq::Error> for CrawlerError {
    fn from(e: wreq::Error) -> Self {
        CrawlerError::Http(e)
    }
}

#[cfg(test)]
#[path = "../tests/unit/error_test.rs"]
mod tests;
