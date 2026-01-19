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

use delulu_travel_agent::{FlightSearchParams, FlightSearchResult, Seat};

/// Fixture structure describing expected properties of parsed results.
struct FixtureTestCase {
    /// Filename in tests/fixtures/ directory (without .zst extension)
    name: &'static str,
    /// Expected minimum number of itineraries to extract
    min_itineraries: usize,
    /// Whether this fixture is expected to have flights with prices
    has_prices: bool,
    /// Description of what this fixture covers
    description: &'static str,
    /// Origin airport code for this fixture (for test params)
    from_airport: &'static str,
    /// Destination airport code for this fixture (for test params)
    to_airport: &'static str,
}

/// Test cases covering different HTML structure variations.
/// Each case exercises different parsing paths in the scraper selectors.
const FIXTURE_TESTS: &[FixtureTestCase] = &[
    FixtureTestCase {
        name: "domestic+business-lax_ord",
        min_itineraries: 5,
        has_prices: true,
        description: "Domestic US business class - short haul, typical domestic layout",
        from_airport: "LAX",
        to_airport: "ORD",
    },
    FixtureTestCase {
        name: "nonstop-sfo_jfk_economy",
        min_itineraries: 5,
        has_prices: true,
        description: "Transcontinental economy with nonstop options visible",
        from_airport: "SFO",
        to_airport: "JFK",
    },
    FixtureTestCase {
        name: "overnight+1day-sfo_lhr_economy",
        min_itineraries: 5,
        has_prices: true,
        description: "International long-haul with +1 day arrival markers",
        from_airport: "SFO",
        to_airport: "LHR",
    },
    FixtureTestCase {
        name: "layover-mad_nrt",
        min_itineraries: 5,
        has_prices: true,
        description: "Europe to Asia with multiple layovers (1-2 stops)",
        from_airport: "MAD",
        to_airport: "NRT",
    },
    FixtureTestCase {
        name: "longhaul-lax_syd",
        min_itineraries: 3,
        has_prices: true,
        description: "Trans-Pacific ultra-long-haul routes",
        from_airport: "LAX",
        to_airport: "SYD",
    },
    FixtureTestCase {
        name: "layover-yyz_cdg",
        min_itineraries: 5,
        has_prices: true,
        description: "Toronto to Paris with potential Montréal layover",
        from_airport: "YYZ",
        to_airport: "CDG",
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
    let fixtures_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures-flights-parsing");
    let fixture_path = fixtures_dir.join(format!("{}.html.zst", name));

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
        let params = FlightSearchParams::builder(
            case.from_airport.into(),
            case.to_airport.into(),
            chrono::NaiveDate::from_ymd_opt(2024, 1, 15).unwrap(),
        )
        .cabin_class(Seat::Economy)
        .build()
        .unwrap();
        let result = FlightSearchResult::from_html(&html, params);

        match result {
            Ok(parsed) => {
                let itinerary_count = parsed.search_flights.results.len();

                assert!(
                    itinerary_count >= case.min_itineraries,
                    "{}: Expected ≥{} itineraries, got {}",
                    case.name,
                    case.min_itineraries,
                    itinerary_count
                );

                // Verify prices consistency
                let has_prices = parsed.search_flights.results.iter().any(|i| i.price.is_some());
                assert_eq!(
                    has_prices, case.has_prices,
                    "{}: has_prices mismatch (expected {}, got {})",
                    case.name, case.has_prices, has_prices
                );

                // Spot-check a few itineraries have reasonable data
                if let Some(first) = parsed.search_flights.results.first() {
                    if let Some(seg) = first.flights.first() {
                        assert!(
                            seg.airline.is_some() && !seg.airline.as_ref().unwrap().is_empty(),
                            "{}: First itinerary has empty airline",
                            case.name
                        );
                        assert!(
                            seg.departure_time.is_some()
                                && !seg.departure_time.as_ref().unwrap().is_empty(),
                            "{}: First itinerary has empty departure_time",
                            case.name
                        );
                    }
                }

                println!("✓ {}: {} itineraries extracted", case.name, itinerary_count);

                results.push((case.name.to_string(), true, itinerary_count));
            }
            Err(e) => {
                println!("✗ {}: PARSE FAILED: {}", case.name, e);
                results.push((case.name.to_string(), false, 0));
                panic!("{}: Failed to parse fixture: {}", case.name, e);
            }
        }
    }

    println!("\n{}", "=".repeat(60));
    println!("FIXTURE TEST SUMMARY");
    println!("{}", "=".repeat(60));

    let passed: usize = results.iter().filter(|(_, ok, _)| *ok).count();
    let total = results.len();

    for (name, ok, count) in &results {
        let status = if *ok { "✓" } else { "✗" };
        println!("{} {:35} {} itineraries", status, name, count);
    }

    println!("\nTotal: {}/{} passed", passed, total);

    assert_eq!(
        passed, total,
        "Some fixture tests failed - parser selectors may be outdated"
    );
}

