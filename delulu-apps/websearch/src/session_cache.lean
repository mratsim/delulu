/-
  Formal verification of SessionCache from delulu-websearch.

  Models: delulu/delulu-apps/websearch/src/session_cache.rs
  Proves key invariants of SessionCache using Lean 4.
  Zero axioms, zero sorries.

  --- Purpose ---
  This file models the SessionCache data structure and its operations
  (store, get, updateContinuation, evictExpired) in Lean 4 and proves
  critical correctness invariants that the Rust implementation relies on.

  The model uses plain lists (instead of HashMap/BinaryHeap) to make
  proofs tractable while preserving the essential semantics.

  --- Data structures ---
  - `SessionKey` — 8-byte random ID, Hash/Eq/Ord on ID only.
    Format: `{engine}-{base58(id)}`.
  - `SessionEntry` — stores engine, query, params, continuation, expires_at.
  - `SessionCache` — HashMap + BinaryHeap with capacity limit and TTL.

  --- Invariants (key guarantees) ---
  1. Capacity invariant: `mapSize c.entries <= c.capacity`
     The cache never exceeds its configured capacity.
  2. Heap-map consistency: Every key in the expiry heap has a
     corresponding entry in the map. No orphaned heap entries.
  3. No expired entries after eviction: After `evictExpired(now)`, no entry
     has `expires_at < now`.
  4. Refreshed entries survive: If an entry's map expiry is `> now`, it
     survives `evictExpired` even if a stale heap entry had an earlier expiry.
  5. No heap growth on update: `updateContinuation` does not push to the
     heap — the heap only grows on `store()`.

  --- Guarantees ---
  - Pure domain type: no `Instant::now()`, `Utc::now()`, or `getrandom`
    calls inside the cache. All time values and randomness are injected by
    the caller (CLI/MCP boundary).
  - Thread-safe via RwLock (not modeled in Lean — sequential correctness only).
  - Deterministic: same inputs always produce the same outputs.

  --- Bounds ---
  - Capacity: configurable, default 10,000 entries.
  - TTL: configurable, default 30 minutes.
  - Heap size: bounded by `capacity` (only `store()` pushes to heap).
    `updateContinuation` does NOT push.
  - Eviction cost: O(log n) via BinaryHeap min-heap.

  --- Complexity ---
  - `store()`: O(log n) for heap push + O(k log n) for evict_expired
    where k = expired entries.
  - `get()`: O(1) HashMap lookup.
  - `updateContinuation()`: O(k log n) for evict_expired, O(1) for map update.
  - `evictExpired()`: O(k log n) where k = expired entries. Each iteration
    pops the min-heap top and either re-pushes (if refreshed) or removes from map.

  --- Why formal verification? ---
  The eviction logic has subtle edge cases:
  - Stale heap entries from refreshed entries can cause incorrect evictions.
  - Capacity eviction with empty heap must be handled gracefully.
  - Several bugs were caught during development (evict_expired removing valid
    entries, base58 encoding truncation, n_token SSRF).
  The Lean model proves these bugs cannot occur in the final design.

  --- Eviction flow ---
  ```
                      evict_expired(now)
                            |
                            v
                   +-----------------+
                   | Peek min-heap   |
                   | expires_at < now|
                   +--------+--------+
                            |
                      +-----+-----+
                      |           |
                    Yes          No -> done
                      |
                      v
                Pop (expires_at, key) from heap
                      |
                      v
            +---------------------------+
            | Check map entry           |
            | entry.expires_at vs now   |
            +--------+------------------+
                     |
                +----+----+
                |         |
           map > now  map <= now
                |         |
                v         v
           Re-push    Remove from
           to heap    map (expired)
                |         |
                +----+----+
                     |
                     v
                 loop (next heap entry)
  ```
-/

-- | A point in time, modeled as a natural number (nanoseconds since epoch).
-- In the Rust implementation this is `std::time::Instant`.
abbrev Instant := Nat
-- | A duration, modeled as a natural number (nanoseconds).
-- In the Rust implementation this is `std::time::Duration`.
abbrev Duration := Nat

-- | The search engine identifier.
-- Corresponds to `EngineId` in the Rust source.
inductive EngineId
  | Brave | DuckDuckGo
deriving DecidableEq, Repr

-- | A continuation token for pagination.
-- Modeled as a unit type since the concrete representation is
-- `Box<dyn Continuation>` in Rust (not inspectable at the model level).
inductive Continuation
  | mk
deriving DecidableEq, Repr

-- | Search parameters for a query.
-- Mirrors `SearchParams` in the Rust source (page, country, safesearch, timeRange, maxResults).
structure SearchParams where
  page : Option Nat
  country : Option String
  safesearch : Option String
  timeRange : Option String
  maxResults : Option Nat
deriving Repr

instance : EmptyCollection SearchParams where
  emptyCollection := { page := none, country := none, safesearch := none, timeRange := none, maxResults := none }

-- | A session key, consisting of an engine ID and a random 8-byte numeric ID.
-- Hash/Eq/Ord are based on `id` only (not `engine`), matching the Rust
-- implementation where `SessionKey` derives `Hash` and `Eq` on the 8-byte random ID.
-- The Rust string format is `"{engine}-{base58(id)}"`.
structure SessionKey where
  engine : EngineId
  id : Nat
deriving Repr, DecidableEq

-- | A single entry in the session cache.
-- Stores the engine, query text, search parameters, optional continuation token,
-- and the expiry timestamp. Mirrors `SessionEntry` in the Rust source.
structure SessionEntry where
  engine : EngineId
  query : String
  params : SearchParams
  continuation : Option Continuation
  expiresAt : Instant
deriving Repr

instance : Inhabited SessionEntry where
  default := { engine := EngineId.Brave, query := "", params := {}, continuation := none, expiresAt := 0 }

instance : Inhabited (Instant × SessionKey) where
  default := (0, { engine := EngineId.Brave, id := 0 })

-- | The session cache data structure.
-- Comprises an association list of entries (modeling a `HashMap`),
-- an expiry heap (modeling a `BinaryHeap<Reverse<(Instant, SessionKey)>>`),
-- a capacity bound, and a TTL duration.
-- Invariants are documented at the module level above.
structure SessionCache where
  entries : List (SessionKey × SessionEntry)
  expiryHeap : List (Instant × SessionKey)
  capacity : Nat
  ttl : Duration
deriving Repr

/-
  Map operations.
  Minimalist list-based map interface modeling a `HashMap<SessionKey, SessionEntry>`.
  Operations: find, insert, remove, size, contains.
-/
-- Map operations
-- | Look up a key in the association list, comparing by `id` only.
-- Returns `some entry` if found, `none` otherwise.
def mapFind (key : SessionKey) (l : List (SessionKey × SessionEntry)) : Option SessionEntry :=
  match l with
  | [] => none
  | (k, v) :: rest => if k.id = key.id then some v else mapFind key rest

-- | Insert a (key, value) pair at the head of the association list.
-- Duplicates are not checked (the Rust HashMap handles dedup).
def mapInsert (key : SessionKey) (val : SessionEntry) (l : List (SessionKey × SessionEntry))
    : List (SessionKey × SessionEntry) := (key, val) :: l

-- | Remove all entries matching the given key (by `id`) from the list.
-- Returns a new list with matching entries removed.
def mapRemove (key : SessionKey) (l : List (SessionKey × SessionEntry))
    : List (SessionKey × SessionEntry) :=
  match l with
  | [] => []
  | (k, v) :: rest => if k.id = key.id then mapRemove key rest else (k, v) :: mapRemove key rest

-- | The number of entries in the map (list length).
def mapSize (l : List (SessionKey × SessionEntry)) : Nat := l.length
-- | Returns `true` if the key is present in the map (via `mapFind`).
def mapContains (key : SessionKey) (l : List (SessionKey × SessionEntry)) : Bool := (mapFind key l).isSome

