//! Delulu Rate-Limited Crawler
//! Copyright (c) 2026 Mamy Ratsimbazafy
//! Licensed under AGPL-3.0.
//!
//! A per-domain rate-limited HTTP client with GCRA gating, exponential retry,
//! and LRU eviction of idle domain queues.

pub mod domain_queue;
pub mod error;
pub mod gcra;

use futures_util::StreamExt;
use rand::Rng;
use std::sync::Arc;
use std::time::Duration;

use quick_cache::sync::Cache;
use url::Url;

use crate::domain_queue::DomainQueue;
use crate::error::CrawlerError;

/// Maximum allowed URL length.
const MAX_URL_LENGTH: usize = 2048;
/// A rate-limited HTTP client that gates requests per domain using GCRA.
pub struct RateLimitedCrawler {
    client: wreq::Client,
    domains: Cache<String, Arc<DomainQueue>>,
    qps: u64,
    burst: u64,
    max_resp_size: Option<usize>,
}

impl RateLimitedCrawler {
    pub fn builder() -> CrawlerBuilder {
        CrawlerBuilder::default()
    }

    pub fn get(&self, url: impl Into<String>) -> GetBuilder<'_> {
        GetBuilder {
            crawler: self,
            url: url.into(),
            headers: Vec::new(),
            exponential_retry_base: None,
            retry_limit: None,
        }
    }

    fn domain_queue(&self, url: &Url) -> Result<Arc<DomainQueue>, CrawlerError> {
        let domain = url.host_str().ok_or_else(|| CrawlerError::MissingDomain {
            url: url.as_str().to_string(),
        })?;
        Ok(self
            .domains
            .get_or_insert_with(domain, || {
                Ok::<_, std::convert::Infallible>(Arc::new(DomainQueue::new(self.qps, self.burst)))
            })
            .expect("domain queue creation should not fail"))
    }

    async fn execute_get(
        &self,
        url: &str,
        headers: &[(String, String)],
    ) -> Result<wreq::Response, CrawlerError> {
        let parsed = Url::parse(url).map_err(CrawlerError::UrlParse)?;
        let queue = self.domain_queue(&parsed)?;
        queue.acquire().await;
        let mut req = self.client.get(url);
        for (name, value) in headers {
            req = req.header(name.as_str(), value.as_str());
        }
        let resp = req.send().await.map_err(CrawlerError::Http)?;

        // Check Content-Length against max_resp_size for early rejection
        if let Some(max) = self.max_resp_size
            && let Some(len) = resp.content_length()
            && len as usize > max
        {
            return Err(CrawlerError::ResponseTooLarge {
                size: len as usize,
                max,
            });
        }

        Ok(resp)
    }

    /// Fetch a URL and return the response body as text with its content-type.
    ///
    /// Performs:
    /// - URL validation (length, scheme)
    /// - Per-domain rate limiting (GCRA)
    /// - Content-Length check + streaming body read with size limit
    ///   (if `with_max_resp_size` was configured on the builder)
    /// - Returns `(body_string, content_type_header)`
    pub async fn fetch_text(&self, url: &str) -> Result<(String, Option<String>), CrawlerError> {
        // 1. Validate URL
        let url = url.trim();
        if url.len() > MAX_URL_LENGTH {
            return Err(CrawlerError::InvalidUrl(
                "URL exceeds maximum length".to_string(),
            ));
        }
        let parsed = Url::parse(url).map_err(CrawlerError::UrlParse)?;
        let scheme = parsed.scheme();
        if scheme != "http" && scheme != "https" {
            return Err(CrawlerError::InvalidUrl(format!(
                "Unsupported URL scheme: '{scheme}'"
            )));
        }

        // 2. Rate-limited fetch with exponential retry
        let resp = self.get(url).with_exponential_retry(1).send().await?;

        // 3. Extract content-type (before consuming body)
        let content_type = resp
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok().map(String::from));

        // 4. Check Content-Length + stream body with size limit
        if let Some(max) = self.max_resp_size {
            if let Some(len) = resp.content_length()
                && len as usize > max
            {
                return Err(CrawlerError::ResponseTooLarge {
                    size: len as usize,
                    max,
                });
            }
            let mut body = Vec::new();
            let mut stream = resp.bytes_stream();
            while let Some(chunk) = stream.next().await {
                let chunk = chunk.map_err(CrawlerError::Http)?;
                body.extend_from_slice(&chunk);
                if body.len() > max {
                    return Err(CrawlerError::ResponseTooLarge {
                        size: body.len(),
                        max,
                    });
                }
            }
            Ok((String::from_utf8_lossy(&body).into_owned(), content_type))
        } else {
            let body = resp.text().await.map_err(CrawlerError::Http)?;
            Ok((body, content_type))
        }
    }
}

