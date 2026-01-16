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

//! Integration test for live Google Hotels queries.
//!
//! This test makes actual HTTP requests to Google Hotels to verify
//! the entire pipeline works end-to-end:
//!   1. Build HotelSearchParams
//!   2. Generate TS protobuf
//!   3. Construct proper URL with cookies
//!   4. Send request with Chrome 126 emulation
//!   5. Parse HTML response
//!   6. Extract hotel listings
//!
//! Rate limited to 1 request/second between queries.
//!
//! ============================================================================
//! CI SAFETY: All live HTTP tests are IGNORED by default
//! ============================================================================
//! To run manually:
//!     cargo test --test t_hotels_integration_live -- --ignored --nocapture
//!
//! Or run a specific test:
//!     cargo test --test t_hotels_integration_live run_quick -- --ignored --nocapture

use anyhow::{Context, Result};
use chrono::{Local, Months, NaiveDate};
use delulu_travel_agent::{Amenity, GoogleHotelsClient, HotelSearchParams};
use tokio::time::sleep;

fn today() -> NaiveDate {
    Local::now().date_naive()
}

fn parse_date(s: &str) -> Result<NaiveDate> {
    chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d")
        .or_else(|_| chrono::NaiveDate::parse_from_str(s, "%Y/%m/%d"))
        .context(format!("Invalid date format: {}", s))
}

fn compute_checkout(checkin: &str, nights: i64) -> NaiveDate {
    let d = parse_date(checkin).unwrap();
    d + chrono::Duration::days(nights)
}

async fn rate_limited_query(
    location: &str,
    checkin: &str,
    checkout: &str,
    adults: u32,
    delay_secs: u64,
) -> Result<(), Box<dyn std::error::Error>> {
    if delay_secs > 0 {
        sleep(std::time::Duration::from_secs(delay_secs)).await;
    }
    execute_query(location, checkin, checkout, adults).await
}

async fn execute_query(
    location: &str,
    checkin: &str,
    checkout: &str,
    adults: u32,
) -> Result<(), Box<dyn std::error::Error>> {
    let checkin_date = parse_date(checkin)?;
    let checkout_date = parse_date(checkout)?;

    let request = HotelSearchParams::builder(
        location.to_string(),
        checkin_date,
        checkout_date,
        adults,
        Vec::new(),
    )
    .build()
    .map_err(|e| anyhow::anyhow!(e))?;

    println!("\n🏨 Query: {} ({} adults)", location, adults);
    println!("Dates: {} to {}", checkin, checkout);

    let client = GoogleHotelsClient::new(4)?;
    let result = client.search_hotels(&request).await?;
    let status = if !result.hotels.is_empty() {
        "VALID"
    } else {
        "EMPTY_OR_INVALID"
    };
    println!(
        "Result: {} hotels found, lowest=${:?}, status={}",
        result.hotels.len(),
        result.lowest_price,
        status
    );

    if result.hotels.is_empty() {
        return Err("No hotels found in response".into());
    }

    Ok(())
}

// ============================================================================
// UNIT TESTS (CI-SAFE)
// ============================================================================

#[test]
fn test_url_construction_sanity() {
    let today = Local::now().date_naive();
    let checkin = today + Months::new(2);
    let checkout = checkin + chrono::Duration::days(2);

    let params = HotelSearchParams::builder("Tokyo".to_string(), checkin, checkout, 2, Vec::new())
        .build()
        .expect("params should build");

    let url = params.get_search_url();
    assert!(
        url.starts_with("https://www.google.com/travel/search?q="),
        "URL should start with search endpoint"
    );
    assert!(url.contains("ts="), "URL should contain ts param");

    println!(
        "Constructed URL (first 100 chars): {}",
        &url[..url.len().min(100)]
    );
}

#[test]
fn test_request_validation_errors() {
    let today = Local::now().date_naive();

    let past_req = HotelSearchParams::builder(
        "Tokyo".to_string(),
        today - chrono::Duration::days(1),
        today,
        2,
        Vec::new(),
    )
    .build();

    let bad_dates = HotelSearchParams::builder(
        "Tokyo".to_string(),
        today + chrono::Duration::days(5),
        today + chrono::Duration::days(2),
        2,
        Vec::new(),
    )
    .build();

    assert!(bad_dates.is_err(), "Bad date ordering should fail");

    assert!(
        past_req.is_ok(),
        "Past checkin should not fail at build time"
    );
}

