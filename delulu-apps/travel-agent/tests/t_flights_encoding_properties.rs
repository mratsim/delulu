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

//! Property-based TFS encoding tests.
//!
//! Tests that encoding produces valid output for various parameter combinations:
//! - All cabin classes encode correctly
//! - Valid passenger combinations encode correctly
//! - Invalid passenger combinations are rejected
//!
//! Run with:
//!     cargo test --test t_flights_encoding_properties

use base64::Engine;
use chrono::NaiveDate;

use delulu_travel_agent::{FlightSearchParams, Passenger, Seat, Trip};

/// Basic sanity check.
#[test]
fn test_basic_encoding() {
    let params = FlightSearchParams::builder(
        "LAX".to_string(),
        "ORD".to_string(),
        NaiveDate::from_ymd_opt(2025, 8, 1).unwrap(),
    )
    .cabin_class(Seat::Economy)
    .trip_type(Trip::OneWay)
    .build()
    .expect("basic params should build");
    let tfs = params.generate_tfs().expect("basic encode should work");
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(&tfs)
        .expect("should decode base64");
    assert!(!bytes.is_empty(), "should produce bytes");
    assert!(bytes[0] == 0x1a, "should start with flightData tag");
    println!("Basic encoding: {} bytes - OK", bytes.len());
}

/// Verify all cabin classes encode without error.
#[test]
fn test_all_cabins() {
    let base_params = FlightSearchParams::builder(
        "LAX".to_string(),
        "ORD".to_string(),
        NaiveDate::from_ymd_opt(2025, 8, 1).unwrap(),
    )
    .cabin_class(Seat::Economy)
    .trip_type(Trip::OneWay);

    for cabin in [
        Seat::Economy,
        Seat::PremiumEconomy,
        Seat::Business,
        Seat::First,
    ] {
        let params = base_params
            .clone()
            .cabin_class(cabin)
            .build()
            .expect("encode");
        let tfs = params.generate_tfs().expect("encode");
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(&tfs)
            .expect("decode");
        println!("{:?} -> {} bytes", cabin, bytes.len());
        assert!(!bytes.is_empty(), "{:?}: should produce bytes", cabin);
        assert!(bytes[0] == 0x1a, "{:?}: should have flightData tag", cabin);
    }
    println!("All cabin classes encode correctly - OK");
}

/// Test that passenger types requiring adult supervision are rejected without adult.
#[test]
fn test_passenger_types_require_adult() {
    let base_params = FlightSearchParams::builder(
        "SFO".to_string(),
        "JFK".to_string(),
        NaiveDate::from_ymd_opt(2025, 7, 15).unwrap(),
    )
    .cabin_class(Seat::Economy)
    .trip_type(Trip::OneWay);

    // Cases that require an adult - these should FAIL validation
    let requires_adult = vec![
        (Passenger::Child, "Child alone"),
        (Passenger::InfantInSeat, "Infant in seat alone"),
        (Passenger::InfantOnLap, "Infant on lap alone"),
    ];

    for (ptype, desc) in requires_adult {
        let params = base_params.clone().passengers(vec![(ptype, 1)]).build();

        assert!(
            params.is_err(),
            "{} should fail validation without adult",
            desc
        );
        println!("{:?} correctly rejected - OK", ptype);
    }

    // Case with adult present - should SUCCEED
    let params = base_params
        .clone()
        .passengers(vec![(Passenger::Adult, 1), (Passenger::Child, 1)])
        .build()
        .expect("adult + child should be valid");
    let tfs = params.generate_tfs().expect("should encode");
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(&tfs)
        .expect("decode");
    assert!(!bytes.is_empty(), "adult + child should encode");
    println!("Adult + child correctly accepted - OK");

    println!("Passenger type validation - OK");
}

/// Test that empty passenger list fails validation.
#[test]
fn test_empty_passengers_fails() {
    let params = FlightSearchParams::builder(
        "SFO".to_string(),
        "JFK".to_string(),
        NaiveDate::from_ymd_opt(2025, 7, 15).unwrap(),
    )
    .cabin_class(Seat::Economy)
    .trip_type(Trip::OneWay)
    .passengers(vec![])
    .build();

    assert!(params.is_err(), "empty passengers should fail validation");
    println!("Empty passengers correctly rejected - OK");
}

/// Test that max_stops is correctly encoded.
#[test]
fn test_max_stops_encoding() {
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

        let tfs = params.generate_tfs().expect("should encode");
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(&tfs)
            .expect("decode");
        assert!(!bytes.is_empty(), "max_stops={:?} should encode", stops);
        println!("max_stops={:?} -> {} bytes - OK", stops, bytes.len());
    }
}

/// Test that preferred_airlines is correctly encoded.
#[test]
fn test_airlines_encoding() {
    let params = FlightSearchParams::builder(
        "SFO".to_string(),
        "JFK".to_string(),
        NaiveDate::from_ymd_opt(2025, 7, 15).unwrap(),
    )
    .cabin_class(Seat::Economy)
    .trip_type(Trip::OneWay)
    .preferred_airlines(Some(vec![
        "AA".to_string(),
        "UA".to_string(),
        "DL".to_string(),
    ]))
    .build()
    .expect("params should build");

    let tfs = params.generate_tfs().expect("should encode");
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(&tfs)
        .expect("decode");
    assert!(!bytes.is_empty(), "airlines should encode");
    assert!(bytes[0] == 0x1a, "should have flightData tag");
    println!("Multiple airlines encoded - OK");
}
