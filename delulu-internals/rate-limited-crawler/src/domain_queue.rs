//! Per-domain queue that gates requests through a GCRA state.
//!
//! No `AsyncSemaphore` — GCRA naturally serializes requests through the gate.
//! At 10 QPS, requests are spaced 100ms apart. At 50 QPS, 20ms apart.
//! Concurrent callers to the same domain form an implicit queue.
//!
//! # Token leak edge case
//!
//! `acquire()` calls `try_consume()` which advances the TAT atomically,
//! then sleeps for inter-request spacing. If the async task is cancelled
//! during that sleep, the TAT has already been advanced but no request was
//! sent — the token is permanently lost.
//!
//! **Impact:** The next request sees a TAT that is `t` ns too far in the
//! future, adding an extra `t` of wait time. Under sustained load with
//! frequent cancellations, effective QPS drifts below the configured rate.
//! The system self-heals during idle periods.
//!
//! **Fix:** `GcraTokenGuard` (RAII) restores the old TAT on drop if not
//! committed. See gcra.rs for the flow diagram.

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

/// RAII guard that restores the GCRA state if dropped without committing.
///
/// When `acquire()` consumes a token and then sleeps for inter-request
/// spacing, this guard holds the old TAT. If the task is cancelled mid-sleep,
/// the guard's `Drop` runs and CAS-es the TAT back to `old_tat`, preventing
/// the token from being permanently lost. See the module docs for impact.
struct GcraTokenGuard<'a> {
    gcra: &'a GcraState,
    old_tat: u64,
    new_tat: u64,
    committed: bool,
}

impl Drop for GcraTokenGuard<'_> {
    fn drop(&mut self) {
        if !self.committed {
            let _ = self.gcra.try_restore(self.new_tat, self.old_tat);
        }
    }
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
    ///
    /// If the calling task is cancelled while waiting for inter-request spacing,
    /// the GCRA state is restored via RAII guard to avoid leaking rate-limit tokens.
    pub async fn acquire(&self) {
        loop {
            let now = self.nanos_since_start();
            match self.gcra.try_consume(now) {
                Ok((old_tat, new_tat)) => {
                    // Token consumed. Use RAII guard to restore on cancellation.
                    let mut guard = GcraTokenGuard {
                        gcra: &self.gcra,
                        old_tat,
                        new_tat,
                        committed: false,
                    };
                    let wait = new_tat.saturating_sub(now);
                    if wait > 0 {
                        tokio::time::sleep(Duration::from_nanos(wait)).await;
                    }
                    guard.committed = true;
                    return;
                }
                Err(tat) => {
                    // Denied. Wait until tat (capped at 60s).
                    // No token was consumed, so no restore needed.
                    let wait = tat.saturating_sub(now);
                    let capped = wait.min(MAX_SLEEP_NS);
                    if capped != wait {
                        tracing::warn!(
                            "rate-limit wait capped at 60s (original: {}ns)",
                            wait
                        );
                    }
                    tokio::time::sleep(Duration::from_nanos(capped)).await;
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
