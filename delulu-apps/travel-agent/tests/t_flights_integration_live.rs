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

//! Live integration tests for Google Flights search.
//!
//! These tests make actual HTTP requests to Google Flights and verify
//! the parser handles real-world responses correctly.
//!
//! Run with: cargo test --test t_flights_integration_live -- --include-ignored

use anyhow::{Context, Result};
use chrono::{Months, NaiveDate};
use delulu_travel_agent::{FlightSearchParams, GoogleFlightsClient, Seat, Trip};

fn today() -> NaiveDate {
    chrono::Local::now().date_naive()
}

fn parse_date(s: &str) -> Result<NaiveDate> {
    NaiveDate::parse_from_str(s, "%Y-%m-%d")
        .or_else(|_| NaiveDate::parse_from_str(s, "%Y/%m/%d"))
        .context(format!("Invalid date format: {}", s))
}

fn dom_flight_date() -> String {
    (today() + Months::new(2)).format("%Y-%m-%d").to_string()
}

fn intl_flight_date() -> String {
    (today() + Months::new(3)).format("%Y-%m-%d").to_string()
}

fn bus_flight_date() -> String {
    (today() + Months::new(2) + chrono::Days::new(15))
        .format("%Y-%m-%d")
        .to_string()
}

fn next_month() -> String {
    (today() + Months::new(1)).format("%Y-%m-%d").to_string()
}

fn next_semester() -> String {
    (today() + Months::new(7)).format("%Y-%m-%d").to_string()
}

async fn rate_limited_query(
    client: &GoogleFlightsClient,
    from: &str,
    to: &str,
    date: &str,
    cabin: Seat,
    delay_secs: u64,
) -> Result<delulu_travel_agent::FlightSearchResult> {
    let params = delulu_travel_agent::FlightSearchParams::builder(
        from.to_uppercase(),
        to.to_uppercase(),
        parse_date(date)?,
    )
    .cabin_class(cabin)
    .build()?;

    let url = params.get_search_url();
    println!("\n🛫 Query: {} → {} on {} ({:?})", from, to, date, cabin);
    println!("Using Tfs URL generation: {}", url);
    println!("URL length: {} chars", url.len());
    println!("🔗 URL for manual check:\n{}", url);

    tokio::time::sleep(tokio::time::Duration::from_secs(delay_secs)).await;

    let result = client.search_flights(&params).await?;
    println!("Parsed {} itineraries", result.itineraries.len());
    let best_price = result.itineraries.iter().filter_map(|i| i.price).min();
    if let Some(price) = best_price {
        println!("Best price: {} USD", price);
    }

    Ok(result)
}

#[tokio::test]
#[ignore]
async fn test_real_query_domestic_us_route() -> Result<()> {
    let client = GoogleFlightsClient::new("en".into(), "USD".into())?;
    println!("=== Domestic US Route Test ===");

    match rate_limited_query(&client, "SFO", "JFK", &dom_flight_date(), Seat::Economy, 0).await {
        Ok(_) => println!("✓ Domestic query succeeded"),
        Err(e) => {
            eprintln!("✗ Domestic query failed: {}", e);
            if e.to_string().contains("HTTP") || e.to_string().contains("network") {
                println!("⚠ Transient network error");
                return Ok(());
            }
            anyhow::bail!("Unexpected error: {}", e);
        }
    }

    Ok(())
}

#[tokio::test]
#[ignore]
async fn test_real_query_international_longhaul() -> Result<()> {
    let client = GoogleFlightsClient::new("en".into(), "USD".into())?;
    println!("\n=== International Long-Haul Test ===");

    match rate_limited_query(&client, "SFO", "LHR", &intl_flight_date(), Seat::Economy, 1).await {
        Ok(_) => println!("✓ International query succeeded"),
        Err(e) => {
            eprintln!("✗ International query failed: {}", e);
            if e.to_string().contains("HTTP") || e.to_string().contains("network") {
                println!("⚠ Transient network error");
                return Ok(());
            }
            anyhow::bail!("Unexpected error: {}", e);
        }
    }

    Ok(())
}

#[tokio::test]
#[ignore]
async fn test_real_query_business_class() -> Result<()> {
    let client = GoogleFlightsClient::new("en".into(), "USD".into())?;
    println!("\n=== Business Class Test ===");

    match rate_limited_query(&client, "LAX", "ORD", &bus_flight_date(), Seat::Business, 1).await {
        Ok(_) => println!("✓ Business class query succeeded"),
        Err(e) => {
            eprintln!("✗ Business class query failed: {}", e);
            if e.to_string().contains("HTTP") || e.to_string().contains("network") {
                println!("⚠ Transient network error");
                return Ok(());
            }
            anyhow::bail!("Unexpected error: {}", e);
        }
    }

    Ok(())
}

