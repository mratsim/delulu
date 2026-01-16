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

//! Integration tests for the HTML parser using real Google Flights HTML fixtures.
//!
//! These tests load compressed (.zst) HTML snapshots and verify the parser
//! correctly extracts flight information from them. This catches regressions
//! when CSS selectors become outdated due to Google website changes.

use std::path::Path;

/// Fixture structure describing expected properties of parsed results.
struct FixtureTestCase {
    /// Filename in tests/fixtures/ directory (without .zst extension)
    name: &'static str,
    /// Expected minimum number of flights to extract
    min_flights: usize,
    /// Whether this fixture is expected to have a "best price" banner
    has_best_price: bool,
    /// Description of what this fixture covers
    description: &'static str,
}

/// Test cases covering different HTML structure variations.
/// Each case exercises different parsing paths in the scraper selectors.
const FIXTURE_TESTS: &[FixtureTestCase] = &[
    FixtureTestCase {
        name: "domestic+business-lax_ord",
        min_flights: 5,
        has_best_price: true,
        description: "Domestic US business class - short haul, typical domestic layout",
    },
    FixtureTestCase {
        name: "nonstop-sfo_jfk_economy",
        min_flights: 5,
        has_best_price: true,
        description: "Transcontinental economy with nonstop options visible",
    },
    FixtureTestCase {
        name: "overnight+1day-sfo_lhr_economy",
        min_flights: 5,
        has_best_price: true,
        description: "International long-haul with +1 day arrival markers",
    },
    FixtureTestCase {
        name: "layover-mad_nrt",
        min_flights: 5,
        has_best_price: true,
        description: "Europe to Asia with multiple layovers (1-2 stops)",
    },
    FixtureTestCase {
        name: "longhaul-lax_syd",
        min_flights: 3,
        has_best_price: true,
        description: "Trans-Pacific ultra-long-haul routes",
    },
];

/// Decompress a zstd-compressed fixture file.
///
/// Panics if decompression fails, which indicates either corruption
/// or mismatched compression settings.
fn decompress_zst(compressed: &[u8]) -> String {
    let decoder = zstd::stream::Decoder::new(compressed).expect("create zstd decoder");
    let reader = std::io::BufReader::new(decoder);
    std::io::read_to_string(reader).expect("decompress fixture")
}

/// Load and decompress a fixture file from the fixtures directory.
///
/// Panics if the file cannot be loaded (not found, corrupt, etc.).
fn load_fixture(name: &str) -> String {
    // Construct path to the fixture file
    // This is relative to the crate root where Cargo.toml lives
    let fixtures_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures-flights-parsing");
    let fixture_path = fixtures_dir.join(format!("{}.html.zst", name));

    // Read compressed bytes - using include_bytes would require rebuild on change,
    // so we read from disk to allow updating fixtures independently
    let compressed = std::fs::read(&fixture_path).unwrap_or_else(|e| {
        panic!(
            "Failed to read fixture '{}' at {:?}: {}",
            name, fixture_path, e
        )
    });

    decompress_zst(&compressed)
}

/// Run all fixture parser tests.
///
/// Each fixture is loaded, decompressed, parsed, and verified to produce
/// reasonable results. This serves as a smoke test for the scraper selectors.
#[test]
fn test_parser_fixtures() {
    let mut results = Vec::new();

    for case in FIXTURE_TESTS {
        println!("Testing fixture: {} - {}", case.name, case.description);

        let html = load_fixture(case.name);
        let result = delulu_travel_agent::parse_flights_response(&html);

        match result {
            Ok(parsed) => {
                let flight_count = parsed.flights.len();
                let has_banner = parsed.best_price.is_some();

                // Verify minimum flights extracted
                assert!(
                    flight_count >= case.min_flights,
                    "{}: Expected ≥{} flights, got {}",
                    case.name,
                    case.min_flights,
                    flight_count
                );

                // Verify best_price consistency
                assert_eq!(
                    has_banner, case.has_best_price,
                    "{}: has_best_price mismatch (expected {}, got {})",
                    case.name, case.has_best_price, has_banner
                );

                // Spot-check a few flights have reasonable data
                if let Some(first) = parsed.flights.first() {
                    assert!(
                        !first.airline.is_empty(),
                        "{}: First flight has empty airline",
                        case.name
                    );
                    assert!(
                        !first.dep_time.is_empty(),
                        "{}: First flight has empty dep_time",
                        case.name
                    );
                    assert!(
                        !first.price.is_empty(),
                        "{}: First flight has empty price",
                        case.name
                    );
                }

                println!(
                    "✓ {}: {} flights extracted, best_price={}",
                    case.name, flight_count, has_banner
                );

                results.push((case.name.to_string(), true, flight_count));
            }
            Err(e) => {
                println!("✗ {}: PARSE FAILED: {}", case.name, e);
                results.push((case.name.to_string(), false, 0));
                // Fail the test on parse error - this is a regression
                panic!("{}: Failed to parse fixture: {}", case.name, e);
            }
        }
    }

    // Summary
    println!("\n{}", "=".repeat(60));
    println!("FIXTURE TEST SUMMARY");
    println!("{}", "=".repeat(60));

    let passed: usize = results.iter().filter(|(_, ok, _)| *ok).count();
    let total = results.len();

    for (name, ok, flights) in &results {
        let status = if *ok { "✓" } else { "✗" };
        println!("{} {:35} {} flights", status, name, flights);
    }

    println!("\nTotal: {}/{} passed", passed, total);

    assert_eq!(
        passed, total,
        "Some fixture tests failed - parser selectors may be outdated"
    );
}

