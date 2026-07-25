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

//! Unit tests for session key module.

use delulu_websearch::{EngineId, SessionKey};
use std::collections::HashSet;

fn fixed_id() -> [u8; 8] {
    [0xAB, 0xCD, 0xEF, 0x01, 0x23, 0x45, 0x67, 0x89]
}

fn alt_id() -> [u8; 8] {
    [0x12, 0x34, 0x56, 0x78, 0x9A, 0xBC, 0xDE, 0xF0]
}

#[test]
fn session_key_format() {
    let key = SessionKey::new(EngineId::Brave, fixed_id());
    let s = key.as_str();
    assert_eq!(s, "brv-Pyz8q4fVDuL");
}

#[test]
fn session_key_deterministic() {
    let key1 = SessionKey::new(EngineId::Brave, fixed_id());
    let key2 = SessionKey::new(EngineId::Brave, fixed_id());
    assert_eq!(key1, key2);
}

#[test]
fn hash_and_eq_use_id_only() {
    let key1 = SessionKey::new(EngineId::Brave, fixed_id());
    let key2 = SessionKey::new(EngineId::DuckDuckGo, fixed_id());
    // Same ID, different engine — must be equal (hash from ID only)
    assert_eq!(key1, key2);
}

#[test]
fn different_ids_are_not_equal() {
    let key1 = SessionKey::new(EngineId::Brave, fixed_id());
    let key2 = SessionKey::new(EngineId::Brave, alt_id());
    assert_ne!(key1, key2);
}

#[test]
fn round_trip_serialization() {
    let key = SessionKey::new(EngineId::Brave, fixed_id());
    let json = serde_json::to_string(&key).unwrap();
    assert_eq!(json, "\"brv-Pyz8q4fVDuL\"");

    let deserialized: SessionKey = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized, key);
}

#[test]
fn round_trip_duckduckgo() {
    let key = SessionKey::new(EngineId::DuckDuckGo, alt_id());
    let json = serde_json::to_string(&key).unwrap();
    assert_eq!(json, "\"ddg-hHjzwFVc9gd\"");

    let deserialized: SessionKey = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized, key);
}

#[test]
fn hashmap_usage() {
    let key1 = SessionKey::new(EngineId::Brave, fixed_id());
    let key2 = SessionKey::new(EngineId::Brave, alt_id());

    let mut map = HashSet::new();
    map.insert(key1.clone());
    assert!(map.contains(&key1));
    assert!(!map.contains(&key2));
}

#[test]
fn display_matches_as_str() {
    let key = SessionKey::new(EngineId::Brave, fixed_id());
    assert_eq!(format!("{}", key), key.as_str());
}

#[test]
fn deserialize_invalid_format() {
    let result: Result<SessionKey, _> = serde_json::from_str("\"invalid\"");
    assert!(result.is_err());

    let result: Result<SessionKey, _> = serde_json::from_str("\"20260725T060000-unknown-Pyz8q4fVDuL\"");
    assert!(result.is_err());

    let result: Result<SessionKey, _> = serde_json::from_str("\"brv-short\"");
    assert!(result.is_err());
}
