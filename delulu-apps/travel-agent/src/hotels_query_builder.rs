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

//! # Hotels Query Builder
//!
//! Side-effect free TS parameter encoding for Google Hotels search.
//! This module builds the protobuf-encoded base64 `ts` parameter.

pub mod proto {
    include!("proto/google_travel_hotels.rs");
}

use anyhow::{Context, Result, ensure};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use chrono::{Datelike, NaiveDate};
use prost::Message;
use serde::{Deserialize, Serialize};

#[cfg(feature = "mcp")]
use schemars::JsonSchema;

use proto::{Amenity as AmenityProto, SortType as SortTypeProto};

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "mcp", derive(schemars::JsonSchema))]
#[repr(i32)]
#[serde(rename_all = "snake_case")]
pub enum Amenity {
    IndoorPool = AmenityProto::IndoorPool as i32,
    OutdoorPool = AmenityProto::OutdoorPool as i32,
    Pool = AmenityProto::Pool as i32,
    Spa = AmenityProto::Spa as i32,
    KidFriendly = AmenityProto::KidFriendly as i32,
    AirConditioned = AmenityProto::AirConditioned as i32,
}

impl From<Amenity> for AmenityProto {
    fn from(a: Amenity) -> AmenityProto {
        match a {
            Amenity::IndoorPool => AmenityProto::IndoorPool,
            Amenity::OutdoorPool => AmenityProto::OutdoorPool,
            Amenity::Pool => AmenityProto::Pool,
            Amenity::Spa => AmenityProto::Spa,
            Amenity::KidFriendly => AmenityProto::KidFriendly,
            Amenity::AirConditioned => AmenityProto::AirConditioned,
        }
    }
}

impl From<Amenity> for i32 {
    fn from(a: Amenity) -> i32 {
        a as i32
    }
}

impl TryFrom<i32> for Amenity {
    type Error = ();
    fn try_from(v: i32) -> std::result::Result<Self, Self::Error> {
        match v {
            v if v == Amenity::IndoorPool as i32 => Ok(Amenity::IndoorPool),
            v if v == Amenity::OutdoorPool as i32 => Ok(Amenity::OutdoorPool),
            v if v == Amenity::Pool as i32 => Ok(Amenity::Pool),
            v if v == Amenity::Spa as i32 => Ok(Amenity::Spa),
            v if v == Amenity::KidFriendly as i32 => Ok(Amenity::KidFriendly),
            v if v == Amenity::AirConditioned as i32 => Ok(Amenity::AirConditioned),
            _ => Err(()),
        }
    }
}

impl Amenity {
    pub fn from_str_name(s: &str) -> Option<Self> {
        let lower = s.to_lowercase();
        match lower.as_str() {
            "indoor_pool" | "indoorpool" | "indoor" => Some(Amenity::IndoorPool),
            "outdoor_pool" | "outdoorpool" | "outdoor" => Some(Amenity::OutdoorPool),
            "pool" => Some(Amenity::Pool),
            "spa" => Some(Amenity::Spa),
            "kid_friendly" | "kidfriendly" | "kid" => Some(Amenity::KidFriendly),
            "air_conditioned" | "airconditioned" | "ac" => Some(Amenity::AirConditioned),
            _ => None,
        }
    }

    pub fn as_str_name(&self) -> &'static str {
        match self {
            Amenity::IndoorPool => "indoor_pool",
            Amenity::OutdoorPool => "outdoor_pool",
            Amenity::Pool => "pool",
            Amenity::Spa => "spa",
            Amenity::KidFriendly => "kid_friendly",
            Amenity::AirConditioned => "air_conditioned",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "mcp", derive(schemars::JsonSchema))]
#[repr(i32)]
#[serde(rename_all = "snake_case")]
pub enum SortType {
    LowestPrice = SortTypeProto::LowestPrice as i32,
    HighestRating = SortTypeProto::HighestRating as i32,
    MostReviewed = SortTypeProto::MostReviewed as i32,
}

impl From<SortType> for SortTypeProto {
    fn from(s: SortType) -> SortTypeProto {
        match s {
            SortType::LowestPrice => SortTypeProto::LowestPrice,
            SortType::HighestRating => SortTypeProto::HighestRating,
            SortType::MostReviewed => SortTypeProto::MostReviewed,
        }
    }
}

impl From<SortType> for i32 {
    fn from(s: SortType) -> i32 {
        s as i32
    }
}

impl TryFrom<i32> for SortType {
    type Error = ();
    fn try_from(v: i32) -> std::result::Result<Self, Self::Error> {
        match v {
            v if v == SortType::LowestPrice as i32 => Ok(SortType::LowestPrice),
            v if v == SortType::HighestRating as i32 => Ok(SortType::HighestRating),
            v if v == SortType::MostReviewed as i32 => Ok(SortType::MostReviewed),
            _ => Err(()),
        }
    }
}

