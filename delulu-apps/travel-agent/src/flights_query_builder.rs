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

use anyhow::{Context, Result, ensure};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use chrono::{Datelike, NaiveDate};
use prost::Message;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use proto::{
    Airport as AirportProto, FlightData, Info, Passenger as PassengerProto, Seat as SeatProto,
    Trip as TripProto,
};

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
#[repr(i32)]
#[serde(rename_all = "snake_case")]
pub enum Seat {
    Unknown = SeatProto::UnknownSeat as i32,
    #[serde(alias = "Economy")]
    Economy = SeatProto::Economy as i32,
    #[serde(alias = "PremiumEconomy")]
    PremiumEconomy = SeatProto::PremiumEconomy as i32,
    #[serde(alias = "Business")]
    Business = SeatProto::Business as i32,
    #[serde(alias = "First")]
    First = SeatProto::First as i32,
}

impl Default for Seat {
    fn default() -> Self {
        Seat::Economy
    }
}

impl From<Seat> for SeatProto {
    fn from(s: Seat) -> SeatProto {
        match s {
            Seat::Unknown => SeatProto::UnknownSeat,
            Seat::Economy => SeatProto::Economy,
            Seat::PremiumEconomy => SeatProto::PremiumEconomy,
            Seat::Business => SeatProto::Business,
            Seat::First => SeatProto::First,
        }
    }
}

impl From<Seat> for i32 {
    fn from(s: Seat) -> i32 {
        s as i32
    }
}

impl TryFrom<i32> for Seat {
    type Error = ();
    fn try_from(v: i32) -> Result<Self, Self::Error> {
        match v {
            v if v == Seat::Unknown as i32 => Ok(Seat::Unknown),
            v if v == Seat::Economy as i32 => Ok(Seat::Economy),
            v if v == Seat::PremiumEconomy as i32 => Ok(Seat::PremiumEconomy),
            v if v == Seat::Business as i32 => Ok(Seat::Business),
            v if v == Seat::First as i32 => Ok(Seat::First),
            _ => Err(()),
        }
    }
}

impl Seat {
    pub fn from_str_name(s: &str) -> Option<Self> {
        match s {
            "unknown" => Some(Seat::Unknown),
            "economy" => Some(Seat::Economy),
            "premium_economy" | "premium" => Some(Seat::PremiumEconomy),
            "business" => Some(Seat::Business),
            "first" => Some(Seat::First),
            _ => None,
        }
    }

