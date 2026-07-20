//! Minimal thirtyfour WebDriver test for Google Hotels
//!
//! Requirements: geckodriver running on localhost:4444
//! Run with: cargo test --test t_hotels_thirtyfour -- --ignored --nocapture

use anyhow::Result;
use thirtyfour::prelude::*;
use thirtyfour::{By, DesiredCapabilities, WebDriver};

async fn setup_driver() -> Result<WebDriver> {
    let caps = DesiredCapabilities::firefox();
    WebDriver::new("http://localhost:4444", caps)
        .await
        .map_err(|e| anyhow::anyhow!("WebDriver error: {}", e))
}

#[tokio::test]
#[ignore]
async fn test_google_hotels_full_flow() -> Result<()> {
    let driver = setup_driver().await?;

    driver.goto("about:blank").await?;
    println!("SUCCESS: Connected and navigated");

    driver
        .goto("https://www.google.com/travel/search?q=Tokyo")
        .await?;

    let has_consent = driver
        .query(By::Css("button[aria-label*='Accept']"))
        .first()
        .await
        .is_ok();
    let has_hotels = driver
        .query(By::Css("div[data-review-id]"))
        .first()
        .await
        .is_ok();

    println!(
        "Consent button found: {}, Hotels found: {}",
        has_consent, has_hotels
    );

    if has_consent {
        println!("BLOCKED by consent wall - clicking accept...");
        if let Ok(el) = driver
            .query(By::Css("button[aria-label*='Accept']"))
            .first()
            .await
        {
            if let Ok(true) = el.is_displayed().await {
                el.click().await?;
                println!("Clicked accept button");
            }
        }
    }

    let hotels_after_click = driver
        .query(By::Css("div[data-review-id]"))
        .first()
        .await
        .is_ok();
    println!("Hotels found after accept: {}", hotels_after_click);

    let html = driver.source().await?;
    driver.quit().await?;

    let names: Vec<_> = regex::Regex::new(r"<h2[^>]*>([^<]+)</h2>")
        .unwrap()
        .captures_iter(&html)
        .map(|c| c.get(1).unwrap().as_str().to_string())
        .take(10)
        .collect();

    println!("Found {} hotel names:", names.len());
    for (i, name) in names.iter().enumerate() {
        println!("  [{}] {}", i + 1, name);
    }

    if !names.is_empty() {
        println!("SUCCESS: Extracted hotels!");
        Ok(())
    } else {
        Err(anyhow::anyhow!("FAILED: No hotels found"))
    }
}
