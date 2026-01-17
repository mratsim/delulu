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

//! # Flights Query Builder
//!
//! Side-effect free TFS parameter encoding for Google Flights search.
//! This module builds the protobuf-encoded base64 `tfs` parameter.

pub mod proto {
    include!("proto/google_travel_flights.rs");
}

use anyhow::{ensure, Context, Result};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use chrono::{Datelike, NaiveDate};
use prost::Message;

pub use proto::{Airport, FlightData, Info, Passenger, Seat, Trip};

#[derive(Debug, Clone)]
pub struct FlightSearchParams {
    pub from_airport: String,
    pub to_airport: String,
    pub depart_date: String,
    pub return_date: Option<String>,
    pub cabin_class: Seat,
    pub passengers: Vec<(Passenger, u32)>,
    pub trip_type: Trip,
    pub max_stops: Option<i32>,
    pub preferred_airlines: Option<Vec<String>>,
}

impl FlightSearchParams {
    fn validate(&self) -> Result<()> {
        ensure!(!self.from_airport.is_empty(), "Origin airport is required");
        ensure!(
            !self.to_airport.is_empty(),
            "Destination airport is required"
        );

        let adults: u32 = self
            .passengers
            .iter()
            .filter(|(t, _)| *t == Passenger::Adult)
            .map(|(_, count)| count)
            .sum();

        let infants_on_lap: u32 = self
            .passengers
            .iter()
            .filter(|(t, _)| *t == Passenger::InfantOnLap)
            .map(|(_, count)| count)
            .sum();

        ensure!(adults > 0, "At least one adult is required");
        ensure!(
            infants_on_lap <= adults,
            "Cannot have more infants on lap ({}) than adults ({})",
            infants_on_lap,
            adults
        );

        let _checkin = NaiveDate::parse_from_str(&self.depart_date, "%Y-%m-%d")
            .context("Invalid depart date")?;

        Ok(())
    }

    pub fn generate_tfs(&self) -> Result<String> {
        self.validate()?;

        let depart_checkin = NaiveDate::parse_from_str(&self.depart_date, "%Y-%m-%d")
            .context(format!("Invalid depart date: {}", self.depart_date))?;

        let return_checkin = match &self.return_date {
            Some(rd) => Some(
                NaiveDate::parse_from_str(rd, "%Y-%m-%d")
                    .context(format!("Invalid return date: {}", rd))?,
            ),
            None => None,
        };

        let passenger_pairs: Vec<(i32, u32)> = self
            .passengers
            .iter()
            .map(|(ptype, count)| (*ptype as i32, *count))
            .collect();

        let outbound = FlightData {
            date: format!(
                "{:04}-{:02}-{:02}",
                depart_checkin.year(),
                depart_checkin.month(),
                depart_checkin.day()
            ),
            max_stops: self.max_stops.filter(|&v| v != 0),
            airlines: self.preferred_airlines.clone().unwrap_or_default(),
            from_flight: Some(Airport {
                airport: self.from_airport.clone(),
            }),
            to_flight: Some(Airport {
                airport: self.to_airport.clone(),
            }),
        };

        let flight_data = match (&self.trip_type, return_checkin) {
            (Trip::RoundTrip, Some(ret)) => {
                let return_flight = FlightData {
                    date: format!("{:04}-{:02}-{:02}", ret.year(), ret.month(), ret.day()),
                    max_stops: self.max_stops.filter(|&v| v != 0),
                    airlines: self.preferred_airlines.clone().unwrap_or_default(),
                    from_flight: Some(Airport {
                        airport: self.to_airport.clone(),
                    }),
                    to_flight: Some(Airport {
                        airport: self.from_airport.clone(),
                    }),
                };
                vec![outbound, return_flight]
            }
            _ => vec![outbound],
        };

        let passengers: Vec<i32> = passenger_pairs
            .iter()
            .flat_map(|(ptype, count)| std::iter::repeat_n(*ptype, *count as usize))
            .collect();

        let info = Info {
            data: flight_data,
            passengers,
            seat: Some(self.cabin_class as i32),
            trip: Some(self.trip_type as i32),
        };

        let mut bytes = Vec::new();
        info.encode(&mut bytes)
            .map_err(|e| anyhow::anyhow!("Failed to encode protobuf: {}", e))?;
        Ok(STANDARD.encode(&bytes))
    }

    pub fn get_search_url(&self) -> String {
        let tfs_param = self.generate_tfs().expect("TFS encoding should work");
        format!(
            "https://www.google.com/travel/flights/search?tfs={}&hl=en&curr=USD&tfu=EgQIABABIgA",
            tfs_param
        )
    }

