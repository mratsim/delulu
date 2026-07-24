//! Error types for the rate-limited crawler.

use std::fmt;

use thiserror::Error;

/// Errors that can occur during crawling operations.
#[derive(Debug, Error)]
pub enum CrawlerError {
    /// An HTTP request failed (network error, timeout, etc.).
    Http(#[source] wreq::Error),

    /// Builder was called with qps=0.
    QpsZero,

    /// Builder was called with burst=0.
    BurstZero,

    /// Builder was called with max_domains=0.
    MaxDomainsZero,

    /// Other configuration validation failure.
    InvalidConfig {
        /// The config field name.
        field: &'static str,
        /// The invalid value.
        value: String,
        /// Why it's invalid.
        reason: &'static str,
    },

    /// The retry loop was exhausted.
    RetryExhausted {
        /// The URL that was being fetched.
        url: String,
        /// Number of retry attempts made.
        retries: u32,
        /// The error from the last retry attempt, if any.
        last_error: Option<Box<CrawlerError>>,
        /// HTTP status code from the last attempt, if it was an HTTP response.
        last_status: Option<u16>,
    },
    /// URL parsing failed.
    UrlParse(#[from] url::ParseError),

    /// The URL has no extractable host (e.g., IP address or file URL).
    MissingDomain {
        /// The URL that caused the error.
        url: String,
    },

    /// Response body exceeded the maximum allowed size.
    ResponseTooLarge {
        /// Actual body size in bytes.
        size: usize,
        /// Maximum allowed size in bytes.
        max: usize,
    },
    /// Invalid URL: unsupported scheme, too long, or malformed.
    InvalidUrl(String),
}

impl fmt::Display for CrawlerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Http(e) => write!(f, "HTTP request failed: {e}"),
            Self::QpsZero => write!(f, "qps must be > 0, got 0"),
            Self::BurstZero => write!(f, "burst must be > 0, got 0"),
            Self::MaxDomainsZero => write!(f, "max_domains must be > 0, got 0"),
            Self::InvalidConfig {
                field,
                value,
                reason,
            } => {
                write!(f, "invalid config: {field}={value} — {reason}")
            }
            Self::RetryExhausted {
                retries,
                last_error,
                last_status,
                ..
            } => {
                write!(f, "retry exhausted after {retries} attempts")?;
                if let Some(status) = last_status {
                    write!(f, " (HTTP {status})")?;
                }
                if let Some(err) = last_error {
                    write!(f, ": {err}")
                } else {
                    Ok(())
                }
            }
            Self::UrlParse(e) => write!(f, "URL parse error: {e}"),
            Self::MissingDomain { url } => write!(f, "URL has no host: {url}"),
            Self::ResponseTooLarge { size, max } => {
                write!(f, "response body too large: {size} bytes (max {max})")
            }
            Self::InvalidUrl(msg) => write!(f, "invalid URL: {msg}"),
        }
    }
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
            | CrawlerError::InvalidConfig { .. }
            | CrawlerError::ResponseTooLarge { .. }
            | CrawlerError::InvalidUrl(..) => false,
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