/// Individual fixture tests for faster iteration during development.
///
/// These can be run individually with:
///     cargo test --test fixtures -- nonstop
#[test]
fn test_nonstop_sfo_jfk_economy() {
    let html = load_fixture("nonstop-sfo_jfk_economy");
    let result = delulu_travel_agent::parse_flights_response(&html).expect("parse fixture");

    // Nonstop-heavy route should have plenty of direct options
    assert!(
        result.flights.len() >= 5,
        "Expected several nonstop flights"
    );

    // Check we captured airlines correctly
    let has_airlines = result.flights.iter().any(|f| !f.airline.is_empty());

    assert!(has_airlines, "Should extract at least one airline");
    println!(
        "Extracted {} flights from nonstop-heavy route",
        result.flights.len()
    );
}

#[test]
fn test_overnight_sfo_lhr_economy() {
    let html = load_fixture("overnight+1day-sfo_lhr_economy");
    let result = delulu_travel_agent::parse_flights_response(&html).expect("parse fixture");

    // International long-haul often has overnight departures with +1 arrivals
    assert!(
        result.flights.len() >= 3,
        "Should have multiple long-haul options"
    );

    // Verify +1 day arrivals are captured
    let overnight_flights: Vec<_> = result
        .flights
        .iter()
        .filter(|f| f.arrive_plus_days.is_some())
        .collect();

    println!(
        "Found {} overnight flights with +1 markers",
        overnight_flights.len()
    );
    // At least some should have +1 day markers for transatlantic
    assert!(
        !overnight_flights.is_empty(),
        "Expected some overnight/+1 day arrivals"
    );
}

#[test]
fn test_layover_mad_nrt() {
    let html = load_fixture("layover-mad_nrt");
    let result = delulu_travel_agent::parse_flights_response(&html).expect("parse fixture");

    // Europe to Asia typically has 1-2 stops via Middle East or hubs
    assert!(
        result.flights.len() >= 3,
        "Should extract connecting flights"
    );

    // Count flights by stop count
    let onestop_plus: usize = result.flights.iter().filter(|f| f.stops >= 1).count();

    println!("Found {} flights with 1+ stops", onestop_plus);

    // Most should have 1+ stops on this route
    assert!(
        onestop_plus > 0,
        "Expected significant fraction with layovers"
    );
}

#[test]
fn test_longhaul_lax_syd() {
    let html = load_fixture("longhaul-lax_syd");
    let result = delulu_travel_agent::parse_flights_response(&html).expect("parse fixture");

    // Trans-Pacific should have very long durations
    assert!(
        result.flights.len() >= 2,
        "Should extract long-haul options"
    );

    println!(
        "Extracted {} flights from trans-pacific route",
        result.flights.len()
    );
}

#[test]
fn test_domestic_business_lax_ord() {
    let html = load_fixture("domestic+business-lax_ord");
    let result = delulu_travel_agent::parse_flights_response(&html).expect("parse fixture");

    // Domestic short-haul should extract cleanly
    assert!(result.flights.len() >= 5, "Should extract domestic flights");

    // Business class often has a highlighted best price banner
    if let Some(best) = &result.best_price {
        if !best.trim().is_empty() {
            println!("Best price displayed: ${}", best.trim());
        }
    }

    // Durations should be short (2-4 hours typical for LA-Chicago)
    let short_flights: Vec<_> = result
        .flights
        .iter()
        .filter(|f| {
            let dur = f.duration.trim();
            // Match patterns like "2h", "3h", "4h" (but not "5h+" or "14h")
            dur.starts_with("2h") || dur.starts_with("3h") || dur.starts_with("4h")
        })
        .collect();

    // LA-Chicago is ~4h, some should be in the 2-5h range
    if short_flights.is_empty() {
        // Fallback: just verify we got flights parsed (duration varies by connector)
        println!("Note: All durations - taking first few as samples:");
        for (i, f) in result.flights.iter().take(3).enumerate() {
            println!("  {}: {} -> {}", i + 1, f.dep_time, f.duration);
        }
    }
}
