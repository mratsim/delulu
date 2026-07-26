//!  Delulu Web Search
//!
//!  Copyright (C) 2026  Mamy Ratsimbazafy
//!
//!  This program is free software: you can redistribute it and/or modify
//!  it under the terms of the GNU Affero General Public License as published by
//!  the Free Software Foundation, either version 3 of the License, or
//!  (at your option) any later version.
//!
//!  This program is distributed in the hope that it will be useful,
//!  but WITHOUT ANY WARRANTY; without even the implied warranty of
//!  MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
//!  GNU Affero General Public License for more details.
//!
//!  You should have received a copy of the GNU Affero General Public License
//!  along with this program.  If not, see <http://www.gnu.org/licenses/>.

//! Thread-safe session cache for pagination state.
//!
//! Stores pagination state keyed by `SessionKey`, with TTL-based lazy
//! eviction, capacity limits enforced via a `BinaryHeap` for O(log n)
//! eviction, and continuation updates.
//!
//! # Pure Domain Type
//! `SessionCache` is a **pure domain type** (inner hexagon). It does NOT
//! call `Utc::now()`, `Instant::now()`, or `getrandom` internally.
//! All time values and random IDs are **injected by the caller** (CLI
//! or MCP boundary), making the cache fully deterministic and testable.
//!
//! # Eviction Strategy
//! A `BinaryHeap<Reverse<(Instant, SessionKey)>>` (min-heap) keeps the
//! oldest-expiring entry at the top for O(log n) eviction. Expired
//! entries are eagerly removed via `evict_expired()` at the start of
//! every write operation — no stale entries accumulate.
//!
//! # Precondition
//! - `capacity` MUST be > 0 (otherwise every store self-evicts).
//!
//! # Postcondition
//! - All public methods are thread-safe (use `std::sync::RwLock` internally).
//! - `SessionEntry` is NOT `Serialize` — `Box<dyn Continuation>` is not serializable.
//!
//! # Continuation Storage
//! Continuations are stored internally as `Arc<dyn Continuation>` to allow
//! `get()` to return a cloned `Arc` without ownership issues. The public
//! methods `store()` and `update_continuation()` accept `Box<dyn Continuation>`,
//! which is converted to `Arc` internally. Callers of `get()` receive an
//! `Arc<dyn Continuation>` that shares ownership with the cache entry.
//! - Panics if the internal `RwLock` is poisoned (lock holder panicked).
//!
//! # Eviction Flow
//!
//! ```text
//! store() or update_continuation() called
//!          |
//!          v
//!   evict_expired(now)
//!          |
//!          v
//!   +-----------------+
//!   | Pop min-heap top |<------ while expires_at < now
//!   +--------+--------+
//!            |
//!            v
//!   +----------------------+
//!   | Check map entry      |
//!   | expires_at vs now    |
//!   +--------+-------------+
//!            |
//!       +----+----+
//!       |         |
//!       v         v
//!  map > now  map <= now
//!       |         |
//!       v         v
//!  Re-push    Remove from
//!  to heap    map + heap
//! ```
//!

use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap};
use std::sync::Arc;
use std::sync::RwLock;
use std::time::{Duration, Instant};

use crate::engine::{Continuation, EngineId, SearchParams};
use crate::error::WebsearchError;
use crate::SessionKey;

/// A single session entry in the cache.
///
/// This struct does NOT derive `Serialize` because `Box<dyn Continuation>`
/// is not serializable. Serialization happens at the MCP boundary via
/// per-concrete-type serde derives.
///
/// The `continuation` field uses `Arc<dyn Continuation>` so that `get()` can
/// return a cloned `Arc` sharing ownership with the cached entry.
pub struct SessionEntry {
    /// The engine used for this search session.
    pub engine: EngineId,
    /// The search query.
    pub query: String,
    /// The search parameters.
    pub params: SearchParams,
    /// The continuation token for pagination.
    pub continuation: Option<Arc<dyn Continuation>>,
    /// The time at which this entry expires and should be evicted.
    pub(crate) expires_at: Instant,
}

/// Thread-safe session cache for pagination state.
///
/// Entries are keyed directly by `SessionKey` (which hashes by its 8-byte
/// random ID). Expired entries are eagerly evicted at the start of `store()`
/// and `update_continuation()` via `evict_expired()`, and also checked
/// lazily on `get()`.
///
/// Capacity enforcement uses a `BinaryHeap` (min-heap) of `(Instant, SessionKey)`
/// keyed by expiry time, providing O(log n) eviction cost.
///
/// # Thread Safety
/// - Uses `std::sync::RwLock` for interior mutability.
/// - All public methods take `&self` (not `&mut self`).
/// - Lock poisoning: if a lock is poisoned, the panic propagates.
pub struct SessionCache {
    entries: RwLock<HashMap<SessionKey, SessionEntry>>,
    /// Min-heap of `(expires_at, SessionKey)` for O(log n) eviction.
    /// Always consistent with `entries` -- every key in the heap has a
    /// corresponding entry in the map.
    expiry_heap: RwLock<BinaryHeap<Reverse<(Instant, SessionKey)>>>,
    capacity: usize,
    ttl: Duration,
}