#[tokio::test]
#[ignore]
async fn test_real_query_different_dates() -> Result<()> {
    let client = GoogleFlightsClient::new("en".into(), "USD".into())?;
    println!("\n=== Different Dates Comparison Test ===");

    match rate_limited_query(&client, "SFO", "JFK", &next_month(), Seat::Economy, 0).await {
        Ok(_) => println!("✓ Off-peak query succeeded"),
        Err(e) => {
            eprintln!("✗ Off-peak query failed: {}", e);
            if e.to_string().contains("HTTP") || e.to_string().contains("network") {
                println!("⚠ Transient error - skipping");
                return Ok(());
            }
            anyhow::bail!("Unexpected error: {}", e);
        }
    }

    match rate_limited_query(&client, "SFO", "JFK", &next_semester(), Seat::Economy, 1).await {
        Ok(_) => println!("✓ Peak season query succeeded"),
        Err(e) => {
            eprintln!("✗ Peak season query failed: {}", e);
            if e.to_string().contains("HTTP") || e.to_string().contains("network") {
                println!("⚠ Transient error - skipping");
                return Ok(());
            }
            anyhow::bail!("Unexpected error: {}", e);
        }
    }

    Ok(())
}

#[tokio::test]
#[ignore]
async fn test_real_query_quick_smoke() -> Result<()> {
    let client = GoogleFlightsClient::new("en".into(), "USD".into())?;
    println!("Quick test: single SFO->JFK query");
    println!("Browser: Safari 18.5, Cookies: YES+ consented");

    match rate_limited_query(&client, "SFO", "JFK", &dom_flight_date(), Seat::Economy, 1).await {
        Ok(_) => println!("✅ Quick test completed successfully"),
        Err(e) => anyhow::bail!("❌ Quick test failed: {}", e),
    }

    Ok(())
}

#[tokio::test]
#[ignore]
async fn test_real_query_overnight_plus_two_days() -> Result<()> {
    let client = GoogleFlightsClient::new("en".into(), "USD".into())?;
    let date = (today() + Months::new(2)).format("%Y-%m-%d").to_string();
    println!("\n=== Overnight +2 Days Test ===");
    println!("Querying: SFO -> LHR on {} (testing +2 day arrival)", date);

    let params = delulu_travel_agent::FlightSearchParams::builder(
        "SFO".to_uppercase(),
        "LHR".to_uppercase(),
        parse_date(&date)?,
    )
    .cabin_class(Seat::Economy)
    .build()?;

    let url = params.get_search_url();
    println!("URL: {}", url);

    tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
    let result = client.search_flights(&params).await?;

    assert!(
        !result.raw_response.contains("consent.google.com"),
        "Should not hit consent wall"
    );
    assert!(
        !result.itineraries.is_empty(),
        "Should parse at least one itinerary"
    );

    Ok(())
}

#[tokio::test]
#[ignore]
async fn test_real_query_premium_economy() -> Result<()> {
    let client = GoogleFlightsClient::new("en".into(), "USD".into())?;
    let date = intl_flight_date();
    println!("\n=== Premium Economy Test ===");

    match rate_limited_query(&client, "LAX", "CDG", &date, Seat::PremiumEconomy, 1).await {
        Ok(_) => println!("✓ Premium economy query succeeded"),
        Err(e) => {
            eprintln!("✗ Premium economy query failed: {}", e);
            if e.to_string().contains("HTTP") || e.to_string().contains("network") {
                println!("⚠ Transient network error");
                return Ok(());
            }
            anyhow::bail!("Unexpected error: {}", e);
        }
    }

    Ok(())
}

#[tokio::test]
#[ignore]
async fn test_real_query_first_class() -> Result<()> {
    let client = GoogleFlightsClient::new("en".into(), "USD".into())?;
    let date = intl_flight_date();
    println!("\n=== First Class Test ===");

    match rate_limited_query(&client, "JFK", "DXB", &date, Seat::First, 1).await {
        Ok(_) => println!("✓ First class query succeeded"),
        Err(e) => {
            eprintln!("✗ First class query failed: {}", e);
            if e.to_string().contains("HTTP") || e.to_string().contains("network") {
                println!("⚠ Transient network error");
                return Ok(());
            }
            anyhow::bail!("Unexpected error: {}", e);
        }
    }

    Ok(())
}

