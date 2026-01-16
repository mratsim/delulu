//! Hardcoded Browser Cookies Test for Hotels
//!
//! Run: cargo test --test t_hardcoded_browser_cookies -- --ignored --nocapture

use anyhow::Result;
use wreq::redirect::Policy;
use wreq_util::Emulation;

fn build_wreq_client() -> wreq::Client {
    wreq::Client::builder()
        .emulation(Emulation::Safari18_5)
        .redirect(Policy::default())
        .build()
        .expect("wreq client")
}

fn extract_hotel_names(html: &str) -> Vec<String> {
    let mut names = Vec::new();
    let patterns = [
        r#"<h2[^>]*>([^<]{3,100})</h2>"#,
        r#"data-name="([^"]+)""#,
        r#"class="[^"]*result[^"]*"[^>]*>([^<]{3,100})"#,
    ];
    for pattern in &patterns {
        if let Ok(re) = regex::Regex::new(pattern) {
            for cap in re.captures_iter(html) {
                if let Some(m) = cap.get(1) {
                    let name = m.as_str().trim().to_string();
                    if name.len() > 3
                        && !name.to_lowercase().contains("popular")
                        && !name.to_lowercase().contains("destination")
                        && !name.to_lowercase().contains("promo")
                    {
                        if !names.contains(&name) {
                            names.push(name);
                        }
                    }
                }
            }
        }
    }
    names
}

#[tokio::test]
#[ignore]
async fn test_socs_only_from_browser() -> Result<()> {
    println!("\n========================================");
    println!("CRITICAL: Browser SOCS Only for Hotels");
    println!("========================================\n");

    let client = build_wreq_client();

    // HARDCODED browser SOCS (from browser capture)
    let browser_socs = "CAISOAgMEitib3FfaWRlbnRpdHlmcm9udGVuZHVpc2VydmVyXzIwMjYwMTA2LjAzX3AwGgVlbi1VUyACGgYIgMT2ygY";
    let header = format!("SOCS={}", browser_socs);

    println!("Using HARDCODED browser SOCS:");
    println!("SOCS={}", &browser_socs[..40]);

    let resp = client
        .get("https://www.google.com/travel/search?q=tokyo&ts=test")
        .header("Cookie", &header)
        .send()
        .await?;

    let text = resp.text().await?;
    let blocked = text.to_lowercase().contains("consent");
    let has_hotel = text.to_lowercase().contains("hotel");

    println!("\nResult:");
    println!("Blocked: {}, Has hotel: {}", blocked, has_hotel);

    // Extract hotels found
    let hotels = extract_hotel_names(&text);
    println!("\n📋 Hotels found: {}", hotels.len());
    for (i, name) in hotels.iter().take(10).enumerate() {
        println!("  {}. {}", i + 1, name);
    }
    if hotels.len() > 10 {
        println!("  ... and {} more", hotels.len() - 10);
    }

    if !blocked && has_hotel {
        println!("\n✅ SUCCESS: Browser SOCS alone works!");
    } else if blocked {
        println!("\n❌ BLOCKED");
    } else {
        println!("\n⚠️  Not blocked but no hotel content");
    }
    Ok(())
}

#[tokio::test]
#[ignore]
async fn test_our_vs_browser_socs() -> Result<()> {
    println!("\n========================================");
    println!("COMPARE: Browser SOCS vs Our Generated SOCS");
    println!("========================================\n");

    let client = build_wreq_client();

    // Browser SOCS
    let browser_socs = "CAISOAgMEitib3FfaWRlbnRpdHlmcm9udGVuZHVpc2VydmVyXzIwMjYwMTA2LjAzX3AwGgVlbi1VUyACGgYIgMT2ygY";
    let browser_header = format!("CONSENT=PENDING+987; SOCS={}", browser_socs);

    // Our SOCS
    let our_header = delulu_travel_agent::consent_cookie::generate_cookie_header();
    let our_socs = our_header
        .split("CONSENT=PENDING+987; ")
        .nth(1)
        .expect("parse our SOCS from header");

    println!("Browser SOCS: {} chars", browser_socs.len());
    println!("Our SOCS:     {} chars\n", our_socs.len());

    // Test browser SOCS
    println!("[Test 1] Browser SOCS:");
    let resp1 = client
        .get("https://www.google.com/travel/search?q=tokyo&ts=test")
        .header("Cookie", &browser_header)
        .send()
        .await?;
    let text1 = resp1.text().await?;
    let b_blocked = text1.to_lowercase().contains("consent");
    let b_hotels = extract_hotel_names(&text1);
    println!("Blocked: {}, Hotels: {}", b_blocked, b_hotels.len());
    for (i, name) in b_hotels.iter().take(5).enumerate() {
        println!("  {}. {}", i + 1, name);
    }

    // Test our SOCS
    println!("\n[Test 2] Our SOCS:");
    let resp2 = client
        .get("https://www.google.com/travel/search?q=tokyo&ts=test")
        .header("Cookie", &our_header)
        .send()
        .await?;
    let text2 = resp2.text().await?;
    let o_blocked = text2.to_lowercase().contains("consent");
    let o_hotels = extract_hotel_names(&text2);
    println!("Blocked: {}, Hotels: {}", o_blocked, o_hotels.len());
    for (i, name) in o_hotels.iter().take(5).enumerate() {
        println!("  {}. {}", i + 1, name);
    }

    println!("\n========================================");
    println!("SUMMARY");
    println!("========================================");
    if b_hotels.len() > 0 && o_hotels.len() > 0 {
        let overlap: usize = b_hotels.iter().filter(|n| o_hotels.contains(n)).count();
        println!(
            "Browser hotels: {}, Our hotels: {}, Overlap: {}",
            b_hotels.len(),
            o_hotels.len(),
            overlap
        );
        if overlap > 0 {
            println!("✅ Both retrieve content - formats work equivalently!");
        } else {
            println!("⚠️ Different hotels returned - quality/nature differs");
        }
    } else {
        println!("Browser: {}, Our: {}", b_hotels.len(), o_hotels.len());
    }
    Ok(())
}