#[test]
fn test_guests_validation() {
    let today = Local::now().date_naive();
    let checkout = today + chrono::Duration::days(2);

    assert!(
        HotelSearchParams::builder("Tokyo".to_string(), today, checkout, 2, Vec::new())
            .build()
            .is_ok()
    );
    assert!(
        HotelSearchParams::builder("Tokyo".to_string(), today, checkout, 1, vec![5])
            .build()
            .is_ok()
    );
    assert!(
        HotelSearchParams::builder("Tokyo".to_string(), today, checkout, 0, Vec::new())
            .build()
            .is_err()
    );
    assert!(HotelSearchParams::builder(
        "Tokyo".to_string(),
        today,
        checkout,
        5,
        vec![5, 5, 5, 5, 5]
    )
    .build()
    .is_err());
}

// ============================================================================
// INTEGRATION TESTS - LIVE HTTP (IGNORED IN CI)
// ============================================================================

#[tokio::test]
#[ignore]
async fn test_real_query_tokyo() {
    println!("=== Tokyo Hotels Test ===");

    let checkin = today() + Months::new(2);
    let checkout = compute_checkout(&checkin.format("%Y-%m-%d").to_string(), 2);

    match rate_limited_query(
        "Tokyo",
        &checkin.format("%Y-%m-%d").to_string(),
        &checkout.format("%Y-%m-%d").to_string(),
        2,
        0,
    )
    .await
    {
        Ok(_) => println!("✓ Tokyo query succeeded"),
        Err(e) => {
            eprintln!("✗ Tokyo query failed: {}", e);
            if e.to_string().contains("HTTP") || e.to_string().contains("network") {
                println!("⚠ Transient network error");
                return;
            }
            panic!("Unexpected error: {}", e);
        }
    }
}

#[tokio::test]
#[ignore]
async fn test_real_query_paris() {
    println!("\n=== Paris Hotels Test ===");

    let checkin = today() + Months::new(3);
    let checkout = compute_checkout(&checkin.format("%Y-%m-%d").to_string(), 3);

    match rate_limited_query(
        "Paris",
        &checkin.format("%Y-%m-%d").to_string(),
        &checkout.format("%Y-%m-%d").to_string(),
        2,
        1,
    )
    .await
    {
        Ok(_) => println!("✓ Paris query succeeded"),
        Err(e) => {
            eprintln!("✗ Paris query failed: {}", e);
            if e.to_string().contains("HTTP") || e.to_string().contains("network") {
                println!("⚠ Transient network error");
                return;
            }
            panic!("Unexpected error: {}", e);
        }
    }
}

#[tokio::test]
#[ignore]
async fn test_real_query_iata_code() {
    println!("\n=== IATA Code (HND → Tokyo) Test ===");

    let checkin = today() + Months::new(2);
    let checkout = compute_checkout(&checkin.format("%Y-%m-%d").to_string(), 2);

    match rate_limited_query(
        "HND",
        &checkin.format("%Y-%m-%d").to_string(),
        &checkout.format("%Y-%m-%d").to_string(),
        2,
        1,
    )
    .await
    {
        Ok(_) => println!("✓ HND (Tokyo) query succeeded"),
        Err(e) => {
            eprintln!("✗ HND query failed: {}", e);
            if e.to_string().contains("HTTP") || e.to_string().contains("network") {
                println!("⚠ Transient network error");
                return;
            }
            panic!("Unexpected error: {}", e);
        }
    }
}

#[tokio::test]
#[ignore]
async fn test_real_query_seasonal_variation() {
    println!("\n=== Date Variation Test ===");

    let earlier_chk = (today() + Months::new(1)).format("%Y-%m-%d").to_string();
    let earlier_out = compute_checkout(&earlier_chk, 3);

    match rate_limited_query(
        "NYC",
        &earlier_chk,
        &earlier_out.format("%Y-%m-%d").to_string(),
        2,
        0,
    )
    .await
    {
        Ok(_) => println!("✓ Earlier NYC query succeeded"),
        Err(e) => {
            eprintln!("✗ Earlier NYC failed: {}", e);
            if e.to_string().contains("HTTP") || e.to_string().contains("network") {
                println!("⚠ Transient error - skipping date comparison");
                return;
            }
            panic!("Unexpected error: {}", e);
        }
    }

    let later_chk = (today() + Months::new(2)).format("%Y-%m-%d").to_string();
    let later_out = compute_checkout(&later_chk, 3);

    match rate_limited_query(
        "NYC",
        &later_chk,
        &later_out.format("%Y-%m-%d").to_string(),
        2,
        1,
    )
    .await
    {
        Ok(_) => println!("✓ Later NYC query succeeded"),
        Err(e) => {
            eprintln!("✗ Later NYC failed: {}", e);
            if e.to_string().contains("HTTP") || e.to_string().contains("network") {
                println!("⚠ Transient error - skipping");
                return;
            }
            panic!("Unexpected error: {}", e);
        }
    }
}