    pub fn as_str_name(&self) -> &'static str {
        match self {
            Seat::Unknown => "unknown",
            Seat::Economy => "economy",
            Seat::PremiumEconomy => "premium_economy",
            Seat::Business => "business",
            Seat::First => "first",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
#[repr(i32)]
#[serde(rename_all = "snake_case")]
pub enum Trip {
    #[serde(alias = "round-trip")]
    RoundTrip = TripProto::RoundTrip as i32,
    #[serde(alias = "one-way")]
    OneWay = TripProto::OneWay as i32,
    MultiCity = TripProto::MultiCity as i32,
}

impl Default for Trip {
    fn default() -> Self {
        Trip::OneWay
    }
}

impl From<Trip> for TripProto {
    fn from(t: Trip) -> TripProto {
        match t {
            Trip::RoundTrip => TripProto::RoundTrip,
            Trip::OneWay => TripProto::OneWay,
            Trip::MultiCity => TripProto::MultiCity,
        }
    }
}

impl From<Trip> for i32 {
    fn from(t: Trip) -> i32 {
        t as i32
    }
}

impl TryFrom<i32> for Trip {
    type Error = ();
    fn try_from(v: i32) -> Result<Self, Self::Error> {
        match v {
            v if v == Trip::RoundTrip as i32 => Ok(Trip::RoundTrip),
            v if v == Trip::OneWay as i32 => Ok(Trip::OneWay),
            v if v == Trip::MultiCity as i32 => Ok(Trip::MultiCity),
            _ => Err(()),
        }
    }
}

impl Trip {
    pub fn from_str_name(s: &str) -> Option<Self> {
        match s {
            "round_trip" | "roundtrip" | "round" => Some(Trip::RoundTrip),
            "one_way" | "oneway" => Some(Trip::OneWay),
            "multi_city" | "multicity" | "multi" => Some(Trip::MultiCity),
            _ => None,
        }
    }

    pub fn as_str_name(&self) -> &'static str {
        match self {
            Trip::RoundTrip => "round_trip",
            Trip::OneWay => "one_way",
            Trip::MultiCity => "multi_city",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
#[repr(i32)]
#[serde(rename_all = "snake_case")]
pub enum Passenger {
    Adult = PassengerProto::Adult as i32,
    Child = PassengerProto::Child as i32,
    InfantOnLap = PassengerProto::InfantOnLap as i32,
    InfantInSeat = PassengerProto::InfantInSeat as i32,
}

impl From<Passenger> for PassengerProto {
    fn from(p: Passenger) -> PassengerProto {
        match p {
            Passenger::Adult => PassengerProto::Adult,
            Passenger::Child => PassengerProto::Child,
            Passenger::InfantOnLap => PassengerProto::InfantOnLap,
            Passenger::InfantInSeat => PassengerProto::InfantInSeat,
        }
    }
}

impl From<Passenger> for i32 {
    fn from(p: Passenger) -> i32 {
        p as i32
    }
}

impl TryFrom<i32> for Passenger {
    type Error = ();
    fn try_from(v: i32) -> Result<Self, Self::Error> {
        match v {
            v if v == Passenger::Adult as i32 => Ok(Passenger::Adult),
            v if v == Passenger::Child as i32 => Ok(Passenger::Child),
            v if v == Passenger::InfantOnLap as i32 => Ok(Passenger::InfantOnLap),
            v if v == Passenger::InfantInSeat as i32 => Ok(Passenger::InfantInSeat),
            _ => Err(()),
        }
    }
}

impl Passenger {
    pub fn from_str_name(s: &str) -> Option<Self> {
        match s {
            "adult" => Some(Passenger::Adult),
            "child" => Some(Passenger::Child),
            "infant_on_lap" | "infant" => Some(Passenger::InfantOnLap),
            "infant_in_seat" => Some(Passenger::InfantInSeat),
            _ => None,
        }
    }

    pub fn as_str_name(&self) -> &'static str {
        match self {
            Passenger::Adult => "adult",
            Passenger::Child => "child",
            Passenger::InfantOnLap => "infant_on_lap",
            Passenger::InfantInSeat => "infant_in_seat",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct FlightSearchParams {
    pub from_airport: String,
    pub to_airport: String,
    pub depart_date: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub return_date: Option<String>,
    pub cabin_class: Seat,
    pub passengers: Vec<(Passenger, u32)>,
    pub trip_type: Trip,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_stops: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preferred_airlines: Option<Vec<String>>,
}

impl FlightSearchParams {
    pub fn validate(&self) -> Result<()> {
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

        let depart_date = NaiveDate::parse_from_str(&self.depart_date, "%Y-%m-%d")
            .context("Invalid depart date format")?;

        if let Some(return_date_str) = &self.return_date {
            let return_date = NaiveDate::parse_from_str(return_date_str, "%Y-%m-%d")
                .context("Invalid return date format")?;

            if self.trip_type == Trip::RoundTrip {
                ensure!(
                    return_date >= depart_date,
                    "Return date must be on or after departure date"
                );
            }
        }

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
            from_flight: Some(AirportProto {
                airport: self.from_airport.clone(),
            }),
            to_flight: Some(AirportProto {
                airport: self.to_airport.clone(),
            }),
        };

        let flight_data = match (&self.trip_type, return_checkin) {
            (Trip::RoundTrip, Some(ret)) => {
                let return_flight = FlightData {
                    date: format!("{:04}-{:02}-{:02}", ret.year(), ret.month(), ret.day()),
                    max_stops: self.max_stops.filter(|&v| v != 0),
                    airlines: self.preferred_airlines.clone().unwrap_or_default(),
                    from_flight: Some(AirportProto {
                        airport: self.to_airport.clone(),
                    }),
                    to_flight: Some(AirportProto {
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
    fn test_get_search_url() {
        let params = FlightSearchParams::builder(
            "SFO".to_string(),
            "JFK".to_string(),
            NaiveDate::from_ymd_opt(2025, 6, 15).unwrap(),
        )
        .passengers(vec![(Passenger::Adult, 1)])
        .cabin_class(Seat::Economy)
        .build()
        .unwrap();

        let url = params.get_search_url();
        assert!(url.starts_with("https://www.google.com/travel/flights/search?tfs="));
    }

    #[test]
    fn test_generate_tfs_oneway() {
        let params = FlightSearchParams::builder(
            "LAX".to_string(),
            "ORD".to_string(),
            NaiveDate::from_ymd_opt(2025, 7, 20).unwrap(),
        )
        .passengers(vec![(Passenger::Adult, 2)])
        .cabin_class(Seat::Business)
        .build()
        .unwrap();

        let tfs = params.generate_tfs().unwrap();
        assert!(!tfs.is_empty());
    }

    #[test]
    fn test_generate_tfs_roundtrip() {
        let params = FlightSearchParams::builder(
            "LAX".to_string(),
            "ORD".to_string(),
            NaiveDate::from_ymd_opt(2025, 7, 20).unwrap(),
        )
        .return_date(NaiveDate::from_ymd_opt(2025, 7, 25).unwrap())
        .passengers(vec![(Passenger::Adult, 1), (Passenger::Child, 1)])
        .cabin_class(Seat::Economy)
        .trip_type(Trip::RoundTrip)
        .build()
        .unwrap();

        let tfs = params.generate_tfs().unwrap();
        assert!(!tfs.is_empty());
    }

    #[test]
    fn test_passenger_validation() {
        let params = FlightSearchParams::builder(
            "LAX".to_string(),
            "ORD".to_string(),
            NaiveDate::from_ymd_opt(2025, 7, 20).unwrap(),
        )
        .passengers(vec![])
        .build();

        assert!(params.is_err());

        let params = FlightSearchParams::builder(
            "LAX".to_string(),
            "ORD".to_string(),
            NaiveDate::from_ymd_opt(2025, 7, 20).unwrap(),
        )
        .passengers(vec![(Passenger::Adult, 0), (Passenger::Child, 1)])
        .build();

        assert!(params.is_err());
    }
}
