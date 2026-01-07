//! Google Flights client using prost for protobuf encoding.
//!
//! Integrates with [`crate::flights_proto`] for TFS encoding and
//! [`crate::consent_cookie`] for SOCS cookie generation.

use std::sync::Arc;

use anyhow::{anyhow, ensure, Context, Result};
use base64::Engine as _;
use scraper::{Html, Selector};
use tracing::warn;

use once_cell::sync::Lazy;
use regex::Regex;

use crate::consent_cookie::generate_cookie_header;
use crate::flights_proto::encode_flight_config;
use delulu_query_queues::QueryQueue;

const FORCE_USD: &str = "USD"; // TODO: For now we only support USD

// =============================================================================
// Cookie Management
// =============================================================================
// SOCS cookies are now generated fresh daily via search_cookie module.
// Static/stale cookies cause 302 redirects.

// =============================================================================
// Client Configuration
// =============================================================================

// =============================================================================
// Cabin Class Enum
// =============================================================================

#[derive(Debug, Clone, Copy, Default, clap::ValueEnum)]
pub enum CabinClass {
    #[default]
    Economy = 1,
    PremiumEconomy = 2,
    Business = 3,
    First = 4,
}

// =============================================================================
// HTTP Client
// =============================================================================

#[derive(Clone)]
pub struct GoogleFlightsClient {
    client: Arc<wreq::Client>,
    query_queue: QueryQueue,
    language: String,
    currency: String,
}

impl GoogleFlightsClient {
    /// Create a new client with Safari browser emulation enabled.
    pub fn new(language: String, currency: String, max_concurrent: u64) -> Result<Self> {
        // Enable auto-redirect (default policy) - we'll trust the SOCS cookie
        let client = wreq::Client::builder()
            .emulation(wreq_util::Emulation::Safari18_5)
            .redirect(wreq::redirect::Policy::default())
            .build()
            .context("Failed to build wreq client with browser emulation")?;

        let query_queue = QueryQueue::with_max_concurrent(max_concurrent);
        Ok(Self {
            client: Arc::new(client),
            query_queue,
            language,
            currency,
        })
    }

    /// Search for flights using the provided configuration.
    ///
    /// Takes a FlightSearchConfig which contains all necessary parameters
    /// including origin, destination, date, cabin class, and passengers.
    pub async fn search(&self, config: &FlightSearchConfig) -> Result<FlightSearchResult> {
        // Create Tfs payload using the unified type

        if self.currency != FORCE_USD {
            warn!("travel-agent: Only USD currency is supported at the moment. Forcing USD.")
        }

        let tfs = Tfs::from_config(config, &self.language, FORCE_USD)?;
        let url = tfs.get_url();

        tracing::info!(url = %url, "Generated search URL");

        // Generate fresh SOCS cookie (avoids 302 redirect from stale cookies)
        let cookie_header = generate_cookie_header(&self.language, None)
            .map_err(|e| anyhow!("Failed to generate SOCS cookie: {:?}", e))?;

        // Clone client arc for use in retry closure (must be cloneable for FnMut closure)
        let client_inner = Arc::clone(&self.client);

        let response = self
            .query_queue
            .with_retry(move || {
                let url = url.clone();
                let cookie = cookie_header.clone();
                let http_client = client_inner.clone();
                async move {
                    let resp = http_client
                        .get(&url)
                        .header("Cookie", &cookie)
                        .send()
                        .await?;
                    Ok(resp)
                }
            })
            .await
            .map_err(|e| anyhow!("Request failed: {:?}", e))?;

        let text = response.text().await.context("Read response body")?;
        let parsed = parse_html_response(&text)?;
        let itineraries = convert_to_itineraries(parsed, config);

        Ok(FlightSearchResult {
            itineraries,
            generated_at: chrono::Utc::now().to_rfc3339(),
        })
    }
}

impl Default for GoogleFlightsClient {
    fn default() -> Self {
        Self::new("en".into(), "USD".into(), 4).expect(
            "GoogleFlightsClient::default() requires wreq client to initialize successfully",
        )
    }
}

