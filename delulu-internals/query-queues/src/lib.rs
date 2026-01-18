//! Delulu Query Queues
//! Copyright (c) 2026 Mamy Ratsimbazafy
//! Licensed and distributed under either of
//!   * MIT license (license terms at the root of the package or at http://opensource.org/licenses/MIT).
//!   * Apache v2 license (license terms at the root of the package or at http://www.apache.org/licenses/LICENSE-2.0).
//! at your option. This file may not be copied, modified, or distributed except according to those terms.

//! delulu-internals/query-queues
//! A simple work queue for rate limiting with backoff and jitter for external service calls

use std::sync::Arc;
use std::time::Duration;

use rand::Rng;
use thiserror::Error;
use tokio::sync::Semaphore;
use tokio::time;

/// Custom error for the work queue
#[derive(Debug, Error)]
pub enum QueryQueueError {
    #[error("max retries exceeded: {0}")]
    MaxRetriesExceeded(#[source] anyhow::Error),
    #[error("queue is closed")]
    QueueClosed,
}

/// Configuration for a work queue
#[derive(Clone, Debug)]
struct QueryQueueConfig {
    /// Maximum number of concurrent requests to the external service
    max_concurrent: u64,
    /// Initial delay for backoff in milliseconds
    initial_delay_ms: u64,
    /// Maximum delay for backoff in milliseconds
    max_delay_ms: u64,
    /// Maximum number of retries
    max_retries: u32,
    /// Jitter factor (0.0 to 1.0). 0.0 = no jitter, 1.0 = full jitter
    jitter_factor: f64,
    /// Whether to use exponential backoff
    exponential: bool,
}

impl Default for QueryQueueConfig {
    fn default() -> Self {
        Self {
            max_concurrent: 4,
            initial_delay_ms: 0,
            max_delay_ms: 30000,
            max_retries: 3,
            jitter_factor: 0.5,
            exponential: true,
        }
    }
}

impl QueryQueueConfig {
    /// Create a new config with the given max concurrent requests
    fn new(max_concurrent: u64) -> Self {
        Self {
            max_concurrent,
            ..Default::default()
        }
    }
}

/// An async semaphore for limiting concurrent operations
#[derive(Clone, Debug)]
struct AsyncSemaphore {
    inner: Arc<Semaphore>,
}

impl AsyncSemaphore {
    /// Create a new semaphore with the given number of permits
    fn new(permits: usize) -> Self {
        Self {
            inner: Arc::new(Semaphore::new(permits)),
        }
    }

    /// Acquire a permit, waiting asynchronously if necessary
    async fn acquire(&self) -> Result<tokio::sync::SemaphorePermit<'_>, tokio::sync::AcquireError> {
        self.inner.acquire().await
    }
}

/// A simple work queue that limits concurrent requests to an external service
/// and uses exponential backoff with jitter for retries
///
/// # Example
///
/// ```ignore
/// let queue = QueryQueue::with_max_concurrent(4);
/// let result = queue.with_retry(|| async {
///     // Your HTTP request here
///     Ok(response)
/// }).await;
/// ```
#[derive(Clone, Debug)]
pub struct QueryQueue {
    semaphore: AsyncSemaphore,
    initial_delay: Duration,
    max_delay: Duration,
    jitter_factor: f64,
    max_retries: u32,
    exponential: bool,
}

impl QueryQueue {
    /// Create a new work queue with the given config
    fn new(config: &QueryQueueConfig) -> Self {
        Self {
            semaphore: AsyncSemaphore::new(config.max_concurrent as usize),
            initial_delay: Duration::from_millis(config.initial_delay_ms),
            max_delay: Duration::from_millis(config.max_delay_ms),
            jitter_factor: config.jitter_factor,
            max_retries: config.max_retries,
            exponential: config.exponential,
        }
    }

    /// Create a new work queue with the given max concurrent requests
    pub fn with_max_concurrent(max_concurrent: u64) -> Self {
        let config = QueryQueueConfig::new(max_concurrent);
        Self::new(&config)
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
        // Acquire a permit
        let _permit = self
            .semaphore
            .acquire()
            .await
            .map_err(|_| QueryQueueError::QueueClosed)?;

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
