//! t_hotels_decoding_fixtures.rs
//! This test validates that our decoder can match values entered through the UI.
//!
//! Some values cannot be entered by hand
//! - The `location_id` can be checked against Wikidata / Freebase or via https://www.google.com/search?kgmid=<location_id>
//! - The `coordinates` have to be parsed from raw `ts`. They are still added to the testing suite as anti-regression
//! - The currency, `EUR`, is likely auto-set depending on where you connect from
//!
//! Combined with `t_hotels_codec_roundtrip.rs`, we can be more confident that our encoder matches Google's
//! and so that our protobuf schema is correct.

use delulu_travel_agent::{HotelSearchParams, SortType};
use std::fs;

#[derive(serde::Deserialize)]
struct TestVectorCase {
    name: String,
    #[allow(dead_code)]
    description: String,
    input: TestVectorInput,
    #[allow(dead_code)]
    expected_ts: String,
}

#[derive(serde::Deserialize)]
struct TestVectorInput {
    display_name: String,
    checkin_date: String,
    #[allow(dead_code)]
    checkout_date: String,
    guests: TestVectorGuests,
    currency: String,
    min_guest_rating: Option<f64>,
    amenities: Option<Vec<String>>,
    hotel_stars: Option<Vec<i32>>,
    price_min: Option<f64>,
    price_max: Option<f64>,
    sort_by: Option<String>,
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
    #[allow(dead_code)]
    description: String,
    cases: Vec<TestVectorCase>,
}