pub struct CrawlerBuilder {
    client: Option<wreq::Client>,
    client_builder: Option<wreq::ClientBuilder>,
    qps: u64,
    burst: u64,
    max_domains: usize,
    max_resp_size: Option<usize>,
}

impl Default for CrawlerBuilder {
    fn default() -> Self {
        Self {
            client: None,
            client_builder: Some(Self::init_builder()),
            qps: 10,
            burst: 1,
            max_domains: 128,
            max_resp_size: None,
        }
    }
}

impl CrawlerBuilder {
    /// Create a wreq ClientBuilder with defaults (used when no user settings provided).
    fn init_builder() -> wreq::ClientBuilder {
        wreq::Client::builder()
            .emulation(wreq_util::Emulation::Safari18_5)
            .redirect(wreq::redirect::Policy::limited(5))
            .timeout(Duration::from_secs(30))
            .connect_timeout(Duration::from_secs(30))
    }

    /// Use a pre-built `wreq::Client`. Cannot be mixed with builder settings.
    pub fn with_client(mut self, client: wreq::Client) -> Self {
        self.client = Some(client);
        self.client_builder = None;
        self
    }

    pub fn with_emulation(mut self, profile: wreq_util::Profile) -> Self {
        let builder = self.client_builder.unwrap_or_else(wreq::Client::builder);
        self.client_builder = Some(builder.emulation(profile));
        self
    }

    pub fn with_redirect(mut self, policy: wreq::redirect::Policy) -> Self {
        let builder = self.client_builder.unwrap_or_else(wreq::Client::builder);
        self.client_builder = Some(builder.redirect(policy));
        self
    }

    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        let builder = self.client_builder.unwrap_or_else(wreq::Client::builder);
        self.client_builder = Some(builder.timeout(timeout));
        self
    }

    pub fn with_connect_timeout(mut self, timeout: Duration) -> Self {
        let builder = self.client_builder.unwrap_or_else(wreq::Client::builder);
        self.client_builder = Some(builder.connect_timeout(timeout));
        self
    }

    pub fn with_qps(mut self, qps: u64) -> Self {
        self.qps = qps;
        self
    }

    pub fn with_burst(mut self, burst: u64) -> Self {
        self.burst = burst;
        self
    }

    pub fn with_max_domains(mut self, max: usize) -> Self {
        self.max_domains = max;
        self
    }

    pub fn with_max_resp_size(mut self, max: usize) -> Self {
        self.max_resp_size = Some(max);
        self
    }
    pub fn build(self) -> Result<RateLimitedCrawler, CrawlerError> {
        if self.qps == 0 {
            return Err(CrawlerError::QpsZero);
        }
        if self.burst == 0 {
            return Err(CrawlerError::BurstZero);
        }
        if self.max_domains == 0 {
            return Err(CrawlerError::MaxDomainsZero);
        }
        let client = match (self.client, self.client_builder) {
            (Some(c), None) => c,
            (None, Some(builder)) => builder.build().map_err(CrawlerError::Http)?,
            (None, None) => unreachable!("Default initializes client_builder"),
            (Some(_), Some(_)) => {
                return Err(CrawlerError::InvalidConfig {
                    field: "client/client_builder",
                    value: "mixed".into(),
                    reason: "use either with_client or builder settings, not both",
                });
            }
        };
        Ok(RateLimitedCrawler {
            client,
            domains: Cache::new(self.max_domains),
            qps: self.qps,
            burst: self.burst,
            max_resp_size: self.max_resp_size,
        })
    }
}