#[tokio::test]
#[ignore]
async fn test_real_query_oneway() -> Result<()> {
    let client = GoogleFlightsClient::new("en".into(), "USD".into())?;
    let date = intl_flight_date();
    println!("\n=== One-Way Test ===");

    let params = delulu_travel_agent::FlightSearchParams::builder(
        "LAX".to_uppercase(),
        "NRT".to_uppercase(),
        parse_date(&date)?,
    )
    .cabin_class(Seat::Economy)
    .trip_type(Trip::OneWay)
    .build()?;

    let url = params.get_search_url();
    println!("Query: LAX → NRT on {} (OneWay)", date);
    println!("URL: {}", url);

    tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
    let result = client.search_flights(&params).await?;

    assert!(
        !result.raw_response.contains("consent.google.com"),
        "Should not hit consent wall"
    );

    Ok(())
}

#[tokio::test]
#[ignore]
async fn test_real_query_response_structure() -> Result<()> {
    let client = GoogleFlightsClient::new("en".into(), "USD".into())?;
    let date = dom_flight_date();
    println!("\n=== Response Structure Test ===");

    let result = rate_limited_query(&client, "SFO", "LAX", &date, Seat::Economy, 5).await?;

    let has_structure_markers =
        result.raw_response.contains("jsname") || result.raw_response.contains("Rk10dc");

    if has_structure_markers {
        println!("✓ Response contains expected HTML structure markers");
    } else {
        println!("⚠ Response may have new structure - manual inspection needed");
    }

    Ok(())
}

// =============================================================================
// FIXTURE FETCHING TESTS (IGNORED - FOR SETUP ONLY)
// =============================================================================
// These tests fetch HTML from Google Flights and save as compressed fixtures.
// Run with: cargo test --test t_flights_integration_live fetch_fixture_xxx -- --ignored --nocapture
// Rate limited to 3 seconds between requests to avoid being banned.

const FLIGHT_FIXTURE_RATE_LIMIT_SECS: u64 = 3;

fn compress_and_save_flight(html: &str, name: &str) {
    use std::fs;
    use std::path::Path;

    let fixtures_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures-flights-parsing");
    fs::create_dir_all(&fixtures_dir).expect("create fixtures dir");

    let output_path = fixtures_dir.join(format!("{}.html.zst", name));
    let file = fs::File::create(&output_path).expect("create output file");

    let mut encoder = zstd::stream::Encoder::new(file, 0).expect("create zstd encoder");
    use std::io::Write;
    encoder.write_all(html.as_bytes()).expect("write bytes");
    encoder.finish().expect("finish compression");

    println!("Saved flight fixture: {:?}", output_path);
}

async fn rate_limited_flight_fetch(
    client: &GoogleFlightsClient,
    params: &FlightSearchParams,
    delay_secs: u64,
    name: &str,
) -> Result<String, Box<dyn std::error::Error>> {
    if delay_secs > 0 {
        tokio::time::sleep(tokio::time::Duration::from_secs(delay_secs)).await;
    }
    fetch_single_flight_fixture(client, params, name).await
}

async fn fetch_single_flight_fixture(
    client: &GoogleFlightsClient,
    params: &FlightSearchParams,
    name: &str,
) -> Result<String, Box<dyn std::error::Error>> {
    let url = params.get_search_url();
    let url_display = &url[0..url.len().min(100)];
    println!("Fetching flight '{}': {}", name, url_display);

    let result = client.search_flights(params).await?;
    let text = result.raw_response;

    println!(
        "Response size: {} bytes, itineraries: {}",
        text.len(),
        result.itineraries.len()
    );

    if text.to_lowercase().contains("consent") {
        return Err(anyhow::anyhow!("Blocked by consent cookie").into());
    }
    if text.len() < 1000 {
        let preview = text.chars().take(500).collect::<String>();
        return Err(
            anyhow::anyhow!("Response too short ({} bytes): {}", text.len(), preview).into(),
        );
    }

    if result.itineraries.is_empty() {
        let preview = text.chars().take(1000).collect::<String>();
        eprintln!("No flights parsed. Body preview:\n{}", preview);
        return Err(anyhow::anyhow!("No flights parsed from response").into());
    }

    Ok(text)
}

#[tokio::test]
#[ignore]
async fn fetch_fixture_sfo_jfk_nonstop() {
    let client = GoogleFlightsClient::new("en".into(), "USD".into()).expect("client");

    let today = chrono::Local::now().date_naive();
    let depart = today + Months::new(2);

    let params = delulu_travel_agent::FlightSearchParams::builder(
        "SFO".to_uppercase(),
        "JFK".to_uppercase(),
        depart,
    )
    .cabin_class(Seat::Economy)
    .build()
    .expect("params should build");

    match rate_limited_flight_fetch(
        &client,
        &params,
        FLIGHT_FIXTURE_RATE_LIMIT_SECS,
        "nonstop-sfo_jfk_economy",
    )
    .await
    {
        Ok(text) => compress_and_save_flight(&text, "nonstop-sfo_jfk_economy"),
        Err(e) => panic!("Failed: {}", e),
    }
}