#[tokio::test]
#[ignore]
async fn test_real_query_single_traveler() {
    println!("\n=== Single Traveler (1 adult) Test ===");

    let checkin = today() + Months::new(2);
    let checkout = compute_checkout(&checkin.format("%Y-%m-%d").to_string(), 1);

    match rate_limited_query(
        "London",
        &checkin.format("%Y-%m-%d").to_string(),
        &checkout.format("%Y-%m-%d").to_string(),
        1,
        1,
    )
    .await
    {
        Ok(_) => println!("✓ Single traveler query succeeded"),
        Err(e) => {
            eprintln!("✗ Single traveler query failed: {}", e);
            if e.to_string().contains("HTTP") || e.to_string().contains("network") {
                println!("⚠ Transient network error");
                return;
            }
            panic!("Unexpected error: {}", e);
        }
    }
}

#[tokio::test]
#[ignore]
async fn run_quick_smoke_test() {
    println!("Quick smoke test: single Tokyo query");
    println!("Rate: 1/sec");

    let checkin = today() + Months::new(2);
    let checkout = compute_checkout(&checkin.format("%Y-%m-%d").to_string(), 2);

    match rate_limited_query(
        "Tokyo",
        &checkin.format("%Y-%m-%d").to_string(),
        &checkout.format("%Y-%m-%d").to_string(),
        2,
        1,
    )
    .await
    {
        Ok(()) => println!("✅ Smoke test completed successfully"),
        Err(e) => {
            eprintln!("❌ Smoke test failed: {}", e);
            panic!("Smoke test failed: {}", e);
        }
    }
}

// =============================================================================
// FIXTURE FETCHING TESTS (IGNORED - FOR SETUP ONLY)
// =============================================================================
// These tests fetch HTML from Google and save as compressed fixtures.
// Run with: cargo test --test t_hotels_integration_live fetch_fixture_xxx -- --ignored --nocapture
// Rate limited to 2 seconds between requests to avoid being banned.

const FIXTURE_RATE_LIMIT_SECS: u64 = 2;

fn compress_and_save(html: &str, name: &str) {
    use std::fs;
    use std::path::Path;

    let fixtures_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures-hotels-parsing");
    fs::create_dir_all(&fixtures_dir).expect("create fixtures dir");

    let output_path = fixtures_dir.join(format!("{}.html.zst", name));
    let file = fs::File::create(&output_path).expect("create output file");

    let mut encoder = zstd::stream::Encoder::new(file, 0).expect("create zstd encoder");
    use std::io::Write;
    encoder.write_all(html.as_bytes()).expect("write bytes");
    encoder.finish().expect("finish compression");

    println!("Saved fixture: {:?}", output_path);
}

async fn rate_limited_fetch(
    client: &wreq::Client,
    url: &str,
    delay_secs: u64,
    name: &str,
) -> Result<String, Box<dyn std::error::Error>> {
    if delay_secs > 0 {
        sleep(std::time::Duration::from_secs(delay_secs)).await;
    }
    fetch_single_fixture(client, url, name).await
}

async fn fetch_single_fixture(
    client: &wreq::Client,
    url: &str,
    name: &str,
) -> Result<String, Box<dyn std::error::Error>> {
    let url_display = &url[0..url.len().min(100)];
    println!("Fetching '{}': {}", name, url_display);

    let resp = client
        .get(url)
        .send()
        .await
        .map_err(|e| anyhow::anyhow!(e))?;
    let status = resp.status();
    if !status.is_success() {
        return Err(anyhow::anyhow!("HTTP {}: {}", status, url_display).into());
    }
    let text = resp.text().await.map_err(|e| anyhow::anyhow!(e))?;

    if text.to_lowercase().contains("consent") {
        return Err(anyhow::anyhow!("Blocked by consent cookie").into());
    }
    if text.len() < 1000 {
        return Err(anyhow::anyhow!("Response too short ({} bytes)", text.len()).into());
    }

    Ok(text)
}

#[tokio::test]
#[ignore]
async fn fetch_fixture_tokyo_standard() {
    let client = wreq::Client::builder()
        .emulation(wreq_util::Emulation::Chrome126)
        .redirect(wreq::redirect::Policy::default())
        .build()
        .expect("wreq client");

    let today = Local::now().date_naive();
    let checkin = today + Months::new(2);
    let checkout = checkin + chrono::Duration::days(2);

    let params = HotelSearchParams::builder("Tokyo".to_string(), checkin, checkout, 2, Vec::new())
        .build()
        .expect("params should build");

    let ts = params.generate_ts().expect("encode ts");
    let url = format!("https://www.google.com/travel/search?q=Tokyo&ts={}", ts);

    match rate_limited_fetch(&client, &url, FIXTURE_RATE_LIMIT_SECS, "tokyo-standard").await {
        Ok(text) => compress_and_save(&text, "tokyo-standard"),
        Err(e) => panic!("Failed: {}", e),
    }
}

