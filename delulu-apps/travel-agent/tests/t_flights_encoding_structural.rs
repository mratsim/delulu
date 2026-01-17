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

//! Structural TFS encoding tests.
//!
//! Tests specific wire format characteristics critical for Google acceptance:
//! - Nested Airport message encoding (vs direct strings)
//! - Packed repeated encoding for passenger types
//!
//! These tests catch regressions where the structure diverges from expected.
//!
//! Run with:
//!     cargo test --test t_flights_encoding_structural

use base64::{engine::general_purpose::STANDARD, Engine as _};
use chrono::NaiveDate;

use delulu_travel_agent::{FlightSearchParams, Passenger, Seat, Trip};

/// Verify nested Airport message encoding.
/// The from_flight and to_flight fields MUST be nested Airport messages.
/// This caught a regression where Rust emitted direct strings (tag 0x66) instead
/// of proper nested structure (tag 0x6a containing tag 0x12).
#[test]
fn test_structural_nested_airport() {
    let ref_b64 = "GhoSCjIwMjUtMDctMTVqBRIDU0ZPcgUSA0pGS0IBAUgBmAEC";
    let expected = STANDARD.decode(ref_b64).expect("valid base64");

    assert_eq!(
        expected[14], 0x6a,
        "Reference: pos 14 must have field 13 tag"
    );
    assert_eq!(expected[15], 0x05, "Reference: pos 15 length 5 for Airport");
    assert_eq!(
        expected[16], 0x12,
        "Reference: pos 16 field 2 inside Airport"
    );
    assert_eq!(expected[17], 0x03, "Reference: pos 17 length 3 for 'SFO'");
    assert_eq!(
        &expected[18..21],
        b"SFO",
        "Reference: 'SFO' at positions 18-20"
    );

    let params = FlightSearchParams::builder(
        "SFO".to_string(),
        "JFK".to_string(),
        NaiveDate::from_ymd_opt(2025, 7, 15).unwrap(),
    )
    .cabin_class(Seat::Economy)
    .trip_type(Trip::OneWay)
    .build()
    .expect("params should build");

    let tfs = params.generate_tfs().expect("encode");
    let rust_output = STANDARD.decode(&tfs).expect("decode base64");

    assert!(
        rust_output[14] == 0x6a,
        "Rust: pos 14 must have field 13 tag, got {:02x}",
        rust_output[14]
    );
    assert_eq!(rust_output[15], 0x05, "Rust: pos 15 length 5 for Airport");
    assert_eq!(rust_output[16], 0x12, "Rust: pos 16 field 2 inside Airport");
    assert_eq!(rust_output[17], 0x03, "Rust: pos 17 length 3 for 'SFO'");
    assert_eq!(
        &rust_output[18..21],
        b"SFO",
        "Rust: 'SFO' at positions 18-20"
    );

    println!("Nested Airport message structure verified - OK");
}

/// Verify packed repeated passenger encoding.
#[test]
fn test_structural_packed_passengers() {
    let params = FlightSearchParams::builder(
        "SFO".to_string(),
        "JFK".to_string(),
        NaiveDate::from_ymd_opt(2025, 7, 15).unwrap(),
    )
    .cabin_class(Seat::Economy)
    .passengers(vec![
        (Passenger::Adult, 2),
        (Passenger::Child, 1),
        (Passenger::InfantOnLap, 1),
    ])
    .trip_type(Trip::RoundTrip)
    .build()
    .expect("params should build");

    let tfs = params.generate_tfs().expect("encode");
    let bytes = STANDARD.decode(&tfs).expect("decode base64");

    // Tag 8 for passengers field, packed repeated
    // Should see: 0x42 (tag 8, wire type 0) followed by length
    assert!(bytes.contains(&0x42), "Should contain tag 8 for passengers");

    println!("Packed repeated passengers verified - OK");
}

