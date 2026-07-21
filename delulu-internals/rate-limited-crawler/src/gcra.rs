//! GCRA (Generic Cell Rate Algorithm) rate-limiting gate.
//!
//! Lock-free via `AtomicU64`. Never returns "allowed" or "denied" —
//! always computes the earliest time a request can proceed.
//!
//! # Algorithm
//!
//! Each call to `try_consume(now)`:
//! 1. Reads the Theoretical Arrival Time (TAT) — an `AtomicU64` in ns.
//! 2. Computes `earliest = tat.saturating_sub(tau)` — the earliest time
//!    a request can be admitted within the burst tolerance.
//! 3. If `now >= earliest`, the request is admitted:
//!    - CAS the TAT from old value to `max(now, tat) + t`.
//!    - Returns `Ok(new_tat)`.
//! 4. Otherwise returns `Err(tat)` — the caller must wait.

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
    /// - `Ok(tat)` — token consumed, TAT advanced to `tat` (absolute ns).
    ///   The caller should wait until `tat` before sending the request
    ///   (if `tat > now`), then send immediately.
    /// - `Err(tat)` — must wait until `tat` to retry. State NOT modified.
    pub fn try_consume(&self, now: u64) -> Result<u64, u64> {
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
                    Ok(_) => return Ok(new_tat),
                    Err(_) => {
                        // CAS failed — another thread updated TAT. Retry.
                        iterations += 1;
                        if iterations >= 128 {
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