impl SessionCache {
    /// Create a new session cache with the given capacity and TTL.
    ///
    /// # Panics
    /// - Panics if `capacity` is 0.
    pub fn new(capacity: usize, ttl: Duration) -> Self {
        assert!(capacity > 0, "SessionCache capacity must be > 0");
        Self {
            entries: RwLock::new(HashMap::new()),
            expiry_heap: RwLock::new(BinaryHeap::new()),
            capacity,
            ttl,
        }
    }

    pub fn evict_expired(&self, now: Instant) {
        let mut entries = self.entries.write().expect("Session cache lock poisoned");
        let mut heap = self.expiry_heap.write().expect("Session cache lock poisoned");

        // Pop expired entries from the min-heap.
        // Min-heap guarantee: if the top is not expired, nothing below is.
        while let Some(Reverse((expires_at, _))) = heap.peek() {
            if *expires_at >= now {
                break;
            }
            let Reverse((_, key)) = heap.pop().unwrap();
            match entries.get(&key) {
                Some(entry) if entry.expires_at > now => {
                    // Entry was refreshed and is still valid.
                    // Re-push with correct expiry so it remains evictable.
                    heap.push(Reverse((entry.expires_at, key)));
                }
                _ => {
                    // Entry genuinely expired (or was already removed).
                    entries.remove(&key);
                }
            }
        }
    }

    /// Store a new session entry in the cache.
    ///
    /// Takes `now` and `random_id` as parameters (pure function — no IO).
    /// The caller (CLI or MCP boundary) is responsible for providing the
    /// current time and cryptographically random bytes.
    ///
    /// Sets `expires_at = now + ttl`.
    ///
    /// Calls `evict_expired(now)` at the start to remove any zombie entries.
    ///
    /// If the cache is at capacity, evicts the entry with the earliest
    /// `expires_at` before inserting (O(log n) via min-heap).
    ///
    /// Returns the generated `SessionKey`.
    pub fn store(
        &self,
        engine: EngineId,
        query: &str,
        params: SearchParams,
        continuation: Option<Box<dyn Continuation>>,
        now: Instant,
        random_id: [u8; 8],
    ) -> SessionKey {
        // Eagerly evict expired entries before doing any work
        self.evict_expired(now);

        let key = SessionKey::new(engine, random_id);
        let expires_at = now + self.ttl;

        let mut entries = self.entries.write().expect("Session cache lock poisoned");
        let mut heap = self.expiry_heap.write().expect("Session cache lock poisoned");

        // Evict oldest-expiring entry if at capacity
        if entries.len() >= self.capacity {
            if let Some(Reverse((_, oldest_key))) = heap.pop() {
                entries.remove(&oldest_key);
            }
        }

        heap.push(Reverse((expires_at, key.clone())));
        entries.insert(
            key.clone(),
            SessionEntry {
                engine,
                query: query.to_string(),
                params,
                continuation: continuation.map(Arc::from),
                expires_at,
            },
        );

        key
    }

    /// Retrieve a session entry by key.
    ///
    /// Returns `None` if the key is not found or the entry has expired
    /// (lazy eviction — expired entries are NOT removed here to avoid
    /// upgrading the read lock to a write lock. They are cleaned up on
    /// the next `store()` or `update_continuation()` via `evict_expired()`).
    ///
    /// The returned entry contains a cloned `Arc<dyn Continuation>` that
    /// shares ownership with the cached entry. The continuation can be
    /// downcast via `as_any()` on the `Arc`.
    pub fn get(&self, key: &SessionKey, now: Instant) -> Option<SessionEntry> {
        let entries = self.entries.read().expect("Session cache lock poisoned");
        let entry = entries.get(key)?;

        if entry.expires_at < now {
            return None;
        }

        Some(SessionEntry {
            engine: entry.engine,
            query: entry.query.clone(),
            params: entry.params.clone(),
            continuation: entry.continuation.clone(),
            expires_at: entry.expires_at,
        })
    }

    /// Update the continuation for an existing session.
    ///
    /// Calls `evict_expired(now)` at the start to remove any zombie entries.
    ///
    /// Refreshes `expires_at` to `now + ttl`.
    ///
    /// Returns `Err(WebsearchError::SessionNotFound)` if the key doesn't
    /// exist or the entry has expired.
    pub fn update_continuation(
        &self,
        key: &SessionKey,
        continuation: Option<Box<dyn Continuation>>,
        now: Instant,
    ) -> Result<(), WebsearchError> {
        // Eagerly evict expired entries before doing any work
        self.evict_expired(now);

        let mut entries = self.entries.write().expect("Session cache lock poisoned");

        let entry = entries.get_mut(key).ok_or(WebsearchError::SessionNotFound)?;

        entry.continuation = continuation.map(Arc::from);
        entry.expires_at = now + self.ttl;

        // No heap push needed -- the original heap entry from store() is
        // sufficient. evict_expired will re-push with the correct expiry
        // when it pops the stale entry and finds the map entry still valid.

        Ok(())
    }
}

#[cfg(test)]
#[path = "../tests/unit/session_cache_test.rs"]
mod session_cache_test;