#[tokio::test]
#[ignore]
async fn fetch_fixture_paris_budget() {
    let client = wreq::Client::builder()
        .emulation(wreq_util::Emulation::Chrome126)
        .redirect(wreq::redirect::Policy::default())
        .build()
        .expect("wreq client");

    let today = Local::now().date_naive();
    let checkin = today + Months::new(3);
    let checkout = checkin + chrono::Duration::days(2);

    let params = HotelSearchParams::builder("Paris".to_string(), checkin, checkout, 2, Vec::new())
        .min_guest_rating(3.0)
        .min_price(Some(50))
        .max_price(Some(150))
        .build()
        .expect("params should build");

    let ts = params.generate_ts().expect("encode ts");
    let url = format!("https://www.google.com/travel/search?q=Paris&ts={}", ts);

    match rate_limited_fetch(&client, &url, FIXTURE_RATE_LIMIT_SECS, "paris-budget").await {
        Ok(text) => compress_and_save(&text, "paris-budget"),
        Err(e) => panic!("Failed: {}", e),
    }
}

#[tokio::test]
#[ignore]
async fn fetch_fixture_tokyo_5star() {
    let client = wreq::Client::builder()
        .emulation(wreq_util::Emulation::Chrome126)
        .redirect(wreq::redirect::Policy::default())
        .build()
        .expect("wreq client");

    let today = Local::now().date_naive();
    let checkin = today + Months::new(2);
    let checkout = checkin + chrono::Duration::days(2);

    let params = HotelSearchParams::builder("Tokyo".to_string(), checkin, checkout, 2, Vec::new())
        .hotel_stars(vec![5])
        .min_guest_rating(4.0)
        .build()
        .expect("params should build");

    let ts = params.generate_ts().expect("encode ts");
    let url = format!("https://www.google.com/travel/search?q=Tokyo&ts={}", ts);

    match rate_limited_fetch(&client, &url, FIXTURE_RATE_LIMIT_SECS, "tokyo-5star").await {
        Ok(text) => compress_and_save(&text, "tokyo-5star"),
        Err(e) => panic!("Failed: {}", e),
    }
}

#[tokio::test]
#[ignore]
async fn fetch_fixture_nyc_families() {
    let client = wreq::Client::builder()
        .emulation(wreq_util::Emulation::Chrome126)
        .redirect(wreq::redirect::Policy::default())
        .build()
        .expect("wreq client");

    let today = Local::now().date_naive();
    let checkin = today + Months::new(2);
    let checkout = checkin + chrono::Duration::days(5);

    let params =
        HotelSearchParams::builder("New York".to_string(), checkin, checkout, 2, vec![5, 8])
            .hotel_stars(vec![4, 5])
            .amenities(vec![Amenity::KidFriendly])
            .max_price(Some(300))
            .build()
            .expect("params should build");

    let ts = params.generate_ts().expect("encode ts");
    let url = format!("https://www.google.com/travel/search?q=New+York&ts={}", ts);

    match rate_limited_fetch(&client, &url, FIXTURE_RATE_LIMIT_SECS, "nyc-families").await {
        Ok(text) => compress_and_save(&text, "nyc-families"),
        Err(e) => panic!("Failed: {}", e),
    }
}

#[tokio::test]
#[ignore]
async fn fetch_fixture_london_long_stay() {
    let client = wreq::Client::builder()
        .emulation(wreq_util::Emulation::Chrome126)
        .redirect(wreq::redirect::Policy::default())
        .build()
        .expect("wreq client");

    let today = Local::now().date_naive();
    let checkin = today + Months::new(3);
    let checkout = checkin + chrono::Duration::days(14);

    let params = HotelSearchParams::builder("London".to_string(), checkin, checkout, 2, Vec::new())
        .amenities(vec![Amenity::KidFriendly])
        .min_guest_rating(3.0)
        .max_price(Some(200))
        .build()
        .expect("params should build");

    let ts = params.generate_ts().expect("encode ts");
    let url = format!("https://www.google.com/travel/search?q=London&ts={}", ts);

    match rate_limited_fetch(&client, &url, FIXTURE_RATE_LIMIT_SECS, "london-long-stay").await {
        Ok(text) => compress_and_save(&text, "london-long-stay"),
        Err(e) => panic!("Failed: {}", e),
    }
}
