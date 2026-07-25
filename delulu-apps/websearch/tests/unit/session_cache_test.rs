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

//! Unit tests for the session cache module.
//!
//! All tests use `Instant::now()` as the base time — the cache is a pure
//! domain type that receives time and randomness from the caller.

use std::any::Any;
use std::time::{Duration, Instant};

use crate::{
    Continuation, EngineId, SearchParams, SessionCache, SessionKey, WebsearchError,
};

/// Fixed 8 random bytes for deterministic tests.
fn fixed_id() -> [u8; 8] {
    [0xAB, 0xCD, 0xEF, 0x01, 0x23, 0x45, 0x67, 0x89]
}

/// Alternate 8 random bytes for deterministic tests.
fn alt_id() -> [u8; 8] {
    [0x12, 0x34, 0x56, 0x78, 0x9A, 0xBC, 0xDE, 0xF0]
}

/// A minimal continuation for testing.
#[derive(Debug)]
struct TestContinuation;

impl Continuation for TestContinuation {
    fn as_any(&self) -> &dyn Any {
        self
    }
}

#[test]
fn session_cache_store_and_get() {
    let cache = SessionCache::new(10, Duration::from_secs(300));
    let now = Instant::now();
    let params = SearchParams {
        page: Some(2),
        ..Default::default()
    };

    let key = cache.store(
        EngineId::Brave,
        "test query",
        params.clone(),
        None,
        now,
        fixed_id(),
    );

    let entry = cache.get(&key, now).expect("should find stored entry");
    assert_eq!(entry.engine, EngineId::Brave);
    assert_eq!(entry.query, "test query");
    assert_eq!(entry.params.page, Some(2));
    assert!(entry.continuation.is_none());
}

#[test]
fn session_cache_store_and_get_with_continuation() {
    let cache = SessionCache::new(10, Duration::from_secs(300));
    let now = Instant::now();
    let params = SearchParams::default();

    let cont: Option<Box<dyn Continuation>> = Some(Box::new(TestContinuation));
    let key = cache.store(
        EngineId::Brave,
        "cont test",
        params.clone(),
        cont,
        now,
        fixed_id(),
    );

    let entry = cache.get(&key, now).expect("should find stored entry");
    assert_eq!(entry.engine, EngineId::Brave);
    assert_eq!(entry.query, "cont test");
    // Continuation should be retrievable (via Arc clone)
    assert!(entry.continuation.is_some(), "continuation should be retrievable from get()");
}

#[test]
fn session_cache_get_expired() {
    // Use 0-second TTL so entries expire at `now + 0 = now`
    let cache = SessionCache::new(10, Duration::from_secs(0));
    let now = Instant::now();

    let key = cache.store(
        EngineId::DuckDuckGo,
        "expired query",
        SearchParams::default(),
        None,
        now,
        fixed_id(),
    );

    // Check with `now + 1ns` so that expires_at < now
    let later = now + Duration::from_nanos(1);
    let entry = cache.get(&key, later);
    assert!(entry.is_none(), "expired entry should return None");
}

#[test]
fn session_cache_update_continuation() {
    let cache = SessionCache::new(10, Duration::from_secs(300));
    let now = Instant::now();

    let key = cache.store(
        EngineId::Brave,
        "update test",
        SearchParams::default(),
        None,
        now,
        fixed_id(),
    );

    let cont: Option<Box<dyn Continuation>> =
        Some(Box::new(TestContinuation));

    let result = cache.update_continuation(&key, cont, now);
    assert!(result.is_ok(), "update should succeed");

    // Verify the entry still exists and continuation is retrievable
    let entry = cache.get(&key, now);
    assert!(entry.is_some(), "entry should still exist after update");
    let entry = entry.unwrap();
    assert_eq!(entry.engine, EngineId::Brave);
    assert!(entry.continuation.is_some(), "continuation should be retrievable after update");
}

#[test]
fn session_cache_update_nonexistent() {
    let cache = SessionCache::new(10, Duration::from_secs(300));
    let now = Instant::now();

    // Create a key that was never stored
    let fake_key = SessionKey::new(
        EngineId::Brave,
        alt_id(),
    );

    let result = cache.update_continuation(
        &fake_key,
        Some(Box::new(TestContinuation)),
        now,
    );

    match result {
        Err(WebsearchError::SessionNotFound) => {} // expected
        other => panic!("expected SessionNotFound, got {:?}", other),
    }
}

