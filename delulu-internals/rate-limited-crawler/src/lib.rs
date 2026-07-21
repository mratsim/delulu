//! Delulu Rate-Limited Crawler
//! Copyright (c) 2026 Mamy Ratsimbazafy
//! Licensed under AGPL-3.0.
//!
//! A per-domain rate-limited HTTP client with GCRA gating, exponential retry,
//! and LRU eviction of idle domain queues.

pub mod domain_queue;
pub mod error;
pub mod gcra;

use rand::Rng;
use std::sync::Arc;
use std::time::Duration;

use quick_cache::sync::Cache;
use url::Url;

use crate::domain_queue::DomainQueue;
use crate::error::CrawlerError;

/// A rate-limited HTTP client that gates requests per domain using GCRA.
pub struct RateLimitedCrawler {
    client: wreq::Client,
    domains: Cache<String, Arc<DomainQueue>>,
    qps: u64,
    burst: u64,
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
                Ok::<_, std::convert::Infallible>(Arc::new(DomainQueue::new(
                    self.qps, self.burst,
                )))
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
        req.send().await.map_err(CrawlerError::Http)
    }
}

pub struct CrawlerBuilder {
    client_builder: Option<wreq::ClientBuilder>,
    client: Option<wreq::Client>,
    qps: u64,
    burst: u64,
    max_domains: usize,
}

impl Default for CrawlerBuilder {
    fn default() -> Self {
        Self {
            client_builder: Some(wreq::Client::builder()),
            client: None,
            qps: 10,
            burst: 1,
            max_domains: 128,
        }
    }
}

impl CrawlerBuilder {
    pub fn with_client(mut self, client: wreq::Client) -> Self {
        self.client = Some(client);
        self.client_builder = None;
        self
    }

    pub fn with_emulation(mut self, emulation: wreq_util::Emulation) -> Self {
        if let Some(builder) = self.client_builder.as_mut() {
            *builder = wreq::Client::builder().emulation(emulation);
        }
        self
    }

    pub fn with_redirect(mut self, policy: wreq::redirect::Policy) -> Self {
        if let Some(builder) = self.client_builder.as_mut() {
            *builder = wreq::Client::builder().redirect(policy);
        }
        self
    }

    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        if let Some(builder) = self.client_builder.as_mut() {
            *builder = wreq::Client::builder().timeout(timeout);
        }
        self
    }

    pub fn with_connect_timeout(mut self, timeout: Duration) -> Self {
        if let Some(builder) = self.client_builder.as_mut() {
            *builder = wreq::Client::builder().connect_timeout(timeout);
        }
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
        let client = match self.client {
            Some(c) => c,
            None => self
                .client_builder
                .unwrap_or_else(|| wreq::Client::builder())
                .emulation(wreq_util::Emulation::Safari18_5)
                .redirect(wreq::redirect::Policy::limited(5))
                .timeout(Duration::from_secs(30))
                .connect_timeout(Duration::from_secs(30))
                .build()
                .map_err(CrawlerError::Http)?,
        };
        Ok(RateLimitedCrawler {
            client,
            domains: Cache::new(self.max_domains),
            qps: self.qps,
            burst: self.burst,
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
                                    self.url, attempt + 1, retry_limit + 1
                                );
                                last_status = Some(status);
                                if attempt < retry_limit {
                                    tokio::time::sleep(compute_backoff(base_secs, attempt)).await;
                                    continue;
                                }
                            }
                            return Ok(resp);
                        }
                        Err(e) if e.is_retryable() && attempt < retry_limit => {
                            tracing::warn!(
                                "retryable error for {} (attempt {}/{}): {e}",
                                self.url, attempt + 1, retry_limit + 1
                            );
                            last_error = Some(e);
                            tokio::time::sleep(compute_backoff(base_secs, attempt)).await;
                        }
                        Err(e) => return Err(e),
                    }
                }

                Err(CrawlerError::RetryExhausted {
                    url: self.url,
                    retries: retry_limit + 1,
                    last_error: Box::new(last_error.unwrap_or(CrawlerError::QpsZero)),
                    last_status,
                })
            }
        }
    }
}

fn compute_backoff(base_secs: u64, attempt: u32) -> Duration {
    let exp = 2u64.saturating_pow(attempt);
    let delay_ns = (base_secs as u64)
        .saturating_mul(1_000_000_000)
        .saturating_mul(exp);
    let capped = delay_ns.min(60_000_000_000);
    let jitter = rand::thread_rng().gen_range(0..=capped / 2);
    Duration::from_nanos(capped.saturating_add(jitter))
}

impl<'a> std::future::IntoFuture for GetBuilder<'a> {
    type Output = Result<wreq::Response, CrawlerError>;
    type IntoFuture = std::pin::Pin<Box<dyn std::future::Future<Output = Self::Output> + Send + 'a>>;

    fn into_future(self) -> Self::IntoFuture {
        Box::pin(async move { self.send().await })
    }
}

#[cfg(test)]
#[path = "../tests/unit/lib_test.rs"]
mod tests;