// =============================================================================
// CSS Selectors for HTML Parsing
// =============================================================================

#[derive(Clone)]
pub struct FlightSelectors {
    best_container: Selector,
    other_containers: Selector,
    flight_card: Selector,
    airline: Selector,
    times: Selector,
    duration: Selector,
    stops: Selector,
    arrives_next_day: Selector,
    price: Selector,
    banner_price: Selector,
}

impl FlightSelectors {
    fn new() -> Self {
        Self {
            best_container: Selector::parse(r#"div[jsname="IWWDBc"]"#).unwrap(),
            other_containers: Selector::parse(r#"div[jsname="YdtKid"]"#).unwrap(),
            flight_card: Selector::parse(r#"ul.Rk10dc li"#).unwrap(),
            airline: Selector::parse(r#"div.sSHqwe.tPgKwe.ogfYpf span"#).unwrap(),
            times: Selector::parse(r#"span.mv1WYe div"#).unwrap(),
            duration: Selector::parse(r#"li div.Ak5kof div"#).unwrap(),
            stops: Selector::parse(r#".BbR8Ec .ogfYpf"#).unwrap(),
            arrives_next_day: Selector::parse(r#"span.bOzv6"#).unwrap(),
            price: Selector::parse(r#".YMlIz.FpEdX"#).unwrap(),
            banner_price: Selector::parse(r#"span.gOatQ"#).unwrap(),
        }
    }
}

// =============================================================================
// Passenger Types (matches protobuf enum values)
// =============================================================================

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PassengerType {
    Adult = 1,
    Child = 2,
    InfantInSeat = 3,
    InfantOnLap = 4,
}

// =============================================================================
// Flight Search Configuration
// =============================================================================

#[derive(Debug, Clone, Default)]
pub struct FlightSearchConfig {
    pub from_airport: String,
    pub to_airport: String,
    pub depart_date: chrono::NaiveDate,
    pub cabin_class: CabinClass,
    pub passengers: Vec<(PassengerType, u32)>,
    pub trip_type: TripType,
    pub max_stops: Option<i32>,
    pub preferred_airlines: Option<Vec<String>>,
}

impl FlightSearchConfig {
    fn validate_passengers(&self) -> Result<()> {
        let adults: u32 = self
            .passengers
            .iter()
            .filter(|(t, _)| *t == PassengerType::Adult)
            .map(|(_, count)| count)
            .sum();

        let infants_on_lap: u32 = self
            .passengers
            .iter()
            .filter(|(t, _)| *t == PassengerType::InfantOnLap)
            .map(|(_, count)| count)
            .sum();

        ensure!(adults > 0, "At least one adult is required");
        ensure!(
            infants_on_lap <= adults,
            "Cannot have more infants on lap ({}) than adults ({})",
            infants_on_lap,
            adults
        );
        Ok(())
    }
}

// =============================================================================
// Trip Type
// =============================================================================

#[derive(Debug, Clone, Copy, Default, clap::ValueEnum)]
pub enum TripType {
    #[default]
    RoundTrip = 1,
    OneWay = 2,
    MultiCity = 3,
}

// =============================================================================
// Protobuf Encoding (via prost)
// =============================================================================

/// Encodes a FlightSearchConfig into TFS protobuf bytes.
/// Validates passenger requirements before encoding.
pub fn encode_tfs(config: &FlightSearchConfig) -> Result<Vec<u8>> {
    config.validate_passengers()?;

    let passenger_pairs: Vec<(i32, u32)> = config
        .passengers
        .iter()
        .map(|(ptype, count)| (*ptype as i32, *count))
        .collect();

    encode_flight_config(
        &config.from_airport,
        &config.to_airport,
        config.depart_date,
        config.cabin_class as i32,
        config.trip_type as i32,
        &passenger_pairs,
        config.max_stops,
        config.preferred_airlines.as_deref(),
    )
}

// =============================================================================
// TFS Payload Type
// =============================================================================

/// Represents a Google Flights TFS payload.
///
/// This type encapsulates the encoded TFS bytes and provides methods
/// for generating complete search URLs. Fields are private to enforce
/// encapsulation and prevent accidental modification after creation.
#[derive(Debug, Clone)]
pub struct Tfs {
    /// Base64-encoded TFS protobuf payload
    encoded_tfs: String,
    /// Response language (e.g., "en")
    language: String,
    /// Pricing currency (e.g., "USD", "EUR")
    currency: String,
}

impl Tfs {
    /// Creates a TFS payload from a flight search configuration.
    ///
    /// This validates the configuration and encodes it into the TFS protobuf format,
    /// then base64-encodes the result for URL inclusion.
    ///
    /// # Errors
    /// Returns an error if configuration validation fails or protobuf encoding fails.
    pub fn from_config(
        config: &FlightSearchConfig,
        language: &str,
        currency: &str,
    ) -> Result<Self> {
        let tfs_bytes = encode_tfs(config)?;
        let encoded_tfs = base64::engine::general_purpose::STANDARD.encode(&tfs_bytes);

        Ok(Self {
            encoded_tfs,
            language: language.to_string(),
            currency: currency.to_string(),
        })
    }

    /// Generates a complete Google Flights search URL.
    ///
    /// Returns a URL ready for HTTP requests, including the TFS payload,
    /// language preference, currency, and tracking parameters.
    pub fn get_url(&self) -> String {
        format!(
            "https://www.google.com/travel/flights/search?tfs={}&hl={}&curr={}&tfu=EgQIABABIgA",
            self.encoded_tfs, self.language, self.currency,
        )
    }
}

// =============================================================================
// HTML Parsing
// =============================================================================

#[derive(Debug, Clone, Default)]
pub struct ParsedFlightResults {
    pub best_price: Option<String>,
    pub flights: Vec<ParsedFlight>,
}

#[derive(Debug, Clone)]
pub struct ParsedFlight {
    pub is_best: bool,
    pub airline: String,
    pub dep_time: String,
    pub arr_time: String,
    pub arrive_plus_days: Option<String>,
    pub duration: String,
    pub stops: i32,
    pub stops_label: String,
    pub price: String,
}

// Regex-based helper to extract price pattern from text (e.g., "$384", "$1,234")
// TODO: This hardcodes "$" as the currency symbol. For international prices (€, £, HKD, etc.)
// we need to capture fixture examples with those currencies and update the regex accordingly.
fn extract_price_from_text(text: &str) -> Option<String> {
    PRICE_RE.find(text).map(|m| m.as_str().to_string())
}

pub fn parse_html_response(html: &str) -> Result<ParsedFlightResults> {
    let selectors = FlightSelectors::new();
    let document = Html::parse_document(html);

    let best_price = document
        .select(&selectors.best_container)
        .next()
        .or_else(|| document.select(&selectors.other_containers).next())
        .and_then(|container| {
            // First try the standard banner_price selector
            container
                .select(&selectors.banner_price)
                .next()
                .map(|el| clean_price(el.text().collect::<String>()))
                // Fall back: scan container text for $ pattern
                .or_else(|| {
                    let container_text = container.text().collect::<String>();
                    extract_price_from_text(&container_text)
                })
        });

    let mut flights = Vec::new();

    for container in document.select(&selectors.other_containers) {
        extract_flights_from_element(container, &selectors, false, &mut flights);
    }

    if let Some(best) = document.select(&selectors.best_container).next() {
        extract_flights_from_element(best, &selectors, true, &mut flights);
    }

    anyhow::ensure!(!flights.is_empty(), "No flights parsed from response");

    Ok(ParsedFlightResults {
        best_price,
        flights,
    })
}

fn extract_flights_from_element<'a>(
    element: scraper::ElementRef<'a>,
    selectors: &FlightSelectors,
    is_best: bool,
    flights: &mut Vec<ParsedFlight>,
) {
    for card in element.select(&selectors.flight_card) {
        if let Some(flight) = parse_single_flight(card, selectors, is_best) {
            flights.push(flight);
        }
    }
}

fn parse_single_flight(
    card: scraper::ElementRef,
    selectors: &FlightSelectors,
    is_best: bool,
) -> Option<ParsedFlight> {
    let airline_el = card.select(&selectors.airline).next()?;
    let airline = airline_el.text().collect();

    let times: Vec<_> = card.select(&selectors.times).collect();
    if times.len() < 2 {
        return None;
    }

    let dep_time = normalize_time(&times[0].text().collect::<String>());
    let arr_time = normalize_time(&times[1].text().collect::<String>());

    let arrive_plus_days = card
        .select(&selectors.arrives_next_day)
        .next()
        .map(|el| el.text().collect());

    let dur_el = card.select(&selectors.duration).next()?;
    let duration = dur_el.text().collect();

    let stops_el = card.select(&selectors.stops).next()?;
    let stops_label: String = stops_el.text().collect();
    let stops = if stops_label.contains("Nonstop") {
        0
    } else {
        stops_label
            .split_whitespace()
            .next()
            .and_then(|s| s.parse().ok())
            .unwrap_or_else(|| {
                tracing::warn!("Could not parse number of stops from: '{}'", stops_label);
                1 // Fallback to 1 stop
            })
    };

    let price_el = card.select(&selectors.price).next()?;
    let price = clean_price(price_el.text().collect());

    Some(ParsedFlight {
        is_best,
        airline,
        dep_time,
        arr_time,
        arrive_plus_days,
        duration,
        stops,
        stops_label,
        price,
    })
}

fn clean_price(s: String) -> String {
    s.chars().filter(|c| c.is_ascii_digit()).collect()
}

// =============================================================================
// Domain Models
// =============================================================================

#[derive(Debug, Clone, Default)]
pub struct FlightSearchResult {
    pub itineraries: Vec<Itinerary>,
    pub generated_at: String,
}

#[derive(Debug, Clone, Default)]
pub struct Itinerary {
    pub id: String,
    pub flights: Vec<FlightSegment>,
    pub price: Option<i32>,
    pub currency: Option<String>,
    pub duration_minutes: Option<i32>,
    pub stops: Option<i32>,
    pub class: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct FlightSegment {
    pub airline: Option<String>,
    pub flight_number: Option<String>,
    pub departure_airport: Option<String>,
    pub arrival_airport: Option<String>,
    pub departure_time: Option<String>,
    pub arrival_time: Option<String>,
    pub arrival_plus_days: Option<i32>, // +1, +2 days for overnight arrivals
    pub layover_city: Option<String>,   // Connection city for multi-leg flights
    pub duration_minutes: Option<i32>,
    pub aircraft: Option<String>,
}

// =============================================================================
// Result Conversion
// =============================================================================

fn convert_to_itineraries(
    parsed: ParsedFlightResults,
    config: &FlightSearchConfig,
) -> Vec<Itinerary> {
    let class_str = format!("{:?}", config.cabin_class);
    let currency = Some(FORCE_USD.into()); // TODO

    let mut itineraries = Vec::new();
    let base_id = u32::from_str_radix(&config.from_airport, 36).unwrap_or(0) << 16
        | u32::from_str_radix(&config.to_airport, 36).unwrap_or(0);

    for (idx, flight) in parsed.flights.into_iter().enumerate() {
        let id = format!("{:06}{:02}", base_id, idx);

        let arrival_plus_days = flight
            .arrive_plus_days
            .as_ref()
            .and_then(|s| {
                let num = s.trim_start_matches('+').split_whitespace().next()?;
                num.parse().ok()
            })
            .unwrap_or(0);

        // Combine arrival time with +N day marker for display
        // arr_time is a String, not Option, so handle directly
        let combined_arrival = if arrival_plus_days == 0 {
            Some(flight.arr_time)
        } else {
            Some(format!("{} +{}d", flight.arr_time, arrival_plus_days))
        };

        let segments = vec![FlightSegment {
            airline: Some(flight.airline),
            departure_time: Some(flight.dep_time),
            arrival_time: combined_arrival,
            arrival_plus_days: Some(arrival_plus_days),
            duration_minutes: Some(parse_duration(&flight.duration)),
            departure_airport: Some(config.from_airport.clone()),
            arrival_airport: Some(config.to_airport.clone()),
            ..Default::default()
        }];

        let price = flight.price.parse().ok();
        let duration = parse_duration(&flight.duration);

        itineraries.push(Itinerary {
            id,
            flights: segments,
            price,
            currency: currency.clone(),
            duration_minutes: Some(duration),
            stops: Some(flight.stops),
            class: Some(class_str.clone()),
        });
    }

    itineraries
}

static DURATION_H_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"(\d+)\s*h").unwrap());
static DURATION_M_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"(\d+)\s*m").unwrap());

static PRICE_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"\$[0-9,]+").unwrap());

fn normalize_time(s: &str) -> String {
    s.split_whitespace().next().unwrap_or(s).to_string()
}

fn parse_duration(s: &str) -> i32 {
    let s = s.trim();
    if s.is_empty() {
        return 0;
    }

    let hours = DURATION_H_RE
        .captures(s)
        .and_then(|caps| caps.get(1))
        .and_then(|m| m.as_str().parse::<i32>().ok())
        .unwrap_or(0);

    let minutes = DURATION_M_RE
        .captures(s)
        .and_then(|caps| caps.get(1))
        .and_then(|m| m.as_str().parse::<i32>().ok())
        .unwrap_or(0);

    if hours == 0 && minutes == 0 {
        tracing::debug!("Could not parse duration from: '{}'", s);
    }

    hours * 60 + minutes
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_date(s: &str) -> Result<chrono::NaiveDate> {
        chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d")
            .or_else(|_| chrono::NaiveDate::parse_from_str(s, "%Y/%m/%d"))
            .context(format!("Invalid date format: {}", s))
    }

    #[test]
    fn test_date_parsing() {
        assert!(parse_date("2025-06-15").is_ok());
        assert!(parse_date("2025/06/15").is_ok());
        assert!(parse_date("invalid").is_err());
    }

    #[test]
    fn test_bad_date_format() {
        assert!(parse_date("15-06-2025").is_err());
        assert!(parse_date("Jun 15, 2025").is_err());
    }

    #[test]
    fn test_duration_parsing() {
        assert_eq!(parse_duration("6h 30m"), 390);
        assert_eq!(parse_duration("1h"), 60);
        assert_eq!(parse_duration("45m"), 45);
        assert_eq!(parse_duration(""), 0);
    }

    #[test]
    fn test_encode_decode_roundtrip() {
        let config = FlightSearchConfig {
            from_airport: "SFO".into(),
            to_airport: "JFK".into(),
            depart_date: chrono::NaiveDate::from_ymd_opt(2025, 7, 15).unwrap(),
            cabin_class: CabinClass::Economy,
            passengers: vec![(PassengerType::Adult, 1)],
            trip_type: TripType::OneWay,
            max_stops: Some(0),
            preferred_airlines: None,
        };

        let encoded = encode_tfs(&config).unwrap();
        assert!(!encoded.is_empty());
    }

    #[test]
    fn test_passenger_validation() {
        let ok_config = FlightSearchConfig {
            passengers: vec![(PassengerType::Adult, 1), (PassengerType::Child, 1)],
            ..Default::default()
        };
        assert!(ok_config.validate_passengers().is_ok());

        let bad_config = FlightSearchConfig {
            passengers: vec![(PassengerType::Adult, 0), (PassengerType::Child, 1)],
            ..Default::default()
        };
        assert!(bad_config.validate_passengers().is_err());
    }

    #[test]
    fn test_cabin_class_value() {
        assert_eq!(CabinClass::Economy as i32, 1);
        assert_eq!(CabinClass::Business as i32, 3);
    }
}