impl SortType {
    pub fn from_str_name(s: &str) -> Option<Self> {
        match s {
            "lowest_price" | "lowest" | "price" => Some(SortType::LowestPrice),
            "highest_rating" | "highest" | "rating" => Some(SortType::HighestRating),
            "most_reviewed" | "reviewed" | "reviews" => Some(SortType::MostReviewed),
            _ => None,
        }
    }

    pub fn as_str_name(&self) -> &'static str {
        match self {
            SortType::LowestPrice => "lowest_price",
            SortType::HighestRating => "highest_rating",
            SortType::MostReviewed => "most_reviewed",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "mcp", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub struct HotelSearchParams {
    pub version: i32,
    pub adults: u32,
    pub children_ages: Vec<i32>,
    pub loc_q_search: String,
    pub loc_ts_name: String,
    pub loc_ts_id: String,
    pub loc_ts_coords: String,
    pub checkin_date: String,
    pub checkout_date: String,
    pub nights: i32,
    pub used_guests_dropdown: i32,
    pub currency: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sort_order: Option<SortType>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_guest_rating: Option<f64>,
    pub hotel_stars: Vec<i32>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub amenities: Vec<Amenity>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_price: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_price: Option<i32>,
}

impl HotelSearchParams {
    pub fn location(&self) -> &str {
        &self.loc_q_search
    }

    fn validate(&self) -> Result<()> {
        let total_guests = self.adults + self.children_ages.len() as u32;
        ensure!(self.adults >= 1, "At least one adult is required");
        ensure!(total_guests <= 6, "Maximum 6 guests allowed");
        ensure!(
            self.children_ages
                .iter()
                .all(|&age| (1..=17).contains(&age)),
            "Children ages must be between 1 and 17 (ages 0-1 are encoded as 1)"
        );

        let checkin = NaiveDate::parse_from_str(&self.checkin_date, "%Y-%m-%d")
            .context("Invalid checkin date")?;
        let checkout = NaiveDate::parse_from_str(&self.checkout_date, "%Y-%m-%d")
            .context("Invalid checkout date")?;

        ensure!(checkout > checkin, "Checkout must be after check-in");
        ensure!(
            checkout - checkin <= chrono::Duration::days(30),
            "Stay must be 30 nights or fewer"
        );
        if let Some(p) = self.max_price {
            ensure!(p > 0, "Price must be positive");
        }
        if let Some(p) = self.min_price {
            ensure!(p > 0, "Price must be positive");
        }
        if let (Some(min), Some(max)) = (self.min_price, self.max_price) {
            ensure!(
                min <= max,
                "Minimum price cannot be greater than maximum price"
            );
        }

        ensure!(
            self.hotel_stars.iter().all(|&star| (2..=5).contains(&star)),
            "Star rating must be between 2 and 5"
        );
        Ok(())
    }

    pub fn builder(
        loc_q_search: String,
        checkin_date: NaiveDate,
        checkout_date: NaiveDate,
        adults: u32,
        children_ages: Vec<i32>,
    ) -> HotelSearchParamsBuilder {
        HotelSearchParamsBuilder {
            loc_q_search,
            checkin_date,
            checkout_date,
            adults,
            children_ages,
            currency: None,
            min_guest_rating: None,
            hotel_stars: Vec::new(),
            amenities: Vec::new(),
            min_price: None,
            max_price: None,
            sort_order: None,
        }
    }

    pub fn generate_ts(&self) -> Result<String> {
        self.validate()?;
        let checkin = NaiveDate::parse_from_str(&self.checkin_date, "%Y-%m-%d")
            .context(format!("Invalid checkin date: {}", self.checkin_date))?;
        let checkout = NaiveDate::parse_from_str(&self.checkout_date, "%Y-%m-%d")
            .context(format!("Invalid checkout date: {}", self.checkout_date))?;
        let nights = (checkout - checkin).num_days() as i32;

        let mut guest_entries: Vec<proto::GuestEntry> = Vec::new();
        for _ in 0..self.adults {
            guest_entries.push(proto::GuestEntry {
                kind: proto::GuestKind::Adult as i32,
                age: 0,
            });
        }
        for &age in &self.children_ages {
            guest_entries.push(proto::GuestEntry {
                kind: proto::GuestKind::Child as i32,
                age,
            });
        }

        let location_data = proto::LocationData {
            details: None,
            marker: Some(proto::UnknownMessage { flags: 0 }),
        };

        let guest_rating_val = self
            .min_guest_rating
            .map(|r| {
                if r >= 4.5 {
                    9
                } else if r >= 4.0 {
                    8
                } else {
                    7
                }
            })
            .unwrap_or(0);

        let price_data = if self.min_price.is_some() || self.max_price.is_some() {
            Some(proto::PriceData {
                min_price: self.min_price.map(|v| proto::PriceValue { value: v }),
                max_price: self.max_price.map(|v| proto::PriceValue { value: v }),
                unknown_price_marker: 0,
            })
        } else {
            None
        };

        let date_range = proto::DateRange {
            checkin: Some(proto::DateDetails {
                year: checkin.year(),
                month: checkin.month() as i32,
                day: checkin.day() as i32,
            }),
            checkout: Some(proto::DateDetails {
                year: checkout.year(),
                month: checkout.month() as i32,
                day: checkout.day() as i32,
            }),
            nights,
        };

        let date_wrapper = proto::DateWrapper {
            date_range: Some(date_range),
            flags: Some(proto::UnknownMessage { flags: 1 }),
        };

        let explicit_guests = self.adults > 2 || !self.children_ages.is_empty();

        let params = proto::ProtoHotelSearch {
            version: 1,
            guests: Some(proto::Guests {
                entries: guest_entries,
                explicit_selection: explicit_guests,
            }),
            search_params: Some(proto::SearchParams {
                location: Some(location_data),
                dates: Some(date_wrapper),
            }),
            filter_config: Some(proto::FilterConfig {
                filters: Some(proto::FilterData {
                    currency: self.currency.clone(),
                    amenity: self.amenities.iter().map(|&a| a as i32).collect(),
                    stars: self.hotel_stars.clone(),
                    sort_type: self.sort_order.map(|s| s as i32).unwrap_or(0),
                    padding: Some(proto::UnknownMessage { flags: 0 }),
                }),
                guest_rating: guest_rating_val,
                padding: Some(proto::UnknownMessage { flags: 0 }),
                price_data,
            }),
        };

        let mut bytes = Vec::new();
        params
            .encode(&mut bytes)
            .context("Failed to encode protobuf")?;
        Ok(URL_SAFE_NO_PAD.encode(&bytes))
    }

    pub fn get_search_url(&self) -> String {
        let ts_param = self.generate_ts().expect("TS encoding should work");
        let encoded_location = urlencoding::encode(&self.loc_q_search);
        format!(
            "https://www.google.com/travel/search?q={}&ts={}",
            encoded_location, ts_param
        )
    }

    pub fn from_ts(ts_base64: &str) -> Result<Self> {
        let ts_bytes = URL_SAFE_NO_PAD
            .decode(ts_base64)
            .map_err(|e| anyhow::anyhow!("Failed to decode base64: {}", e))?;
        let params = proto::ProtoHotelSearch::decode(ts_bytes.as_slice())
            .context("Failed to decode protobuf")?;

        let guests = params.guests.as_ref();
        let search_params = params.search_params.as_ref();
        let filter_config = params.filter_config.as_ref();

        let mut adults: u32 = 0;
        let mut children_ages: Vec<i32> = Vec::new();
        if let Some(g) = guests {
            for e in &g.entries {
                if e.kind == proto::GuestKind::Adult as i32 {
                    adults += 1;
                } else {
                    children_ages.push(e.age);
                }
            }
        }
        if adults == 0 {
            adults = 2;
        }

        let mut loc_ts_id = String::new();
        let mut loc_ts_coords = String::new();
        let mut loc_ts_name = String::new();

        if let Some(sp) = search_params {
            if let Some(loc) = &sp.location {
                if let Some(details) = &loc.details {
                    loc_ts_id = details.location_id.clone();
                    loc_ts_coords = details.coordinates.clone();
                    loc_ts_name = details.display_name.clone();
                }
            }
        }

        let mut checkin_date = String::new();
        let mut checkout_date = String::new();
        let mut nights = 0;

        if let Some(sp) = search_params {
            if let Some(dates) = &sp.dates {
                if let Some(range) = &dates.date_range {
                    if let Some(checkin) = &range.checkin {
                        checkin_date = format!(
                            "{:04}-{:02}-{:02}",
                            checkin.year, checkin.month, checkin.day
                        );
                    }
                    if let Some(checkout) = &range.checkout {
                        checkout_date = format!(
                            "{:04}-{:02}-{:02}",
                            checkout.year, checkout.month, checkout.day
                        );
                    }
                    nights = range.nights;
                }
            }
        }

        let mut currency = String::new();
        let mut sort_order = None;
        let mut min_guest_rating = None;
        let mut hotel_stars = Vec::new();
        let mut amenities = Vec::new();
        let mut min_price = None;
        let mut max_price = None;

        if let Some(fc) = filter_config {
            if let Some(f) = &fc.filters {
                currency = f.currency.clone();
                for &amenity in &f.amenity {
                    if amenity != 0 {
                        if let Ok(a) = Amenity::try_from(amenity) {
                            amenities.push(a);
                        }
                    }
                }
                for &star in &f.stars {
                    hotel_stars.push(star);
                }
                if f.sort_type != 0 {
                    if let Ok(s) = SortType::try_from(f.sort_type) {
                        sort_order = Some(s);
                    }
                }
            }
            if let Some(pd) = &fc.price_data {
                if let Some(v) = &pd.min_price {
                    if v.value != 0 {
                        min_price = Some(v.value);
                    }
                }
                if let Some(v) = &pd.max_price {
                    if v.value != 0 {
                        max_price = Some(v.value);
                    }
                }
            }
            if fc.guest_rating != 0 {
                let rating = fc.guest_rating as f64 / 2.0;
                if rating > 0.0 {
                    min_guest_rating = Some(rating);
                }
            }
        }

        Ok(HotelSearchParams {
            version: params.version,
            adults,
            children_ages,
            loc_q_search: String::new(),
            loc_ts_name,
            loc_ts_id,
            loc_ts_coords,
            checkin_date,
            checkout_date,
            nights,
            used_guests_dropdown: guests.map_or(0, |g| g.explicit_selection as i32),
            currency,
            sort_order,
            min_guest_rating,
            hotel_stars,
            amenities,
            min_price,
            max_price,
        })
    }
}

#[derive(Clone)]
pub struct HotelSearchParamsBuilder {
    loc_q_search: String,
    checkin_date: NaiveDate,
    checkout_date: NaiveDate,
    adults: u32,
    children_ages: Vec<i32>,
    currency: Option<String>,
    min_guest_rating: Option<f64>,
    hotel_stars: Vec<i32>,
    amenities: Vec<Amenity>,
    min_price: Option<i32>,
    max_price: Option<i32>,
    sort_order: Option<SortType>,
}

impl HotelSearchParamsBuilder {
    pub fn currency(mut self, currency: String) -> Self {
        self.currency = Some(currency);
        self
    }

    pub fn min_guest_rating(mut self, rating: f64) -> Self {
        self.min_guest_rating = Some(rating);
        self
    }

    pub fn hotel_stars(mut self, stars: Vec<i32>) -> Self {
        self.hotel_stars = stars;
        self
    }

    pub fn amenities(mut self, amenities: Vec<Amenity>) -> Self {
        self.amenities = amenities;
        self
    }

    pub fn min_price(mut self, price: Option<i32>) -> Self {
        self.min_price = price;
        self
    }

    pub fn max_price(mut self, price: Option<i32>) -> Self {
        self.max_price = price;
        self
    }

    pub fn sort_order(mut self, sort: Option<SortType>) -> Self {
        self.sort_order = sort;
        self
    }

    pub fn build(self) -> Result<HotelSearchParams> {
        let params = HotelSearchParams {
            version: 1,
            adults: self.adults,
            children_ages: self.children_ages,
            loc_q_search: self.loc_q_search,
            loc_ts_name: String::new(),
            loc_ts_id: String::new(),
            loc_ts_coords: String::new(),
            checkin_date: self.checkin_date.format("%Y-%m-%d").to_string(),
            checkout_date: self.checkout_date.format("%Y-%m-%d").to_string(),
            nights: (self.checkout_date - self.checkin_date).num_days() as i32,
            used_guests_dropdown: 0,
            currency: self.currency.unwrap_or_default(),
            sort_order: self.sort_order,
            min_guest_rating: self.min_guest_rating,
            hotel_stars: self.hotel_stars,
            amenities: self.amenities,
            min_price: self.min_price,
            max_price: self.max_price,
        };
        params.validate()?;
        Ok(params)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode_paris_basic() {
        let ts = "CAEaIAoCGgASGhIUCgcI6g8QARgZEgcI6g8QARgfGAYyAggBKgkKBToDRVVSGgA";
        let decoded = HotelSearchParams::from_ts(ts).unwrap();
        assert_eq!(decoded.version, 1);
        assert_eq!(decoded.adults, 2);
        assert!(decoded.children_ages.is_empty());
        assert_eq!(decoded.checkin_date, "2026-01-25");
        assert_eq!(decoded.checkout_date, "2026-01-31");
    }

    #[test]
    fn encode_decode_roundtrip() {
        let builder = HotelSearchParams::builder(
            "Paris".to_string(),
            NaiveDate::from_ymd_opt(2026, 1, 25).unwrap(),
            NaiveDate::from_ymd_opt(2026, 1, 31).unwrap(),
            2,
            Vec::new(),
        );
        let params = builder.build().unwrap();
        let ts = params.generate_ts().unwrap();
        let decoded = HotelSearchParams::from_ts(&ts).unwrap();
        assert_eq!(decoded.checkin_date, "2026-01-25");
        assert_eq!(decoded.checkout_date, "2026-01-31");
        assert_eq!(decoded.adults, 2);
        assert!(decoded.children_ages.is_empty());
    }
}
