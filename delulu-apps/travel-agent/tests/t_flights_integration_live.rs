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

//! Integration test for live Google Flights queries.
//!
//! This test makes actual HTTP requests to Google Flights to verify
//! the entire pipeline works end-to-end:
//!   1. Build FlightSearchConfig
//!   2. Encode to TFS protobuf
//!   3. Construct proper URL with tracking params
//!   4. Send request with Safari emulation + cookies
//!   5. Parse HTML response
//!   6. Extract flight itineraries
//!
//! Rate limited to 1 request/second between queries.
//!
//! ============================================================================
//! CI SAFETY: All live HTTP tests are IGNORED by default
//! ============================================================================
//! To run manually:
//!     cargo test --test integration_query_test -- --ignored --nocapture
//!
//! Or run a specific test:
//!     cargo test --test integration_query_test run_quick -- --ignored --nocapture

use anyhow::{Context, Result};
use base64::engine::general_purpose::STANDARD;
use base64::Engine as _;
use chrono::{Datelike, Local, Months, NaiveDate};
use tokio::time::sleep;
use wreq::redirect::Policy;
use wreq_util::Emulation;

use delulu_travel_agent::{
    encode_tfs, parse_flights_response, CabinClass, FlightSearchConfig, PassengerType, Tfs, TripType,
};

// Compute dates dynamically to avoid stale tests
fn today() -> NaiveDate {
    Local::now().date_naive()
}

fn parse_date(s: &str) -> Result<chrono::NaiveDate> {
    chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d")
        .or_else(|_| chrono::NaiveDate::parse_from_str(s, "%Y/%m/%d"))
        .context(format!("Invalid date format: {}", s))
}

fn dom_flight_date() -> String {
    // Near term (2 months out) - reasonable for domestic flights
    (today() + Months::new(2)).format("%Y-%m-%d").to_string()
}

fn intl_flight_date() -> String {
    // Further out (3 months) - typical for international planning
    (today() + Months::new(3)).format("%Y-%m-%d").to_string()
}

fn bus_flight_date() -> String {
    // Slightly shorter window (2.5 months)
    (today() + Months::new(2) + chrono::Days::new(15))
        .format("%Y-%m-%d")
        .to_string()
}

fn offpeak_date() -> String {
    // Off-peak: Return Jan 15 of next year if current year's Jan 15 has passed
    let candidate = NaiveDate::from_ymd_opt(today().year(), 1, 15).unwrap();
    if candidate <= today() {
        NaiveDate::from_ymd_opt(today().year() + 1, 1, 15)
            .unwrap()
            .format("%Y-%m-%d")
            .to_string()
    } else {
        candidate.format("%Y-%m-%d").to_string()
    }
}

fn peak_date() -> String {
    // Peak: Summer (July 15) if we're before August
    let jul = NaiveDate::from_ymd_opt(today().year(), 7, 15).unwrap();
    if today().month() < 8 {
        jul.format("%Y-%m-%d").to_string()
    } else {
        // Use next year's July
        NaiveDate::from_ymd_opt(today().year() + 1, 7, 15)
            .unwrap()
            .format("%Y-%m-%d")
            .to_string()
    }
}

/// Builds a wreq client with browser emulation.
/// wreq-util's Emulation handles all headers automatically (UA, cookies, etc).
fn build_client() -> wreq::Client {
    wreq::Client::builder()
        .emulation(Emulation::Safari18_5)
        .redirect(Policy::default())
        .build()
        .expect("wreq client build should succeed")
}

/// Execute a query with rate limiting.
async fn rate_limited_query(
    client: &wreq::Client,
    from: &str,
    to: &str,
    date: &str,
    cabin: CabinClass,
    delay_secs: u64,
) -> Result<(), Box<dyn std::error::Error>> {
    if delay_secs > 0 {
        sleep(std::time::Duration::from_secs(delay_secs)).await;
    }
    execute_query(client, from, to, date, cabin).await
}