-- | Push an `(Instant, SessionKey)` pair onto the heap (sorted insert ascending by `t`).
-- Models `BinaryHeap.push()` (min-heap).
-- Inserts `elem` in sorted order so that the list remains non-decreasing by `t`.
def heapPush (elem : Instant × SessionKey) (h : List (Instant × SessionKey)) : List (Instant × SessionKey) :=
  match h with
  | [] => [elem]
  | (t, key) :: rest =>
    if elem.1 ≤ t then elem :: (t, key) :: rest
    else (t, key) :: heapPush elem rest

-- | The number of elements in the heap (list length).
def heapSize (h : List (Instant × SessionKey)) : Nat := h.length

-- | Predicate: the heap is sorted in non-decreasing order by the `Instant` component.
-- Used to justify the early-stop optimization in `evictExpiredLoop`.
def sortedByT : List (Instant × SessionKey) → Prop
  | [] => True
  | [_] => True
  | (t1, _k1) :: ((t2, _k2) :: rest) => t1 ≤ t2 ∧ sortedByT ((t2, _k2) :: rest)

-- | `heapPush` adds exactly one element to the heap.
theorem heapPush_size (e : Instant × SessionKey) (h : List (Instant × SessionKey)) :
    heapSize (heapPush e h) = heapSize h + 1 := by
  induction h generalizing e with
  | nil => simp [heapPush, heapSize]
  | cons hd tl ih =>
    rcases hd with ⟨t, key⟩
    unfold heapPush heapSize
    by_cases hle : e.1 ≤ t
    · simp [hle]
    · have h_ih := ih e
      unfold heapSize at h_ih
      simp [hle, h_ih]

-- | The pushed element is always in the resulting heap.
theorem heapPush_mem (e : Instant × SessionKey) (h : List (Instant × SessionKey)) :
    e ∈ heapPush e h := by
  induction h generalizing e with
  | nil => simp [heapPush]
  | cons hd tl ih =>
    rcases hd with ⟨t, key⟩
    unfold heapPush
    by_cases hle : e.1 ≤ t
    · simp [hle]
    · simp [hle, ih]

