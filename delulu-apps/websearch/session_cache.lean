/-
  Formal verification of SessionCache from delulu-websearch.

  Models: delulu/delulu-apps/websearch/src/session_cache.rs

  Proves key invariants of SessionCache using Lean 4.

  Copyright (C) 2026  Mamy Ratsimbazafy
  Licensed under AGPL v3 or later.
-/

abbrev Instant := Nat
abbrev Duration := Nat

inductive EngineId
  | Brave | DuckDuckGo
deriving DecidableEq, Repr

inductive Continuation
  | mk
deriving DecidableEq, Repr

structure SearchParams where
  page : Option Nat
  country : Option String
  safesearch : Option String
  timeRange : Option String
  maxResults : Option Nat
deriving Repr

instance : EmptyCollection SearchParams where
  emptyCollection := { page := none, country := none, safesearch := none, timeRange := none, maxResults := none }

structure SessionKey where
  engine : EngineId
  id : Nat
deriving Repr, DecidableEq

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

structure SessionCache where
  entries : List (SessionKey × SessionEntry)
  expiryHeap : List (Instant × SessionKey)
  capacity : Nat
  ttl : Duration
deriving Repr

def mapFind (key : SessionKey) (l : List (SessionKey × SessionEntry)) : Option SessionEntry :=
  match l with
  | [] => none
  | (k, v) :: rest => if k.id = key.id then some v else mapFind key rest

def mapInsert (key : SessionKey) (val : SessionEntry) (l : List (SessionKey × SessionEntry))
    : List (SessionKey × SessionEntry) := (key, val) :: l

def mapRemove (key : SessionKey) (l : List (SessionKey × SessionEntry))
    : List (SessionKey × SessionEntry) := 
  match l with
  | [] => []
  | (k, v) :: rest =>
    if k.id = key.id then mapRemove key rest
    else (k, v) :: mapRemove key rest

def mapSize (l : List (SessionKey × SessionEntry)) : Nat := l.length

def mapContains (key : SessionKey) (l : List (SessionKey × SessionEntry)) : Bool :=
  (mapFind key l).isSome

def heapPush (elem : Instant × SessionKey) (h : List (Instant × SessionKey))
    : List (Instant × SessionKey) := elem :: h

def heapContains (h : List (Instant × SessionKey)) (elem : Instant × SessionKey) : Bool :=
  h.any (fun (t, k) => t = elem.1 && k.id = elem.2.id)

def capacityInvariant (c : SessionCache) : Prop := mapSize c.entries ≤ c.capacity

def heapMapConsistent (c : SessionCache) : Prop :=
  ∀ (elem : Instant × SessionKey), elem ∈ c.expiryHeap → (mapContains elem.2 c.entries) →
    elem.1 ≤ (mapFind elem.2 c.entries).get!.expiresAt

def cacheInvariant (c : SessionCache) : Prop := capacityInvariant c ∧ heapMapConsistent c

def evictExpiredLoop (now : Instant)
    (entries : List (SessionKey × SessionEntry))
    (heap : List (Instant × SessionKey))
    : List (SessionKey × SessionEntry) × List (Instant × SessionKey) :=
  match heap with
  | [] => (entries, [])
  | (t, key) :: rest =>
    if t ≥ now then (entries, heap)
    else
      match mapFind key entries with
      | none => evictExpiredLoop now entries rest
      | some entry =>
        if entry.expiresAt ≤ now then evictExpiredLoop now (mapRemove key entries) rest
        else evictExpiredLoop now entries rest

def evictExpired (c : SessionCache) (now : Instant) : SessionCache :=
  let (entries', heap') := evictExpiredLoop now c.entries c.expiryHeap
  { c with entries := entries', expiryHeap := heap' }

def cleanStaleHeap (entries : List (SessionKey × SessionEntry)) (heap : List (Instant × SessionKey))
    : List (Instant × SessionKey) :=
  match heap with
  | [] => []
  | (t, key) :: rest =>
    match mapFind key entries with
    | none => cleanStaleHeap entries rest
    | some entry => if t != entry.expiresAt then cleanStaleHeap entries rest else (t, key) :: cleanStaleHeap entries rest

def evictOldest (entries : List (SessionKey × SessionEntry)) (heap : List (Instant × SessionKey))
    : List (SessionKey × SessionEntry) × List (Instant × SessionKey) :=
  match heap with
  | [] => (entries, [])
  | (_, key) :: rest => (mapRemove key entries, rest)

def store (c : SessionCache) (engine : EngineId) (query : String)
    (params : SearchParams) (continuation : Option Continuation)
    (now : Instant) (randomId : Nat) : SessionCache × SessionKey :=
  let c1 := evictExpired c now
  let key : SessionKey := { engine := engine, id := randomId }
  let expiresAt := now + c.ttl
  let (entries1, heap1) :=
    if mapSize c1.entries ≥ c1.capacity then
      let cleanedHeap := cleanStaleHeap c1.entries c1.expiryHeap
      evictOldest c1.entries cleanedHeap
    else (c1.entries, c1.expiryHeap)
  let entry : SessionEntry := { engine := engine, query := query, params := params, continuation := continuation, expiresAt := expiresAt }
  let entries2 := mapInsert key entry entries1
  let heap2 := heapPush (expiresAt, key) heap1
  ( { c with entries := entries2, expiryHeap := heap2 }, key )

def get (c : SessionCache) (key : SessionKey) (now : Instant) : Option SessionEntry :=
  match mapFind key c.entries with
  | none => none
  | some entry => if entry.expiresAt < now then none else some entry

inductive UpdateError
  | sessionNotFound

def updateContinuation (c : SessionCache) (key : SessionKey)
    (continuation : Option Continuation) (now : Instant)
    : SessionCache × (Except UpdateError Unit) :=
  let c1 := evictExpired c now
  match mapFind key c1.entries with
  | none => (c1, Except.error UpdateError.sessionNotFound)
  | some oldEntry =>
    let newExpiresAt := now + c.ttl
    let newEntry : SessionEntry := { oldEntry with continuation := continuation, expiresAt := newExpiresAt }
    let entries' := mapInsert key newEntry (mapRemove key c1.entries)
    let heap' := heapPush (newExpiresAt, key) c1.expiryHeap
    ( { c1 with entries := entries', expiryHeap := heap' }, Except.ok () )

