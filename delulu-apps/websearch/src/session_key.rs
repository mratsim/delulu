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

//! Session key type for MCP pagination.
//!
//! Format: `<engine:3>-<id:11base58>`
//! Example: `brv-Pyz8q4fVDuL`
//!
//! Hash and equality are computed from the 8-byte random ID only (cryptographically random → ideal dispersion).

use crate::engine::EngineId;
use serde::{
    de::{self, Visitor},
    Deserialize, Serialize, Serializer,
};
use std::fmt;
use std::hash::{Hash, Hasher};


/// A session key for MCP pagination.
///
/// Format: `<engine:3>-<id:11base58>`
/// Example: `brv-Pyz8q4fVDuL`
///
/// Hash and equality use the 8-byte random ID only.
/// The engine is for debugging (visible in the serialized form).
#[derive(Debug, Clone)]
pub struct SessionKey {
    /// Engine identifier for debugging.
    engine: EngineId,
    /// 8 cryptographically random bytes — used for hash and equality.
    id: [u8; 8],
}

impl SessionKey {
    /// Create a new session key from its components.
    /// Pure function — same inputs always produce the same key.
    /// The caller provides 8 cryptographically random bytes.
    pub fn new(engine: EngineId, id: [u8; 8]) -> Self {
        SessionKey { engine, id }
    }

    /// The serialized form: `<engine:3>-<id:11base58>`
    pub fn as_str(&self) -> String {
        format!("{}-{}", self.engine.abbreviation(), base58_encode(&self.id))
    }
}

// Hash and equality from the random ID only — no engine involved.
impl Hash for SessionKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.id.hash(state);
    }
}

impl PartialEq for SessionKey {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl Eq for SessionKey {}

impl PartialOrd for SessionKey {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for SessionKey {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.id.cmp(&other.id)
    }
}

impl fmt::Display for SessionKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

// --- Serialization: produces "<engine>-<base58_id>" ---

impl Serialize for SessionKey {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.as_str())
    }
}

impl<'de> Deserialize<'de> for SessionKey {
    fn deserialize<D: de::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct KeyVisitor;
        impl<'de> Visitor<'de> for KeyVisitor {
            type Value = SessionKey;

            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                write!(f, "a session key in format <engine>-<base58_id>")
            }

            fn visit_str<E: de::Error>(self, s: &str) -> Result<SessionKey, E> {
                // Parse "<engine>-<base58_id>"
                let parts: Vec<&str> = s.splitn(2, '-').collect();
                if parts.len() != 2 {
                    return Err(E::custom(format!(
                        "expected 2 dash-separated parts, got {}",
                        parts.len()
                    )));
                }
                let engine_str = parts[0];
                let b58_id = parts[1];

                // Parse engine
                let engine = match engine_str {
                    "brv" => EngineId::Brave,
                    "ddg" => EngineId::DuckDuckGo,
                    other => {
                        return Err(E::custom(format!("unknown engine abbreviation '{}'", other)));
                    }
                };

                // Parse base58 ID (11 chars = 8 bytes)
                if b58_id.len() != 11 {
                    return Err(E::custom(format!(
                        "expected 11 base58 chars for ID, got {}",
                        b58_id.len()
                    )));
                }
                let id = base58_decode(b58_id).ok_or_else(|| {
                    E::custom(format!("invalid base58 ID '{}'", b58_id))
                })?;

                Ok(SessionKey { engine, id })
            }
        }
        deserializer.deserialize_str(KeyVisitor)
    }
}


/// Base58 alphabet (no 0, O, I, l to avoid ambiguity).
const BASE58_ALPHABET: &[u8] = b"123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";

/// Encode 8 bytes as base58 (produces 11 characters).
fn base58_encode(bytes: &[u8; 8]) -> String {
    let mut value = u64::from_le_bytes(*bytes);
    let mut result = Vec::new();
    while value > 0 {
        result.push(BASE58_ALPHABET[(value % 58) as usize]);
        value /= 58;
    }
    result.reverse();
    String::from_utf8(result).unwrap()
}

/// Decode 11 base58 characters back into 8 bytes.
fn base58_decode(s: &str) -> Option<[u8; 8]> {
    let mut value: u64 = 0;
    for c in s.chars() {
        let idx = BASE58_ALPHABET.iter().position(|&b| b == c as u8)?;
        value = value.checked_mul(58)?;
        value = value.checked_add(idx as u64)?;
    }
    Some(value.to_le_bytes())
}
