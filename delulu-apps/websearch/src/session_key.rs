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
//! Format: `<timestamp>-<engine:3>-<id:11base58>`
//! Example: `20260725T060000-brv-Pyz8q4fVDuL`
//!
//! Hash is computed from the 8-byte random ID only (cryptographically random → ideal dispersion).
//! The timestamp and engine are for debugging (eviction, log analysis).

use chrono::{DateTime, Utc};
use crate::engine::EngineId;
use serde::{
    de::{self, Visitor},
    Deserialize, Serialize, Serializer,
};
use std::fmt;
use std::hash::{Hash, Hasher};


/// A session key for MCP pagination.
///
/// Format: `<timestamp>-<engine:3>-<id:11base58>`
/// Example: `20260725T060000-brv-Pyz8q4fVDuL`
///
/// Hash and equality use the 8-byte random ID only.
/// The timestamp and engine are for debugging (visible in the serialized form).
#[derive(Debug, Clone)]
pub struct SessionKey {
    /// UTC timestamp for debugging eviction/logs.
    timestamp: DateTime<Utc>,
    /// Engine identifier for debugging.
    engine: EngineId,
    /// 8 cryptographically random bytes — used for hash and equality.
    id: [u8; 8],
}

impl SessionKey {
    /// Create a new session key from its components.
    /// Pure function — same inputs always produce the same key.
    /// The caller provides 8 cryptographically random bytes.
    pub fn new(engine: EngineId, timestamp: DateTime<Utc>, id: [u8; 8]) -> Self {
        SessionKey { timestamp, engine, id }
    }

    /// The serialized form: `<timestamp>-<engine:3>-<id:11base58>`
    pub fn as_str(&self) -> String {
        let ts = self.timestamp.format("%Y%m%dT%H%M%S");
        format!("{}-{}-{}", ts, self.engine.abbreviation(), base58_encode(&self.id))
    }
}

// Hash and equality from the random ID only — no timestamp/engine involved.
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

impl fmt::Display for SessionKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

// --- Serialization: produces "<timestamp>-<engine>-<hex_id>" ---

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
                write!(f, "a session key in format <timestamp>-<engine>-<hex_id>")
            }

            fn visit_str<E: de::Error>(self, s: &str) -> Result<SessionKey, E> {
                // Parse "<timestamp>-<engine>-<hex_id>"
                let parts: Vec<&str> = s.splitn(3, '-').collect();
                if parts.len() != 3 {
                    return Err(E::custom(format!(
                        "expected 3 dash-separated parts, got {}",
                        parts.len()
                    )));
                }
                let timestamp_str = parts[0];
                let engine_str = parts[1];
                let hex_id = parts[2];

                // Parse timestamp
                let ts_format = "%Y%m%dT%H%M%S";
                let timestamp = DateTime::parse_from_str(
                    &format!("{} +0000", timestamp_str),
                    &format!("{} %z", ts_format),
                )
                .map_err(|e| E::custom(format!("invalid timestamp '{}': {}", timestamp_str, e)))?
                .with_timezone(&Utc);

                // Parse engine
                let engine = match engine_str {
                    "brv" => EngineId::Brave,
                    "ddg" => EngineId::DuckDuckGo,
                    other => {
                        return Err(E::custom(format!("unknown engine abbreviation '{}'", other)));
                    }
                };

                // Parse base58 ID (11 chars = 8 bytes)
                if hex_id.len() != 11 {
                    return Err(E::custom(format!(
                        "expected 11 base58 chars for ID, got {}",
                        hex_id.len()
                    )));
                }
                let id = base58_decode(hex_id).ok_or_else(|| {
                    E::custom(format!("invalid base58 ID '{}'", hex_id))
                })?;

                Ok(SessionKey { timestamp, engine, id })
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
