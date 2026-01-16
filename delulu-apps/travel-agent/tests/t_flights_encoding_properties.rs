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
//! Tests various encoder properties without relying on fixtures:
//! - All cabin classes encode correctly
//! - Valid passenger combinations encode correctly
//! - Invalid passenger combinations are rejected
//!
//! Run with:
//!     cargo test --test t_flights_encoding_properties

use chrono::NaiveDate;

use delulu_travel_agent::{encode_tfs, CabinClass, FlightSearchConfig, PassengerType, TripType};

/// Basic sanity check.
#[test]
fn test_basic_encoding() {
    let config = FlightSearchConfig {
        from_airport: "LAX".into(),
        to_airport: "ORD".into(),
        depart_date: NaiveDate::from_ymd_opt(2025, 8, 1).unwrap(),
        cabin_class: CabinClass::Economy,
        passengers: vec![(PassengerType::Adult, 1)],
        trip_type: TripType::OneWay,
        max_stops: None,
        preferred_airlines: None,
    };
    let bytes = encode_tfs(&config).expect("basic encode should work");
    assert!(!bytes.is_empty(), "should produce bytes");
    assert!(bytes[0] == 0x1a, "should start with flightData tag");
    println!("Basic encoding: {} bytes - OK", bytes.len());
}

/// Verify all cabin classes encode without error.
#[test]
fn test_all_cabins() {
    let base = FlightSearchConfig {
        from_airport: "LAX".into(),
        to_airport: "ORD".into(),
        depart_date: NaiveDate::from_ymd_opt(2025, 8, 1).unwrap(),
        cabin_class: CabinClass::Economy,
        passengers: vec![(PassengerType::Adult, 1)],
        trip_type: TripType::OneWay,
        max_stops: None,
        preferred_airlines: None,
    };

    for cabin in [
        CabinClass::Economy,
        CabinClass::PremiumEconomy,
        CabinClass::Business,
        CabinClass::First,
    ] {
        let mut cfg = base.clone();
        cfg.cabin_class = cabin;

        let bytes = encode_tfs(&cfg).expect("encode");
        println!("{:?} -> {} bytes", cabin, bytes.len());
        assert!(!bytes.is_empty(), "{:?}: should produce bytes", cabin);
        assert!(bytes[0] == 0x1a, "{:?}: should have flightData tag", cabin);
    }
    println!("All cabin classes encode correctly - OK");
}

/// Test that passenger types requiring adult supervision are rejected without adult.
#[test]
fn test_passenger_types_require_adult() {
    let base = FlightSearchConfig {
        from_airport: "SFO".into(),
        to_airport: "JFK".into(),
        depart_date: NaiveDate::from_ymd_opt(2025, 7, 15).unwrap(),
        cabin_class: CabinClass::Economy,
        passengers: vec![],
        trip_type: TripType::OneWay,
        max_stops: None,
        preferred_airlines: None,
    };

    // Cases that require an adult - these should FAIL validation
    let requires_adult = vec![
        (PassengerType::Child, "Child alone"),
        (PassengerType::InfantInSeat, "Infant in seat alone"),
        (PassengerType::InfantOnLap, "Infant on lap alone"),
    ];

    for (ptype, name) in &requires_adult {
        let mut cfg = base.clone();
        cfg.passengers = vec![(*ptype, 1)];
        let result = encode_tfs(&cfg);
        assert!(
            result.is_err(),
            "{}: should be rejected without adult",
            name
        );
        println!("{} correctly rejected - OK", name);
    }

    // Case that works alone - Adult alone succeeds
    let mut adult_cfg = base.clone();
    adult_cfg.passengers = vec![(PassengerType::Adult, 1)];
    let bytes = encode_tfs(&adult_cfg).expect("Adult alone should encode");
    assert!(!bytes.is_empty(), "Adult alone: should produce bytes");
    assert!(bytes[0] == 0x1a, "Adult alone: should have flightData tag");
    println!("Adult alone encodes correctly - OK");
}

/// Test that infant-on-lap ratio limits are enforced.
#[test]
fn test_infant_on_lap_limit() {
    let config = FlightSearchConfig {
        from_airport: "SFO".into(),
        to_airport: "LHR".into(),
        depart_date: NaiveDate::from_ymd_opt(2025, 8, 20).unwrap(),
        cabin_class: CabinClass::Business,
        passengers: vec![
            (PassengerType::Adult, 1),
            (PassengerType::InfantOnLap, 2), // 2 infants, only 1 adult
        ],
        trip_type: TripType::RoundTrip,
        max_stops: None,
        preferred_airlines: None,
    };

    let result = encode_tfs(&config);
    assert!(
        result.is_err(),
        "2 infants on lap with 1 adult should be rejected"
    );
    println!("Infant-to-adult limit enforced - OK");
}

/// Test valid passenger combinations encode correctly.
#[test]
fn test_valid_passenger_combinations() {
    let config = FlightSearchConfig {
        from_airport: "SFO".into(),
        to_airport: "LHR".into(),
        depart_date: NaiveDate::from_ymd_opt(2025, 8, 20).unwrap(),
        cabin_class: CabinClass::Business,
        passengers: vec![
            (PassengerType::Adult, 2),
            (PassengerType::Child, 1),
            (PassengerType::InfantOnLap, 1),
        ],
        trip_type: TripType::RoundTrip,
        max_stops: None,
        preferred_airlines: None,
    };

    let bytes = encode_tfs(&config).expect("valid family config should encode");
    println!(
        "Family (2 adults + 1 child + 1 infant on lap) -> {} bytes",
        bytes.len()
    );
    assert!(
        bytes.len() > 35,
        "family config should have multiple passenger entries"
    );
    println!("Valid passenger combination encodes correctly - OK");
}
