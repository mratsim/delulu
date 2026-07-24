//! GCRA (Generic Cell Rate Algorithm) rate-limiting gate.
//!
//! Lock-free via `AtomicU64`. Never returns "allowed" or "denied" —
//! always computes the earliest time a request can proceed.
//!
//! # Algorithm
//!
//! ```text
//!  try_consume(now):
//!     Load TAT ──► earliest = TAT - tau
//!                     │
//!              ┌──────┴──────┐
//!              │ now <       │
//!              │ earliest?   │
//!              └──────┬──────┘
//!            No       │       Yes
//!         ┌──────────┘       └──────────┐
//!         ▼                             ▼
//!   Err(TAT)                     new_tat = max(now, TAT) + t
//!   (not consumed)                     │
//!                               CAS TAT -> new_tat
//!                               ┌──────┴──────┐
//!                               │ CAS ok?     │
//!                               └──────┬──────┘
//!                             No       │       Yes
//!                        ┌─────────────┘       └──────────┐
//!                        ▼                                ▼
//!                  Retry CAS loop               Ok((old_tat, new_tat))
//!                  (spin-loop after 3 failures)
//! ```
//!
//! # Edge case: token leak on task cancellation
//!
//! The `Ok` path advances the TAT atomically BEFORE the caller
//! sleeps for inter-request spacing. If the async task is cancelled
//! during that sleep (JoinHandle::abort(), runtime shutdown, timeout drop),
//! the TAT has already been advanced but no request was sent.
//! The slot is permanently burned — the token is "leaked".
//!
//! **Impact:** The next legitimate request sees a TAT that is `t`
//! nanoseconds further in the future than it should be, adding an
//! extra `t` of wait time. Under sustained load with frequent
//! cancellations, the effective QPS drifts below the configured rate.
//! The system self-heals during idle periods (max(now, tat) resets
//! the TAT to `now` when the domain goes idle).
//!
//! **Mitigation:** DomainQueue::acquire() in domain_queue.rs uses
//! GcraTokenGuard, an RAII guard that restores the old TAT via
//! try_restore() if the guard is dropped without being committed.
//! Restoration is best-effort (CAS may fail if another thread advanced
//! the TAT in the meantime).

use std::sync::atomic::{AtomicU64, Ordering};
use tokio::time::Instant;

/// GCRA state for per-domain rate limiting.
///
/// Uses a single `AtomicU64` to store the theoretical arrival time (TAT)
/// in nanoseconds relative to `start`. `2^64 ns ≈ 584 years`, so `u64` is
/// sufficient.
pub struct GcraState {
    /// Theoretical arrival time in ns relative to `start`.
    tat: AtomicU64,
    /// Minimum spacing between requests: `1_000_000_000 / qps`
    t: u64,
    /// Burst tolerance: `(burst - 1) * t`. With `tau = 0` (burst=1),
    /// requests are strictly spaced by `t`. With `tau = 2*t` (burst=3),
    /// up to 3 requests can arrive back-to-back.
    tau: u64,
    /// Monotonic clock reference point recorded at construction.
    start: Instant,
}

impl GcraState {
    /// Create a new GCRA state.
    ///
    /// # Parameters
    /// - `qps`: requests per second. Must be > 0.
    /// - `burst`: burst capacity. 1 means no burst (strict pacing).
    ///
    /// # Panics
    /// Panics if `qps == 0`.
    pub fn new(qps: u64, burst: u64) -> Self {
        assert!(qps > 0, "GcraState::new: qps must be > 0");
        let t = 1_000_000_000 / qps;
        // tau = (burst - 1) * t gives exactly `burst` requests back-to-back.
        // For burst=1: tau=0, strict pacing.
        let tau = t.saturating_mul(burst.saturating_sub(1));
        Self {
            tat: AtomicU64::new(0),
            t,
            tau,
            start: Instant::now(),
        }
    }

    /// Try to consume a token at the given time `now`.
    ///
    /// `now` is nanoseconds since `self.start`, computed as:
    /// `Instant::now().duration_since(self.start).as_nanos() as u64`
    ///
    /// # Returns
    /// - `Ok((old_tat, new_tat))` — token consumed, TAT advanced from `old_tat` to `new_tat`.
    ///   The caller should wait until `new_tat` before sending the request
    ///   (if `new_tat > now`), then send immediately.
    ///   `old_tat` is provided so the caller can restore the state on cancellation.
    /// - `Err(tat)` — must wait until `tat` to retry. State NOT modified.
    pub fn try_consume(&self, now: u64) -> Result<(u64, u64), u64> {
        let mut iterations = 0u32;
        loop {
            let tat = self.tat.load(Ordering::Acquire);
            let earliest = tat.saturating_sub(self.tau);

            if now >= earliest {
                let new_tat = now.max(tat) + self.t;
                match self.tat.compare_exchange_weak(
                    tat,
                    new_tat,
                    Ordering::AcqRel,
                    Ordering::Acquire,
                ) {
                    Ok(_) => return Ok((tat, new_tat)),
                    Err(_) => {
                        // CAS failed — another thread updated TAT. Retry.
                        iterations += 1;
                        if iterations >= 3 {
                            // Hint the CPU to let other threads make progress.
                            std::hint::spin_loop();
                            iterations = 0;
                        }
                        continue;
                    }
                }
            } else {
                return Err(tat);
            }
        }
    }

    /// Attempt to restore a previous TAT value.
    ///
    /// Used to undo a `try_consume` when the caller is cancelled before
    /// sending the request. Only succeeds if no other thread has advanced
    /// the TAT past `expected_current`.
    #[allow(clippy::result_unit_err)]
    pub fn try_restore(&self, expected_current: u64, target: u64) -> Result<(), ()> {
        self.tat
            .compare_exchange(
                expected_current,
                target,
                Ordering::AcqRel,
                Ordering::Acquire,
            )
            .map(|_| ())
            .map_err(|_| ())
    }

    /// The minimum spacing between requests in nanoseconds.
    pub fn interval_ns(&self) -> u64 {
        self.t
    }

    /// The burst tolerance in nanoseconds.
    pub fn burst_tolerance_ns(&self) -> u64 {
        self.tau
    }

    /// The monotonic clock reference point.
    pub fn start_instant(&self) -> Instant {
        self.start
    }
}

#[cfg(test)]
#[path = "../tests/unit/gcra_test.rs"]
mod tests;