pub struct GetBuilder<'a> {
    crawler: &'a RateLimitedCrawler,
    url: String,
    headers: Vec<(String, String)>,
    exponential_retry_base: Option<u64>,
    retry_limit: Option<u32>,
}

impl GetBuilder<'_> {
    pub fn with_headers(mut self, headers: Vec<(String, String)>) -> Self {
        self.headers = headers;
        self
    }

    pub fn with_exponential_retry(mut self, base_delay_secs: u64) -> Self {
        self.exponential_retry_base = Some(base_delay_secs);
        self
    }

    pub fn with_retry_limit(mut self, limit: u32) -> Self {
        self.retry_limit = Some(limit);
        self
    }

    pub async fn send(self) -> Result<wreq::Response, CrawlerError> {
        let retry_base = self.exponential_retry_base;
        let retry_limit = self.retry_limit.unwrap_or(3);

        match retry_base {
            None => self.crawler.execute_get(&self.url, &self.headers).await,
            Some(mut base_secs) => {
                if base_secs == 0 {
                    tracing::warn!("retry base_secs was 0, clamping to 1");
                    base_secs = 1;
                }
                let mut last_error: Option<CrawlerError> = None;
                let mut last_status: Option<u16> = None;

                for attempt in 0..=retry_limit {
                    match self.crawler.execute_get(&self.url, &self.headers).await {
                        Ok(resp) => {
                            let status = resp.status().as_u16();
                            if status == 429 || (500..=599).contains(&status) {
                                tracing::warn!(
                                    "retryable HTTP {status} for {} (attempt {}/{})",
                                    self.url,
                                    attempt + 1,
                                    retry_limit + 1
                                );
                                last_status = Some(status);
                                if attempt < retry_limit {
                                    tokio::time::sleep(compute_backoff(base_secs, attempt)).await;
                                    continue;
                                }
                                // Retries exhausted for HTTP errors — fall through to RetryExhausted
                                continue;
                            }
                            return Ok(resp);
                        }
                        Err(e) if e.is_retryable() => {
                            tracing::warn!(
                                "retryable error for {} (attempt {}/{}): {e}",
                                self.url,
                                attempt + 1,
                                retry_limit + 1
                            );
                            last_error = Some(e);
                            if attempt < retry_limit {
                                tokio::time::sleep(compute_backoff(base_secs, attempt)).await;
                                continue;
                            }
                            // Retries exhausted for connection errors — fall through to RetryExhausted
                        }
                        Err(e) => return Err(e),
                    }
                }

                Err(CrawlerError::RetryExhausted {
                    url: self.url,
                    retries: retry_limit + 1,
                    last_error: last_error.map(Box::new),
                    last_status,
                })
            }
        }
    }
}

fn compute_backoff(base_secs: u64, attempt: u32) -> Duration {
    let exp = 2u64.saturating_pow(attempt);
    let delay_ns = base_secs.saturating_mul(1_000_000_000).saturating_mul(exp);
    let capped = delay_ns.min(60_000_000_000);
    let jitter = rand::thread_rng().gen_range(0..=capped / 2);
    Duration::from_nanos(capped.saturating_add(jitter))
}

impl<'a> std::future::IntoFuture for GetBuilder<'a> {
    type Output = Result<wreq::Response, CrawlerError>;
    type IntoFuture =
        std::pin::Pin<Box<dyn std::future::Future<Output = Self::Output> + Send + 'a>>;

    fn into_future(self) -> Self::IntoFuture {
        Box::pin(async move { self.send().await })
    }
}

#[cfg(test)]
#[path = "../tests/unit/lib_test.rs"]
mod tests;

// ---------------------------------------------------------------------------
// Anti-regression tests
// ---------------------------------------------------------------------------
