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

//! # Hotels Results Parser
//!
//! Side-effect free HTML parsing for Google Hotels search results.
//! Extracts hotel information from the HTML response.
//!
//! ## MCP API Response Schema (Optimized)
//!
//! The `to_mcp_api_response()` method serializes results to the following JSON schema:
//! Optimized for context compression - currency/search_url moved to query, ratings simplified,
//! amenities as compact array, and stars/rating merged where possible.
//!
//! ```json
//! {
//!   "$schema": "http://json-schema.org/draft-07/schema#",
//!   "type": "object",
//!   "required": ["search_hotels"],
//!   "properties": {
//!     "search_hotels": {
//!       "type": "object",
//!       "required": ["total", "query", "results"],
//!       "properties": {
//!         "total": {"type": "integer", "minimum": 0},
//!         "query": {
//!           "type": "object",
//!           "required": ["loc", "in", "out", "curr", "search_url"],
//!           "properties": {
//!             "loc": {"type": "string"},
//!             "in": {"type": "string"},
//!             "out": {"type": "string"},
//!             "curr": {"type": "string"},
//!             "search_url": {"type": "string"}
//!           }
//!         },
//!         "results": {
//!           "type": "array",
//!           "items": {
//!             "type": "object",
//!             "required": ["name", "price", "rating", "amenities"],
//!             "properties": {
//!               "name": {"type": "string"},
//!               "price": {"type": "integer", "minimum": 0},
//!               "rating": {"type": "number"},
//!               "stars": {"type": "integer"},
//!               "amenities": {"type": "array", "items": {"type": "string"}}
//!             }
//!           }
//!         }
//!       }
//!     }
//!   }
//! }
//! ```
//!
//! ## MCP API Response Schema (Optimized)
//!
//! The `to_mcp_api_response()` method serializes results to the following JSON schema:
//! Optimized for context compression - currency moved to query, ratings simplified,
//! amenities as compact array, and stars/rating merged where possible.
//!
//! ```json
//! {
//!   "$schema": "http://json-schema.org/draft-07/schema#",
//!   "type": "object",
//!   "required": ["search_hotels"],
//!   "properties": {
//!     "search_hotels": {
//!       "type": "object",
//!       "required": ["total", "query", "results"],
//!       "properties": {
//!         "total": {"type": "integer", "minimum": 0},
//!         "query": {
//!           "type": "object",
//!           "required": ["loc", "in", "out", "curr"],
//!           "properties": {
//!             "loc": {"type": "string"},
//!             "in": {"type": "string"},
//!             "out": {"type": "string"},
//!             "curr": {"type": "string"}
//!           }
//!         },
//!         "results": {
//!           "type": "array",
//!           "items": {
//!             "type": "object",
//!             "required": ["name", "price", "rating", "amenities"],
//!             "properties": {
//!               "name": {"type": "string"},
//!               "price": {"type": "integer", "minimum": 0},
//!               "rating": {"type": "number"},
//!               "stars": {"type": "integer"},
//!               "amenities": {"type": "array", "items": {"type": "string"}}
//!             }
//!           }
//!         }
//!       }
//!     }
//!   }
//! }
//! ```