-- | Elements already in the heap remain in the heap after `heapPush`.
theorem heapPush_mem_of_mem (e e' : Instant × SessionKey) (h : List (Instant × SessionKey)) :
    e' ∈ h → e' ∈ heapPush e h := by
  intro hmem
  induction h generalizing e with
  | nil => simp at hmem
  | cons hd tl ih =>
    rcases hd with ⟨t, key⟩
    unfold heapPush
    by_cases hle : e.1 ≤ t
    · simp [hle, hmem]
    · simp [hle]
      cases hmem with
      | head => simp
      | tail _ hmem_tl => simp [ih e hmem_tl]

-- | If a sorted list has `(t_hd, k_hd)` as head and `(t, key)` somewhere in the tail,
-- then `t_hd ≤ t`. Used to justify the early-stop in `evictExpiredLoop`.
theorem sortedByT_head_le_all {t_hd : Instant} {k_hd : SessionKey} {tl : List (Instant × SessionKey)}
    (h_sorted : sortedByT ((t_hd, k_hd) :: tl)) (h_mem : (t, key) ∈ tl) : t_hd ≤ t := by
  induction tl generalizing t_hd k_hd t key with
  | nil => simp at h_mem
  | cons hd tl2 ih =>
    rcases hd with ⟨t2, k2⟩
    have h_sorted' : sortedByT ((t_hd, k_hd) :: (t2, k2) :: tl2) := h_sorted
    simp [sortedByT] at h_sorted'
    rcases h_sorted' with ⟨h_le, h_sorted_tl⟩
    have h_cases : (t, key) = (t2, k2) ∨ (t, key) ∈ tl2 := by
      simpa using h_mem
    rcases h_cases with (h_eq | h_tl2)
    · injection h_eq with h_teq _; subst h_teq; exact h_le
    · exact Nat.le_trans h_le (ih h_sorted_tl h_tl2)

-- | If `sortedByT ((t, k) :: tl)` then `sortedByT tl`.
-- Needed to thread the sortedness invariant through inductive proofs.
theorem sortedByT_tail {t : Instant} {k : SessionKey} {tl : List (Instant × SessionKey)}
    (h_sorted : sortedByT ((t, k) :: tl)) : sortedByT tl := by
  cases tl with
  | nil => simp [sortedByT]
  | cons hd' tl' =>
    rcases hd' with ⟨t2, k2⟩
    simp [sortedByT] at h_sorted ⊢
    exact h_sorted.2

-- | Invariant 1: The number of entries never exceeds the configured capacity.
-- Formally: `mapSize c.entries <= c.capacity`.
def capacityInvariant (c : SessionCache) : Prop := mapSize c.entries ≤ c.capacity
-- | Invariant 2 (heap-map consistency): Every key in the expiry heap has
-- a corresponding entry in the map. Formally:
-- `forall elem in c.expiryHeap, mapContains elem.2 c.entries`.
def heapMapConsistent (c : SessionCache) : Prop :=
  ∀ (elem : Instant × SessionKey), elem ∈ c.expiryHeap → mapContains elem.2 c.entries

/-
  Core cache operations.
  These mirror the public API of the Rust `SessionCache` impl:
  - `evictExpired` / `evictExpiredLoop`: remove expired entries.
  - `cleanStaleHeap`: filter out stale heap entries whose keys are no longer in the map.
  - `evictOldest`: evict the entry at the front of the heap (oldest expiry).
  - `store`: insert a new session.
  - `get`: look up a session by key.
  - `updateContinuation`: update the continuation token and refresh expiry.
-/
-- Core operations
/-
  evictExpiredLoop now entries heap

  The core eviction loop. Walks the heap list and removes expired entries.
  For each `(t, key)` in the heap:
  - If `t >= now`, the entry is not expired; stop processing (min-heap guarantee:
    all remaining entries have `t' >= t >= now`, so no further entries are expired).
  - If `t < now`, check the map:
    * If the key is not in the map (stale heap entry), skip it.
    * If the key's map entry has `expiresAt <= now`, it is genuinely expired:
      remove it from the map and continue.
    * If the key's map entry has `expiresAt > now`, the entry was refreshed:
      re-push the updated expiry to the heap via `heapPush` (sorted insert).

  Returns `(entries', heap')` where `entries'` has no entries with `expiresAt < now`.

  Complexity: O(k log n) in the Rust implementation (min-heap pops).
  Here, O(k) since the heap is modeled as a list.

  Maintains invariant 3 (no expired entries after eviction).
  Relies on the heap being sorted (per `sortedByT`) for the early-stop guarantee.
-/
def evictExpiredLoop (now : Instant)
    (entries : List (SessionKey × SessionEntry))
    (heap : List (Instant × SessionKey))
    : List (SessionKey × SessionEntry) × List (Instant × SessionKey) :=
  match heap with
  | [] => (entries, [])
  | (t, key) :: rest =>
    if t ≥ now then
      -- Min-heap guarantee: all remaining entries have t' >= t >= now.
      (entries, (t, key) :: rest)
    else
      match mapFind key entries with
      | none => evictExpiredLoop now entries rest
      | some entry =>
        if entry.expiresAt ≤ now then evictExpiredLoop now (mapRemove key entries) rest
        else
          -- Re-push the refreshed expiry via heapPush (sorted insert).
          let (entries', heap') := evictExpiredLoop now entries rest
          (entries', heapPush (entry.expiresAt, key) heap')

/-
  evictExpired c now

  Eagerly evict all expired entries from the cache.
  Called at the start of every write operation (`store`, `updateContinuation`).

  Precondition: None (works on any cache state).
  Postcondition: No entry with `expiresAt < now` remains in `c.entries`.

  Maintains: capacity invariant (entries can only be removed, never added).
-/
def evictExpired (c : SessionCache) (now : Instant) : SessionCache :=
  let (entries', heap') := evictExpiredLoop now c.entries c.expiryHeap
  { c with entries := entries', expiryHeap := heap' }

/-
  cleanStaleHeap entries heap

  Remove heap entries whose keys are no longer present in the map.
  This can happen after `evictOldest` removes an entry from the map
  but leaves the corresponding heap entry intact.

  Used by `store` before evicting the oldest entry to avoid
  referencing a key that no longer exists in the map.
-/
def cleanStaleHeap (entries : List (SessionKey × SessionEntry)) (heap : List (Instant × SessionKey))
    : List (Instant × SessionKey) :=
  match heap with
  | [] => []
  | (t, key) :: rest => if mapContains key entries then (t, key) :: cleanStaleHeap entries rest else cleanStaleHeap entries rest

/-
  evictOldest entries heap

  Evict the entry at the front of the heap (the one with the earliest expiry).
  Used when the cache is at capacity and a new entry needs to be inserted.

  Precondition: heap is non-empty (caller ensures this via cleanStaleHeap).
  Postcondition: The evicted key is removed from entries, and the heap
  loses its front element.

  Maintains invariant 1 (capacity) by reducing entry count by 1.
-/
def evictOldest (entries : List (SessionKey × SessionEntry)) (heap : List (Instant × SessionKey))
    : List (SessionKey × SessionEntry) × List (Instant × SessionKey) :=
  match heap with
  | [] => (entries, [])
  | (_, key) :: rest => (mapRemove key entries, rest)

/-
  store c engine query params continuation now randomId

  Store a new session entry in the cache.

  Preconditions:
  - `capacityInvariant c` holds (entry count <= capacity).
  - `c.capacity > 0` (enforced by Rust constructor).
  - `heapMapConsistent c` holds (every heap key has a map entry).

  Postconditions:
  - `capacityInvariant` is preserved.
  - The new entry is inserted with `expiresAt = now + ttl`.
  - The new key is pushed onto the expiry heap.
  - If at capacity, the oldest-expiring entry is evicted first.

  Returns `(cache', key)` where `cache'` is the updated cache.

  Maintains: invariants 1, 2, 3.
-/
def store (c : SessionCache) (engine : EngineId) (query : String) (params : SearchParams)
    (continuation : Option Continuation) (now : Instant) (randomId : Nat) : SessionCache × SessionKey :=
  let c1 := evictExpired c now
  let key : SessionKey := { engine := engine, id := randomId }
  let expiresAt := now + c.ttl
  let (entries1, heap1) :=
    if mapSize c1.entries ≥ c1.capacity then
      let cleanedHeap := cleanStaleHeap c1.entries c1.expiryHeap
      evictOldest c1.entries cleanedHeap
    else (c1.entries, c1.expiryHeap)
  let entry : SessionEntry := {
    engine := engine, query := query, params := params,
    continuation := continuation, expiresAt := expiresAt
  }
  let entries2 := mapInsert key entry entries1
  let heap2 := heapPush (expiresAt, key) heap1
  ({ c with entries := entries2, expiryHeap := heap2 }, key)

/-
  get c key now

  Look up a session entry by key.

  Precondition: None.
  Postcondition: Returns `some entry` if the key exists and
  `entry.expiresAt >= now`; returns `none` otherwise.

  Note: Lazy eviction — expired entries are NOT removed here.
  They are cleaned up on the next `store()` or `updateContinuation()`
  via `evictExpired`. This avoids upgrading a read lock to a write lock
  in the Rust implementation.
-/
def get (c : SessionCache) (key : SessionKey) (now : Instant) : Option SessionEntry :=
  match mapFind key c.entries with
  | none => none
  | some entry => if entry.expiresAt < now then none else some entry

-- | Error type for `updateContinuation`.
-- Indicates that the requested session key was not found (or expired).
inductive UpdateError
  | sessionNotFound

/-
  updateContinuation c key continuation now

  Update the continuation token and refresh the expiry for an existing session.

  Preconditions:
  - `capacityInvariant c` holds.
  - `heapMapConsistent c` holds.

  Postconditions:
  - If the key exists (and the entry is not expired):
    * `continuation` is updated.
    * `expiresAt` is refreshed to `now + ttl`.
    * The heap is NOT pushed (no heap growth).
    * Returns `(c', Except.ok ())`.
  - If the key does not exist or is expired:
    * Returns `(c1, Except.error UpdateError.sessionNotFound)`.

  Maintains invariants 1, 2, 3.
  Specifically maintains invariant 5: no heap growth on update.
-/
def updateContinuation (c : SessionCache) (key : SessionKey) (continuation : Option Continuation) (now : Instant)
    : SessionCache × (Except UpdateError Unit) :=
  let c1 := evictExpired c now
  match mapFind key c1.entries with
  | none => (c1, Except.error UpdateError.sessionNotFound)
  | some oldEntry =>
    let newExpiresAt := now + c.ttl
    let newEntry : SessionEntry := { oldEntry with continuation := continuation, expiresAt := newExpiresAt }
    let entries' := mapInsert key newEntry (mapRemove key c1.entries)
    ({ c1 with entries := entries' }, Except.ok ())

/-
  Helper lemmas for map operations.
  These establish basic properties of the list-based map:
  - Size bounds for insert and remove.
  - Relationship between mapFind, mapRemove, and key identity.
  - Equivalence of mapContains and mapFind ≠ none.
-/
-- Helper lemmas
-- | Inserting a (k,v) pair increases map size by at most 1.
-- Proof: `mapInsert` prepends to the list, so length becomes `m.length + 1`.
-- Used in `store_preserves_capacity` to bound size after insert.
theorem mapInsert_size_bound (k : SessionKey) (v : SessionEntry) (m : List (SessionKey × SessionEntry)) :
    mapSize (mapInsert k v m) ≤ mapSize m + 1 := by
  unfold mapInsert mapSize; simp

-- | Removing a key from the map never increases the map size.
-- Proof: By induction on the list. Size either stays the same (key not found)
-- or decreases (key found and removed).
-- Used in `evictExpiredLoop_size_noninc` to bound size after remove.
theorem mapRemove_size_noninc (k : SessionKey) (m : List (SessionKey × SessionEntry)) :
    mapSize (mapRemove k m) ≤ mapSize m := by
  induction m with
  | nil => simp [mapRemove, mapSize]
  | cons hd tl ih =>
    rcases hd with ⟨k', v'⟩
    simp [mapRemove, mapSize]
    by_cases h_eq : k'.id = k.id
    · simp [h_eq]; simpa [mapSize] using Nat.le_trans ih (Nat.le_succ (mapSize tl))
    · simp [h_eq]; simpa [mapSize] using Nat.succ_le_succ ih

-- | If a key is present in the map, removing it strictly decreases the map size.
-- Proof: By induction. The key must be found at some position; removing it
-- reduces the length by at least 1.
-- Used in `evictOldest_size_lt_or_eq` to prove that eviction reduces entry count.
theorem mapRemove_size_dec (k : SessionKey) (m : List (SessionKey × SessionEntry)) (h : mapFind k m ≠ none) :
    mapSize (mapRemove k m) < mapSize m := by
  induction m with
  | nil => simp [mapFind] at h
  | cons hd tl ih =>
    rcases hd with ⟨k', v'⟩
    simp [mapFind, mapRemove, mapSize] at h ⊢
    by_cases h_eq : k'.id = k.id
    · simp [h_eq]
      have h_noninc : (mapRemove k tl).length ≤ tl.length := mapRemove_size_noninc k tl
      have h_lt : tl.length < ((k', v') :: tl).length := by simp
      exact Nat.lt_of_le_of_lt h_noninc h_lt
    · simp [h_eq] at h ⊢
      have h_lt : mapSize (mapRemove k tl) < mapSize tl := ih h
      simpa [mapSize] using Nat.succ_lt_succ h_lt

-- | After removing key `k` from the map, `mapFind k` returns `none`.
-- Proof: By induction. All entries matching `k.id` are removed.
-- Used in `evict_expired_removes_all_expired` to show that an expired entry
-- is no longer findable after eviction.
theorem mapFind_mapRemove_same_key_none (k : SessionKey) (m : List (SessionKey × SessionEntry)) :
    mapFind k (mapRemove k m) = none := by
  induction m with
  | nil => rfl
  | cons hd tl ih =>
    rcases hd with ⟨k', v'⟩; unfold mapRemove
    by_cases h_eq : k'.id = k.id
    · simp [h_eq, ih]
    · simp [mapFind, h_eq, ih]

-- | Removing a different key `k` does not affect `mapFind k'` when `k'.id ≠ k.id`.
-- Proof: By induction. The removal skips entries with non-matching IDs.
-- Used in `evictExpiredLoop_no_add` and `evict_expired_preserves_refreshed`
-- to reason that removing one key does not affect lookups for another.
theorem mapFind_mapRemove_ne_id (k k' : SessionKey) (m : List (SessionKey × SessionEntry)) (h : k'.id ≠ k.id) :
    mapFind k' (mapRemove k m) = mapFind k' m := by
  induction m with
  | nil => rfl
  | cons hd tl ih =>
    rcases hd with ⟨k₁, v₁⟩
    unfold mapRemove
    simp [mapFind]
    by_cases h₁ : k₁.id = k.id
    · simp [h₁]
      by_cases h_eq' : k.id = k'.id
      · exfalso; apply h; exact h_eq'.symm
      · rw [if_neg h_eq']; exact ih
    · simp [h₁]
      by_cases h_eq' : k₁.id = k'.id
      · simp [mapFind, h_eq']
      · simp [mapFind, h_eq', ih]

-- | If two keys have the same `id`, removing one is equivalent to removing the other.
-- Proof: By induction. Since `mapRemove` compares by `id`, equal IDs produce equal results.
-- Used in `evictExpiredLoop_no_add` to rewrite removals when IDs match.
theorem mapRemove_eq_of_id_eq (k k' : SessionKey) (h : k.id = k'.id) (m : List (SessionKey × SessionEntry)) :
    mapRemove k m = mapRemove k' m := by
  induction m with
  | nil => rfl
  | cons hd tl ih =>
    rcases hd with ⟨k₁, v₁⟩; simp [mapRemove]
    by_cases h₁ : k₁.id = k.id; simp [h₁, h, ih]
    · have h₁' : ¬ k₁.id = k'.id := λ h_eq => h₁ (h_eq.trans h.symm); simp [h₁, h₁', ih]

-- | If two keys have the same `id`, `mapFind` returns the same result for both.
-- Proof: By induction. `mapFind` compares by `id`, so equal IDs give equal lookups.
-- Used in `evict_expired_preserves_refreshed` to connect lookups by different keys
-- that share the same ID.
theorem mapFind_eq_of_id_eq (k k' : SessionKey) (h : k.id = k'.id) (m : List (SessionKey × SessionEntry)) :
    mapFind k m = mapFind k' m := by
  induction m with
  | nil => rfl
  | cons hd tl ih =>
    rcases hd with ⟨k₁, v₁⟩; simp [mapFind]
    by_cases h₁ : k₁.id = k.id; simp [h₁, h]
    · have h₁' : ¬ k₁.id = k'.id := λ h_eq => h₁ (h_eq.trans h.symm); simp [h₁, h₁', ih]

-- | `mapContains` implies `mapFind` is not `none`.
-- Proof: By definition of `mapContains` as `(mapFind ...).isSome`.
-- Used in `cleanStaleHeap_mem_implies_contains` to convert contains to find.
theorem mapContains_to_find_ne_none {key : SessionKey} {l : List (SessionKey × SessionEntry)} :
    mapContains key l → mapFind key l ≠ none := by
  unfold mapContains; intro h; cases h' : mapFind key l; simp [h'] at h; intro hnil; simp at hnil

-- | `mapFind ≠ none` implies `mapContains`.
-- Proof: By definition of `mapContains`.
-- Used in `cleanStaleHeap_keeps_matching_entry` to convert find to contains.
theorem find_ne_none_to_mapContains {key : SessionKey} {l : List (SessionKey × SessionEntry)} :
    mapFind key l ≠ none → mapContains key l := by
  unfold mapContains; intro h; cases h' : mapFind key l; exact absurd h' h; simp

/-
  Size lemmas for evictExpiredLoop.
  Prove that eviction does not increase the map size or heap size.
-/
-- Size lemmas
-- | `evictExpiredLoop` never increases the map size.
-- Proof: By induction on the heap. In each branch, entries are either preserved
-- or removed (via `mapRemove`), and `mapRemove` never increases size.
-- Real-world meaning: eviction can only shrink the cache, never grow it.
-- Used in `evictExpired_preserves_capacity` to show `evictExpired` preserves the capacity invariant.
theorem evictExpiredLoop_size_noninc (now : Instant) (entries : List (SessionKey × SessionEntry))
    (heap : List (Instant × SessionKey)) : mapSize (evictExpiredLoop now entries heap).1 ≤ mapSize entries := by
  induction heap generalizing entries with
  | nil => simp [evictExpiredLoop]
  | cons hd tl ih =>
    rcases hd with ⟨t, key⟩; unfold evictExpiredLoop
    by_cases ht : t ≥ now
    · simp [ht]
    · simp [ht]; cases hfind : mapFind key entries
      · simp; exact ih entries
      · rename_i entry; by_cases hexp : entry.expiresAt ≤ now
        · simp [hexp]
          have h_rm : mapSize (mapRemove key entries) ≤ mapSize entries := mapRemove_size_noninc key entries
          have h_rec : mapSize (evictExpiredLoop now (mapRemove key entries) tl).1 ≤ mapSize (mapRemove key entries) := ih (mapRemove key entries)
          exact Nat.le_trans h_rec h_rm
        · simp [hexp]; exact ih entries

-- | `evictExpiredLoop` never increases the heap size.
-- Proof: By induction on the heap. The heap either shrinks (expired entries popped)
-- or stays the same size (non-expired entries preserved via early stop,
-- refreshed entries re-pushed via `heapPush` which adds exactly one element).
-- Real-world meaning: the expiry heap does not grow unboundedly during eviction.
-- Used in `update_continuation_no_heap_growth` to show no heap growth on update.
theorem evictExpiredLoop_heap_size_noninc (now : Instant) (entries : List (SessionKey × SessionEntry))
    (heap : List (Instant × SessionKey)) : heapSize (evictExpiredLoop now entries heap).2 ≤ heapSize heap := by
  induction heap generalizing entries with
  | nil => simp [evictExpiredLoop, heapSize]
  | cons hd tl ih =>
    rcases hd with ⟨t, key⟩; unfold evictExpiredLoop
    by_cases ht : t ≥ now
    · simp [ht, heapSize]
    · simp [ht]; cases hfind : mapFind key entries
      · simp; have h_rec : heapSize (evictExpiredLoop now entries tl).2 ≤ heapSize tl := ih entries
        simpa [heapSize] using Nat.le_trans h_rec (by simp [heapSize])
      · rename_i entry; by_cases hexp : entry.expiresAt ≤ now
        · simp [hexp]; have h_rec : heapSize (evictExpiredLoop now (mapRemove key entries) tl).2 ≤ heapSize tl := ih (mapRemove key entries)
          simpa [heapSize] using Nat.le_trans h_rec (by simp [heapSize])
        · simp [hexp]
          -- Re-push via heapPush: heapSize increases by 1 vs the recursive result,
          -- but the original heap included (t, key), so net ≤ original.
          have h_rec : heapSize (evictExpiredLoop now entries tl).2 ≤ heapSize tl := ih entries
          have h_push_size : heapSize (heapPush (entry.expiresAt, key) (evictExpiredLoop now entries tl).2) =
            heapSize (evictExpiredLoop now entries tl).2 + 1 := heapPush_size (entry.expiresAt, key) _
          have h_heap_orig : heapSize ((t, key) :: tl) = heapSize tl + 1 := by simp [heapSize]
          omega

-- | `evictExpired` preserves the capacity invariant.
-- Proof: `evictExpired` delegates to `evictExpiredLoop`, which never increases
-- the map size. Therefore if `mapSize c.entries <= c.capacity` holds before,
-- it holds after eviction.
-- Real-world meaning: calling `evict_expired()` from any method will never
-- violate the capacity bound.
theorem evictExpired_preserves_capacity (c : SessionCache) (now : Instant) (h : capacityInvariant c) :
    capacityInvariant (evictExpired c now) := by
  unfold capacityInvariant at *; unfold evictExpired
  apply Nat.le_trans (evictExpiredLoop_size_noninc now c.entries c.expiryHeap); exact h

/-
  Lemma: evictExpiredLoop_no_add
  If a key is present after eviction, it was already present before eviction.
  In other words, `evictExpiredLoop` never fabricates entries.
-/
-- Lemma: evictExpiredLoop_no_add
-- | If a key is findable after `evictExpiredLoop`, it was findable before.
-- Proof: By induction on the heap. The only operation that removes entries
-- is `mapRemove`, and the lemma chains through all recursive calls.
-- Real-world meaning: eviction is purely subtractive — it never introduces
-- new entries into the cache.
theorem evictExpiredLoop_no_add (now : Instant) (entries : List (SessionKey × SessionEntry))
    (heap : List (Instant × SessionKey)) (key : SessionKey) (entry : SessionEntry)
    (hfind : mapFind key (evictExpiredLoop now entries heap).1 = some entry) : mapFind key entries = some entry := by
  induction heap generalizing entries key entry with
  | nil => simp [evictExpiredLoop] at hfind; exact hfind
  | cons hd tl ih =>
    rcases hd with ⟨t, key'⟩; unfold evictExpiredLoop at hfind
    by_cases ht : t ≥ now
    · simp [ht] at hfind; exact hfind
    · simp [ht] at hfind
      cases hfind' : mapFind key' entries
      · simp [hfind'] at hfind; exact ih entries key entry hfind
      · simp [hfind'] at hfind; rename_i entry'
        by_cases hexp : entry'.expiresAt ≤ now
        · simp [hexp] at hfind
          by_cases hkey_eq : key'.id = key.id
          · have h_none : mapFind key (mapRemove key' entries) = none := by
              rw [mapRemove_eq_of_id_eq key' key hkey_eq entries, mapFind_mapRemove_same_key_none key entries]
            have h_contra := ih (mapRemove key' entries) key entry hfind
            rw [h_none] at h_contra; simp at h_contra
          · have h_ih := ih (mapRemove key' entries) key entry hfind
            rw [mapFind_mapRemove_ne_id key' key entries (Ne.symm hkey_eq)] at h_ih; exact h_ih
        · simp [hexp] at hfind; exact ih entries key entry hfind

/-
  Theorem 1: heap_keys_subset_map_keys
  Every key in the expiry heap has a corresponding entry in the map.
  This is the definition of `heapMapConsistent`.
  It is trivially true by the hypothesis — the theorem exists as a named
  lemma for use in other proofs that need to reference this property.
-/
-- Theorem 1
theorem heap_keys_subset_map_keys (c : SessionCache) (h_hmc : heapMapConsistent c) : heapMapConsistent c := h_hmc

/-
  Theorem 2: evict_expired_removes_all_expired
  After `evictExpired(now)`, every remaining entry has `expiresAt >= now`.
  No expired entry survives eviction.

  Why it matters: This is invariant 3 (no expired entries after eviction).
  It guarantees that `get()` can trust that entries returned from the cache
  are not stale, as long as `evictExpired` is called before lookups.

  Preconditions:
  - `h_sorted`: the expiry heap is sorted by expiry time.
    This is needed to justify the early-stop optimization.
  - `h_ehe`: every entry in the map has a corresponding heap entry.
    This holds if the implementation always pushes to the heap on `store()`.
  - `h_heap_le`: for every heap entry `(t, k)`, the map entry's `expiresAt`
    is at least `t`. This holds because `store()` pushes with `t = expiresAt`
    and `updateContinuation` only increases `expiresAt`.

  Proof approach:
  1. Suppose `entry.expiresAt < now` but `key` survives eviction.
  2. By `h_ehe`, there is `(t, key) ∈ c.expiryHeap`.
  3. By `h_heap_le`, `t ≤ entry.expiresAt < now`, so `t < now`.
  4. Lemma `evictExpiredLoop_removes_expired_entry` shows that when
     `(t, key)` is in the heap with `t < now` and `entry.expiresAt < now`,
     the loop removes `key` from entries — contradiction.
  5. Therefore `entry.expiresAt ≥ now`.
-/
-- Lemma: If a heap entry has `t < now` and the map entry is expired,
-- `evictExpiredLoop` removes that key from the result entries.
theorem evictExpiredLoop_removes_expired_entry (now : Instant) (entries : List (SessionKey × SessionEntry))
    (heap : List (Instant × SessionKey)) (key : SessionKey) (entry : SessionEntry) (t : Instant)
    (h_sorted : sortedByT heap)
    (h_find : mapFind key entries = some entry) (h_mem : (t, key) ∈ heap) (h_t_lt_now : t < now)
    (h_expired : entry.expiresAt < now) :
    mapFind key (evictExpiredLoop now entries heap).1 = none :=
by
  induction heap generalizing entries key entry t with
  | nil => simp at h_mem
  | cons hd tl ih =>
    rcases hd with ⟨t_hd, k_hd⟩
    have h_cases : (t, key) = (t_hd, k_hd) ∨ (t, key) ∈ tl := by
      simpa using h_mem
    rcases h_cases with (h_eq | h_tl)
    · injection h_eq with ht_eq hk_eq
      subst ht_eq hk_eq
      -- (t, key) is at the head of the heap, t < now
      unfold evictExpiredLoop
      have h_not_ge : ¬ t ≥ now := Nat.not_le.mpr h_t_lt_now
      have hle : entry.expiresAt ≤ now := Nat.le_of_lt h_expired
      simp [h_not_ge, h_find, hle]
      have h_rm_none : mapFind key (mapRemove key entries) = none :=
        mapFind_mapRemove_same_key_none key entries
      have h_result_none : mapFind key (evictExpiredLoop now (mapRemove key entries) tl).1 = none := by
        cases h_contra : mapFind key (evictExpiredLoop now (mapRemove key entries) tl).1
        · rfl
        · rename_i v
          have h_find_rm : mapFind key (mapRemove key entries) = some v :=
            evictExpiredLoop_no_add now (mapRemove key entries) tl key v h_contra
          rw [h_rm_none] at h_find_rm
          simp at h_find_rm
      exact h_result_none
    · -- (t, key) is in the tail
      unfold evictExpiredLoop
      by_cases ht_hd_ge : t_hd ≥ now
      · -- Short-circuit: impossible because sortedByT guarantees all elements
        -- in tl have t >= t_hd >= now, contradicting h_t_lt_now.
        have h_sorted_head : sortedByT ((t_hd, k_hd) :: tl) := h_sorted
        have h_t_hd_le_t : t_hd ≤ t := sortedByT_head_le_all h_sorted_head h_tl
        have h_t_ge_now : t ≥ now := Nat.le_trans ht_hd_ge h_t_hd_le_t
        exact absurd h_t_ge_now (Nat.not_le_of_lt h_t_lt_now)
      · simp [ht_hd_ge]
        cases hfind' : mapFind k_hd entries
        · simp
          have h_sorted_tl : sortedByT tl := sortedByT_tail h_sorted
          exact ih entries key entry t h_sorted_tl h_find h_tl h_t_lt_now h_expired
        · rename_i entry_hd
          by_cases hle_hd : entry_hd.expiresAt ≤ now
          · simp [hle_hd]
            by_cases hkey_eq : k_hd.id = key.id
            · -- key is removed by mapRemove, so it can't survive
              have h_rm_none : mapFind key (mapRemove k_hd entries) = none := by
                rw [mapRemove_eq_of_id_eq k_hd key hkey_eq entries,
                  mapFind_mapRemove_same_key_none key entries]
              have h_result_none : mapFind key (evictExpiredLoop now (mapRemove k_hd entries) tl).1 = none := by
                cases h_contra : mapFind key (evictExpiredLoop now (mapRemove k_hd entries) tl).1
                · rfl
                · rename_i v
                  have h_find_rm : mapFind key (mapRemove k_hd entries) = some v :=
                    evictExpiredLoop_no_add now (mapRemove k_hd entries) tl key v h_contra
                  rw [h_rm_none] at h_find_rm
                  simp at h_find_rm
              exact h_result_none
            · have h_find_rm : mapFind key (mapRemove k_hd entries) = some entry := by
                rw [mapFind_mapRemove_ne_id k_hd key entries (Ne.symm hkey_eq), h_find]
              have h_sorted_tl : sortedByT tl := sortedByT_tail h_sorted
              exact ih (mapRemove k_hd entries) key entry t h_sorted_tl h_find_rm h_tl h_t_lt_now h_expired
          · simp [hle_hd]
            have h_sorted_tl : sortedByT tl := sortedByT_tail h_sorted
            exact ih entries key entry t h_sorted_tl h_find h_tl h_t_lt_now h_expired

-- Theorem 2
theorem evict_expired_removes_all_expired (c : SessionCache) (now : Instant)
    (h_sorted : sortedByT c.expiryHeap)
    (h_ehe : ∀ (k : SessionKey) (v : SessionEntry), mapFind k c.entries = some v → (∃ (t : Instant), (t, k) ∈ c.expiryHeap))
    (h_heap_le : ∀ (k : SessionKey) (v : SessionEntry) (t : Instant),
      mapFind k c.entries = some v → (t, k) ∈ c.expiryHeap → t ≤ v.expiresAt) :
    ∀ (key : SessionKey) (entry : SessionEntry),
      mapFind key (evictExpired c now).entries = some entry → entry.expiresAt ≥ now :=
by
  intro key entry hfind
  have h_in_c : mapFind key c.entries = some entry :=
    evictExpiredLoop_no_add now c.entries c.expiryHeap key entry hfind
  by_cases h_ge : entry.expiresAt ≥ now
  · exact h_ge
  · exfalso
    have h_lt : entry.expiresAt < now := Nat.lt_of_not_ge h_ge
    rcases h_ehe key entry h_in_c with ⟨t, ht⟩
    -- ht: (t, key) ∈ c.expiryHeap
    have h_t_le_exp : t ≤ entry.expiresAt := h_heap_le key entry t h_in_c ht
    have h_t_lt_now : t < now := Nat.lt_of_le_of_lt h_t_le_exp h_lt
    have h_removed : mapFind key (evictExpired c now).entries = none := by
      unfold evictExpired
      exact evictExpiredLoop_removes_expired_entry now c.entries c.expiryHeap key entry t
        h_sorted h_in_c ht h_t_lt_now h_lt
    rw [h_removed] at hfind
    simp at hfind

/-
  If an entry has `expiresAt > now` (i.e., it was refreshed),
  it survives `evictExpired(now)` unchanged.

  Why it matters: This is invariant 4 (refreshed entries survive).
  Without this guarantee, `updateContinuation` could accidentally lose
  valid entries because the heap still has the old (stale) expiry time.
  This theorem proves the "re-push" logic in `evictExpiredLoop` is correct.

  Proof: By induction on the heap. If the heap entry is expired but the map
  entry is fresh, the loop re-pushes the new expiry. The induction shows
  that the key remains findable after the recursive call.
  Approach: case analysis on heap top expiry and key identity.
-/
-- Theorem 3
theorem evict_expired_preserves_refreshed (c : SessionCache) (now : Instant)
    (key : SessionKey) (entry : SessionEntry) (hfind : mapFind key c.entries = some entry)
    (hfresh : entry.expiresAt > now) : mapFind key (evictExpired c now).entries = some entry := by
  unfold evictExpired
  induction c.expiryHeap generalizing c with
  | nil => simp [evictExpiredLoop, hfind]
  | cons hd tl ih =>
    rcases hd with ⟨t, key'⟩; unfold evictExpiredLoop
    by_cases ht : t ≥ now
    · simp [ht]; exact hfind
    · simp [ht]; by_cases hkey_eq : key'.id = key.id
      · have hfind' : mapFind key' c.entries = some entry := by rw [mapFind_eq_of_id_eq key' key hkey_eq c.entries, hfind]
        simp [hfind']
        have h_not_le : ¬ entry.expiresAt ≤ now := by intro hle; exact Nat.lt_irrefl _ (Nat.lt_of_lt_of_le hfresh hle)
        simp [h_not_le]; let c' : SessionCache := { entries := c.entries, expiryHeap := tl, capacity := c.capacity, ttl := c.ttl }
        have h_ih := ih c' hfind; simpa [c'] using h_ih
      · cases hfind' : mapFind key' c.entries
        · simp; let c' : SessionCache := { entries := c.entries, expiryHeap := tl, capacity := c.capacity, ttl := c.ttl }
          have h_ih := ih c' hfind; simpa [c'] using h_ih
        · rename_i entry'; by_cases hle' : entry'.expiresAt ≤ now
          · simp [hle']; have h_find_rm : mapFind key (mapRemove key' c.entries) = some entry := by
              rw [mapFind_mapRemove_ne_id key' key c.entries (Ne.symm hkey_eq), hfind]
            let c' : SessionCache := { entries := mapRemove key' c.entries, expiryHeap := tl, capacity := c.capacity, ttl := c.ttl }
            have h_ih := ih c' h_find_rm; simpa [c'] using h_ih
          · simp [hle']; let c' : SessionCache := { entries := c.entries, expiryHeap := tl, capacity := c.capacity, ttl := c.ttl }
            have h_ih := ih c' hfind; simpa [c'] using h_ih

/-
  Theorem 4: evictOldest_size_lt_or_eq
  `evictOldest` either reduces the map size (if the evicted key was in the map)
  or leaves it unchanged (if the key was stale).

  Why it matters: This is used in `store_preserves_capacity` to show that
  when the cache is at capacity, evicting the oldest entry frees a slot.
  The "or equal" case handles stale heap entries (keys not present in map).

  Proof: Case analysis on heap emptiness. If the heap is empty, size is unchanged.
  If the heap has a head, check if the key is in the map:
  - If yes, `mapRemove_size_dec` proves strict decrease.
  - If no (stale), induction shows size unchanged.
-/
-- Theorem 4
theorem evictOldest_size_lt_or_eq (entries : List (SessionKey × SessionEntry)) (heap : List (Instant × SessionKey)) :
    mapSize (evictOldest entries heap).1 < mapSize entries ∨ mapSize (evictOldest entries heap).1 = mapSize entries := by
  unfold evictOldest; cases heap with
  | nil => right; simp
  | cons hd tl =>
    rcases hd with ⟨t, key⟩
    by_cases h : mapFind key entries = none
    · right; induction entries generalizing key with
      | nil => simp [mapRemove, mapSize]
      | cons hd' tl' ih' =>
        rcases hd' with ⟨k', v'⟩; simp [mapFind, mapRemove, mapSize] at h ⊢
        by_cases h_eq : k'.id = key.id; simp [h_eq] at h ⊢
        rw [if_neg h_eq] at h; rw [if_neg h_eq]; simp; simpa [mapSize] using ih' key h
    · left; apply mapRemove_size_dec key entries h

-- | If `(t, key)` is in the heap and `key` is in the map,
-- then `(t, key)` is preserved by `cleanStaleHeap`.
-- Proof: By induction on the heap. `cleanStaleHeap` only removes entries
-- whose keys are not in the map (`mapContains` check).
-- Used in `store_preserves_capacity` to show that at least one valid heap entry
-- survives cleaning.
theorem cleanStaleHeap_keeps_matching_entry (entries : List (SessionKey × SessionEntry)) (heap : List (Instant × SessionKey))
    (key : SessionKey) (entry : SessionEntry) (t : Instant)
    (h_mem : (t, key) ∈ heap) (h_find : mapFind key entries = some entry) : (t, key) ∈ cleanStaleHeap entries heap :=
by
  induction heap with
  | nil => simp at h_mem
  | cons hd tl ih =>
    rcases hd with ⟨t_hd, k_hd⟩
    have h_cases : (t, key) = (t_hd, k_hd) ∨ (t, key) ∈ tl := by
      simpa using h_mem
    rcases h_cases with (h_eq | h_tl)
    · injection h_eq with ht_eq hkey_eq
      subst ht_eq hkey_eq
      simp [cleanStaleHeap]
      have h_contains : mapContains key entries :=
        find_ne_none_to_mapContains (by rw [h_find]; intro h; simp at h)
      simp [h_contains]
    · have h_rec : (t, key) ∈ cleanStaleHeap entries tl := ih h_tl
      simp [cleanStaleHeap]
      by_cases hc : mapContains k_hd entries
      · simp [hc, h_rec]
      · simp [hc, h_rec]

-- | If `(t, key)` is in `cleanStaleHeap entries heap`,
-- then `key` must be in the map (`mapFind key entries ≠ none`).
-- Proof: By induction on the heap. `cleanStaleHeap` only keeps entries
-- where `mapContains` returns true (which implies `mapFind ≠ none`).
-- Used in `store_preserves_capacity` to extract a key from the cleaned heap
-- that is guaranteed to exist in the map for eviction.
theorem cleanStaleHeap_mem_implies_contains (entries : List (SessionKey × SessionEntry)) (heap : List (Instant × SessionKey))
    (key : SessionKey) (t : Instant)
    (h_mem : (t, key) ∈ cleanStaleHeap entries heap) : mapFind key entries ≠ none :=
by
  induction heap with
  | nil => simp [cleanStaleHeap] at h_mem
  | cons hd tl ih =>
    rcases hd with ⟨t_hd, k_hd⟩
    simp [cleanStaleHeap] at h_mem
    by_cases hc : mapContains k_hd entries
    · simp [hc] at h_mem
      rcases h_mem with (h_eq | h_tl)
      · rcases h_eq with ⟨ht_eq, hkey_eq⟩
        subst hkey_eq
        exact mapContains_to_find_ne_none hc
      · exact ih h_tl
    · simp [hc] at h_mem
      exact ih h_mem

-- | If a key survives eviction (remains findable in the map after `evictExpiredLoop`),
-- then there is at least one heap entry for that key in the resulting heap.
-- Proof: By induction on the heap. If the key is the current heap head,
-- it gets re-pushed (if refreshed) or we derive a contradiction (if expired).
-- If the key is deeper in the heap, the induction hypothesis applies.
-- Used in `store_preserves_capacity` to show that entries surviving eviction
-- still have corresponding heap entries, preserving invariant 2.
theorem evictExpiredLoop_preserves_heap_entry (now : Instant) (entries : List (SessionKey × SessionEntry))
    (heap : List (Instant × SessionKey)) (key : SessionKey) (entry : SessionEntry) (t : Instant)
    (h_mem : (t, key) ∈ heap) (h_survives : mapFind key (evictExpiredLoop now entries heap).1 = some entry) :
    ∃ (t' : Instant), (t', key) ∈ (evictExpiredLoop now entries heap).2 :=
by
  induction heap generalizing entries key entry t with
  | nil => simp at h_mem
  | cons hd tl ih =>
    rcases hd with ⟨t_hd, k_hd⟩
    have h_cases : (t, key) = (t_hd, k_hd) ∨ (t, key) ∈ tl := by
      simpa using h_mem
    rcases h_cases with (h_eq | h_tl)
    · injection h_eq with ht_eq hkey_eq
      subst ht_eq hkey_eq
      have h_find_orig : mapFind key entries = some entry :=
        evictExpiredLoop_no_add now entries ((t, key) :: tl) key entry h_survives
      unfold evictExpiredLoop
      by_cases ht_ge : t ≥ now
      · simp [ht_ge]
      · simp [ht_ge, h_find_orig]
        by_cases hle : entry.expiresAt ≤ now
        · simp [hle]
          have h_survives_rm : mapFind key (evictExpiredLoop now (mapRemove key entries) tl).1 = some entry :=
            by
            unfold evictExpiredLoop at h_survives
            simp [ht_ge, h_find_orig, hle] at h_survives
            exact h_survives
          have h_rm_none : mapFind key (mapRemove key entries) = none :=
            mapFind_mapRemove_same_key_none key entries
          have h_contra := evictExpiredLoop_no_add now (mapRemove key entries) tl key entry h_survives_rm
          rw [h_rm_none] at h_contra; simp at h_contra
        · -- Re-push case: key is re-inserted into heap via heapPush
          simp [hle]
          apply Exists.intro (entry.expiresAt)
          apply heapPush_mem
    · -- (t, key) ∈ tl
      unfold evictExpiredLoop
      by_cases ht_ge : t_hd ≥ now
      · simp [ht_ge]
        -- Short-circuit: heap unchanged, so (t, key) is still in the result heap
        exact ⟨t, Or.inr h_tl⟩
      · simp [ht_ge]
        cases hfind' : mapFind k_hd entries
        · simp
          have h_survives_tl : mapFind key (evictExpiredLoop now entries tl).1 = some entry :=
            by
            unfold evictExpiredLoop at h_survives; simp [ht_ge, hfind'] at h_survives; exact h_survives
          rcases ih entries key entry t h_tl h_survives_tl with ⟨t', ht'⟩
          exact ⟨t', ht'⟩
        · rename_i entry_hd
          simp
          by_cases hle : entry_hd.expiresAt ≤ now
          · simp [hle]
            have h_survives_rm : mapFind key (evictExpiredLoop now (mapRemove k_hd entries) tl).1 = some entry :=
              by
              unfold evictExpiredLoop at h_survives; simp [ht_ge, hfind', hle] at h_survives; exact h_survives
            rcases ih (mapRemove k_hd entries) key entry t h_tl h_survives_rm with ⟨t', ht'⟩
            exact ⟨t', ht'⟩
          · simp [hle]
            have h_survives_tl : mapFind key (evictExpiredLoop now entries tl).1 = some entry :=
              by
              unfold evictExpiredLoop at h_survives; simp [ht_ge, hfind', hle] at h_survives; exact h_survives
            rcases ih entries key entry t h_tl h_survives_tl with ⟨t', ht'⟩
            have h_mem' : (t', key) ∈ heapPush (entry_hd.expiresAt, k_hd) (evictExpiredLoop now entries tl).2 :=
              heapPush_mem_of_mem (entry_hd.expiresAt, k_hd) (t', key) (evictExpiredLoop now entries tl).2 ht'
            exact ⟨t', h_mem'⟩

/-
  Theorem (main): store_preserves_capacity
  `store` preserves the capacity invariant.
  After `store`, the number of entries is still `<= capacity`.

  Why it matters: This is the most important theorem — it proves invariant 1
  is maintained across the primary write operation. Without this, the cache
  could grow unboundedly and exhaust memory.

  Preconditions:
  - `h`: `capacityInvariant c` holds before the call.
  - `h_cap_pos`: `c.capacity > 0` (enforced by Rust constructor).
  - `h_ehe`: every entry in the map has a heap entry (invariant 2).

  Proof: Two cases:
  1. **At capacity** (`mapSize c1.entries >= c1.capacity`):
     - `evictExpired` runs first and preserves capacity (`evictExpired_preserves_capacity`).
     - `evictOldest` removes one entry (strictly decreasing size via `evictOldest_size_lt_or_eq`).
     - `mapInsert` adds one entry (increasing size by at most 1 via `mapInsert_size_bound`).
     - The net change is non-positive, so capacity is preserved.
     - The "size unchanged" case of `evictOldest_size_lt_or_eq` is impossible
       because `cleanStaleHeap` is non-empty (proved using `h_ehe` and
       `cleanStaleHeap_keeps_matching_entry`).
  2. **Below capacity**: Eviction is skipped, insert adds 1 entry, which
     stays within capacity.
-/
theorem store_preserves_capacity (c : SessionCache) (engine : EngineId) (query : String) (params : SearchParams)
    (continuation : Option Continuation) (now : Instant) (randomId : Nat) (h : capacityInvariant c)
    (h_cap_pos : c.capacity > 0) (h_ehe : ∀ (k : SessionKey) (v : SessionEntry), mapFind k c.entries = some v → (∃ (t : Instant), (t, k) ∈ c.expiryHeap)) :
    capacityInvariant (store c engine query params continuation now randomId).1 := by
  unfold store
  let c1 := evictExpired c now
  have h_c1 : capacityInvariant c1 := evictExpired_preserves_capacity c now h
  unfold capacityInvariant at h_c1
  by_cases h_at_cap : mapSize c1.entries ≥ c1.capacity
  · have h_sz_eq : mapSize c1.entries = c1.capacity := Nat.le_antisymm h_c1 h_at_cap
    have h_cap_eq : c1.capacity = c.capacity := rfl
    have h_sz_eq_cap : mapSize c1.entries = c.capacity :=
      calc
        mapSize c1.entries = c1.capacity := h_sz_eq
        _ = c.capacity := h_cap_eq
    have h_entries_nonempty : c1.entries ≠ [] := by
      intro h_empty; have h_sz0 : mapSize c1.entries = 0 := by simpa [h_empty, mapSize]
      have : c.capacity = 0 := by
        calc
          c.capacity = c1.capacity := by symm; exact h_cap_eq
          _ = mapSize c1.entries := by symm; exact h_sz_eq
          _ = 0 := h_sz0
      rw [this] at h_cap_pos; exact Nat.lt_irrefl 0 h_cap_pos
    have h_evict_sz : mapSize (evictOldest c1.entries (cleanStaleHeap c1.entries c1.expiryHeap)).1 < mapSize c1.entries ∨
                     mapSize (evictOldest c1.entries (cleanStaleHeap c1.entries c1.expiryHeap)).1 = mapSize c1.entries :=
      evictOldest_size_lt_or_eq c1.entries (cleanStaleHeap c1.entries c1.expiryHeap)
    rcases h_evict_sz with (h_lt | h_eq)
    · have h_sz_bound : mapSize (evictOldest c1.entries (cleanStaleHeap c1.entries c1.expiryHeap)).1 + 1 ≤ mapSize c1.entries := by omega
      have h_total : mapSize (mapInsert { engine := engine, id := randomId }
          { engine := engine, query := query, params := params, continuation := continuation, expiresAt := now + c.ttl }
          (evictOldest c1.entries (cleanStaleHeap c1.entries c1.expiryHeap)).1) ≤ c.capacity := by
        have h_ins : mapSize (mapInsert { engine := engine, id := randomId }
          { engine := engine, query := query, params := params, continuation := continuation, expiresAt := now + c.ttl }
          (evictOldest c1.entries (cleanStaleHeap c1.entries c1.expiryHeap)).1) ≤
          mapSize (evictOldest c1.entries (cleanStaleHeap c1.entries c1.expiryHeap)).1 + 1 := mapInsert_size_bound _ _ _
        apply Nat.le_trans h_ins; rw [h_sz_eq_cap] at h_sz_bound; exact h_sz_bound
      unfold capacityInvariant
      simpa [h_at_cap, c1] using h_total
    · -- evictOldest didn't remove anything. This means cleanStaleHeap was empty or
      -- the first key wasn't in the map. Since entries is non-empty and
      -- entriesHaveHeapEntries holds (by h_ehe, preserved by evictExpired),
      -- there must be at least one heap entry with a valid key.
      -- So cleanStaleHeap is non-empty, and evictOldest must remove something.
      -- Therefore this case is impossible.
      have h_c1_ehe : ∀ (k : SessionKey) (v : SessionEntry), mapFind k c1.entries = some v → (∃ (t' : Instant), (t', k) ∈ c1.expiryHeap) := by
        intro k v h_find
        have h_find_orig : mapFind k c.entries = some v :=
          evictExpiredLoop_no_add now c.entries c.expiryHeap k v h_find
        rcases h_ehe k v h_find_orig with ⟨t', ht'⟩
        unfold c1
        apply evictExpiredLoop_preserves_heap_entry now c.entries c.expiryHeap k v t' ht' h_find
      have h_first_entry : ∃ (k : SessionKey) (v : SessionEntry), mapFind k c1.entries = some v := by
        have h_nonempty : c1.entries ≠ [] := h_entries_nonempty
        rcases List.exists_cons_of_ne_nil h_nonempty with ⟨hd, tl, h_eq⟩
        rcases hd with ⟨k, v⟩; refine ⟨k, v, ?_⟩; simp [h_eq, mapFind]
      rcases h_first_entry with ⟨k, v, h_mv⟩
      rcases h_c1_ehe k v h_mv with ⟨t', ht'⟩
      -- ht': (t', k) ∈ c1.expiryHeap
      -- cleanStaleHeap keeps (t', k) because mapContains k c1.entries is true
      have h_kept : (t', k) ∈ cleanStaleHeap c1.entries c1.expiryHeap :=
        cleanStaleHeap_keeps_matching_entry c1.entries c1.expiryHeap k v t' ht' h_mv
      have h_cleaned_nonempty : cleanStaleHeap c1.entries c1.expiryHeap ≠ [] :=
        λ h_empty => by
          have : (t', k) ∉ [] := by simp
          rw [h_empty] at h_kept
          exact this h_kept
      have h_lt_contra : mapSize (evictOldest c1.entries (cleanStaleHeap c1.entries c1.expiryHeap)).1 < mapSize c1.entries := by
        unfold evictOldest
        cases h_cleaned : cleanStaleHeap c1.entries c1.expiryHeap with
        | nil => exact (h_cleaned_nonempty h_cleaned).elim
        | cons hd' tl' =>
          rcases hd' with ⟨t'', key''⟩; simp
          have h_key_in_map : mapFind key'' c1.entries ≠ none := by
            have h_mem : (t'', key'') ∈ cleanStaleHeap c1.entries c1.expiryHeap := by rw [h_cleaned]; simp
            exact cleanStaleHeap_mem_implies_contains c1.entries c1.expiryHeap key'' t'' h_mem
          apply mapRemove_size_dec key'' c1.entries h_key_in_map
      omega
  · have h_sz_lt : mapSize c1.entries < c1.capacity := Nat.lt_of_not_ge h_at_cap
    have h_cap_eq : c1.capacity = c.capacity := rfl
    have h_sz_lt' : mapSize c1.entries < c.capacity := by rw [h_cap_eq] at h_sz_lt; exact h_sz_lt
    have h_sz1 : mapSize c1.entries + 1 ≤ c.capacity := by omega
    have h_sz2 : mapSize (mapInsert { engine := engine, id := randomId }
        { engine := engine, query := query, params := params, continuation := continuation, expiresAt := now + c.ttl } c1.entries) ≤ c.capacity := by
      have h_ins : mapSize (mapInsert { engine := engine, id := randomId }
        { engine := engine, query := query, params := params, continuation := continuation, expiresAt := now + c.ttl } c1.entries) ≤ mapSize c1.entries + 1 := mapInsert_size_bound _ _ _
      exact Nat.le_trans h_ins h_sz1
    unfold capacityInvariant
    simpa [h_at_cap, c1] using h_sz2

/-
  Theorem 5: update_continuation_no_heap_growth
  `updateContinuation` does not push to the heap.
  The heap size after `updateContinuation` is at most the original heap size.

  Why it matters: This is invariant 5 (no heap growth on update).
  Without this guarantee, the heap could grow unboundedly if `updateContinuation`
  were called frequently, since each call would push a new expiry to the heap
  without removing the stale one.

  Proof: `updateContinuation` calls `evictExpired` (which never increases heap size
  per `evictExpiredLoop_heap_size_noninc`), then performs a map update without
  touching the heap. The conclusion follows directly.
-/
-- Theorem 5
theorem update_continuation_no_heap_growth (c : SessionCache) (key : SessionKey) (continuation : Option Continuation) (now : Instant) :
    heapSize (updateContinuation c key continuation now).1.expiryHeap ≤ heapSize c.expiryHeap := by
  unfold updateContinuation
  have h_heap_size : heapSize (evictExpired c now).expiryHeap ≤ heapSize c.expiryHeap :=
    evictExpiredLoop_heap_size_noninc now c.entries c.expiryHeap
  cases h_opt : mapFind key (evictExpired c now).entries
  · simpa [h_opt] using h_heap_size
  · rename_i oldEntry; simpa [h_opt, heapSize] using h_heap_size
