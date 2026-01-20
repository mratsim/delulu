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

//! Fixture-based TFS encoding tests.
//!
//! Loads test vectors from `tfs_vectors.json` and compares Rust encoder output
//! byte-for-byte with expected encoding.
//!
//! Run with:
//!     cargo test --test t_flights_encoding_fixtures

use std::path::Path;

use base64::{Engine as _, engine::general_purpose::STANDARD};
use chrono::NaiveDate;
use serde::Deserialize;

use delulu_travel_agent::{FlightSearchParams, Passenger, Seat, Trip};

/// Test vector case from JSON file.
#[derive(Deserialize, Debug)]
struct TestCase {
    name: String,
    input: serde_json::Value,
    expected_tfs: String,
}

/// JSON file root.
#[derive(Deserialize, Debug)]
struct VectorsFile {
    total_cases: usize,
    cases: Vec<TestCase>,
}

/// Parse input JSON into FlightSearchParams.
fn params_from_json(json: &serde_json::Value) -> Result<FlightSearchParams, String> {
    let obj = json.as_object().ok_or("input must be an object")?;

    let from = obj
        .get("from_airport")
        .and_then(|v| v.as_str())
        .ok_or("missing from_airport")?
        .to_uppercase();
    let to = obj
        .get("to_airport")
        .and_then(|v| v.as_str())
        .ok_or("missing to_airport")?
        .to_uppercase();

    let date_str = obj
        .get("depart_date")
        .and_then(|v| v.as_str())
        .ok_or("missing depart_date")?;
    let parts: Vec<i32> = date_str
        .split('-')
        .map(|s| s.parse().map_err(|_| s.to_string()))
        .collect::<Result<_, _>>()
        .map_err(|s| format!("invalid date part: {}", s))?;
    if parts.len() != 3 {
        return Err(format!("invalid date format: {}", date_str));
    }
    let date = NaiveDate::from_ymd_opt(parts[0], parts[1] as u32, parts[2] as u32)
        .ok_or_else(|| format!("invalid date: {}", date_str))?;

    let cabin = match obj.get("cabin_class").and_then(|v| v.as_str()) {
        Some("economy") => Seat::Economy,
        Some("premium-economy") => Seat::PremiumEconomy,
        Some("business") => Seat::Business,
        Some("first") => Seat::First,
        Some(s) => return Err(format!("unknown cabin_class: {}", s)),
        None => return Err("missing cabin_class".into()),
    };

    let trip = match obj.get("trip_type").and_then(|v| v.as_str()) {
        Some("one-way") => Trip::OneWay,
        Some("round-trip") => Trip::RoundTrip,
        Some("multi-city") => Trip::MultiCity,
        Some(s) => return Err(format!("unknown trip_type: {}", s)),
        None => return Err("missing trip_type".into()),
    };

    let passengers_obj = obj.get("passengers").and_then(|v| v.as_object());
    let adults = passengers_obj
        .and_then(|v| v.get("adults").and_then(|v| v.as_u64()))
        .unwrap_or(0) as u32;
    let children = passengers_obj
        .and_then(|v| v.get("children").and_then(|v| v.as_u64()))
        .unwrap_or(0) as u32;
    let infants_in_seat = passengers_obj
        .and_then(|v| v.get("infants_in_seat").and_then(|v| v.as_u64()))
        .unwrap_or(0) as u32;
    let infants_on_lap = passengers_obj
        .and_then(|v| v.get("infants_on_lap").and_then(|v| v.as_u64()))
        .unwrap_or(0) as u32;

    let mut passengers = Vec::new();
    if adults > 0 {
        passengers.push((Passenger::Adult, adults));
    }
    if children > 0 {
        passengers.push((Passenger::Child, children));
    }
    if infants_in_seat > 0 {
        passengers.push((Passenger::InfantInSeat, infants_in_seat));
    }
    if infants_on_lap > 0 {
        passengers.push((Passenger::InfantOnLap, infants_on_lap));
    }

    let max_stops = obj
        .get("max_stops")
        .and_then(|v| v.as_i64())
        .map(|v| v as i32);

    let builder = FlightSearchParams::builder(from, to, date)
        .cabin_class(cabin)
        .passengers(passengers)
        .trip_type(trip);
    let params = builder
        .max_stops(max_stops)
        .build()
        .map_err(|e| e.to_string())?;
    Ok(params)
}

/// Decode base64 string.
fn decode_b64(s: &str) -> Result<Vec<u8>, String> {
    STANDARD
        .decode(s)
        .map_err(|e| format!("base64 error: {}", e))
}

/// Verify the Rust encoder produces byte-exact matching protobuf.
#[test]
fn test_encoding_vectors() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let vectors_path = manifest_dir.join("tests/fixtures-flights-encoding/tfs_vectors.json");

    let content = std::fs::read_to_string(&vectors_path)
        .unwrap_or_else(|e| panic!("failed to read {}: {}", vectors_path.display(), e));

    let vectors: VectorsFile =
        serde_json::from_str(&content).unwrap_or_else(|e| panic!("failed to parse JSON: {}", e));

    println!("Loaded {} test vectors", vectors.total_cases);
    println!("{}", "=".repeat(70));

    let mut passed = 0usize;
    let mut failed = 0usize;

    for (i, case) in vectors.cases.iter().enumerate() {
        println!("[{:2}/{:2}] {}", i + 1, vectors.cases.len(), case.name);

        let params = match params_from_json(&case.input) {
            Ok(c) => c,
            Err(e) => {
                println!("  ✗ PARSE ERROR: {}", e);
                failed += 1;
                continue;
            }
        };

        let rust_out: String = match params.generate_tfs() {
            Ok(b) => b,
            Err(e) => {
                println!("  ✗ ENCODE ERROR: {:?}", e);
                failed += 1;
                continue;
            }
        };

        if rust_out.is_empty() {
            println!("  ✗ EMPTY OUTPUT");
            failed += 1;
            continue;
        }

        match decode_b64(&case.expected_tfs) {
            Ok(expected) => {
                let expected_b64 = STANDARD.encode(&expected);
                if rust_out == expected_b64 {
                    println!("  ✓ BYTE-EXACT MATCH ({} bytes)", rust_out.len());
                    passed += 1;
                } else {
                    println!(
                        "  ✗ SIZE MISMATCH: Rust {} vs Expected {}",
                        rust_out.len(),
                        expected_b64.len()
                    );
                    failed += 1;
                }
            }
            Err(e) => {
                println!("  ✗ REFERENCE UNREADABLE: {}", e);
                failed += 1;
            }
        };
    }

    println!("\n{}", "=".repeat(70));
    println!("RESULTS: {} passed, {} failed", passed, failed);

    if failed > 0 {
        panic!("{} tests failed", failed);
    }
}