#[tokio::test]
#[ignore]
async fn fetch_fixture_lax_ord_business() {
    let client = GoogleFlightsClient::new("en".into(), "USD".into()).expect("client");

    let today = chrono::Local::now().date_naive();
    let depart = today + Months::new(2) + chrono::Duration::days(15);

    let params = delulu_travel_agent::FlightSearchParams::builder(
        "LAX".to_uppercase(),
        "ORD".to_uppercase(),
        depart,
    )
    .cabin_class(Seat::Business)
    .build()
    .expect("params should build");

    match rate_limited_flight_fetch(
        &client,
        &params,
        FLIGHT_FIXTURE_RATE_LIMIT_SECS,
        "domestic+business-lax_ord",
    )
    .await
    {
        Ok(text) => compress_and_save_flight(&text, "domestic+business-lax_ord"),
        Err(e) => panic!("Failed: {}", e),
    }
}

#[tokio::test]
#[ignore]
async fn fetch_fixture_sfo_lhr_overnight() {
    let client = GoogleFlightsClient::new("en".into(), "USD".into()).expect("client");

    let today = chrono::Local::now().date_naive();
    let depart = today + Months::new(2);

    let params = delulu_travel_agent::FlightSearchParams::builder(
        "SFO".to_uppercase(),
        "LHR".to_uppercase(),
        depart,
    )
    .build()
    .expect("params should build");

    match rate_limited_flight_fetch(
        &client,
        &params,
        FLIGHT_FIXTURE_RATE_LIMIT_SECS,
        "overnight+1day-sfo_lhr_economy",
    )
    .await
    {
        Ok(text) => compress_and_save_flight(&text, "overnight+1day-sfo_lhr_economy"),
        Err(e) => panic!("Failed: {}", e),
    }
}

#[tokio::test]
#[ignore]
async fn fetch_fixture_lax_syd_longhaul() {
    let client = GoogleFlightsClient::new("en".into(), "USD".into()).expect("client");

    let today = chrono::Local::now().date_naive();
    let depart = today + Months::new(3);

    let params = delulu_travel_agent::FlightSearchParams::builder(
        "LAX".to_uppercase(),
        "SYD".to_uppercase(),
        depart,
    )
    .build()
    .expect("params should build");

    match rate_limited_flight_fetch(
        &client,
        &params,
        FLIGHT_FIXTURE_RATE_LIMIT_SECS,
        "longhaul-lax_syd",
    )
    .await
    {
        Ok(text) => compress_and_save_flight(&text, "longhaul-lax_syd"),
        Err(e) => panic!("Failed: {}", e),
    }
}

#[tokio::test]
#[ignore]
async fn fetch_fixture_mad_nrt_layover() {
    let client = GoogleFlightsClient::new("en".into(), "USD".into()).expect("client");

    let today = chrono::Local::now().date_naive();
    let depart = today + Months::new(3);

    let params = delulu_travel_agent::FlightSearchParams::builder(
        "MAD".to_uppercase(),
        "NRT".to_uppercase(),
        depart,
    )
    .build()
    .expect("params should build");

    match rate_limited_flight_fetch(
        &client,
        &params,
        FLIGHT_FIXTURE_RATE_LIMIT_SECS,
        "layover-mad_nrt",
    )
    .await
    {
        Ok(text) => compress_and_save_flight(&text, "layover-mad_nrt"),
        Err(e) => panic!("Failed: {}", e),
    }
}

#[tokio::test]
#[ignore]
async fn fetch_fixture_yyz_cdg_layover() {
    let client = GoogleFlightsClient::new("en".into(), "USD".into()).expect("client");

    let today = chrono::Local::now().date_naive();
    let depart = today + Months::new(2);

    let params = delulu_travel_agent::FlightSearchParams::builder(
        "YYZ".to_uppercase(),
        "CDG".to_uppercase(),
        depart,
    )
    .cabin_class(Seat::Economy)
    .build()
    .expect("params should build");

    match rate_limited_flight_fetch(
        &client,
        &params,
        FLIGHT_FIXTURE_RATE_LIMIT_SECS,
        "layover-yyz_cdg",
    )
    .await
    {
        Ok(text) => compress_and_save_flight(&text, "layover-yyz_cdg"),
        Err(e) => panic!("Failed: {}", e),
    }
}