    pub fn builder(
        from_airport: String,
        to_airport: String,
        depart_date: NaiveDate,
    ) -> FlightSearchParamsBuilder {
        FlightSearchParamsBuilder {
            from_airport,
            to_airport,
            depart_date,
            return_date: None,
            cabin_class: Seat::Economy,
            passengers: vec![(Passenger::Adult, 1)],
            trip_type: Trip::RoundTrip,
            max_stops: None,
            preferred_airlines: None,
        }
    }
}

#[derive(Clone)]
pub struct FlightSearchParamsBuilder {
    from_airport: String,
    to_airport: String,
    depart_date: NaiveDate,
    return_date: Option<NaiveDate>,
    cabin_class: Seat,
    passengers: Vec<(Passenger, u32)>,
    trip_type: Trip,
    max_stops: Option<i32>,
    preferred_airlines: Option<Vec<String>>,
}

impl FlightSearchParamsBuilder {
    pub fn cabin_class(mut self, cabin_class: Seat) -> Self {
        self.cabin_class = cabin_class;
        self
    }

    pub fn passengers(mut self, passengers: Vec<(Passenger, u32)>) -> Self {
        self.passengers = passengers;
        self
    }

    pub fn max_stops(mut self, max_stops: Option<i32>) -> Self {
        self.max_stops = max_stops;
        self
    }

    pub fn preferred_airlines(mut self, preferred_airlines: Option<Vec<String>>) -> Self {
        self.preferred_airlines = preferred_airlines;
        self
    }

    pub fn return_date(mut self, return_date: NaiveDate) -> Self {
        self.return_date = Some(return_date);
        self
    }

    pub fn trip_type(mut self, trip_type: Trip) -> Self {
        self.trip_type = trip_type;
        self
    }

    pub fn build(self) -> Result<FlightSearchParams> {
        let params = FlightSearchParams {
            from_airport: self.from_airport,
            to_airport: self.to_airport,
            depart_date: self.depart_date.format("%Y-%m-%d").to_string(),
            return_date: self.return_date.map(|d| d.format("%Y-%m-%d").to_string()),
            cabin_class: self.cabin_class,
            passengers: self.passengers,
            trip_type: self.trip_type,
            max_stops: self.max_stops,
            preferred_airlines: self.preferred_airlines,
        };
        params.validate()?;
        Ok(params)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_tfs_roundtrip() {
        let params = FlightSearchParams::builder(
            "SFO".to_string(),
            "JFK".to_string(),
            NaiveDate::from_ymd_opt(2025, 7, 15).unwrap(),
        )
        .cabin_class(Seat::Economy)
        .passengers(vec![(Passenger::Adult, 1)])
        .build()
        .unwrap();

        let tfs = params.generate_tfs().unwrap();
        assert!(!tfs.is_empty());
    }

    #[test]
    fn test_get_search_url() {
        let params = FlightSearchParams::builder(
            "LAX".to_string(),
            "CDG".to_string(),
            NaiveDate::from_ymd_opt(2025, 8, 1).unwrap(),
        )
        .cabin_class(Seat::Business)
        .passengers(vec![(Passenger::Adult, 2)])
        .trip_type(Trip::RoundTrip)
        .preferred_airlines(Some(vec!["AF".to_string(), "DL".to_string()]))
        .max_stops(Some(1))
        .build()
        .unwrap();

        let url = params.get_search_url();
        assert!(url.contains("tfs="));
        assert!(url.contains("https://www.google.com/travel/flights/search"));
    }

    #[test]
    fn test_passenger_validation() {
        let ok_params = FlightSearchParams::builder(
            "SFO".to_string(),
            "JFK".to_string(),
            NaiveDate::from_ymd_opt(2025, 7, 15).unwrap(),
        )
        .cabin_class(Seat::Economy)
        .passengers(vec![(Passenger::Adult, 1), (Passenger::Child, 1)])
        .trip_type(Trip::OneWay)
        .build()
        .unwrap();
        assert!(ok_params.validate().is_ok());

        let bad_result = FlightSearchParams::builder(
            "SFO".to_string(),
            "JFK".to_string(),
            NaiveDate::from_ymd_opt(2025, 7, 15).unwrap(),
        )
        .cabin_class(Seat::Economy)
        .passengers(vec![(Passenger::Adult, 0), (Passenger::Child, 1)])
        .trip_type(Trip::OneWay)
        .build();
        assert!(bad_result.is_err(), "Building with 0 adults should fail");
    }
}
