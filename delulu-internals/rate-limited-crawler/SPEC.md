# rate-limited-crawler

A per-domain rate-limited HTTP client with GCRA gating, exponential retry,
and LRU eviction of idle domain queues.

## Location

`delulu/delulu-internals/rate-limited-crawler/`

## Dependencies

- `wreq` — hardcoded HTTP client (universally used across delulu)
- `wreq-util` — browser emulation profiles (Safari18_5, etc.)
- `quick_cache` — per-domain queue cache with S3-FIFO eviction
- `tokio` — async runtime (already in every consumer)
- `url` — domain extraction
- `rand` — jitter for retry backoff

No trait, no adapter, no pluggable backends. One HTTP client, one way.

---

## GCRA Gate (inline, zero additional deps)

The rate-limiting primitive. Lock-free via `AtomicU64`. Never returns
"allowed" or "denied" — always enqueues until ready.

```rust
struct GcraState {
    /// Theoretical arrival time in ns since epoch.
    /// AtomicU64 is sufficient: 2^64 ns ≈ 584 years.
    tat: AtomicU64,
    /// Minimum spacing between requests: 1_000_000_000 / qps
    t: u64,
    /// Burst tolerance: t * burst
    tau: u64,
}

impl GcraState {
    /// Try to consume a token.
    ///   Ok(tat)  — token consumed, TAT advanced to `tat`
    ///   Err(tat) — must wait until `tat` (state NOT modified)
    fn try_consume(&self, now: u64) -> Result<u64, u64>;
}
```

Key property: on **denial** the state is **not modified**. Concurrent waiters
all see the same TAT. The first to retry after `tat` wins and advances it.

---

## Retry Builder

Fluent API on the response, not on the client:

```rust
// No retry (default)
client.get("https://export.arxiv.org/api/query?search_query=all:electron").await?;

// With exponential retry
client.get("https://...")
    .with_exponential_retry(2)   // base delay in seconds
    .with_retry_limit(5)         // max retries (default: 3)
    .await?;
```

Retry triggers on:
- HTTP 429 (rate limited)
- HTTP 5xx (server errors)
- Connection/timeout errors

Backoff: `base * 2^retry` + random jitter up to 50% of the delay.

If `with_exponential_retry` is not called, no retry occurs (single attempt).

---

## Per-Domain Queue Cache

```rust
struct DomainQueue {
    gcra: GcraState,
}

impl DomainQueue {
    /// Wait until a token is available, then return.
    async fn acquire(&self) {
        loop {
            let now = nanos_now();
            match self.gcra.try_consume(now) {
                Ok(tat) => {
                    let wait = tat.saturating_sub(now);
                    if wait > 0 {
                        tokio::time::sleep(Duration::from_nanos(wait)).await;
                    }
                    return;
                }
                Err(tat) => {
                    let wait = tat.saturating_sub(now);
                    tokio::time::sleep(Duration::from_nanos(wait)).await;
                }
            }
        }
    }
}
```

No `AsyncSemaphore` — GCRA naturally serializes requests through the gate.
At 10 QPS, requests are spaced 100ms apart. At 50 QPS, 20ms apart.
Concurrent callers to the same domain form an implicit queue.

---

## RateLimitedCrawler

```rust
pub struct RateLimitedCrawler {
    client: wreq::Client,
    domains: quick_cache::sync::Cache<String, Arc<DomainQueue>>,
    qps: u64,
    burst: u64,
}

impl RateLimitedCrawler {
    pub fn builder() -> CrawlerBuilder { ... }

    /// Start building a GET request.
    pub fn get(&self, url: impl Into<String>) -> GetBuilder<'_>;
}
```

---

## Builder

Wraps wreq's `ClientBuilder` with sensible defaults. The builder exposes
the wreq settings that travel-search and webfetch both configure,
plus the rate-limiting parameters:

```rust
let crawler = RateLimitedCrawler::builder()
    // wreq client settings (from travel-search & webfetch patterns)
    .with_emulation(Emulation::Safari18_5)   // wreq_util; default
    .with_redirect(Policy::limited(5))         // wreq; default
    .with_timeout(Duration::from_secs(30))     // wreq; default
    .with_connect_timeout(Duration::from_secs(30)) // wreq; default
    // rate-limiting settings
    .with_qps(10)                              // default
    .with_burst(1)                             // default (no burst)
    .with_max_domains(128)                     // quick_cache capacity; default
    .build();

// Usage:
let resp = crawler.get("https://export.arxiv.org/api/query?search_query=all:electron")
    .with_exponential_retry(2)
    .with_retry_limit(5)
    .await?;
```

Builder defaults:
| Parameter | Default | Description |
|-----------|---------|-------------|
| `emulation` | `Emulation::Safari18_5` | Browser fingerprint (wreq_util) |
| `redirect` | `Policy::limited(5)` | Follow up to 5 redirects |
| `timeout` | 30s | Per-request timeout |
| `connect_timeout` | 30s | Connection timeout |
| `qps` | 10 | Requests per second per domain |
| `burst` | 1 | Burst capacity (1 = no burst) |
| `max_domains` | 128 | LRU cache capacity |
---

## What about retry on success / idempotency?

Retry is only on errors. Success (2xx) returns immediately.
Callers are responsible for idempotency — the crate assumes GET is idempotent.

---

## Module structure

```
src/
  lib.rs          — RateLimitedCrawler, CrawlerBuilder, GetBuilder
  gcra.rs         — GcraState (lock-free AtomicU64)
  domain_queue.rs — DomainQueue (gcra + acquire)
  error.rs        — CrawlerError
```

## Summary of what was removed from QueryQueue

| QueryQueue feature | Status in crawler |
|---|---|
| Token bucket + `Mutex<Instant>` | ❌ Replaced by GCRA `AtomicU64` |
| `AsyncSemaphore` | ❌ Removed (GCRA gates implicitly) |
| 100ms polling loop | ❌ Removed (exact `sleep`) |
| Per-domain HashMap (caller-managed) | ❌ Replaced by `quick_cache` |
| Retry with backoff + jitter | ✅ Kept, but moved to `GetBuilder` |
| `with_retry()` closure API | ❌ Replaced by `.with_exponential_retry()` |