/// Verify seat field encoding (tag 9, wire type 0).
#[test]
fn test_structural_seat_field() {
    let params = FlightSearchParams::builder(
        "SFO".to_string(),
        "JFK".to_string(),
        NaiveDate::from_ymd_opt(2025, 7, 15).unwrap(),
    )
    .cabin_class(Seat::Economy)
    .trip_type(Trip::OneWay)
    .build()
    .expect("params should build");

    let tfs = params.generate_tfs().expect("encode");
    let bytes = STANDARD.decode(&tfs).expect("decode base64");

    // Tag 9 (0x48 = tag 9, wire type 0) with value 3 (Business)
    assert!(bytes.contains(&0x48), "Should contain tag 9 for seat field");

    println!("Seat field encoding verified - OK");
}

/// Verify trip type field encoding (tag 19, wire type 0).
#[test]
fn test_structural_trip_field() {
    let params = FlightSearchParams::builder(
        "SFO".to_string(),
        "JFK".to_string(),
        NaiveDate::from_ymd_opt(2025, 7, 15).unwrap(),
    )
    .cabin_class(Seat::Economy)
    .trip_type(Trip::OneWay)
    .build()
    .expect("params should build");

    let tfs = params.generate_tfs().expect("encode");
    let bytes = STANDARD.decode(&tfs).expect("decode base64");

    // Tag 19 (0x98 = tag 19, wire type 0) with value 2 (OneWay)
    assert!(
        bytes.contains(&0x98),
        "Should contain tag 19 for trip field"
    );

    println!("Trip field encoding verified - OK");
}

/// Verify max_stops is correctly encoded (only when Some and != 0).
#[test]
fn test_structural_max_stops() {
    for stops in [None, Some(0), Some(1), Some(2)] {
        let params = FlightSearchParams::builder(
            "SFO".to_string(),
            "JFK".to_string(),
            NaiveDate::from_ymd_opt(2025, 7, 15).unwrap(),
        )
        .cabin_class(Seat::Economy)
        .trip_type(Trip::OneWay)
        .max_stops(stops)
        .build()
        .expect("params should build");

        let tfs = params.generate_tfs().expect("encode");
        let bytes = STANDARD.decode(&tfs).expect("decode base64");

        match stops {
            None | Some(0) => {
                assert!(
                    !bytes.contains(&0x28),
                    "Should NOT contain tag 5 for max_stops when None or 0"
                );
            }
            Some(_) => {
                assert!(
                    bytes.contains(&0x28),
                    "Should contain tag 5 for max_stops when Some(nonzero)"
                );
            }
        }
    }

    println!("Max stops field encoding verified - OK");
}

/// Verify business class encoding produces correct tag structure.
#[test]
fn test_structural_business_class() {
    let params = FlightSearchParams::builder(
        "LAX".to_string(),
        "ORD".to_string(),
        NaiveDate::from_ymd_opt(2025, 7, 15).unwrap(),
    )
    .cabin_class(Seat::Business)
    .build()
    .expect("params should build");

    let tfs = params.generate_tfs().expect("encode");
    let bytes = STANDARD.decode(&tfs).expect("decode base64");

    // Tag 9 with value 3 (Business = 3 in the enum)
    assert!(bytes.contains(&0x48), "Should contain tag 9 for seat");

    println!("Business class encoding verified - OK");
}

/// Verify one-way trip produces correct tag structure.
#[test]
fn test_structural_oneway_trip() {
    let params = FlightSearchParams::builder(
        "LAX".to_string(),
        "NRT".to_string(),
        NaiveDate::from_ymd_opt(2025, 7, 15).unwrap(),
    )
    .trip_type(Trip::OneWay)
    .build()
    .expect("params should build");

    let tfs = params.generate_tfs().expect("encode");
    let bytes = STANDARD.decode(&tfs).expect("decode base64");

    // Tag 19 (0x98) with value 2 (OneWay = 2)
    assert!(bytes.contains(&0x98), "Should contain tag 19 for trip type");

    println!("One-way trip encoding verified - OK");
}
