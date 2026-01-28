//! Delulu Query Queues
//! Copyright (c) 2026 Mamy Ratsimbazafy
//! Licensed and distributed under either of
//!   * MIT license (license terms at the root of the package or at http://opensource.org/licenses/MIT).
//!   * Apache v2 license (license terms at the root of the package or at http://www.apache.org/licenses/LICENSE-2.0).
//! at your option. This file may not be copied, modified, or distributed except according to those terms.

//! delulu-internals/query-queues
//! A simple work queue for rate limiting with backoff and jitter for external service calls

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use rand::Rng;
use thiserror::Error;
use tokio::sync::Semaphore;
use tokio::sync::{Mutex, Notify};
use tokio::time;

/// Custom error for the work queue
#[derive(Debug, Error)]
pub enum QueryQueueError {
    #[error("max retries exceeded: {0}")]
    MaxRetriesExceeded(#[source] anyhow::Error),
    #[error("queue is closed")]
    QueueClosed,
}

/// Rate limiting mode
#[derive(Clone, Debug)]
enum RateLimit {
    ConcurrencyOnly,
    Qps {
        limit: u64,
        tokens: Arc<AtomicU64>,
        last_refill: Arc<Mutex<Instant>>,
        refill_interval: Duration,
        notify: Arc<tokio::sync::Notify>,
    },
}

impl Default for RateLimit {
    fn default() -> Self {
        Self::ConcurrencyOnly
    }
}

/// An async semaphore for limiting concurrent operations
#[derive(Clone, Debug)]
struct AsyncSemaphore {
    inner: Arc<Semaphore>,
}

impl AsyncSemaphore {
    fn new(permits: usize) -> Self {
        Self {
            inner: Arc::new(Semaphore::new(permits)),
        }
    }

    async fn acquire(&self) -> Result<tokio::sync::SemaphorePermit<'_>, tokio::sync::AcquireError> {
        self.inner.acquire().await
    }
}

/// A simple work queue that limits concurrent requests to an external service
/// and uses exponential backoff with jitter for retries
///
/// # Examples
///
/// Concurrency only (4 concurrent requests):
/// ```ignore
/// let queue = QueryQueue::with_concurrency_limit(4);
/// ```
///
/// QPS limit (4 requests per second):
/// ```ignore
/// let queue = QueryQueue::with_qps_limit(4);
/// ```
#[derive(Clone, Debug)]
pub struct QueryQueue {
    semaphore: AsyncSemaphore,
    initial_delay: Duration,
    max_delay: Duration,
    jitter_factor: f64,
    max_retries: u32,
    exponential: bool,
    rate_limit: RateLimit,
}

impl Default for QueryQueue {
    fn default() -> Self {
        Self {
            semaphore: AsyncSemaphore::new(4),
            initial_delay: Duration::from_millis(100),
            max_delay: Duration::from_millis(30000),
            jitter_factor: 0.5,
            max_retries: 3,
            exponential: true,
            rate_limit: RateLimit::ConcurrencyOnly,
        }
    }
}

impl QueryQueue {
    /// Create a new work queue with max concurrent requests
    pub fn with_concurrency_limit(max_concurrent: u64) -> Self {
        let max_concurrent = max_concurrent.max(1);
        Self {
            semaphore: AsyncSemaphore::new(max_concurrent as usize),
            ..Default::default()
        }
    }

    /// Create a new work queue with QPS limit
    pub fn with_qps_limit(qps_limit: u64) -> Self {
        let qps_limit = qps_limit.max(1);
        Self {
            semaphore: AsyncSemaphore::new(qps_limit as usize),
            rate_limit: RateLimit::Qps {
                limit: qps_limit,
                tokens: Arc::new(AtomicU64::new(qps_limit)),
                last_refill: Arc::new(Mutex::new(Instant::now())),
                refill_interval: Duration::from_secs(1),
                notify: Arc::new(Notify::new()),
            },
            ..Default::default()
        }
    }

    /// Refill tokens based on elapsed time
    async fn refill_tokens(&self) {
        match &self.rate_limit {
            RateLimit::ConcurrencyOnly => {}
            RateLimit::Qps {
                limit,
                tokens,
                last_refill,
                refill_interval,
                notify,
                ..
            } => {
                let mut last = last_refill.lock().await;
                let now = Instant::now();
                let elapsed = now.duration_since(*last);
                if elapsed >= *refill_interval {
                    let new_tokens = (elapsed.as_secs_f64() * *limit as f64) as u64;
                    if new_tokens > 0 {
                        let _ = tokens.fetch_update(Ordering::SeqCst, Ordering::SeqCst, |cur| {
                            Some(cur.saturating_add(new_tokens).min(*limit))
                        });
                        // Wake up any waiters that token is now available
                        notify.notify_waiters();
                    }
                    *last = now;
                }
            }
        }
    }

    // Acquire a token for rate limiting using async notification
    async fn acquire_token(&self) {
        match &self.rate_limit {
            RateLimit::ConcurrencyOnly => {}
            RateLimit::Qps { tokens, notify, .. } => loop {
                self.refill_tokens().await;
                let available = tokens.load(Ordering::SeqCst);
                if available > 0 {
                    if tokens
                        .compare_exchange(
                            available,
                            available - 1,
                            Ordering::SeqCst,
                            Ordering::SeqCst,
                        )
                        .is_ok()
                    {
                        return;
                    }
                } else {
                    let _ = time::timeout(Duration::from_millis(100), notify.notified()).await;
                }
            },
        }
    }

    /// Execute a function with rate limiting and retry
    ///
    /// The function `f` should return `Result<T, E>` where `E` implements `std::error::Error`.
    /// If the function returns `Err`, it will be retried with exponential backoff and jitter.
    pub async fn with_retry<T, F, Fut>(&self, mut f: F) -> Result<T, QueryQueueError>
    where
        F: FnMut() -> Fut + Send,
        Fut: std::future::Future<Output = Result<T, anyhow::Error>> + Send,
    {
        // Acquire a permit (for concurrency control)
        let _permit = self
            .semaphore
            .acquire()
            .await
            .map_err(|_| QueryQueueError::QueueClosed)?;

        // Acquire a token (for QPS rate limiting)
        self.acquire_token().await;

        // Execute with backoff
        let mut retry_count = 0;
        let mut delay = self.initial_delay;

        loop {
            match f().await {
                Ok(result) => return Ok(result),
                Err(e) => {
                    retry_count += 1;
                    if retry_count > self.max_retries {
                        return Err(QueryQueueError::MaxRetriesExceeded(e));
                    }

                    // Apply jitter to the delay
                    let jittered_delay = self.apply_jitter(delay);
                    time::sleep(jittered_delay).await;

                    // Increase delay for next retry if exponential is enabled
                    if self.exponential {
                        delay = std::cmp::min(delay * 2, self.max_delay);
                    }
                }
            }
        }
    }

    /// Apply jitter to the delay
    fn apply_jitter(&self, delay: Duration) -> Duration {
        if self.jitter_factor == 0.0 {
            return delay;
        }

        let jitter_ms = (delay.as_millis() as f64 * self.jitter_factor) as u64;
        let rand_jitter = rand::thread_rng().gen_range(0..=jitter_ms);

        Duration::from_millis(delay.as_millis() as u64 + rand_jitter)
    }
}