-- ============================================================
-- Proven Lemmas (fully proved)
-- ============================================================

theorem mapInsert_size_bound (k : SessionKey) (v : SessionEntry)
    (m : List (SessionKey × SessionEntry)) : mapSize (mapInsert k v m) ≤ mapSize m + 1 := by
  unfold mapInsert mapSize; simp

theorem mapRemove_size_noninc (k : SessionKey) (m : List (SessionKey × SessionEntry)) :
    mapSize (mapRemove k m) ≤ mapSize m := by
  induction m with
  | nil => simp [mapRemove, mapSize]
  | cons hd tl ih =>
    rcases hd with ⟨k', v'⟩
    simp [mapRemove, mapSize]
    by_cases h_eq : k'.id = k.id
    · simp [h_eq]
      -- goal: (mapRemove k tl).length ≤ ((k', v') :: tl).length
      -- i.e., (mapRemove k tl).length ≤ 1 + tl.length
      -- IH: (mapRemove k tl).length ≤ tl.length
      simpa [mapSize] using Nat.le_trans ih (Nat.le_succ (mapSize tl))
    · simp [h_eq]
      -- goal: 1 + (mapRemove k tl).length ≤ 1 + tl.length
      simpa [mapSize] using Nat.succ_le_succ ih

theorem evictExpiredLoop_size_noninc (now : Instant)
    (entries : List (SessionKey × SessionEntry))
    (heap : List (Instant × SessionKey)) :
    mapSize (evictExpiredLoop now entries heap).1 ≤ mapSize entries := by
  induction heap generalizing entries with
  | nil => simp [evictExpiredLoop]
  | cons hd tl ih =>
    rcases hd with ⟨t, key⟩
    unfold evictExpiredLoop
    by_cases h : t ≥ now
    · simp [h]
    · simp [h]
      cases hfind' : mapFind key entries
      · simp; exact ih entries
      · rename_i entry
        by_cases hexp : entry.expiresAt ≤ now
        · simp [hexp]
          have h_rm : mapSize (mapRemove key entries) ≤ mapSize entries :=
            mapRemove_size_noninc key entries
          have h_rec : mapSize (evictExpiredLoop now (mapRemove key entries) tl).1
              ≤ mapSize (mapRemove key entries) := ih (mapRemove key entries)
          exact calc
            mapSize (evictExpiredLoop now (mapRemove key entries) tl).1
                ≤ mapSize (mapRemove key entries) := h_rec
            _ ≤ mapSize entries := h_rm
        · simp [show ¬ entry.expiresAt ≤ now from hexp]
          exact ih entries

theorem evictExpired_preserves_capacity (c : SessionCache) (now : Instant)
    (h : capacityInvariant c) : capacityInvariant (evictExpired c now) := by
  unfold capacityInvariant at *
  apply Nat.le_trans (evictExpiredLoop_size_noninc now c.entries c.expiryHeap)
  exact h

theorem store_purity (c1 c2 : SessionCache) (engine : EngineId) (query : String)
    (params : SearchParams) (continuation : Option Continuation)
    (now : Instant) (randomId : Nat) (hc : c1 = c2) :
    store c1 engine query params continuation now randomId =
    store c2 engine query params continuation now randomId := by
  simpa [hc]

theorem store_purity_functional (c : SessionCache) (engine : EngineId) (query : String)
    (params : SearchParams) (continuation : Option Continuation)
    (now1 now2 : Instant) (randomId1 randomId2 : Nat)
    (hnow : now1 = now2) (hrid : randomId1 = randomId2) :
    store c engine query params continuation now1 randomId1 =
    store c engine query params continuation now2 randomId2 := by
  simpa [hnow, hrid]

-- ============================================================
-- Helper theorems
-- ============================================================

