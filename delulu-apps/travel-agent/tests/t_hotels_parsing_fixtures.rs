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

use std::path::Path;

use delulu_travel_agent::HotelSearchResult;

fn decompress_zst(compressed: &[u8]) -> String {
    let decoder = zstd::stream::Decoder::new(compressed).expect("create zstd decoder");
    let reader = std::io::BufReader::new(decoder);
    std::io::read_to_string(reader).expect("decompress fixture")
}

fn load_fixture(name: &str) -> String {
    let fixtures_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures-hotels-parsing");
    let fixture_path = fixtures_dir.join(format!("{}.html.zst", name));

    let compressed = std::fs::read(&fixture_path).unwrap_or_else(|e| {
        panic!(
            "Failed to read fixture '{}' at {:?}: {}\n\
             Run `cargo test --test t_hotels_integration_live fetch_fixtures -- --ignored --nocapture` first.",
            name, fixture_path, e
        )
    });

    decompress_zst(&compressed)
}

#[test]
fn test_parse_tokyo_standard() {
    let html = load_fixture("tokyo-standard");
    let result = HotelSearchResult::from_html(&html).expect("parse fixture");

    assert!(
        result.hotels.len() >= 5,
        "Expected at least 5 hotels, got {}",
        result.hotels.len()
    );

    for hotel in &result.hotels {
        assert!(!hotel.name.is_empty(), "Hotel name should not be empty");
        assert!(!hotel.price.is_empty(), "Hotel price should not be empty");
    }

    println!("Parsed {} hotels from Tokyo fixture", result.hotels.len());
    if let Some(lowest) = result.lowest_price {
        println!("Lowest price: ${:.2}", lowest);
    }
}

#[test]
fn test_parse_paris_budget() {
    let html = load_fixture("paris-budget");
    let result = HotelSearchResult::from_html(&html).expect("parse fixture");

    assert!(
        result.hotels.len() >= 3,
        "Expected at least 3 hotels, got {}",
        result.hotels.len()
    );

    for hotel in &result.hotels {
        assert!(!hotel.name.is_empty(), "Hotel name should not be empty");
    }

    println!("Parsed {} hotels from Paris budget fixture", result.hotels.len());
}

#[test]
fn test_parse_tokyo_5star() {
    let html = load_fixture("tokyo-5star");
    let result = HotelSearchResult::from_html(&html).expect("parse fixture");

    assert!(
        result.hotels.len() >= 3,
        "Expected at least 3 hotels, got {}",
        result.hotels.len()
    );

    println!("Parsed {} hotels from Tokyo 5-star fixture", result.hotels.len());
}

#[test]
fn test_parse_nyc_families() {
    let html = load_fixture("nyc-families");
    let result = HotelSearchResult::from_html(&html).expect("parse fixture");

    assert!(
        result.hotels.len() >= 3,
        "Expected at least 3 hotels, got {}",
        result.hotels.len()
    );

    println!("Parsed {} hotels from NYC families fixture", result.hotels.len());
}

#[test]
fn test_parse_london_long_stay() {
    let html = load_fixture("london-long-stay");
    let result = HotelSearchResult::from_html(&html).expect("parse fixture");

    assert!(
        result.hotels.len() >= 3,
        "Expected at least 3 hotels, got {}",
        result.hotels.len()
    );

    println!("Parsed {} hotels from London long stay fixture", result.hotels.len());
}
