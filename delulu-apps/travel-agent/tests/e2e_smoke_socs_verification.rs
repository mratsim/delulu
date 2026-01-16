//! SOCS Cookie Smoke Test - Verifies consent_cookie.rs works correctly
//!
//! Run: cargo test --test e2e_smoke_socs_verification -- --ignored --nocapture

use anyhow::Result;
use base64::{engine::general_purpose::STANDARD, Engine as _};
use delulu_travel_agent::consent_cookie;
use wreq::redirect::Policy;
use wreq_util::Emulation;

fn build_wreq_client() -> wreq::Client {
    wreq::Client::builder()
        .emulation(Emulation::Safari18_5)
        .redirect(Policy::default())
        .build()
        .expect("wreq client")
}

fn is_consent_page(text: &str) -> bool {
    text.to_lowercase().contains("consent")
}

fn has_flight_content(text: &str) -> bool {
    text.to_lowercase().contains("flight")
}

fn has_hotel_content(text: &str) -> bool {
    text.to_lowercase().contains("hotel")
}

// =============================================================================
// ONLINE TESTS (--ignored)
// =============================================================================

#[tokio::test]
#[ignore]
async fn test_flights_socs() -> Result<()> {
    println!("\n=== Flights + SOCS ===");
    let client = build_wreq_client();
    let header = consent_cookie::generate_cookie_header();
    let resp = client
        .get("https://www.google.com/travel/flights?q=sfo+to+lax")
        .header("Cookie", &header)
        .send()
        .await?;
    let txt = resp.text().await?;
    assert!(!is_consent_page(&txt), "Not blocked");
    assert!(has_flight_content(&txt), "Has flight content");
    println!("test_flights_socs: PASS (not blocked, has flight content)");
    Ok(())
}

#[tokio::test]
#[ignore]
async fn test_hotels_socs() -> Result<()> {
    println!("\n=== Hotels + SOCS ===");
    let client = build_wreq_client();
    let header = consent_cookie::generate_cookie_header();
    let resp = client
        .get("https://www.google.com/travel/search?tokyo")
        .header("Cookie", &header)
        .send()
        .await?;
    let txt = resp.text().await?;
    assert!(!is_consent_page(&txt), "Not blocked");
    assert!(has_hotel_content(&txt), "Has hotel content");
    println!("test_hotels_socs: PASS (not blocked, has flight content)");
    Ok(())
}