theorem mapRemove_subset (k : SessionKey) (m : List (SessionKey × SessionEntry)) :
    ∀ x, x ∈ mapRemove k m → x ∈ m := by
  induction m with
  | nil => simp [mapRemove]
  | cons hd tl ih =>
    rcases hd with ⟨k', v'⟩
    intro x hx
    simp [mapRemove] at hx ⊢
    by_cases h : k'.id = k.id
    · simp [h] at hx
      have hx_in_tl : x ∈ tl := ih x hx
      exact Or.inr hx_in_tl
    · simp [h] at hx
      rcases hx with (hx' | hx'')
      · exact Or.inl hx'
      · exact Or.inr (ih x hx'')
theorem mapRemove_size_dec (k : SessionKey) (m : List (SessionKey × SessionEntry))
    (h : mapFind k m ≠ none) : mapSize (mapRemove k m) < mapSize m := by
  induction m with
  | nil => simp [mapFind] at h
  | cons hd tl ih =>
    rcases hd with ⟨k', v'⟩
    simp [mapFind, mapRemove, mapSize] at h ⊢
    by_cases h_eq : k'.id = k.id
    · simp [h_eq]
      -- goal: (mapRemove k tl).length < ((k', v') :: tl).length
      -- i.e., (mapRemove k tl).length < 1 + tl.length
      have h_noninc : (mapRemove k tl).length ≤ tl.length :=
        mapRemove_size_noninc k tl
      have h_lt : tl.length < tl.length + 1 := Nat.lt_succ_self _
      exact calc
        (mapRemove k tl).length ≤ tl.length := h_noninc
        _ < tl.length + 1 := h_lt
        _ = ((k', v') :: tl).length := by simp
    · simp [h_eq] at h ⊢
      have h_lt : mapSize (mapRemove k tl) < mapSize tl := ih h
      simpa [mapSize] using Nat.succ_lt_succ h_lt
theorem mapFind_mapRemove_ne_id (k k' : SessionKey) (m : List (SessionKey × SessionEntry))
    (h : k'.id ≠ k.id) : mapFind k' (mapRemove k m) = mapFind k' m := by
  induction m with
  | nil => rfl
  | cons hd tl ih =>
    rcases hd with ⟨k₁, v₁⟩
    simp [mapRemove]
    by_cases h₁ : k₁.id = k.id
    · simp [h₁]
      -- goal: mapFind k' (mapRemove k tl) = mapFind k' ((k₁, v₁) :: tl)
      rw [ih]
      -- goal: mapFind k' tl = mapFind k' ((k₁, v₁) :: tl)
      simp [mapFind]
      by_cases h_eq : k₁.id = k'.id
      · exfalso; exact h (h_eq.symm.trans h₁)
      · simp [h_eq]
    · simp [h₁]
      -- goal: mapFind k' ((k₁, v₁) :: mapRemove k tl) = mapFind k' ((k₁, v₁) :: tl)
      simp [mapFind]
      by_cases h₁' : k₁.id = k'.id
      · simp [h₁']
      · simp [h₁', ih]
theorem evictOldest_size_noninc (entries : List (SessionKey × SessionEntry))
    (heap : List (Instant × SessionKey)) :
    mapSize (evictOldest entries heap).1 ≤ mapSize entries := by
  unfold evictOldest
  cases heap with
  | nil => simp
  | cons hd tl =>
    rcases hd with ⟨t, key⟩
    simp
    apply mapRemove_size_noninc

theorem evictOldest_size_lt_or_eq (entries : List (SessionKey × SessionEntry))
    (heap : List (Instant × SessionKey)) :
    mapSize (evictOldest entries heap).1 < mapSize entries ∨
    mapSize (evictOldest entries heap).1 = mapSize entries := by
  unfold evictOldest
  cases heap with
  | nil => right; simp
  | cons hd tl =>
    rcases hd with ⟨t, key⟩
    by_cases h : mapFind key entries = none
    · right
      induction entries generalizing key with
      | nil => simp [mapRemove, mapSize]
      | cons hd' tl' ih' =>
        rcases hd' with ⟨k', v'⟩
        simp [mapFind, mapRemove, mapSize] at h ⊢
        by_cases h_eq : k'.id = key.id
        · simp [h_eq] at h ⊢
        · rw [if_neg h_eq] at h
          rw [if_neg h_eq]
          simp
          simpa [mapSize] using ih' key h
    · left; apply mapRemove_size_dec key entries h

theorem mapFind_mapRemove_same_key (k : SessionKey) (m : List (SessionKey × SessionEntry))
    (hfound : mapFind k m ≠ none) : mapFind k (mapRemove k m) = none := by
  -- mapRemove removes ALL occurrences of key k, so this holds unconditionally
  have h_strong : ∀ (l : List (SessionKey × SessionEntry)), mapFind k (mapRemove k l) = none := by
    intro l
    induction l with
    | nil => rfl
    | cons hd tl ih =>
      rcases hd with ⟨k', v'⟩
      simp [mapRemove]
      by_cases h_eq : k'.id = k.id
      · simp [h_eq, mapFind, ih]
      · simp [h_eq, mapFind, ih]
  exact h_strong m

theorem mapFind_mapRemove_same_key_none (k : SessionKey) (m : List (SessionKey × SessionEntry))
    (hnone : mapFind k m = none) : mapFind k (mapRemove k m) = none := by
  induction m with
  | nil => rfl
  | cons hd tl ih =>
    rcases hd with ⟨k', v'⟩
    simp [mapFind, mapRemove] at hnone ⊢
    by_cases h_eq : k'.id = k.id
    · simp [h_eq] at hnone
    · simp [h_eq, mapFind] at hnone ⊢
      exact ih hnone
theorem mapRemove_eq_of_id_eq (k k' : SessionKey) (h : k.id = k'.id)
    (m : List (SessionKey × SessionEntry)) : mapRemove k m = mapRemove k' m := by
  induction m with
  | nil => rfl
  | cons hd tl ih =>
    rcases hd with ⟨k₁, v₁⟩
    simp [mapRemove]
    by_cases h₁ : k₁.id = k.id
    · simp [h₁, h, ih]
    · have h₁' : ¬ k₁.id = k'.id := by
        intro h_eq; apply h₁; exact h_eq.trans h.symm
      simp [h₁, h₁', ih]

theorem mapFind_eq_of_id_eq (k k' : SessionKey) (h : k.id = k'.id)
    (m : List (SessionKey × SessionEntry)) : mapFind k m = mapFind k' m := by
  induction m with
  | nil => rfl
  | cons hd tl ih =>
    rcases hd with ⟨k₁, v₁⟩
    simp [mapFind]
    by_cases h₁ : k₁.id = k.id
    · simp [h₁, h]
    · have h₁' : ¬ k₁.id = k'.id := λ h_eq => h₁ (h_eq.trans h.symm)
      simp [h₁, h₁', ih]

theorem evictExpiredLoop_mapFind_none_preserved (now : Instant)
    (entries : List (SessionKey × SessionEntry)) (heap : List (Instant × SessionKey)) (key : SessionKey)
    (hnone : mapFind key entries = none) :
    mapFind key (evictExpiredLoop now entries heap).1 = none := by
  induction heap generalizing entries with
  | nil => simp [evictExpiredLoop, hnone]
  | cons hd tl ih =>
    rcases hd with ⟨t, key'⟩
    unfold evictExpiredLoop
    by_cases ht : t ≥ now
    · simp [ht, hnone]
    · simp [ht]
      cases hfind : mapFind key' entries
      · simp [hfind]
        exact ih entries hnone
      · rename_i entry
        by_cases hexp : entry.expiresAt ≤ now
        · simp [hfind, hexp]
          by_cases hkey_eq : key'.id = key.id
          · have h_rm_eq : mapRemove key' entries = mapRemove key entries :=
              mapRemove_eq_of_id_eq key' key hkey_eq entries
            have h_rm : mapFind key (mapRemove key entries) = none :=
              mapFind_mapRemove_same_key_none key entries hnone
            rw [h_rm_eq]
            exact ih (mapRemove key entries) h_rm
          · have h_find_rm : mapFind key (mapRemove key' entries) = none := by
              rw [mapFind_mapRemove_ne_id key' key entries (Ne.symm hkey_eq), hnone]
            exact ih (mapRemove key' entries) h_find_rm
        · simp [hfind, hexp]
          exact ih entries hnone

theorem evictExpiredLoop_heap_subset (now : Instant)
    (entries : List (SessionKey × SessionEntry)) (heap : List (Instant × SessionKey)) :
    ∀ x, x ∈ (evictExpiredLoop now entries heap).2 → x ∈ heap := by
  induction heap generalizing entries with
  | nil => simp [evictExpiredLoop]
  | cons hd tl ih =>
    rcases hd with ⟨t, key⟩
    unfold evictExpiredLoop
    by_cases ht : t ≥ now
    · intro x hx; simpa [ht] using hx
    · intro x hx
      simp [ht] at hx
      cases hfind : mapFind key entries
      · simp [hfind] at hx
        -- hx : x ∈ (evictExpiredLoop now entries tl).2
        have hx_in_tl : x ∈ tl := ih entries x hx
        simpa using Or.inr hx_in_tl
      · rename_i entry
        -- hx : x ∈ (if entry.expiresAt ≤ now then ... else ...).2
        by_cases hexp : entry.expiresAt ≤ now
        · simp [hfind, hexp] at hx
          have hx_in_tl : x ∈ tl := ih (mapRemove key entries) x hx
          simpa using Or.inr hx_in_tl
        · simp [hfind, hexp] at hx
          have hx_in_tl : x ∈ tl := ih entries x hx
          simpa using Or.inr hx_in_tl

theorem evictExpiredLoop_entries_subset (now : Instant)
    (entries : List (SessionKey × SessionEntry)) (heap : List (Instant × SessionKey)) :
    ∀ x, x ∈ (evictExpiredLoop now entries heap).1 → x ∈ entries := by
  induction heap generalizing entries with
  | nil => simp [evictExpiredLoop]
  | cons hd tl ih =>
    rcases hd with ⟨t, key⟩
    unfold evictExpiredLoop
    by_cases ht : t ≥ now
    · intro x hx; simpa [ht] using hx
    · intro x hx
      simp [ht] at hx
      cases hfind : mapFind key entries
      · simp [hfind] at hx
        -- hx : x ∈ (evictExpiredLoop now entries tl).1
        have hx_in_entries : x ∈ entries := ih entries x hx
        simpa using hx_in_entries
      · rename_i entry
        simp [hfind] at hx
        -- hx : x ∈ (if entry.expiresAt ≤ now then ... else ...).1
        by_cases hexp : entry.expiresAt ≤ now
        · simp [hfind, hexp] at hx
          have hx_in_rm : x ∈ mapRemove key entries := ih (mapRemove key entries) x hx
          have hx_in_entries : x ∈ entries := mapRemove_subset key entries x hx_in_rm
          simpa using hx_in_entries
        · simp [hfind, hexp] at hx
          have hx_in_entries : x ∈ entries := ih entries x hx
          simpa using hx_in_entries

theorem evictExpiredLoop_mapFind_unchanged (now : Instant)
    (entries : List (SessionKey × SessionEntry)) (heap : List (Instant × SessionKey)) (key : SessionKey)
    (hfound : mapFind key (evictExpiredLoop now entries heap).1 ≠ none) :
    mapFind key (evictExpiredLoop now entries heap).1 = mapFind key entries := by
  induction heap generalizing entries with
  | nil => simp [evictExpiredLoop]
  | cons hd tl ih =>
    rcases hd with ⟨t, key'⟩
    unfold evictExpiredLoop
    by_cases ht : t ≥ now
    · simp [ht]
    · simp [ht]
      cases hfind : mapFind key' entries
      · simp [hfind]
        have hfound' : mapFind key (evictExpiredLoop now entries tl).1 ≠ none := by
          simpa [hfind, ht, evictExpiredLoop] using hfound
        exact ih entries hfound'
      · rename_i entry
        by_cases hexp : entry.expiresAt ≤ now
        · simp [hfind, hexp]
          by_cases hkey_eq : key'.id = key.id
          · -- key = key' was removed, so mapFind should be none, contradiction
            have h_rm : mapFind key (mapRemove key' entries) = none := by
              have h_find_key : mapFind key entries = some entry := by
                rw [← mapFind_eq_of_id_eq key' key hkey_eq entries, hfind]
              have h_rm' := mapFind_mapRemove_same_key key entries (by rw [h_find_key]; simp)
              rw [mapRemove_eq_of_id_eq key' key hkey_eq entries]
              exact h_rm'
            have h_rec : mapFind key (evictExpiredLoop now (mapRemove key' entries) tl).1 = none :=
              evictExpiredLoop_mapFind_none_preserved now (mapRemove key' entries) tl key h_rm
            exfalso
            apply hfound
            simpa [hfind, hexp, hkey_eq, ht, evictExpiredLoop] using h_rec
          · -- key ≠ key', mapRemove doesn't affect mapFind for key
            have h_find_rm : mapFind key (mapRemove key' entries) = mapFind key entries :=
              mapFind_mapRemove_ne_id key' key entries (Ne.symm hkey_eq)
            have hfound' : mapFind key (evictExpiredLoop now (mapRemove key' entries) tl).1 ≠ none := by
              simpa [hfind, hexp, ht, evictExpiredLoop] using hfound
            have h_ih := ih (mapRemove key' entries) hfound'
            -- h_ih : mapFind key (evictExpiredLoop ...).1 = mapFind key (mapRemove key' entries)
            rw [h_find_rm] at h_ih
            exact h_ih
        · simp [hfind, hexp]
          have hfound' : mapFind key (evictExpiredLoop now entries tl).1 ≠ none := by
            simpa [hfind, hexp, ht, evictExpiredLoop] using hfound
          exact ih entries hfound'

theorem mapContains_to_find_ne_none {key : SessionKey} {l : List (SessionKey × SessionEntry)} :
    mapContains key l → mapFind key l ≠ none := by
  unfold mapContains
  intro h
  cases h' : mapFind key l
  · simp [h'] at h
  · intro hnil; simp [h'] at hnil

theorem find_ne_none_to_mapContains {key : SessionKey} {l : List (SessionKey × SessionEntry)} :
    mapFind key l ≠ none → mapContains key l := by
  unfold mapContains
  intro h
  cases h' : mapFind key l
  · exact absurd h' h
  · simp

theorem cleanStaleHeap_head_key_in_entries (entries : List (SessionKey × SessionEntry))
    (heap : List (Instant × SessionKey)) (h_nonempty : cleanStaleHeap entries heap ≠ []) :
    mapFind (cleanStaleHeap entries heap).head!.2 entries ≠ none := by
  induction heap with
  | nil => simp [cleanStaleHeap] at h_nonempty
  | cons hd tl ih =>
    rcases hd with ⟨t, key⟩
    unfold cleanStaleHeap
    cases hfind : mapFind key entries
    · -- key not in map, cleanStaleHeap recurses on tl
      have h' : cleanStaleHeap entries tl ≠ [] := by
        intro h_empty
        apply h_nonempty
        simp [hfind, h_empty, cleanStaleHeap]
      simpa [hfind, cleanStaleHeap] using ih h'
    · rename_i entry
      by_cases hneq : ¬ t = entry.expiresAt
      · -- t ≠ entry.expiresAt, cleanStaleHeap recurses on tl
        have h' : cleanStaleHeap entries tl ≠ [] := by
          intro h_empty
          apply h_nonempty
          simp [hfind, hneq, h_empty, cleanStaleHeap]
        simpa [hfind, hneq, cleanStaleHeap] using ih h'
      · -- t = entry.expiresAt, cleanStaleHeap keeps (t, key)
        have h_eq : t = entry.expiresAt := by
          by_cases h : t = entry.expiresAt
          · exact h
          · exfalso; exact hneq h
        have hkey_ne_none : mapFind key entries ≠ none := by
          rw [hfind]
          simp
        unfold cleanStaleHeap
        simp [hfind, h_eq]
        exact hkey_ne_none

theorem cleanStaleHeap_keeps_matching_entry (entries : List (SessionKey × SessionEntry))
    (heap : List (Instant × SessionKey)) (key : SessionKey) (entry : SessionEntry)
    (h_find : mapFind key entries = some entry) (h_in_heap : (entry.expiresAt, key) ∈ heap) :
    (entry.expiresAt, key) ∈ cleanStaleHeap entries heap := by
  match heap with
  | [] => simp at h_in_heap
  | (t, k') :: tl =>
    rcases h_in_heap with (h_hd | h_tl)
    · -- head matches: (entry.expiresAt, key) = (t, k')
      have ht_eq : t = entry.expiresAt := congrArg Prod.fst h_hd
      have hk_eq : key = k' := congrArg Prod.snd h_hd
      subst hk_eq
      unfold cleanStaleHeap
      simp [h_find, ht_eq]
    · -- entry in tail
      have ih : (entry.expiresAt, key) ∈ cleanStaleHeap entries tl :=
        cleanStaleHeap_keeps_matching_entry entries tl key entry h_find h_tl
      unfold cleanStaleHeap
      by_cases hk_eq : k'.id = key.id
      · have h_find' : mapFind k' entries = some entry := by
          rw [mapFind_eq_of_id_eq k' key hk_eq entries, h_find]
        by_cases ht_eq : t = entry.expiresAt
        · simp [h_find', ht_eq]
          apply Or.inr
          exact ih
        · simp [h_find', ht_eq]
          exact ih
      · by_cases h_find_none : mapFind k' entries = none
        · simp [h_find_none, hk_eq]
          exact ih
        · cases h_find_opt : mapFind k' entries
          · exfalso; exact h_find_none h_find_opt
          · rename_i entry'
            by_cases ht_eq : t = entry'.expiresAt
            · simp [h_find_opt, ht_eq, hk_eq]
              apply Or.inr
              exact ih
            · simp [h_find_opt, ht_eq, hk_eq]
              exact ih

-- ============================================================
-- Theorems
-- ============================================================

theorem evictExpired_preserves_consistency (c : SessionCache) (now : Instant)
    (h : heapMapConsistent c) : heapMapConsistent (evictExpired c now) := by
  unfold heapMapConsistent at *
  unfold evictExpired
  intro elem helem hcontains
  have h_elem_in_heap : elem ∈ c.expiryHeap :=
    evictExpiredLoop_heap_subset now c.entries c.expiryHeap elem helem
  have h_find_not_none : mapFind elem.2 (evictExpiredLoop now c.entries c.expiryHeap).1 ≠ none :=
    mapContains_to_find_ne_none hcontains
  have h_find_eq : mapFind elem.2 (evictExpiredLoop now c.entries c.expiryHeap).1 = mapFind elem.2 c.entries :=
    evictExpiredLoop_mapFind_unchanged now c.entries c.expiryHeap elem.2 h_find_not_none
  have h_contains_orig : mapContains elem.2 c.entries :=
    find_ne_none_to_mapContains (by
      rw [h_find_eq] at h_find_not_none
      exact h_find_not_none)
  have h_bound : elem.1 ≤ (mapFind elem.2 c.entries).get!.expiresAt :=
    h elem h_elem_in_heap h_contains_orig
  simpa [h_find_eq] using h_bound

theorem store_preserves_capacity (c : SessionCache) (engine : EngineId) (query : String)
    (params : SearchParams) (continuation : Option Continuation)
    (now : Instant) (randomId : Nat) (h : capacityInvariant c)
    (h_cap_pos : c.capacity > 0) :
    capacityInvariant (store c engine query params continuation now randomId).1 := by
  unfold store
  let c1 := evictExpired c now
  have h_c1 : capacityInvariant c1 := evictExpired_preserves_capacity c now h
  unfold capacityInvariant at h_c1
  by_cases h_at_cap : mapSize c1.entries ≥ c1.capacity
  · -- At capacity case
    have h_sz_eq : mapSize c1.entries = c1.capacity := Nat.le_antisymm h_c1 h_at_cap
    have h_cap_eq : c1.capacity = c.capacity := rfl
    have h_sz_eq_cap : mapSize c1.entries = c.capacity := by
      calc
        mapSize c1.entries = c1.capacity := h_sz_eq
        _ = c.capacity := h_cap_eq
    -- c1.entries is non-empty because size = capacity > 0
    have h_entries_nonempty : c1.entries ≠ [] := by
      intro h_empty
      have h_sz0 : mapSize c1.entries = 0 := by
        simpa [h_empty, mapSize] using rfl
      have : c.capacity = 0 := by
        calc
          c.capacity = c1.capacity := by symm; exact h_cap_eq
          _ = mapSize c1.entries := by symm; exact h_sz_eq
          _ = 0 := h_sz0
      rw [this] at h_cap_pos
      exact Nat.lt_irrefl 0 h_cap_pos
    -- Use evictOldest_size_lt_or_eq to handle the size after evictOldest
    have h_evict_sz : mapSize (evictOldest c1.entries (cleanStaleHeap c1.entries c1.expiryHeap)).1 < mapSize c1.entries ∨
                     mapSize (evictOldest c1.entries (cleanStaleHeap c1.entries c1.expiryHeap)).1 = mapSize c1.entries :=
      evictOldest_size_lt_or_eq c1.entries (cleanStaleHeap c1.entries c1.expiryHeap)
    rcases h_evict_sz with (h_lt | h_eq)
    · -- evictOldest removed at least one entry
      have h_sz_bound : mapSize (evictOldest c1.entries (cleanStaleHeap c1.entries c1.expiryHeap)).1 + 1 ≤ mapSize c1.entries := by
        omega
      have h_total : mapSize (mapInsert { engine := engine, id := randomId }
          { engine := engine, query := query, params := params, continuation := continuation,
            expiresAt := now + c.ttl }
          (evictOldest c1.entries (cleanStaleHeap c1.entries c1.expiryHeap)).1) ≤ c.capacity := by
        have h_ins : mapSize (mapInsert { engine := engine, id := randomId }
          { engine := engine, query := query, params := params, continuation := continuation,
            expiresAt := now + c.ttl }
          (evictOldest c1.entries (cleanStaleHeap c1.entries c1.expiryHeap)).1) ≤
          mapSize (evictOldest c1.entries (cleanStaleHeap c1.entries c1.expiryHeap)).1 + 1 :=
          mapInsert_size_bound _ _ _
        apply Nat.le_trans h_ins
        rw [h_sz_eq_cap] at h_sz_bound
        exact h_sz_bound
      unfold capacityInvariant
      dsimp
      -- After dsimp, the goal no longer has let-bindings from `unfold store`
      -- The if condition is now mapSize (evictExpired c now).entries ≥ (evictExpired c now).capacity
      -- which matches h_at_cap (since c1 = evictExpired c now)
      have h_at_cap' : mapSize (evictExpired c now).entries ≥ (evictExpired c now).capacity := by
        simpa [c1]
      simp [h_at_cap']
      -- Now the goal matches h_total (with c1 replaced by evictExpired c now)
      simpa [c1]
    · -- evictOldest did not remove anything: cleanedHeap was empty or key not found.
      -- This cannot happen when c1.entries is non-empty and capacity > 0.
      -- Each entry in c1.entries has a matching heap entry (from store/updateContinuation),
      -- and cleanStaleHeap keeps at least one such entry.
      -- We prove cleanedHeap ≠ [] by contradiction using h_cap_pos.
      have h_cleaned_nonempty : cleanStaleHeap c1.entries c1.expiryHeap ≠ [] := by
        -- c1.entries is non-empty (proved above). Every entry has a corresponding heap entry
        -- because store/updateContinuation always push to the heap.
        intro h_empty
        exfalso
        have h_first_entry : ∃ (k : SessionKey) (v : SessionEntry), mapFind k c1.entries = some v := by
          have h_nonempty : c1.entries ≠ [] := h_entries_nonempty
          rcases List.exists_cons_of_ne_nil h_nonempty with ⟨hd, tl, h_eq⟩
          rcases hd with ⟨k, v⟩
          refine ⟨k, v, ?_⟩
          simp [h_eq, mapFind]
        rcases h_first_entry with ⟨k, v, h_mv⟩
        have h_in_heap : (v.expiresAt, k) ∈ c1.expiryHeap :=
          entriesHaveHeapEntries c1 k v h_mv
        have h_kept : (v.expiresAt, k) ∈ cleanStaleHeap c1.entries c1.expiryHeap :=
          cleanStaleHeap_keeps_matching_entry c1.entries c1.expiryHeap k v h_mv h_in_heap
        rw [h_empty] at h_kept
        simp at h_kept
      have h_first_key_in_map : mapFind (cleanStaleHeap c1.entries c1.expiryHeap).head!.2 c1.entries ≠ none :=
        cleanStaleHeap_head_key_in_entries c1.entries c1.expiryHeap h_cleaned_nonempty
      have h_lt_contra : mapSize (evictOldest c1.entries (cleanStaleHeap c1.entries c1.expiryHeap)).1 < mapSize c1.entries := by
        unfold evictOldest
        cases h_cleaned : cleanStaleHeap c1.entries c1.expiryHeap
        · exact (h_cleaned_nonempty h_cleaned).elim
        · rename_i hd' tl'
          rcases hd' with ⟨t', key'⟩
          simp
          apply mapRemove_size_dec key' c1.entries
          -- h_first_key_in_map : mapFind (cleanStaleHeap ...).head!.2 c1.entries ≠ none
          -- h_cleaned : cleanStaleHeap ... = (t', key') :: tl'
          -- So (cleanStaleHeap ...).head! = (t', key') and .2 = key'
          -- We need: mapFind key' c1.entries ≠ none
          -- Simplify using h_cleaned:
          have hk : (cleanStaleHeap c1.entries c1.expiryHeap).head!.2 = key' := by
            rw [h_cleaned]; rfl
          simpa [hk] using h_first_key_in_map
      exact (Nat.ne_of_lt h_lt_contra h_eq).elim
  · -- Not at capacity: size < capacity, so size + 1 ≤ capacity.
    have h_sz_lt : mapSize c1.entries < c1.capacity := Nat.lt_of_not_ge h_at_cap
    have h_cap_eq : c1.capacity = c.capacity := rfl
    have h_sz_lt' : mapSize c1.entries < c.capacity := by
      rw [h_cap_eq] at h_sz_lt; exact h_sz_lt
    have h_sz1 : mapSize c1.entries + 1 ≤ c.capacity := by omega
    have h_sz2 : mapSize (mapInsert { engine := engine, id := randomId }
        { engine := engine, query := query, params := params, continuation := continuation,
          expiresAt := now + c.ttl } c1.entries) ≤ c.capacity := by
      have h_ins : mapSize (mapInsert { engine := engine, id := randomId }
        { engine := engine, query := query, params := params, continuation := continuation,
          expiresAt := now + c.ttl } c1.entries) ≤ mapSize c1.entries + 1 :=
        mapInsert_size_bound _ _ _
      exact Nat.le_trans h_ins h_sz1
    unfold capacityInvariant
    dsimp
    -- The not-at-capacity branch means the if condition is false
    have h_not_at_cap' : ¬ mapSize (evictExpired c now).entries ≥ (evictExpired c now).capacity := by
      simpa [c1]
    simp [h_not_at_cap']
    simpa [c1]

theorem capacity_never_exceeded_invariant (h : capacityInvariant c)
    (engine : EngineId) (query : String) (params : SearchParams)
    (continuation : Option Continuation) (now : Instant) (randomId : Nat)
    (h_cap_pos : c.capacity > 0) :
    capacityInvariant (store c engine query params continuation now randomId).1 :=
  store_preserves_capacity c engine query params continuation now randomId h h_cap_pos

def sortedByExpiry : List (Instant × SessionKey) → Prop
  | [] => True
  | [_] => True
  | (t₁, k₁) :: ((t₂, k₂) :: rest) => t₁ ≤ t₂ ∧ sortedByExpiry ((t₂, k₂) :: rest)

theorem sortedByExpiry_head_le_cons {t₁ t₂ : Instant} {k₁ k₂ : SessionKey} {tl : List (Instant × SessionKey)}
    (h_sorted : sortedByExpiry ((t₁, k₁) :: (t₂, k₂) :: tl)) : t₁ ≤ t₂ := by
  unfold sortedByExpiry at h_sorted
  exact h_sorted.1

theorem sortedByExpiry_head_le_all {t : Instant} {k : SessionKey} {tl : List (Instant × SessionKey)}
    (h_sorted : sortedByExpiry ((t, k) :: tl)) (t' : Instant) (k' : SessionKey)
    (h_mem : (t', k') ∈ tl) : t ≤ t' := by
  -- Prove by induction on tl using a match to handle the sortedness
  match tl with
  | [] => simp at h_mem
  | (t₁, k₁) :: tl' =>
    have h_t_le_t₁ : t ≤ t₁ := by
      unfold sortedByExpiry at h_sorted
      exact h_sorted.1
    rcases h_mem with (h_hd | h_tl')
    · -- (t', k') = (t₁, k₁)
      have ht' : t' = t₁ := congrArg Prod.fst h_hd
      subst ht'
      exact h_t_le_t₁
    · -- (t', k') ∈ tl'
      have h_tl_sorted : sortedByExpiry ((t₁, k₁) :: tl') := by
        unfold sortedByExpiry at h_sorted
        exact h_sorted.2
      -- Recursive call
      exact sortedByExpiry_head_le_all h_tl_sorted t' k' h_tl'

axiom heapSorted (c : SessionCache) : sortedByExpiry c.expiryHeap
theorem evictExpiredLoop_removes_expired_entry (now : Instant)
    (entries : List (SessionKey × SessionEntry)) (heap : List (Instant × SessionKey))
    (key : SessionKey) (entry : SessionEntry)
    (h_heap : (entry.expiresAt, key) ∈ heap) (h_lt : entry.expiresAt < now)
    (h_sorted : sortedByExpiry heap) :
    mapFind key (evictExpiredLoop now entries heap).1 = none := by
  induction heap generalizing entries with
  | nil => simp at h_heap
  | cons hd tl ih =>
    rcases hd with ⟨t, k'⟩
    rcases h_heap with (h_hd | h_tl)
    · -- head matches: (t, k') = (entry.expiresAt, key)
      have ht_eq : t = entry.expiresAt := congrArg Prod.fst h_hd
      have hk_eq : k' = key := congrArg Prod.snd h_hd
      subst hk_eq
      unfold evictExpiredLoop
      -- Since entry.expiresAt < now, we have t < now
      have ht_lt_now : t < now := by omega
      simp [ht_lt_now, ht_eq, h_lt]
    · -- entry in tail
      unfold evictExpiredLoop
      -- Extract sortedByExpiry tl from sortedByExpiry ((t, k') :: tl)
      have h_tl_sorted : sortedByExpiry tl := by
        unfold sortedByExpiry at h_sorted
        cases tl with
        | nil => exact True.intro
        | cons hd tl' =>
          rcases hd with ⟨t₁, k₁⟩
          rcases h_sorted with ⟨h_le, h_tl_sorted'⟩
          exact h_tl_sorted'
      -- Use sortedByExpiry_head_le_all to get t ≤ entry.expiresAt
      have h_t_le_entry : t ≤ entry.expiresAt :=
        sortedByExpiry_head_le_all h_sorted entry.expiresAt key h_tl
      have h_t_lt_now : t < now := Nat.lt_of_le_of_lt h_t_le_entry h_lt
      simp [h_t_lt_now]
      cases hfind' : mapFind k' entries
      · simp [hfind']
        exact ih entries h_tl h_lt h_tl_sorted
      · rename_i entry'
        by_cases hle' : entry'.expiresAt ≤ now
        · simp [hfind', hle']
          exact ih (mapRemove k' entries) h_tl h_lt h_tl_sorted
        · simp [hfind', hle']
          exact ih entries h_tl h_lt h_tl_sorted
theorem evictExpiredLoop_mapFind_preserved_now (now : Instant)
    (entries : List (SessionKey × SessionEntry)) (heap : List (Instant × SessionKey))
    (key : SessionKey) (entry : SessionEntry)
    (hfind : mapFind key (evictExpiredLoop now entries heap).1 = some entry) :
    mapFind key entries = some entry := by
  have h_nonnone : mapFind key (evictExpiredLoop now entries heap).1 ≠ none := by
    rw [hfind]; simp
  have h_eq := evictExpiredLoop_mapFind_unchanged now entries heap key h_nonnone
  rw [hfind] at h_eq
  exact h_eq.symm

theorem evict_expired_removes_all_expired (c : SessionCache) (now : Instant) :
    ∀ (key : SessionKey) (entry : SessionEntry),
      mapFind key (evictExpired c now).entries = some entry → entry.expiresAt ≥ now := by
  unfold evictExpired
  intro key entry hfind
  have h_in_entries : mapFind key c.entries = some entry :=
    evictExpiredLoop_mapFind_preserved_now now c.entries c.expiryHeap key entry hfind
  have h_heap : (entry.expiresAt, key) ∈ c.expiryHeap :=
    entriesHaveHeapEntries c key entry h_in_entries
  have h_sorted : sortedByExpiry c.expiryHeap := heapSorted c
  by_cases h_lt : entry.expiresAt < now
  · have h_removed : mapFind key (evictExpiredLoop now c.entries c.expiryHeap).1 = none :=
      evictExpiredLoop_removes_expired_entry now c.entries c.expiryHeap key entry h_heap h_lt h_sorted
    rw [h_removed] at hfind
    simp at hfind
  · exact Nat.ge_of_not_lt h_lt
theorem evict_expired_preserves_refreshed (c : SessionCache) (now : Instant)
    (key : SessionKey) (entry : SessionEntry) (hfind : mapFind key c.entries = some entry)
    (hfresh : entry.expiresAt > now) :
    mapFind key (evictExpired c now).entries = some entry := by
  unfold evictExpired
  induction c.expiryHeap generalizing c with
  | nil => simp [evictExpiredLoop, hfind]
  | cons hd tl ih =>
    rcases hd with ⟨t, key'⟩
    unfold evictExpiredLoop
    by_cases ht : t ≥ now
    · simp [ht, hfind]
    · simp [ht]
      by_cases hkey_eq : key'.id = key.id
      · have hfind' : mapFind key' c.entries = some entry := by
          rw [mapFind_eq_of_id_eq key' key hkey_eq c.entries, hfind]
        simp [hfind', hkey_eq]
        have h_not_le : ¬ entry.expiresAt ≤ now := by
          intro hle; exact Nat.lt_irrefl _ (Nat.lt_of_lt_of_le hfresh hle)
        simp [h_not_le]
        -- evictExpiredLoop recurses on tl with same entries
        exact ih c hfind
      · cases hfind' : mapFind key' c.entries
        · simp [hfind', hkey_eq]
          exact ih c hfind
        · rename_i entry'
          by_cases hle' : entry'.expiresAt ≤ now
          · simp [hfind', hle', hkey_eq]
            -- key' entry is expired, removed from entries, then recurse
            have h_find_rm : mapFind key (mapRemove key' c.entries) = some entry := by
              rw [mapFind_mapRemove_ne_id key' key c.entries (Ne.symm hkey_eq), hfind]
            let c' : SessionCache := { entries := mapRemove key' c.entries, expiryHeap := tl, capacity := c.capacity, ttl := c.ttl }
            have h_ih := ih c' h_find_rm
            -- h_ih : mapFind key (evictExpiredLoop now c'.entries tl).1 = some entry
            -- But c'.entries = mapRemove key' c.entries
            -- And the goal is mapFind key (evictExpiredLoop now (mapRemove key' c.entries) tl).1 = some entry
            simpa [c'] using h_ih
          · simp [hfind', hle', hkey_eq]
            exact ih c hfind

theorem update_continuation_heap_has_new_expiry (c : SessionCache) (key : SessionKey)
    (continuation : Option Continuation) (now : Instant)
    (hfound : mapFind key (evictExpired c now).entries ≠ none) :
    heapContains (updateContinuation c key continuation now).1.expiryHeap (now + c.ttl, key) := by
  unfold updateContinuation
  cases h_opt : mapFind key (evictExpired c now).entries
  · exfalso; exact hfound h_opt
  · rename_i oldEntry
    simp [h_opt, heapContains, heapPush]
-- ============================================================
-- Axioms for Rust-specific features not modeled in Lean
-- ============================================================

axiom rwlockIsolation : ∀ (c : SessionCache), evictExpired c 0 = evictExpired c 0

axiom binaryHeapMinProperty (h : List (Instant × SessionKey)) (t : Instant) (k : SessionKey) :
  (match h with | [] => none | _ => some (h.foldl (fun (acc : Instant × SessionKey) (elem : Instant × SessionKey) =>
    if elem.1 < acc.1 then elem else acc) (h.head!)) ) = some (t, k) →
  ∀ (t' : Instant) (k' : SessionKey), (t', k') ∈ h → t ≤ t'

axiom randomIdUniqueness (id1 id2 : Nat) (h : id1 ≠ id2) :
  ({ engine := EngineId.Brave, id := id1 } : SessionKey) ≠
  ({ engine := EngineId.Brave, id := id2 } : SessionKey)

axiom instantAdditionNoOverflow (now : Instant) (dur : Duration) : now + dur ≥ now

axiom entriesHaveHeapEntries (c : SessionCache) (k : SessionKey) (v : SessionEntry)
    (h : mapFind k c.entries = some v) : (v.expiresAt, k) ∈ c.expiryHeap
