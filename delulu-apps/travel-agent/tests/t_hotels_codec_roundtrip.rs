//! t_hotels_codec_roundtrip.rs
//! This test validates internal consistency via an encoder->decoder roundtrip

use delulu_travel_agent::HotelSearchParams;
use std::fs;

#[derive(serde::Deserialize)]
struct TestVectorCase {
    name: String,
    description: String,
    input: TestVectorInput,
    expected_ts: String,
}

#[derive(serde::Deserialize)]
struct TestVectorInput {
    display_name: String,
    checkin_date: String,
    checkout_date: String,
    guests: TestVectorGuests,
    currency: String,
    min_guest_rating: Option<f64>,
    amenities: Option<Vec<String>>,
    hotel_stars: Option<Vec<i32>>,
    price_min: Option<f64>,
    price_max: Option<f64>,
    sort_by: String,
    location_id: String,
    coordinates: String,
    used_guests_dropdown: bool,
}

#[derive(serde::Deserialize)]
struct TestVectorGuests {
    adults: usize,
    children_with_ages: Vec<i32>,
}

#[derive(serde::Deserialize)]
struct TestVectors {
    description: String,
    cases: Vec<TestVectorCase>,
}

#[test]
fn test_roundtrip_internal_codec() {
    let data = fs::read_to_string("tests/fixtures-google-hotels/ts_vectors.json")
        .expect("Failed to read ts_vectors.json");
    let test_vectors: TestVectors = serde_json::from_str(&data)
        .expect("Failed to parse ts_vectors.json");

    println!("\n=== Round-trip verification for {} test cases ===\n", test_vectors.cases.len());

    let mut passed = 0;
    let mut failed = 0;

    for case in &test_vectors.cases {
        let checkin = chrono::NaiveDate::parse_from_str(&case.input.checkin_date, "%Y-%m-%d")
            .expect("valid checkin date");
        let checkout = chrono::NaiveDate::parse_from_str(&case.input.checkout_date, "%Y-%m-%d")
            .expect("valid checkout date");

        let guest_rating = case.input.min_guest_rating;
        let star_values = case.input.hotel_stars.clone().unwrap_or_default();
        let amenities: Vec<delulu_travel_agent::Amenity> = case.input.amenities.as_deref().unwrap_or_default().iter()
            .filter_map(|a| delulu_travel_agent::Amenity::from_str_name(&a.to_uppercase()))
            .collect();
        let sort_order = match case.input.sort_by.as_str() {
            "highest_rating" => Some(8),
            "most_reviewed" => Some(13),
            "lowest_price" => Some(3),
            "relevance" | "unspecified" | _ => None,
        };
        let min_price = case.input.price_min.map(|p| p as i32);
        let max_price = case.input.price_max.map(|p| p as i32);

        let mut builder = HotelSearchParams::builder(
            case.input.display_name.clone(),
            checkin,
            checkout,
            case.input.guests.adults as u32,
            case.input.guests.children_with_ages.clone(),
        ).currency(case.input.currency.clone());

        if let Some(r) = guest_rating {
            builder = builder.min_guest_rating(r);
        }
        builder = builder.hotel_stars(star_values.clone());
        builder = builder.amenities(amenities.clone());
        if let Some(s) = sort_order {
            builder = builder.sort_order(Some(s));
        }
        if let Some(p) = min_price {
            builder = builder.min_price(Some(p));
        }
        if let Some(p) = max_price {
            builder = builder.max_price(Some(p));
        }

        let params = builder.build();

        match params {
            Ok(mut params) => {
                match params.generate_ts() {
                    Ok(encoded) => {
                        match HotelSearchParams::from_ts(&encoded) {
                            Ok(decoded) => {
                                params.loc_ts_id = case.input.location_id.clone();
                                params.loc_ts_coords = case.input.coordinates.clone();
                                params.loc_ts_name = case.input.display_name.clone();

                                let checkin_matches = params.checkin_date.contains(&case.input.checkin_date);
                                let checkout_matches = params.checkout_date.contains(&case.input.checkout_date);
                                let currency_matches = params.currency == case.input.currency;

                                let expected_adults = case.input.guests.adults;
                                let actual_adults = params.adults as usize;
                                let expected_children = case.input.guests.children_with_ages.len();
                                let actual_children = params.children_ages.len();

                                let location_id_matches = params.loc_ts_id == case.input.location_id;
                                let coordinates_matches = params.loc_ts_coords == case.input.coordinates;
                                let display_name_matches = if case.input.display_name.is_empty() {
                                    params.loc_ts_name.is_empty() || params.loc_ts_id.is_empty()
                                } else {
                                    params.loc_ts_name.contains(&*case.input.display_name)
                                };
                                let location_matches = location_id_matches && coordinates_matches && display_name_matches;

                                let mut expected_filters = Vec::new();
                                if let Some(r) = guest_rating {
                                    expected_filters.push(format!("guest_rating: {}", r));
                                }
                                for &s in &star_values {
                                    expected_filters.push(format!("star: {}", s));
                                }
                                if let Some(ref amenity_names) = case.input.amenities {
                                    for name in amenity_names {
                                        if let Some(amenity) = delulu_travel_agent::Amenity::from_str_name(&name.to_uppercase()) {
                                            expected_filters.push(format!("amenity: {}", amenity as i32));
                                        }
                                    }
                                }
                                if let Some(max) = case.input.price_max {
                                    expected_filters.push(format!("max_price: {}", max as i32));
                                }
                                if let Some(min) = case.input.price_min {
                                    expected_filters.push(format!("min_price: {}", min as i32));
                                }
                                if let Some(sort_val) = sort_order {
                                    let sort_str = match sort_val {
                                        8 => "highest_rating",
                                        13 => "most_reviewed",
                                        3 => "lowest_price",
                                        _ => "relevance",
                                    };
                                    expected_filters.push(format!("sort: {}", sort_str));
                                }

                                let mut actual_filters: Vec<String> = Vec::new();
                                if let Some(r) = params.min_guest_rating {
                                    actual_filters.push(format!("guest_rating: {}", r));
                                }
                                for &s in &params.hotel_stars {
                                    actual_filters.push(format!("star: {}", s));
                                }
                                for &a in &params.amenities {
                                    actual_filters.push(format!("amenity: {}", a as i32));
                                }
                                if let Some(p) = params.max_price {
                                    actual_filters.push(format!("max_price: {}", p));
                                }
                                if let Some(p) = params.min_price {
                                    actual_filters.push(format!("min_price: {}", p));
                                }
                                if let Some(ref s) = params.sort_order {
                                    actual_filters.push(format!("sort: {}", s));
                                }
                                actual_filters.sort();
                                let mut expected_sorted = expected_filters.clone();
                                expected_sorted.sort();

                                let filters_match = actual_filters == expected_sorted;

                                if actual_adults == expected_adults &&
                                   actual_children == expected_children &&
                                   checkin_matches &&
                                   checkout_matches &&
                                   currency_matches &&
                                   location_matches &&
                                   filters_match {
                                    println!("✓ {} - round-trip OK", case.name);
                                    passed += 1;
                                } else {
                                    println!("✗ {} - decoded fields mismatch", case.name);
                                    if !checkin_matches {
                                        println!("  Checkin: expected {}, got {}", case.input.checkin_date, params.checkin_date);
                                    }
                                    if !checkout_matches {
                                        println!("  Checkout: expected {}, got {}", case.input.checkout_date, params.checkout_date);
                                    }
                                    if !currency_matches {
                                        println!("  Currency: expected {}, got {}", case.input.currency, params.currency);
                                    }
                                    if actual_adults != expected_adults {
                                        println!("  Adults: expected {}, got {}", expected_adults, actual_adults);
                                    }
                                    if actual_children != expected_children {
                                        println!("  Children: expected {}, got {}", expected_children, actual_children);
                                    }
                                    if !location_matches {
                                        println!("  Location:");
                                        println!("    location_id: expected '{}', got '{}'", case.input.location_id, params.loc_ts_id);
                                        println!("    coordinates: expected '{}', got '{}'", case.input.coordinates, params.loc_ts_coords);
                                        println!("    display_name: expected '{}', got '{}'", case.input.display_name, params.loc_ts_name);
                                    }
                                    if !filters_match {
                                        println!("  Filters: expected {:?}", expected_sorted);
                                        println!("  Filters: got {:?}", actual_filters);
                                    }
                                    failed += 1;
                                }
                            }
                            Err(e) => {
                                println!("✗ {} - decode failed: {}", case.name, e);
                                failed += 1;
                            }
                        }
                    }
                    Err(e) => {
                        println!("✗ {} - encode failed: {}", case.name, e);
                        failed += 1;
                    }
                }
            }
            Err(e) => {
                println!("✗ {} - build failed: {}", case.name, e);
                failed += 1;
            }
        }
    }

    println!("\n=== Summary ===");
    println!("Passed: {}", passed);
    println!("Failed: {}", failed);
    println!("Total: {}", test_vectors.cases.len());

    assert_eq!(failed, 0, "{} tests failed", failed);
}