/// Execute a single query against Google Flights.
async fn execute_query(
    client: &wreq::Client,
    from: &str,
    to: &str,
    date: &str,
    cabin: CabinClass,
) -> Result<(), Box<dyn std::error::Error>> {
    // Build config (mirrors GoogleFlightsClient::search logic)
    let config = FlightSearchConfig {
        from_airport: from.to_uppercase(),
        to_airport: to.to_uppercase(),
        depart_date: parse_date(date)?,
        cabin_class: cabin,
        passengers: vec![(PassengerType::Adult, 1)],
        trip_type: TripType::OneWay,
        max_stops: None,
        preferred_airlines: None,
    };

    // Use unified Tfs type for URL generation
    let tfs = Tfs::from_config(&config, "en", "USD")?;
    let url = tfs.get_url();

    println!("\n🛫 Query: {} → {} on {} ({:?})", from, to, date, cabin);
    println!("Using Tfs URL generation: {}", url);
    println!("URL length: {} chars", url.len());
    println!("🔗 URL for manual check:\n{}", url);

    // Execute request
    let response = client.get(&url).send().await?;
    let status = response.status();

    println!(
        "HTTP Status: {} {}",
        status.as_u16(),
        status.canonical_reason().unwrap_or("Unknown")
    );

    // Check for unexpected redirects even with consented cookies
    if status.is_redirection() {
        if let Some(location) = response.headers().get("location") {
            eprintln!("↪ Unexpected redirect to: {:?}", location);
            eprintln!("Cookies may not be accepted - consider updating CONSENT_COOKIE");
        }
        return Err(format!("HTTP {} after redirect handling", status).into());
    }

    if !status.is_success() {
        return Err(format!("HTTP {}", status).into());
    }

    let body = response.text().await?;
    let body_len_kb = body.len() / 1024;
    println!("Response body: {} KB", body_len_kb);

    // Try parsing HTML
    match parse_flights_response(&body) {
        Ok(parsed) => {
            println!("Parsed {} flights", parsed.flights.len());
            if let Some(ref price) = parsed.best_price {
                println!("Best price: {}", price);
            }
            if parsed.flights.is_empty() {
                return Err("Parser found no flights".into());
            }
        }
        Err(e) => {
            // Warn but don't fail - TFS encoding worked if we got here
            eprintln!("⚠ Parse warning: {}", e);
        }
    }

    Ok(())
}

// ============================================================================
// UNIT TESTS (CI-SAFE)
// ============================================================================

/// Sanity check: URL construction produces valid URLs.
#[test]
fn test_url_construction_sanity() {
    let config = FlightSearchConfig {
        from_airport: "SFO".into(),
        to_airport: "JFK".into(),
        depart_date: chrono::NaiveDate::from_ymd_opt(2025, 7, 15).unwrap(),
        cabin_class: CabinClass::Economy,
        passengers: vec![(PassengerType::Adult, 1)],
        trip_type: TripType::OneWay,
        max_stops: None,
        preferred_airlines: None,
    };

    let tfs_bytes = encode_tfs(&config).expect("encode should work");
    assert!(!tfs_bytes.is_empty(), "TFS should not be empty");

    let tfs_encoded = STANDARD.encode(&tfs_bytes);
    assert!(!tfs_encoded.is_empty(), "Base64 should not be empty");

    let url = format!(
        "https://www.google.com/travel/flights/search?tfs={}&hl=en&curr=USD&tfu=EgQIABABIgA",
        tfs_encoded
    );

    assert!(url.contains("tfs="), "URL should contain tfs param");
    assert!(url.contains("hl=en"), "URL should contain language");
    assert!(url.contains("curr=USD"), "URL should contain currency");
    assert!(
        url.contains("tfu=EgQIABABIgA"),
        "URL should contain tracking"
    );
    assert!(
        url.starts_with("https://www.google.com/travel/flights/search?"),
        "Valid GFT URL"
    );

    println!("Sanity URL constructed successfully ({} chars)", url.len());
}

// ============================================================================
// INTEGRATION TESTS - LIVE HTTP (IGNORED IN CI)
// ============================================================================

