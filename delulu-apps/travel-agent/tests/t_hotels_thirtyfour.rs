//! Minimal thirtyfour WebDriver test for Google Hotels
//!
//! Requirements: geckodriver running on localhost:4444
//! Run with: cargo test --test t_hotels_thirtyfour -- --ignored --nocapture

use anyhow::Result;
use std::time::Duration;
use thirtyfour::{DesiredCapabilities, WebDriver};
use tokio::time::sleep;

#[tokio::test]
#[ignore]
async fn test_connection() -> Result<()> {
    println!("Testing WebDriver connection...");

    let caps = DesiredCapabilities::firefox();
    let driver = WebDriver::new("http://localhost:4444", caps).await?;

    driver.goto("about:blank").await?;
    println!("SUCCESS: Connected and navigated");

    driver.quit().await?;
    Ok(())
}

#[tokio::test]
#[ignore]
async fn test_google_consent() -> Result<()> {
    println!("Testing Google Hotels consent...");

    let caps = DesiredCapabilities::firefox();
    let driver = WebDriver::new("http://localhost:4444", caps).await?;

    driver
        .goto("https://www.google.com/travel/search?q=Tokyo")
        .await?;
    sleep(Duration::from_secs(4)).await;

    let html = driver.source().await?;
    let has_consent = html.to_lowercase().contains("before you continue");
    let has_hotels = html.contains("BcKagd");

    println!("Consent: {}, Hotels: {}", has_consent, has_hotels);

    driver.quit().await?;

    if has_consent {
        println!("BLOCKED by consent wall");
    } else if has_hotels {
        println!("SUCCESS: Hotels accessible!");
    }
    Ok(())
}

#[tokio::test]
#[ignore]
async fn test_click_accept() -> Result<()> {
    println!("Testing consent acceptance click...");

    let caps = DesiredCapabilities::firefox();
    let driver = WebDriver::new("http://localhost:4444", caps).await?;

    driver
        .goto("https://www.google.com/travel/search?q=Tokyo")
        .await?;
    sleep(Duration::from_secs(3)).await;

    // Try to find and click accept button
    match driver
        .find(thirtyfour::By::Css("button[aria-label*='Accept']"))
        .await
    {
        Ok(el) => {
            if let Ok(true) = el.is_displayed().await {
                el.click().await?;
                println!("Clicked accept button");
                sleep(Duration::from_secs(2)).await;
            }
        }
        Err(_) => println!("No accept button found"),
    }

    let html = driver.source().await?;
    let has_hotels = html.contains("BcKagd");

    println!("After click - Hotels found: {}", has_hotels);

    driver.quit().await?;

    if has_hotels {
        println!("SUCCESS: Bypassed consent and found hotels!");
    }
    Ok(())
}

#[tokio::test]
#[ignore]
async fn test_extraction() -> Result<()> {
    println!("Testing full extraction...");

    let caps = DesiredCapabilities::firefox();
    let driver = WebDriver::new("http://localhost:4444", caps).await?;

    driver
        .goto("https://www.google.com/travel/search?q=Tokyo")
        .await?;

    // Click accept if present
    if let Ok(el) = driver
        .find(thirtyfour::By::Css("button[aria-label*='Accept']"))
        .await
    {
        let _ = el.click().await;
        println!("Clicked accept");
    }

    sleep(Duration::from_secs(5)).await;

    let html = driver.source().await?;
    driver.quit().await?;

    // Parse hotels with simple regex
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
    } else {
        println!("FAILED: No hotels found");
    }
    Ok(())
}