#[test]
fn session_cache_capacity_eviction() {
    // Create cache with capacity 1
    let cache = SessionCache::new(1, Duration::from_secs(300));
    let now = Instant::now();

    let key1 = cache.store(
        EngineId::Brave,
        "first query",
        SearchParams::default(),
        None,
        now,
        fixed_id(),
    );

    let key2 = cache.store(
        EngineId::DuckDuckGo,
        "second query",
        SearchParams::default(),
        None,
        now,
        alt_id(),
    );

    // First entry should be evicted (capacity 1)
    assert!(cache.get(&key1, now).is_none(), "first entry should be evicted");
    // Second entry should still be present
    assert!(cache.get(&key2, now).is_some(), "second entry should be present");
}

#[test]
fn session_cache_thread_safety() {
    let cache = std::sync::Arc::new(SessionCache::new(10, Duration::from_secs(300)));
    let cache2 = cache.clone();
    let cache3 = cache.clone();
    let now = Instant::now();

    let handle1 = std::thread::spawn(move || {
        let key = cache2.store(
            EngineId::Brave,
            "thread1 query",
            SearchParams::default(),
            None,
            now,
            fixed_id(),
        );
        key
    });

    let handle2 = std::thread::spawn(move || {
        let key = cache3.store(
            EngineId::DuckDuckGo,
            "thread2 query",
            SearchParams::default(),
            None,
            now,
            alt_id(),
        );
        key
    });

    let key1 = handle1.join().expect("thread 1 panicked");
    let key2 = handle2.join().expect("thread 2 panicked");

    // Both entries should be retrievable
    assert!(cache.get(&key1, now).is_some(), "thread1's entry should exist");
    assert!(cache.get(&key2, now).is_some(), "thread2's entry should exist");
}

#[test]
fn session_cache_defaults() {
    let cache = SessionCache::new(10_000, Duration::from_secs(30 * 60));
    let now = Instant::now();

    // Store 10,001 entries — capacity is 10,000, so the oldest should be evicted
    let mut first_key = None;
    for i in 0..10_001 {
        // Use a unique ID per entry so each key is different
        let id = [
            (i >> 0) as u8,
            (i >> 8) as u8,
            (i >> 16) as u8,
            (i >> 24) as u8,
            0x00, 0x00, 0x00, 0x00,
        ];
        let key = cache.store(
            EngineId::Brave,
            &format!("query {}", i),
            SearchParams::default(),
            None,
            now,
            id,
        );
        if i == 0 {
            first_key = Some(key);
        }
    }

    // The first entry should be evicted
    assert!(
        cache.get(&first_key.unwrap(), now).is_none(),
        "first entry should be evicted when exceeding default capacity"
    );
}

#[test]
fn session_cache_store_deterministic() {
    // Same inputs produce the same SessionKey.
    let cache = SessionCache::new(10, Duration::from_secs(300));
    let now = Instant::now();

    let key1 = cache.store(
        EngineId::Brave,
        "deterministic",
        SearchParams::default(),
        None,
        now,
        fixed_id(),
    );

    let key2 = cache.store(
        EngineId::Brave,
        "deterministic",
        SearchParams::default(),
        None,
        now,
        fixed_id(),
    );

    assert_eq!(key1, key2, "same inputs should produce same key");
}

#[test]
fn session_cache_store_different_ids_different_keys() {
    // Different random IDs produce different keys.
    let cache = SessionCache::new(10, Duration::from_secs(300));
    let now = Instant::now();

    let key1 = cache.store(
        EngineId::Brave,
        "test",
        SearchParams::default(),
        None,
        now,
        fixed_id(),
    );

    let key2 = cache.store(
        EngineId::Brave,
        "test",
        SearchParams::default(),
        None,
        now,
        alt_id(),
    );

    assert_ne!(key1, key2, "different IDs should produce different keys");
}

// --- evict_expired tests ---

#[test]
fn evict_expired_removes_expired() {
    let cache = SessionCache::new(10, Duration::from_secs(60));
    let now = Instant::now();

    let key = cache.store(
        EngineId::Brave,
        "test",
        SearchParams::default(),
        None,
        now,
        fixed_id(),
    );

    // Entry should exist before expiry
    assert!(cache.get(&key, now).is_some(), "entry should exist before expiry");

    // Advance time past TTL
    let later = now + Duration::from_secs(61);
    cache.evict_expired(later);

    // Entry should be gone
    assert!(cache.get(&key, later).is_none(), "entry should be removed after expiry");
}

