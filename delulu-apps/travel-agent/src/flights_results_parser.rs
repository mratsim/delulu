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

//! # Flights Results Parser
//!
//! Side-effect free HTML parsing for Google Flights search results.
//! Extracts flight information from the HTML response.

use anyhow::Result;
use once_cell::sync::Lazy;
use regex::Regex;
use scraper::{Html, Selector};

use crate::FlightSearchParams;

#[derive(Debug, Clone)]
pub struct FlightSearchResult {
    pub search_params: FlightSearchParams,
    pub itineraries: Vec<Itinerary>,
    pub raw_response: String,
}

impl FlightSearchResult {
    pub fn from_html(html: &str, search_params: FlightSearchParams) -> Result<Self> {
        let flights = parse_flights_response(html)?;
        let itineraries = convert_to_itineraries(
            flights,
            &search_params.from_airport,
            &search_params.to_airport,
        );
        anyhow::ensure!(!itineraries.is_empty(), "No flights parsed from response");
        Ok(Self {
            search_params,
            itineraries,
            raw_response: html.to_string(),
        })
    }

    pub fn len(&self) -> usize {
        self.itineraries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.itineraries.is_empty()
    }
}

#[derive(Debug, Clone)]
pub struct Layover {
    pub airport_code: String,
    pub airport_name: Option<String>,
    pub duration_minutes: Option<i32>,
}

#[derive(Debug, Clone)]
pub struct Itinerary {
    pub id: String,
    pub flights: Vec<FlightSegment>,
    pub price: Option<i32>,
    pub currency: Option<String>,
    pub duration_minutes: Option<i32>,
    pub stops: Option<i32>,
    pub class: Option<String>,
    pub layovers: Vec<Layover>,
}

#[derive(Debug, Clone)]
pub struct FlightSegment {
    pub airline: Option<String>,
    pub flight_number: Option<String>,
    pub departure_airport: Option<String>,
    pub arrival_airport: Option<String>,
    pub departure_time: Option<String>,
    pub arrival_time: Option<String>,
    pub arrival_plus_days: Option<i32>,
    pub duration_minutes: Option<i32>,
    pub aircraft: Option<String>,
}

#[derive(Debug, Clone)]
struct Flight {
    airline: String,
    dep_time: String,
    arr_time: String,
    arrive_plus_days: Option<String>,
    duration: String,
    stops: i32,
    price: String,
    layovers: Vec<Layover>,
}

#[derive(Clone)]
struct FlightSelectors {
    other_containers: Selector,
    flight_card: Selector,
    airline: Selector,
    _flight_number: Selector,
    _aircraft: Selector,
    times: Selector,
    duration: Selector,
    stops: Selector,
    stops_container: Selector,
    arrives_next_day: Selector,
    price: Selector,
}

