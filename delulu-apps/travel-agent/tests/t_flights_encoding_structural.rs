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

use base64::engine::general_purpose::STANDARD;
use base64::Engine as _;
use chrono::NaiveDate;

use delulu_travel_agent::{encode_tfs, CabinClass, FlightSearchConfig, PassengerType, TripType};

/// Verify nested Airport message encoding.
/// The from_flight and to_flight fields MUST be nested Airport messages.
/// This caught a regression where Rust emitted direct strings (tag 0x66) instead
/// of proper nested structure (tag 0x6a containing tag 0x12).
#[test]
fn test_structural_nested_airport() {
    // Reference: nested Airport message structure at specific byte offsets
    let ref_b64 = "GhoSCjIwMjUtMDctMTVqBRIDU0ZPcgUSA0pGS0IBAUgBmAEC";
    let expected = STANDARD.decode(ref_b64).expect("valid base64");

    // Position 14: Field 13 tag (0x6a = field 13, wire type 2)
    // Position 15: Length of nested Airport message (5 bytes)
    // Position 16: Field 2 tag (0x12 = field 2, wire type 2)
    // Position 17: Length of airport code string (3 bytes)
    // Position 18-20: Airport code "SFO"
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

    // Verify Rust produces identical structure
    let config = FlightSearchConfig {
        from_airport: "SFO".into(),
        to_airport: "JFK".into(),
        depart_date: NaiveDate::from_ymd_opt(2025, 7, 15).unwrap(),
        cabin_class: CabinClass::Economy,
        passengers: vec![(PassengerType::Adult, 1)],
        trip_type: TripType::OneWay,
        max_stops: None,
        preferred_airlines: None,
    };

    let rust_output = encode_tfs(&config).expect("encode");

    assert!(
        rust_output.len() >= 21,
        "Output must have enough bytes for Airport encoding"
    );
    assert_eq!(rust_output[14], 0x6a, "Must emit tag 0x6a for from_flight");
    assert_eq!(rust_output[15], 0x05, "Nested Airport must be 5 bytes");
    assert_eq!(rust_output[16], 0x12, "Inside Airport, must emit tag 0x12");
    assert_eq!(rust_output[17], 0x03, "Airport code length must be 3");
    assert_eq!(
        &rust_output[18..21],
        b"SFO",
        "'SFO' must be inside nested Airport"
    );

    println!("✓ Nested Airport message structure verified");
}

/// Verify packed repeated encoding for passenger types.
#[test]
fn test_structural_packed_repeated_passengers() {
    // Reference with 2 adults packed: tag 0x42, len 2, [0x01, 0x01]
    let ref_b64 = "GhoSCjIwMjUtMDgtMTVqBRIDU0ZPcgUSA0pGS0ICAQFIAZgBAg==";
    let expected = STANDARD.decode(ref_b64).expect("valid base64");

    // Position 28: Field 8 tag (0x42 = field 8, wire type 2 for packed)
    // Position 29: Length of packed content (2 bytes for 2 adults)
    // Positions 30-31: Packed varint values (both 0x01 = Adult)
    assert_eq!(expected[28], 0x42, "Reference: pos 28 must use tag 0x42");
    assert_eq!(
        expected[29], 0x02,
        "Reference: pos 29 length 2 for 2 adults"
    );
    assert_eq!(expected[30], 0x01, "Reference: pos 30 first adult (1)");
    assert_eq!(expected[31], 0x01, "Reference: pos 31 second adult (1)");

    // Verify Rust produces packed format
    let config = FlightSearchConfig {
        from_airport: "SFO".into(),
        to_airport: "JFK".into(),
        depart_date: NaiveDate::from_ymd_opt(2025, 7, 15).unwrap(),
        cabin_class: CabinClass::Economy,
        passengers: vec![(PassengerType::Adult, 2)],
        trip_type: TripType::OneWay,
        max_stops: None,
        preferred_airlines: None,
    };

    let rust_output = encode_tfs(&config).expect("encode");

    assert!(
        rust_output.len() >= 32,
        "Output must have enough bytes for packed"
    );
    assert_eq!(
        rust_output[28], 0x42,
        "Must use packed tag 0x42 for field 8"
    );
    assert_eq!(rust_output[29], 0x02, "Packed length must be 2 bytes");
    assert_eq!(rust_output[30], 0x01, "First passenger must be Adult (1)");
    assert_eq!(rust_output[31], 0x01, "Second passenger must be Adult (1)");

    println!("✓ Packed repeated passenger encoding verified");
}

/// Reference test: single adult, economy, one-way.
#[test]
fn test_ref_single_adult_economy_oneway() {
    let config = FlightSearchConfig {
        from_airport: "SFO".into(),
        to_airport: "JFK".into(),
        depart_date: NaiveDate::from_ymd_opt(2025, 7, 15).unwrap(),
        cabin_class: CabinClass::Economy,
        passengers: vec![(PassengerType::Adult, 1)],
        trip_type: TripType::OneWay,
        max_stops: None,
        preferred_airlines: None,
    };

    let rust_bytes = encode_tfs(&config).expect("encode");
    let expected_b64 = "GhoSCjIwMjUtMDctMTVqBRIDU0ZPcgUSA0pGS0IBAUgBmAEC";
    let expected = STANDARD.decode(expected_b64).expect("valid base64");

    assert_eq!(
        rust_bytes, expected,
        "Single adult economy one-way is still encoded as a repeated field"
    );
    println!("✓ Single adult economy one-way is still encoded as a repeated field");
}

/// Reference test: single adult, business, one-way.
#[test]
fn test_ref_single_adult_business_oneway() {
    let config = FlightSearchConfig {
        from_airport: "SFO".into(),
        to_airport: "JFK".into(),
        depart_date: NaiveDate::from_ymd_opt(2025, 7, 15).unwrap(),
        cabin_class: CabinClass::Business,
        passengers: vec![(PassengerType::Adult, 1)],
        trip_type: TripType::OneWay,
        max_stops: None,
        preferred_airlines: None,
    };

    let rust_bytes = encode_tfs(&config).expect("encode");
    let expected_b64 = "GhoSCjIwMjUtMDctMTVqBRIDU0ZPcgUSA0pGS0IBAUgDmAEC";
    let expected = STANDARD.decode(expected_b64).expect("valid base64");

    assert_eq!(
        rust_bytes, expected,
        "Single adult business one-way is still encoded as a repeated field"
    );
    println!("✓ Single adult business one-way is still encoded as a repeated field");
}

/// Reference test: two adults + one child.
#[test]
fn test_ref_two_adults_one_child() {
    let config = FlightSearchConfig {
        from_airport: "SFO".into(),
        to_airport: "JFK".into(),
        depart_date: NaiveDate::from_ymd_opt(2025, 7, 15).unwrap(),
        cabin_class: CabinClass::Economy,
        passengers: vec![(PassengerType::Adult, 2), (PassengerType::Child, 1)],
        trip_type: TripType::OneWay,
        max_stops: None,
        preferred_airlines: None,
    };

    let rust_bytes = encode_tfs(&config).expect("encode");
    let expected_b64 = "GhoSCjIwMjUtMDctMTVqBRIDU0ZPcgUSA0pGS0IDAQECSAGYAQI=";
    let expected = STANDARD.decode(expected_b64).expect("valid base64");

    assert_eq!(
        rust_bytes, expected,
        "Two adults + one child is still encoded as a repeated field"
    );
    println!("✓ Two adults + one child is still encoded as a repeated field");
}
