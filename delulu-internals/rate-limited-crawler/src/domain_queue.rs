//! Per-domain queue that gates requests through a GCRA state.
//!
//! No `AsyncSemaphore` — GCRA naturally serializes requests through the gate.
//! At 10 QPS, requests are spaced 100ms apart. At 50 QPS, 20ms apart.
//! Concurrent callers to the same domain form an implicit queue.

use std::time::Duration;

use tokio::time::Instant;

use crate::gcra::GcraState;

/// Maximum sleep duration for a denied request (60 seconds).
/// Prevents clock-jump-induced hangs.
const MAX_SLEEP_NS: u64 = 60_000_000_000;

/// A per-domain queue that serializes requests through a GCRA gate.
pub struct DomainQueue {
    gcra: GcraState,
}

impl DomainQueue {
    /// Create a new domain queue with the given QPS and burst capacity.
    ///
    /// # Panics
    /// Panics if `qps == 0` (see `GcraState::new`).
    pub fn new(qps: u64, burst: u64) -> Self {
        Self {
            gcra: GcraState::new(qps, burst),
        }
    }

    /// Wait until a token is available, then return.
    ///
    /// This loops, sleeping the exact GCRA-computed wait time. No busy-waiting.
    /// Sleeps are capped at 60 seconds to prevent clock-jump-induced hangs.
    pub async fn acquire(&self) {
        loop {
            let now = self.nanos_since_start();
            match self.gcra.try_consume(now) {
                Ok(tat) => {
                    // Token consumed. If the GCRA says we should wait
                    // (inter-request spacing from burst), sleep that long.
                    let wait = tat.saturating_sub(now);
                    if wait > 0 {
                        tokio::time::sleep(Duration::from_nanos(wait)).await;
                    }
                    return;
                }
                Err(tat) => {
                    // Denied. Wait until `tat` (capped at 60s).
                    let wait = tat.saturating_sub(now);
                    let capped = wait.min(MAX_SLEEP_NS);
                    if capped != wait {
                        tracing::warn!(
                            "rate-limit wait capped at 60s (original: {}ns)",
                            wait
                        );
                    }
                    tokio::time::sleep(Duration::from_nanos(capped)).await;
                    // Loop back to re-check the gate.
                }
            }
        }
    }

    /// Reference to the underlying GCRA state.
    pub fn gcra(&self) -> &GcraState {
        &self.gcra
    }

    /// Compute nanoseconds since the GCRA state's start instant.
    fn nanos_since_start(&self) -> u64 {
        Instant::now()
            .duration_since(self.gcra.start_instant())
            .as_nanos() as u64
    }
}

/// Compile-time assertion that DomainQueue is Send + Sync.
const _: () = {
    const fn assert_send<T: Send>() {}
    const fn assert_sync<T: Sync>() {}
    assert_send::<DomainQueue>();
    assert_sync::<DomainQueue>();
};

#[cfg(test)]
#[path = "../tests/unit/domain_queue_test.rs"]
mod tests;