#[test]
fn test_validate_decoder_with_ui() {
    let data = fs::read_to_string("tests/fixtures-google-hotels/ts_vectors.json")
        .expect("Failed to read ts_vectors.json");
    let test_vectors: TestVectors =
        serde_json::from_str(&data).expect("Failed to parse ts_vectors.json");

    println!("\n=== VALIDATION: Decoded values against Google's actual protobuf ===\n");
    println!("Validating:");
    println!("  - Location: location_id, coordinates, display_name");
    println!("  - Filters: prices, amenities, guest_rating, stars (hotel_class), sort_order");
    println!();

    let mut passed = 0;
    let mut failed = 0;

    for case in &test_vectors.cases {
        match HotelSearchParams::from_ts(&case.expected_ts) {
            Ok(decoded) => {
                let mut failures = Vec::new();

                let location_id_matches = if case.input.location_id.is_empty() {
                    true
                } else {
                    decoded.loc_ts_id == case.input.location_id
                };

                let coordinates_matches = if case.input.coordinates.is_empty() {
                    true
                } else {
                    decoded.loc_ts_coords == case.input.coordinates
                };

                let display_name_matches = if case.input.display_name.is_empty() {
                    true
                } else {
                    decoded.loc_ts_name.contains(&*case.input.display_name)
                };

                if !location_id_matches {
                    failures.push(format!(
                        "location_id: expected '{}', got '{}'",
                        case.input.location_id, decoded.loc_ts_id
                    ));
                }
                if !coordinates_matches {
                    failures.push(format!(
                        "coordinates: expected '{}', got '{}'",
                        case.input.coordinates, decoded.loc_ts_coords
                    ));
                }
                if !display_name_matches {
                    failures.push(format!(
                        "display_name: expected '{}', got '{}'",
                        case.input.display_name, decoded.loc_ts_name
                    ));
                }

                let expected_adults = case.input.guests.adults;
                let actual_adults = decoded.adults as usize;
                let expected_children = case.input.guests.children_with_ages.len();
                let actual_children = decoded.children_ages.len();

                if actual_adults != expected_adults {
                    failures.push(format!(
                        "Adults: expected {}, got {}",
                        expected_adults, actual_adults
                    ));
                }
                if actual_children != expected_children {
                    failures.push(format!(
                        "Children: expected {}, got {}",
                        expected_children, actual_children
                    ));
                }

                let used_guests_dropdown_matches =
                    decoded.used_guests_dropdown == case.input.used_guests_dropdown as i32;
                if !used_guests_dropdown_matches {
                    failures.push(format!(
                        "used_guests_dropdown: expected {}, got {}",
                        case.input.used_guests_dropdown, decoded.used_guests_dropdown
                    ));
                }

                let date_matches = decoded.checkin_date.contains(&case.input.checkin_date);
                let currency_matches = decoded.currency == case.input.currency;

                if !date_matches {
                    failures.push(format!(
                        "Date: expected {}, got {}",
                        case.input.checkin_date, decoded.checkin_date
                    ));
                }
                if !currency_matches {
                    failures.push(format!(
                        "Currency: expected {}, got {}",
                        case.input.currency, decoded.currency
                    ));
                }

                let expected_stars: Vec<i32> = case.input.hotel_stars.clone().unwrap_or_default();
                let expected_amenities: Vec<i32> = case
                    .input
                    .amenities
                    .as_deref()
                    .unwrap_or_default()
                    .iter()
                    .filter_map(|a| {
                        delulu_travel_agent::Amenity::from_str_name(a).map(|a| a as i32)
                    })
                    .collect();
                let expected_guest_rating = case.input.min_guest_rating;

                let expected_sort: Option<SortType> = match case.input.sort_by.as_deref() {
                    Some("relevance") | None => None,
                    Some(s) => SortType::from_str_name(s),
                };

                let actual_stars: Vec<i32> = decoded.hotel_stars.clone();

                let actual_amenities: Vec<i32> =
                    decoded.amenities.iter().map(|a| *a as i32).collect();

                let actual_guest_rating = decoded.min_guest_rating;

                let actual_sort = decoded.sort_order;

                let mut star_mismatch = false;
                if expected_stars.is_empty() && !actual_stars.is_empty() {
                    failures.push(format!("stars: expected none, got {:?}", actual_stars));
                    star_mismatch = true;
                } else if !expected_stars.is_empty() {
                    let mut exp_sorted = expected_stars.clone();
                    let mut act_sorted = actual_stars.clone();
                    exp_sorted.sort();
                    act_sorted.sort();
                    if exp_sorted != act_sorted {
                        failures.push(format!(
                            "stars: expected {:?}, got {:?}",
                            exp_sorted, act_sorted
                        ));
                        star_mismatch = true;
                    }
                }

                let mut amenity_mismatch = false;
                if expected_amenities.is_empty() && !actual_amenities.is_empty() {
                    failures.push(format!(
                        "amenities: expected none, got {:?}",
                        actual_amenities
                    ));
                    amenity_mismatch = true;
                } else if !expected_amenities.is_empty() {
                    let mut exp_sorted = expected_amenities.clone();
                    let mut act_sorted = actual_amenities.clone();
                    exp_sorted.sort();
                    act_sorted.sort();
                    if exp_sorted != act_sorted {
                        failures.push(format!(
                            "amenities: expected {:?}, got {:?}",
                            exp_sorted, act_sorted
                        ));
                        amenity_mismatch = true;
                    }
                }

                if expected_guest_rating.is_none() && actual_guest_rating.is_some() {
                    failures.push(format!(
                        "guest_rating: expected none, got {:?}",
                        actual_guest_rating
                    ));
                } else if expected_guest_rating.is_some() && actual_guest_rating.is_none() {
                    failures.push(format!(
                        "guest_rating: expected {:?}, got none",
                        expected_guest_rating
                    ));
                } else if expected_guest_rating != actual_guest_rating {
                    failures.push(format!(
                        "guest_rating: expected {:?}, got {:?}",
                        expected_guest_rating, actual_guest_rating
                    ));
                }

                let sort_matches = match (&expected_sort, &actual_sort) {
                    (None, None) => true,
                    (None, Some(_)) => false,
                    (Some(_), None) => false,
                    (Some(e), Some(a)) => e == a,
                };
                if !sort_matches {
                    failures.push(format!(
                        "sort: expected {:?}, got {:?}",
                        expected_sort, actual_sort
                    ));
                }

                let expected_max_price = case.input.price_max.map(|p| p as i32);
                let expected_min_price = case.input.price_min.map(|p| p as i32);

                let actual_max_price = decoded.max_price;
                let actual_min_price = decoded.min_price;

                if expected_max_price.is_none() && actual_max_price.is_some() {
                    failures.push(format!(
                        "max_price: expected none, got {:?}",
                        actual_max_price
                    ));
                } else if expected_max_price.is_some() && actual_max_price.is_none() {
                    failures.push(format!(
                        "max_price: expected {:?}, got none",
                        expected_max_price
                    ));
                } else if expected_max_price != actual_max_price {
                    failures.push(format!(
                        "max_price: expected {:?}, got {:?}",
                        expected_max_price, actual_max_price
                    ));
                }

                if expected_min_price.is_none() && actual_min_price.is_some() {
                    failures.push(format!(
                        "min_price: expected none, got {:?}",
                        actual_min_price
                    ));
                } else if expected_min_price.is_some() && actual_min_price.is_none() {
                    failures.push(format!(
                        "min_price: expected {:?}, got none",
                        expected_min_price
                    ));
                } else if expected_min_price != actual_min_price {
                    failures.push(format!(
                        "min_price: expected {:?}, got {:?}",
                        expected_min_price, actual_min_price
                    ));
                }

                let price_matches = expected_max_price == actual_max_price
                    && expected_min_price == actual_min_price;
                let all_match = location_id_matches
                    && coordinates_matches
                    && display_name_matches
                    && actual_adults == expected_adults
                    && actual_children == expected_children
                    && used_guests_dropdown_matches
                    && date_matches
                    && currency_matches
                    && !star_mismatch
                    && !amenity_mismatch
                    && sort_matches
                    && price_matches;

                if all_match {
                    println!("✓ {} - {}", case.name, case.description);
                    passed += 1;
                } else {
                    println!("✗ {} - {}", case.name, case.description);
                    for failure in &failures {
                        println!("  {}", failure);
                    }
                    failed += 1;
                }
            }
            Err(e) => {
                println!("✗ {} - Failed to decode: {}", case.name, e);
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
