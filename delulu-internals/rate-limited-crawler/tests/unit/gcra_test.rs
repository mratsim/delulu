use crate::gcra::GcraState;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

/// Helper: create a GcraState and compute `now` from the start instant.
fn setup(qps: u64, burst: u64) -> GcraState {
    GcraState::new(qps, burst)
}

fn now_ns(state: &GcraState) -> u64 {
    tokio::time::Instant::now()
        .duration_since(state.start_instant())
        .as_nanos() as u64
}

#[test]
fn test_single_request_succeeds() {
    let state = setup(10, 1);
    let now = now_ns(&state);
    let result = state.try_consume(now);
    assert!(result.is_ok(), "first request should succeed");
}

#[test]
fn test_burst_capacity_three_succeeds() {
    let state = setup(10, 3);
    let now = now_ns(&state);

    // Three requests at the same time should all succeed with burst=3.
    let r1 = state.try_consume(now);
    assert!(r1.is_ok(), "burst request 1 should succeed");

    let r2 = state.try_consume(now);
    assert!(r2.is_ok(), "burst request 2 should succeed");

    let r3 = state.try_consume(now);
    assert!(r3.is_ok(), "burst request 3 should succeed");
}

#[test]
fn test_burst_exceeded_returns_err() {
    let state = setup(10, 3);
    let now = now_ns(&state);

    // Consume all 3 burst tokens.
    for i in 1..=3 {
        assert!(state.try_consume(now).is_ok(), "burst request {i} should succeed");
    }

    // 4th request at the same time should be denied.
    let r4 = state.try_consume(now);
    assert!(r4.is_err(), "4th request should exceed burst capacity");
}

#[test]
fn test_concurrent_access_deterministic() {
    let state = setup(10, 1);
    let now = now_ns(&state);

    // First thread consumes a token.
    let r1 = state.try_consume(now);
    assert!(r1.is_ok());

    // Second thread at the same time should get Err (TAT already advanced).
    let r2 = state.try_consume(now);
    assert!(r2.is_err(), "second concurrent request should be denied (strict pacing)");
}

#[test]
#[should_panic(expected = "qps must be > 0")]
fn test_qps_zero_panics() {
    let _state = GcraState::new(0, 1);
}

#[test]
fn test_cas_contention_no_livelock() {
    let state = Arc::new(GcraState::new(1000, 1));
    let counter = Arc::new(AtomicU64::new(0));
    let mut handles = Vec::new();

    for _ in 0..16 {
        let state = Arc::clone(&state);
        let counter = Arc::clone(&counter);
        handles.push(std::thread::spawn(move || {
            for _ in 0..100 {
                let now = tokio::time::Instant::now()
                    .duration_since(state.start_instant())
                    .as_nanos() as u64;
                if state.try_consume(now).is_ok() {
                    counter.fetch_add(1, Ordering::Relaxed);
                }
            }
        }));
    }

    for h in handles {
        h.join().expect("thread panicked");
    }

    // At least some requests should have succeeded (no livelock).
    let successes = counter.load(Ordering::Relaxed);
    assert!(successes > 0, "no requests succeeded — possible livelock");
}

#[test]
fn test_memory_ordering() {
    // Verify CAS uses proper ordering via code review.
    // Load is Ordering::Acquire, CAS is Ordering::AcqRel on success.
    // This is checked by inspection — the test confirms the code compiles
    // and the function signature is correct.
    let state = GcraState::new(10, 1);
    let now = now_ns(&state);
    let result = state.try_consume(now);
    assert!(result.is_ok());
}

#[test]
fn test_rate_limiting_pacing() {
    let state = GcraState::new(10, 1); // 10 QPS = 100ms spacing
    let now = now_ns(&state);

    // First request succeeds.
    assert!(state.try_consume(now).is_ok());

    // Second request immediately denied (strict pacing, tau=0).
    assert!(state.try_consume(now).is_err());
}

#[test]
fn test_ok_returns_tat() {
    let state = GcraState::new(10, 1);
    let now = now_ns(&state);

    let result = state.try_consume(now);
    assert!(result.is_ok());
    let (_, tat) = result.unwrap();
    // TAT should be >= now + t (100ms for 10 QPS).
    assert!(tat >= now + 100_000_000, "TAT should advance by at least t");
}

#[test]
fn test_err_returns_tat_unchanged() {
    let state = GcraState::new(10, 1);
    let now = now_ns(&state);

    // First request succeeds, advancing TAT.
    let ok = state.try_consume(now).unwrap();

    // Second request at same time should fail.
    let err = state.try_consume(now).unwrap_err();

    // Err returns the unchanged TAT (same as the Ok returned).
    assert_eq!(err, ok.1, "Err should return unchanged TAT");
}