use anyhow::Result;
use schemars::JsonSchema;
use scraper::{Html, Selector};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct Hotel {
    pub name: String,
    pub price: String,
    pub rating: Option<f64>,
    pub reviews: Option<u32>,
    #[serde(default)]
    pub amenities: Vec<String>,
    pub location_rating: Option<String>,
    pub star_class: Option<String>,
    pub url: Option<String>,
    pub address: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct HotelSearchResult {
    pub hotels: Vec<Hotel>,
    pub lowest_price: Option<String>,
    pub current_price: Option<String>,
}

impl HotelSearchResult {
    pub fn to_mcp_api_response(
        &self,
        location: String,
        checkin_date: String,
        checkout_date: String,
        currency: String,
        search_url: String,
    ) -> McpHotelResponse {
        let results: Vec<McpHotel> = self
            .hotels
            .iter()
            .map(|hotel| {
                let price = hotel
                    .price
                    .chars()
                    .filter(|c| c.is_ascii_digit())
                    .collect::<String>()
                    .parse()
                    .unwrap_or(0);
                let stars = hotel
                    .star_class
                    .as_ref()
                    .and_then(|s| s.trim().parse().ok())
                    .filter(|&s| s > 0);
                let rating = hotel.rating.unwrap_or(0.0);
                let amenities: Vec<String> = hotel
                    .amenities
                    .iter()
                    .map(|a| a.replace(" ", "").replace("-", "_").to_lowercase())
                    .filter(|a| a.len() > 2)
                    .collect();

                McpHotel {
                    name: hotel.name.clone(),
                    price,
                    rating,
                    stars,
                    amenities,
                }
            })
            .collect();

        McpHotelResponse {
            search_hotels: McpHotelsResponse {
                total: results.len(),
                query: McpHotelQuery {
                    loc: location,
                    in_: checkin_date,
                    out: checkout_date,
                    curr: currency,
                    search_url,
                },
                results,
            },
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct McpHotelResponse {
    pub search_hotels: McpHotelsResponse,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct McpHotelsResponse {
    pub total: usize,
    pub query: McpHotelQuery,
    pub results: Vec<McpHotel>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct McpHotelQuery {
    pub loc: String,
    #[serde(rename = "in")]
    pub in_: String,
    pub out: String,
    pub curr: String,
    pub search_url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct McpHotel {
    pub name: String,
    pub price: i32,
    pub rating: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stars: Option<i32>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub amenities: Vec<String>,
}

impl HotelSearchResult {
    pub fn from_html(html: &str) -> Result<Self> {
        let selectors = HotelSelectors::new();
        let document = Html::parse_document(html);
        let mut hotels = Vec::new();

        for card in document.select(&selectors.hotel_card) {
            let name = card
                .select(&selectors.name)
                .next()
                .map(|e| e.text().collect::<String>())
                .filter(|s| !s.is_empty());
            let Some(name) = name else {
                continue;
            };
            let price = card
                .select(&selectors.price)
                .next()
                .map(|e| e.text().collect::<String>());
            let Some(price) = price else {
                continue;
            };

            let rating = card
                .select(&selectors.rating)
                .next()
                .or_else(|| card.select(&selectors.rating_aria).next())
                .and_then(|e| e.text().collect::<String>().trim().parse().ok());
            let reviews = card.select(&selectors.reviews).next().and_then(|e| {
                let text: String = e.text().collect();
                let digits: String = text.chars().filter(|c| c.is_ascii_digit()).collect();
                digits.parse().ok()
            });
            let amenities: Vec<String> = card
                .select(&selectors.amenities)
                .map(|e| e.text().collect::<String>())
                .filter(|s| !s.is_empty() && s.len() > 2)
                .collect();
            let location_rating = card
                .select(&selectors.location_rating)
                .next()
                .map(|e| e.text().collect::<String>());
            let star_class = card
                .select(&selectors.star_class)
                .next()
                .map(|e| e.text().collect::<String>());
            let url = card
                .select(&selectors.link)
                .next()
                .and_then(|e| e.value().attr("href"))
                .map(|h| {
                    if h.starts_with("/travel/") {
                        format!("https://www.google.com{}", h)
                    } else {
                        h.to_string()
                    }
                });

            hotels.push(Hotel {
                name,
                price,
                rating,
                reviews,
                amenities,
                location_rating,
                star_class,
                url,
                address: None,
            });
        }

        let result = HotelSearchResult {
            hotels,
            lowest_price: None,
            current_price: None,
        };

        anyhow::ensure!(result.is_valid(), "No valid hotel results found");
        Ok(result)
    }

    pub fn hotels(&self) -> impl Iterator<Item = &Hotel> {
        self.hotels.iter()
    }

    fn is_valid(&self) -> bool {
        !self.hotels.is_empty() && self.hotels.iter().any(|h| !h.price.is_empty())
    }
}

struct HotelSelectors {
    hotel_card: Selector,
    name: Selector,
    rating: Selector,
    rating_aria: Selector,
    reviews: Selector,
    price: Selector,
    amenities: Selector,
    location_rating: Selector,
    star_class: Selector,
    link: Selector,
}

impl HotelSelectors {
    fn new() -> Self {
        Self {
            hotel_card: Selector::parse(r#"div.uaTTDe"#).unwrap(),
            name: Selector::parse(r#"h2.BgYkof"#).unwrap(),
            rating: Selector::parse(r#"span.KFi5wf.lA0BZ"#).unwrap(),
            rating_aria: Selector::parse(r#"span[aria-label*="out of 5 stars"]"#).unwrap(),
            reviews: Selector::parse(r#"span.jdzyld"#).unwrap(),
            price: Selector::parse(r#"span.qQOQpe"#).unwrap(),
            amenities: Selector::parse(r#"span.LtjZ2d"#).unwrap(),
            location_rating: Selector::parse(r#"span.uTUoTb"#).unwrap(),
            star_class: Selector::parse(r#"span.UqrZme"#).unwrap(),
            link: Selector::parse(r#"a[href]"#).unwrap(),
        }
    }
}