/// Individual fixture tests for faster iteration during development.
#[test]
fn test_nonstop_sfo_jfk_economy() {
    let html = load_fixture("nonstop-sfo_jfk_economy");
    let params = FlightSearchParams::builder(
        "SFO".into(),
        "JFK".into(),
        chrono::NaiveDate::from_ymd_opt(2024, 1, 15).unwrap(),
    )
    .cabin_class(Seat::Economy)
    .build()
    .unwrap();
    let result = FlightSearchResult::from_html(&html, params).expect("parse fixture");

    assert!(
        result.search_flights.results.len() >= 5,
        "Expected several nonstop itineraries"
    );

    let has_airlines = result.search_flights.results.iter().any(|i| {
        i.flights
            .first()
            .map(|s| s.airline.is_some() && !s.airline.as_ref().unwrap().is_empty())
            .unwrap_or(false)
    });

    assert!(has_airlines, "Should extract at least one airline");
    println!(
        "Extracted {} itineraries from nonstop-heavy route",
        result.search_flights.results.len()
    );
}

#[test]
fn test_overnight_sfo_lhr_economy() {
    let html = load_fixture("overnight+1day-sfo_lhr_economy");
    let params = FlightSearchParams::builder(
        "SFO".into(),
        "LHR".into(),
        chrono::NaiveDate::from_ymd_opt(2024, 1, 15).unwrap(),
    )
    .cabin_class(Seat::Economy)
    .build()
    .unwrap();
    let result = FlightSearchResult::from_html(&html, params).expect("parse fixture");

    assert!(
        result.search_flights.results.len() >= 3,
        "Should have multiple long-haul options"
    );

    let overnight_itineraries: Vec<_> = result
        .search_flights.results
        .iter()
        .filter(|i| {
            i.flights
                .first()
                .map(|s| s.arrival_plus_days.is_some() && s.arrival_plus_days.unwrap() > 0)
                .unwrap_or(false)
        })
        .collect();

    println!(
        "Found {} overnight itineraries with +1 markers",
        overnight_itineraries.len()
    );
    assert!(
        !overnight_itineraries.is_empty(),
        "Expected some overnight/+1 day arrivals"
    );
}

#[test]
fn test_layover_mad_nrt() {
    let html = load_fixture("layover-mad_nrt");
    let params = FlightSearchParams::builder(
        "MAD".into(),
        "NRT".into(),
        chrono::NaiveDate::from_ymd_opt(2024, 1, 15).unwrap(),
    )
    .cabin_class(Seat::Economy)
    .build()
    .unwrap();
    let result = FlightSearchResult::from_html(&html, params).expect("parse fixture");

    assert!(
        result.search_flights.results.len() >= 3,
        "Should have multi-leg options"
    );

    let multi_stop = result
        .search_flights.results
        .iter()
        .filter(|i| i.stops.map(|s| s > 1).unwrap_or(false))
        .count();

    println!("Found {} multi-stop itineraries via Madrid", multi_stop);
    assert!(multi_stop > 0, "Should have some 2+ stop options");
}

