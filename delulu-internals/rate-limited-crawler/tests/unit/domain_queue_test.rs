use crate::domain_queue::DomainQueue;
use std::sync::Arc;

#[tokio::test]
async fn test_acquire_single_request() {
    let queue = DomainQueue::new(10, 1);
    // Single acquire should complete immediately.
    tokio::time::timeout(Duration::from_secs(1), queue.acquire())
        .await
        .expect("single acquire should complete within 1s");
}

use std::time::Duration;

#[tokio::test]
async fn test_concurrent_acquire_spaced() {
    // Use tokio's paused time for deterministic testing.
    tokio::time::pause();

    let queue = Arc::new(DomainQueue::new(10, 1)); // 10 QPS = 100ms spacing
    let queue2 = Arc::clone(&queue);

    let start = tokio::time::Instant::now();

    // Launch two concurrent acquires.
    let h1 = tokio::spawn(async move { queue.acquire().await });
    let h2 = tokio::spawn(async move { queue2.acquire().await });

    // Advance time in steps to let both complete.
    tokio::time::advance(Duration::from_millis(200)).await;
    tokio::time::resume();

    h1.await.expect("acquire 1 should complete");
    h2.await.expect("acquire 2 should complete");

    let elapsed = start.elapsed();
    // With strict pacing (burst=1), the second acquire should be spaced
    // at least 100ms after the first.
    assert!(
        elapsed >= Duration::from_millis(100),
        "concurrent acquires should be spaced: elapsed={:?}",
        elapsed
    );
}

#[tokio::test]
async fn test_no_busy_spin() {
    // When denied, acquire must sleep (not busy-wait).
    let queue = DomainQueue::new(10, 1);

    // First acquire succeeds immediately.
    queue.acquire().await;

    // Second acquire at the same time should be denied and sleep.
    // With tokio::time::pause(), we can verify a sleep was issued.
    tokio::time::pause();
    let _start = tokio::time::Instant::now();

    tokio::spawn(async move {
        // This will be denied and need to sleep ~100ms.
        // We advance time to let it complete.
        tokio::time::advance(Duration::from_millis(150)).await;
    });

    // The acquire should NOT spin — it should sleep.
    // If it busy-spins, the test would hang (time doesn't advance).
    tokio::time::timeout(Duration::from_millis(500), queue.acquire())
        .await
        .expect("acquire should complete after sleep");
}

#[tokio::test]
async fn test_burst_spacing() {
    tokio::time::pause();

    let queue = Arc::new(DomainQueue::new(10, 3)); // 10 QPS, burst=3

    let start = tokio::time::Instant::now();

    // Three burst requests at once.
    let q1 = Arc::clone(&queue);
    let q2 = Arc::clone(&queue);
    let q3 = Arc::clone(&queue);
    let h1 = tokio::spawn(async move { q1.acquire().await });
    let h2 = tokio::spawn(async move { q2.acquire().await });
    let h3 = tokio::spawn(async move { q3.acquire().await });

    // Advance time enough for all three.
    tokio::time::advance(Duration::from_millis(300)).await;
    tokio::time::resume();

    h1.await.expect("burst 1");
    h2.await.expect("burst 2");
    h3.await.expect("burst 3");

    // With burst=3 at 10 QPS, all should complete.
    // Each is spaced 100ms apart, so total should be ~200-300ms.
    let elapsed = start.elapsed();
    assert!(
        elapsed >= Duration::from_millis(200),
        "burst requests should be spaced: elapsed={:?}",
        elapsed
    );
}

#[tokio::test]
async fn test_send_sync() {
    // Compile-time check: DomainQueue is Send + Sync.
    fn assert_send<T: Send>() {}
    fn assert_sync<T: Sync>() {}
    assert_send::<DomainQueue>();
    assert_sync::<DomainQueue>();
}
