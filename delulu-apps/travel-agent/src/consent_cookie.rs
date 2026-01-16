//!  Delulu Travel Agent
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

//! SOCS cookie generation for Google services.

use base64::{engine::general_purpose::STANDARD, Engine as _};
use chrono::{Datelike, Local};

// =============================================================================
// Constants - Known-Good Browser Values
// =============================================================================

/// Binary blob (5 bytes)
/// Original: [0x08, 0x80, 0xc4, 0xf6, 0xca]
const DEFAULT_BINARY_BLOB: &[u8] = &[0x08, 0x80, 0xc4, 0xf6, 0xca];

// =============================================================================
// Low-Level Protobuf Encoding
// =============================================================================

const WIRE_LENGTH_DELIMITED: u8 = 2;

/// Encode a 32-bit unsigned integer as protobuf varint.
fn encode_varint(mut value: u32) -> Vec<u8> {
    let mut result = Vec::new();
    while value > 0x7F {
        result.push(((value & 0x7F) | 0x80) as u8);
        value >>= 7;
    }
    result.push((value & 0x7F) as u8);
    result
}

/// Build a length-delimited protobuf field.
/// Structure: <tag_byte><length_varint><data_bytes>
fn make_length_delimited(field_number: u8, data: &[u8]) -> Vec<u8> {
    let length_bytes = encode_varint(data.len() as u32);
    let mut field = vec![(field_number << 3) | WIRE_LENGTH_DELIMITED];
    field.extend(length_bytes);
    field.extend_from_slice(data);
    field
}

// =============================================================================
// Public API
// =============================================================================

/// Generate universal SOCS cookie compatible with BOTH Flights and Hotels.
///
/// Uses Hotels/Browser-style format (required by Hotels, accepted by Flights):
/// - Tag 2 (length-delimited): Server product ID + "en" locale
/// - Tag 3 (length-delimited): Binary blob (default stable bytes)
///
/// ## Returns
///
/// Base64-encoded SOCS value (without "SOCS=" prefix)
fn generate_socs_cookie() -> String {
    let yesterday = Local::now().date_naive().pred_opt().unwrap_or(Local::now().date_naive());
    let server_tag = format!("boq_identityfrontenduiserver_{}{:02}{:02}.03_p0en", yesterday.year(), yesterday.month(), yesterday.day());
    let tag2 = make_length_delimited(2, server_tag.as_bytes());
    let tag3 = make_length_delimited(3, DEFAULT_BINARY_BLOB);

    let protobuf = [tag2, tag3].concat();
    STANDARD.encode(&protobuf)
}

/// Generate complete cookie header with CONSENT+SOCs pair.
///
/// ## Returns
///
/// Complete header: "CONSENT=PENDING+987;<base64>"
pub fn generate_cookie_header() -> String {
    let socs = generate_socs_cookie();
    format!("CONSENT=PENDING+987; {}", socs)
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn produces_well_formed_protobuf() {
        let socs = generate_socs_cookie();
        let decoded = STANDARD.decode(&socs).expect("valid base64");

        assert!(decoded.len() > 10, "too short: {} bytes", decoded.len());
        // First byte should be tag=2, wire type=2 = 0x12
        assert_eq!(decoded[0] & 0x07, 2, "first field must be length-delimited");
        assert_eq!(decoded[0] >> 3, 2, "first field must be tag=2");
    }

    #[test]
    fn header_format_correct() {
        let header = generate_cookie_header();

        assert!(header.starts_with("CONSENT=PENDING+987;"));

        // After CONSENT=, should be raw base64 SOCS value (no SOCS= prefix)
        if let Some(eq_pos) = header.find(';') {
            let value = header[eq_pos + 1..].trim();
            STANDARD.decode(value).expect("valid b64");
        }
    }

    #[test]
    fn any_protobuf_bytes_work() {
        let socs = generate_socs_cookie();
        let decoded = STANDARD.decode(&socs).expect("always decodes base64");

        assert!(
            decoded.len() > 5,
            "default blob produced too-short protobuf"
        );
    }
}