#[test]
fn evict_expired_keeps_valid() {
    let cache = SessionCache::new(10, Duration::from_secs(60));
    let now = Instant::now();

    let key = cache.store(
        EngineId::Brave,
        "test",
        SearchParams::default(),
        None,
        now,
        fixed_id(),
    );

    // Entry should survive eviction before expiry
    cache.evict_expired(now);
    assert!(cache.get(&key, now).is_some(), "entry should survive eviction before expiry");
}

#[test]
fn evict_expired_removes_only_expired() {
    let cache = SessionCache::new(10, Duration::from_secs(60));
    let now = Instant::now();

    let key1 = cache.store(
        EngineId::Brave,
        "q1",
        SearchParams::default(),
        None,
        now,
        fixed_id(),
    );
    let key2 = cache.store(
        EngineId::DuckDuckGo,
        "q2",
        SearchParams::default(),
        None,
        now,
        alt_id(),
    );

    // Both valid at `now`
    assert!(cache.get(&key1, now).is_some());
    assert!(cache.get(&key2, now).is_some());

    // Evict at `now` — neither should be removed (before expiry)
    cache.evict_expired(now);
    assert!(cache.get(&key1, now).is_some(), "key1 should survive before expiry");
    assert!(cache.get(&key2, now).is_some(), "key2 should survive before expiry");

    // Evict after both expire
    let later = now + Duration::from_secs(61);
    cache.evict_expired(later);
    assert!(cache.get(&key1, later).is_none(), "key1 should be removed after expiry");
    assert!(cache.get(&key2, later).is_none(), "key2 should be removed after expiry");
}

#[test]
fn evict_expired_called_on_store() {
    let cache = SessionCache::new(10, Duration::from_secs(60));
    let now = Instant::now();

    let key1 = cache.store(
        EngineId::Brave,
        "q1",
        SearchParams::default(),
        None,
        now,
        fixed_id(),
    );
    assert!(cache.get(&key1, now).is_some());

    // Advance time past TTL, store another entry (triggers evict_expired internally)
    let later = now + Duration::from_secs(61);
    let key2 = cache.store(
        EngineId::DuckDuckGo,
        "q2",
        SearchParams::default(),
        None,
        later,
        alt_id(),
    );

    // key1 should have been evicted by evict_expired called from store()
    assert!(cache.get(&key1, later).is_none(), "entry should be evicted by store()");
    // key2 should still be present
    assert!(cache.get(&key2, later).is_some(), "new entry should exist");
}

#[test]
fn evict_expired_called_on_update_continuation() {
    let cache = SessionCache::new(10, Duration::from_secs(60));
    let now = Instant::now();

    let key = cache.store(
        EngineId::Brave,
        "test",
        SearchParams::default(),
        None,
        now,
        fixed_id(),
    );
    assert!(cache.get(&key, now).is_some());

    // Advance time past TTL, update_continuation (triggers evict_expired internally)
    let later = now + Duration::from_secs(61);
    let result = cache.update_continuation(&key, None, later);

    // evict_expired should remove the expired entry, so update should fail
    assert!(result.is_err(), "expired entry should return SessionNotFound");
}

#[test]
fn evict_expired_after_update_continuation_keeps_refreshed_entry() {
    /// After update_continuation refreshes expires_at, evict_expired with
    /// now between old and new expiry must keep the entry alive.
    /// The stale heap entry (old expiry) is popped but the map entry survives.
    let cache = SessionCache::new(10, Duration::from_secs(60));
    let now = Instant::now();

    let key = cache.store(
        EngineId::Brave,
        "refresh test",
        SearchParams::default(),
        None,
        now,
        fixed_id(),
    );
    assert!(cache.get(&key, now).is_some());

    // Refresh the entry at now + 10s, extending expiry to now + 70s
    let refresh_time = now + Duration::from_secs(10);
    let result = cache.update_continuation(&key, None, refresh_time);
    assert!(result.is_ok(), "update should succeed");

    // evict_expired at now + 30s — stale heap entry says old expiry
    // (now + 60s), but map entry says new expiry (now + 70s)
    let evict_time = now + Duration::from_secs(30);
    cache.evict_expired(evict_time);

    // Entry should still be alive (refreshed expiry > evict_time)
    let entry = cache.get(&key, evict_time);
    assert!(entry.is_some(), "refreshed entry should survive evict_expired");
    assert_eq!(entry.unwrap().engine, EngineId::Brave);
}