#[test]
fn test_layover_doha_parsing() {
    let html = load_fixture("layover-mad_nrt");
    let params = FlightSearchParams::builder(
        "MAD".into(),
        "NRT".into(),
        chrono::NaiveDate::from_ymd_opt(2024, 1, 15).unwrap(),
    )
    .cabin_class(Seat::Economy)
    .build()
    .unwrap();
    let result = FlightSearchResult::from_html(&html, params).expect("parse fixture");

    let doha_layovers: Vec<_> = result
        .search_flights.results
        .iter()
        .filter(|i| {
            i.layovers
                .iter()
                .any(|l| l.airport_city.as_ref().is_some_and(|n| n.contains("Doha")))
        })
        .collect();

    println!(
        "Found {} itineraries with Doha layover",
        doha_layovers.len()
    );
    assert!(
        !doha_layovers.is_empty(),
        "Should have itineraries with Doha layover"
    );

    for itinerary in &doha_layovers {
        let doha = itinerary
            .layovers
            .iter()
            .find(|l| l.airport_city.as_ref().is_some_and(|n| n.contains("Doha")))
            .unwrap();
        println!(
            "Doha layover: {} - duration: {:?} min",
            doha.airport_city.as_deref().unwrap_or("Unknown"),
            doha.duration_minutes
        );
        assert!(
            doha.duration_minutes.is_some() && doha.duration_minutes.unwrap() > 0,
            "Doha layover should have positive duration"
        );
    }
}

#[test]
fn test_longhaul_lax_syd() {
    let html = load_fixture("longhaul-lax_syd");
    let params = FlightSearchParams::builder(
        "LAX".into(),
        "SYD".into(),
        chrono::NaiveDate::from_ymd_opt(2024, 1, 15).unwrap(),
    )
    .cabin_class(Seat::Economy)
    .build()
    .unwrap();
    let result = FlightSearchResult::from_html(&html, params).expect("parse fixture");

    assert!(
        result.search_flights.results.len() >= 2,
        "Should have ultra long-haul options"
    );

    let long_duration = result
        .search_flights.results
        .iter()
        .filter(|i| i.duration_minutes.map(|d| d > 900).unwrap_or(false))
        .count();

    println!(
        "Found {} ultra long-haul itineraries (>15h) LAX->SYD",
        long_duration
    );
    assert!(long_duration > 0, "Should have very long flights");
}

#[test]
fn test_layover_yyz_cdg() {
    let html = load_fixture("layover-yyz_cdg");
    let params = FlightSearchParams::builder(
        "YYZ".into(),
        "CDG".into(),
        chrono::NaiveDate::from_ymd_opt(2024, 1, 15).unwrap(),
    )
    .cabin_class(Seat::Economy)
    .build()
    .unwrap();
    let result = FlightSearchResult::from_html(&html, params).expect("parse fixture");

    assert!(
        result.search_flights.results.len() >= 3,
        "Should have multi-leg options"
    );

    let multi_stop = result
        .search_flights.results
        .iter()
        .filter(|i| i.stops.map(|s| s > 1).unwrap_or(false))
        .count();

    println!(
        "Found {} multi-stop itineraries via Canada/Europe",
        multi_stop
    );
    assert!(multi_stop > 0, "Should have some 2+ stop options");

    let montreal_layovers: Vec<_> = result
        .search_flights.results
        .iter()
        .filter(|i| {
            i.layovers
                .iter()
                .any(|l| l.airport_city.as_ref().is_some_and(|n| n.contains("Montr")))
        })
        .collect();

    println!(
        "Found {} itineraries with Montréal layover",
        montreal_layovers.len()
    );
    if !montreal_layovers.is_empty() {
        for itinerary in &montreal_layovers {
            let montreal = itinerary
                .layovers
                .iter()
                .find(|l| l.airport_city.as_ref().is_some_and(|n| n.contains("Montr")))
                .unwrap();
            println!(
                "Montréal layover: {} - duration: {:?} min",
                montreal.airport_city.as_deref().unwrap_or("Unknown"),
                montreal.duration_minutes
            );
        }
    }
}