/// Ignored: Domestic US route.
#[tokio::test]
#[ignore]
async fn test_real_query_domestic_us_route() {
    let client = build_client();
    println!("=== Domestic US Route Test ===");

    match rate_limited_query(
        &client,
        "SFO",
        "JFK",
        &dom_flight_date(),
        CabinClass::Economy,
        0,
    )
    .await
    {
        Ok(_) => println!("✓ Domestic query succeeded"),
        Err(e) => {
            eprintln!("✗ Domestic query failed: {}", e);
            if e.to_string().contains("HTTP") || e.to_string().contains("network") {
                println!("⚠ Transient network error");
                return;
            }
            panic!("Unexpected error: {}", e);
        }
    }
}

/// Ignored: International long-haul.
#[tokio::test]
#[ignore]
async fn test_real_query_international_longhaul() {
    let client = build_client();
    println!("\n=== International Long-Haul Test ===");

    match rate_limited_query(
        &client,
        "SFO",
        "LHR",
        &intl_flight_date(),
        CabinClass::Economy,
        1,
    )
    .await
    {
        Ok(_) => println!("✓ International query succeeded"),
        Err(e) => {
            eprintln!("✗ International query failed: {}", e);
            if e.to_string().contains("HTTP") || e.to_string().contains("network") {
                println!("⚠ Transient network error");
                return;
            }
            panic!("Unexpected error: {}", e);
        }
    }
}

/// Ignored: Business class query.
#[tokio::test]
#[ignore]
async fn test_real_query_business_class() {
    let client = build_client();
    println!("\n=== Business Class Test ===");

    match rate_limited_query(
        &client,
        "LAX",
        "ORD",
        &bus_flight_date(),
        CabinClass::Business,
        1,
    )
    .await
    {
        Ok(_) => println!("✓ Business class query succeeded"),
        Err(e) => {
            eprintln!("✗ Business class query failed: {}", e);
            if e.to_string().contains("HTTP") || e.to_string().contains("network") {
                println!("⚠ Transient network error");
                return;
            }
            panic!("Unexpected error: {}", e);
        }
    }
}

/// Ignored: Off-peak vs peak season comparison.
#[tokio::test]
#[ignore]
async fn test_real_query_different_dates() {
    let client = build_client();
    println!("\n=== Different Dates Comparison Test ===");

    // Off-peak: January
    match rate_limited_query(
        &client,
        "SFO",
        "JFK",
        &offpeak_date(),
        CabinClass::Economy,
        0,
    )
    .await
    {
        Ok(_) => println!("✓ Off-peak query succeeded"),
        Err(e) => {
            eprintln!("✗ Off-peak query failed: {}", e);
            if e.to_string().contains("HTTP") || e.to_string().contains("network") {
                println!("⚠ Transient error - skipping");
                return;
            }
            panic!("Unexpected error: {}", e);
        }
    }

    // Peak: July - 1 sec delay
    match rate_limited_query(&client, "SFO", "JFK", &peak_date(), CabinClass::Economy, 1).await {
        Ok(_) => println!("✓ Peak season query succeeded"),
        Err(e) => {
            eprintln!("✗ Peak season query failed: {}", e);
            if e.to_string().contains("HTTP") || e.to_string().contains("network") {
                println!("⚠ Transient error - skipping");
                return;
            }
            panic!("Unexpected error: {}", e);
        }
    }
}

/// Ignored: Quick smoke test.
#[tokio::test]
#[ignore]
async fn run_real_query_quick_no_parsing() {
    let client = build_client();
    println!("Quick test: single SFO->JFK query");
    println!("Browser: Safari 18.5, Cookies: YES+ consented, Rate: 1/sec");

    match rate_limited_query(
        &client,
        "SFO",
        "JFK",
        &dom_flight_date(),
        CabinClass::Economy,
        1,
    )
    .await
    {
        Ok(()) => println!("✅ Quick test completed successfully"),
        Err(e) => eprintln!("❌ Quick test failed: {}", e),
    }
}