impl FlightSelectors {
    fn new() -> Self {
        Self {
            other_containers: Selector::parse(r#"div[jsname="YdtKid"]"#).unwrap(),
            flight_card: Selector::parse(r#"ul.Rk10dc li"#).unwrap(),
            airline: Selector::parse(r#"div.sSHqwe.tPgKwe.ogfYpf span"#).unwrap(),
            _flight_number: Selector::parse(r#"span.Xsgmwe.sI2Nye"#).unwrap(),
            _aircraft: Selector::parse(r#"span.Xsgmwe"#).unwrap(),
            times: Selector::parse(r#"span.mv1WYe div"#).unwrap(),
            duration: Selector::parse(r#"li div.Ak5kof div"#).unwrap(),
            stops: Selector::parse(r#".BbR8Ec .ogfYpf"#).unwrap(),
            stops_container: Selector::parse(r#".BbR8Ec .sSHqwe"#).unwrap(),
            arrives_next_day: Selector::parse(r#"span.bOzv6"#).unwrap(),
            price: Selector::parse(r#".YMlIz.FpEdX"#).unwrap(),
        }
    }
}

static DURATION_H_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"(\d+)\s*h").unwrap());
static DURATION_M_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"(\d+)\s*m").unwrap());
static LAYOVER_ARIA_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(\d+)\s*hr\s*(?:(\d+)\s*min)?[^.]*?in\s+([A-Za-z][A-Za-z\s]*)").unwrap()
});

fn parse_flights_response(html: &str) -> Result<Vec<Flight>> {
    let selectors = FlightSelectors::new();
    let document = Html::parse_document(html);

    let mut flights = Vec::new();

    for container in document.select(&selectors.other_containers) {
        extract_flights_from_element(container, &selectors, &mut flights);
    }

    anyhow::ensure!(!flights.is_empty(), "No flights parsed from response");
    Ok(flights)
}

fn extract_flights_from_element<'a>(
    element: scraper::ElementRef<'a>,
    selectors: &FlightSelectors,
    flights: &mut Vec<Flight>,
) {
    for card in element.select(&selectors.flight_card) {
        if let Some(flight) = parse_single_flight(card, selectors) {
            flights.push(flight);
        }
    }
}

fn parse_single_flight(card: scraper::ElementRef, _selectors: &FlightSelectors) -> Option<Flight> {
    let airline_el = card.select(&_selectors.airline).next()?;
    let airline = airline_el.text().collect();

    let times: Vec<_> = card.select(&_selectors.times).collect();
    if times.len() < 2 {
        return None;
    }

    let dep_time = normalize_time(&times[0].text().collect::<String>());
    let arr_time = normalize_time(&times[1].text().collect::<String>());

    let arrive_plus_days = card
        .select(&_selectors.arrives_next_day)
        .next()
        .map(|el| el.text().collect());

    let dur_el = card.select(&_selectors.duration).next()?;
    let duration = dur_el.text().collect();

    let stops_el = card.select(&_selectors.stops).next()?;
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
                1
            })
    };

    let layovers = parse_layovers_from_card(card, _selectors);

    let price_el = card.select(&_selectors.price).next()?;
    let price = clean_price(price_el.text().collect());

    Some(Flight {
        airline,
        dep_time,
        arr_time,
        arrive_plus_days,
        duration,
        stops,
        price,
        layovers,
    })
}

fn parse_layovers_from_card(
    card: scraper::ElementRef,
    selectors: &FlightSelectors,
) -> Vec<Layover> {
    let mut layovers = Vec::new();

    for container in card.select(&selectors.stops_container) {
        if let Some(aria_label) = container.value().attr("aria-label") {
            for cap in LAYOVER_ARIA_RE.captures_iter(&aria_label) {
                let hours = cap.get(1).map(|m| m.as_str()).unwrap_or("");
                let mins = cap.get(2).map(|m| m.as_str()).unwrap_or("0");
                let duration_str = format!("{}h {}m", hours, mins);
                let city_name = cap
                    .get(3)
                    .map(|m| m.as_str().to_string().trim().to_string())
                    .unwrap_or_default();

                layovers.push(Layover {
                    airport_code: city_name.clone(),
                    airport_name: Some(city_name),
                    duration_minutes: Some(parse_duration(&duration_str)),
                });
            }
        }
    }

    layovers
}

fn clean_price(s: String) -> String {
    s.chars().filter(|c| c.is_ascii_digit()).collect()
}

fn convert_to_itineraries(
    flights: Vec<Flight>,
    from_airport: &str,
    to_airport: &str,
) -> Vec<Itinerary> {
    let currency = Some("USD".to_string());

    let mut itineraries = Vec::new();
    let base_id = u32::from_str_radix(from_airport, 36).unwrap_or(0) << 16
        | u32::from_str_radix(to_airport, 36).unwrap_or(0);

    for (idx, flight) in flights.into_iter().enumerate() {
        let id = format!("{:06}{:02}", base_id, idx);

        let arrival_plus_days = flight
            .arrive_plus_days
            .as_ref()
            .and_then(|s| {
                let num = s.trim_start_matches('+').split_whitespace().next()?;
                num.parse().ok()
            })
            .unwrap_or(0);

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
            departure_airport: Some(from_airport.to_string()),
            arrival_airport: Some(to_airport.to_string()),
            flight_number: None,
            aircraft: None,
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
            class: None,
            layovers: flight.layovers,
        });
    }

    itineraries
}

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_duration_parsing() {
        assert_eq!(parse_duration("6h 30m"), 390);
        assert_eq!(parse_duration("1h"), 60);
        assert_eq!(parse_duration("45m"), 45);
        assert_eq!(parse_duration(""), 0);
    }

    #[test]
    fn test_normalize_time() {
        assert_eq!(normalize_time("10:30 AM"), "10:30");
        assert_eq!(normalize_time("22:45"), "22:45");
    }

    #[test]
    fn test_layover_parsing() {
        let aria_label = "Layover (1 of 2) is a 11 hr 29 min layover at Los Angeles International Airport in Los Angeles. Layover (2 of 2) is a 3 hr layover at Nadi International Airport in Nadi.";
        let mut layovers = Vec::new();

        eprintln!("Testing aria_label: {}", aria_label);
        eprintln!("Regex: {}", LAYOVER_ARIA_RE.as_str());
        eprintln!("is_match: {}", LAYOVER_ARIA_RE.is_match(aria_label));

        for (i, cap) in LAYOVER_ARIA_RE.captures_iter(aria_label).enumerate() {
            let hours = cap.get(1).map(|m| m.as_str()).unwrap_or("");
            let mins = cap.get(2).map(|m| m.as_str()).unwrap_or("0");
            let city_name = cap
                .get(3)
                .map(|m| m.as_str().to_string().trim().to_string())
                .unwrap_or_default();
            eprintln!("Match {}: {}h {}m in {}", i + 1, hours, mins, city_name);

            let duration_str = format!("{}h {}m", hours, mins);
            layovers.push(Layover {
                airport_code: city_name.clone(),
                airport_name: Some(city_name),
                duration_minutes: Some(parse_duration(&duration_str)),
            });
        }
        eprintln!("Total layovers: {}", layovers.len());
        assert_eq!(layovers.len(), 2);
        assert_eq!(layovers[0].airport_code, "Los Angeles");
        assert_eq!(layovers[0].duration_minutes, Some(689)); // 11h 29m
        assert_eq!(layovers[1].airport_code, "Nadi");
        assert_eq!(layovers[1].duration_minutes, Some(180)); // 3h
    }

    #[test]
    fn test_single_layover_parsing() {
        let aria_label = "Layover (1 of 1) is a 9 hr 7 min layover at Los Angeles International Airport in Los Angeles.";
        let mut layovers = Vec::new();

        for cap in LAYOVER_ARIA_RE.captures_iter(aria_label) {
            let hours = cap.get(1).map(|m| m.as_str()).unwrap_or("");
            let mins = cap.get(2).map(|m| m.as_str()).unwrap_or("0");
            let duration_str = format!("{}h {}m", hours, mins);
            let city_name = cap
                .get(3)
                .map(|m| m.as_str().to_string().trim().to_string())
                .unwrap_or_default();

            layovers.push(Layover {
                airport_code: city_name.clone(),
                airport_name: Some(city_name),
                duration_minutes: Some(parse_duration(&duration_str)),
            });
        }
        assert_eq!(layovers.len(), 1);
        assert_eq!(layovers[0].airport_code, "Los Angeles");
        assert_eq!(layovers[0].duration_minutes, Some(547)); // 9h 7m
    }
}
