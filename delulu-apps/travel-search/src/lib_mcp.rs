//!  Delulu Travel Agent — MCP Server
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
//!
//! # MCP Server (library)
//!
//! Provides the [`TravelAgentServer`] shared by the standalone
//! `delulu-travel-mcp` binary and `delulu-all-mcp`.
//!
//! Tool names, descriptions and input schemas are byte-identical to the
//! standalone binary's previous implementation.

use crate::{
    Amenity, FlightSearchParams, GoogleFlightsClient, GoogleHotelsClient, HotelSearchParams, Seat,
    Trip,
};
use delulu_mcp_server_helper::impl_server_handler;
use delulu_mcp_server_helper::rmcp::handler::server::tool::ToolRouter;
use delulu_mcp_server_helper::rmcp::handler::server::wrapper::Parameters;
use delulu_mcp_server_helper::rmcp::tool;
use delulu_mcp_server_helper::rmcp::tool_router;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// Input parameters for the `search_flights` tool.
#[derive(Serialize, Deserialize)]
#[cfg_attr(feature = "mcp", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub struct FlightsInput {
    pub from: String,
    pub to: String,
    pub date: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub return_date: Option<String>,
    #[serde(default)]
    pub seat: Seat,
    pub adults: u32,
    #[serde(default)]
    pub children_ages: Vec<i32>,
    #[serde(default)]
    #[serde(alias = "round-trip")]
    #[serde(alias = "one-way")]
    pub trip_type: Trip,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_stops: Option<i32>,
    // pub preferred_airlines: Option<Vec<String>>,
    // pub currency: Option<String>,
}

/// Input parameters for the `search_hotels` tool.
#[derive(Serialize, Deserialize, Default)]
#[cfg_attr(feature = "mcp", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub struct HotelsInput {
    pub location: String,
    pub checkin_date: String,
    pub checkout_date: String,
    pub adults: u32,
    #[serde(default)]
    pub children_ages: Vec<i32>,
    // pub currency: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_guest_rating: Option<f64>,
    #[serde(default)]
    pub stars: Vec<i32>,
    #[serde(default)]
    pub amenities: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_price: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_price: Option<i32>,
}

/// MCP server exposing travel tools (`search_flights`, `search_hotels`)
/// over stdio or HTTP transports.
///
/// Shared by the standalone `delulu-travel-mcp` binary and `delulu-all-mcp`.
///
/// Pre: constructed via [`TravelAgentServer::new`] with an `Arc<GoogleFlightsClient>`
/// and an `Arc<GoogleHotelsClient>`.
/// Post: tools are registered in `tool_router` and callable through the MCP `ServerHandler` impl.
#[derive(Clone)]
pub struct TravelAgentServer {
    flights_client: Arc<GoogleFlightsClient>,
    hotels_client: Arc<GoogleHotelsClient>,
    tool_router: ToolRouter<Self>,
}

impl TravelAgentServer {
    /// Create a new MCP server for the given travel clients.
    ///
    /// Pre: `flights_client` and `hotels_client` are `Arc`-wrapped clients (the
    /// server holds shared references).
    /// Post: returns a server with the tool router initialized; feed it to
    /// `run_stdio`/`run_http` from `delulu-mcp-server-helper`.
    /// Panic-if: never (infallible constructor).
    pub fn new(
        flights_client: Arc<GoogleFlightsClient>,
        hotels_client: Arc<GoogleHotelsClient>,
    ) -> Self {
        Self {
            flights_client,
            hotels_client,
            tool_router: Self::tool_router(),
        }
    }
}

#[tool_router]
impl TravelAgentServer {
    #[tool(
        name = "search_flights",
        description = "Search for flights using Google Flights. Parameters: from (IATA), to (IATA), date (YYYY-MM-DD), return_date (YYYY-MM-DD, optional), seat (Economy/PremiumEconomy/Business/First), adults (1+), children_ages (1-17), trip_type (round-trip/one-way), max_stops."
    )]
    async fn search_flights(&self, params: Parameters<FlightsInput>) -> Result<String, String> {
        let input = params.0;
        let mut passengers = vec![(crate::Passenger::Adult, input.adults)];
        if !input.children_ages.is_empty() {
            passengers.push((crate::Passenger::Child, input.children_ages.len() as u32));
        }
        let params = FlightSearchParams {
            from_airport: input.from,
            to_airport: input.to,
            depart_date: input.date,
            return_date: input.return_date,
            cabin_class: input.seat,
            passengers,
            trip_type: input.trip_type,
            max_stops: input.max_stops,
            preferred_airlines: None,
        };

        let result = self
            .flights_client
            .search_flights(&params)
            .await
            .map_err(|e| format!("Flight search failed: {e}"))?;

        serde_json::to_string(&result.to_mcp_api_response(Vec::new())).map_err(|e| e.to_string())
    }

    #[tool(
        name = "search_hotels",
        description = "Search for hotels using Google Hotels. Parameters: location (city/area/POI), checkin_date (YYYY-MM-DD), checkout_date (YYYY-MM-DD), adults (1+), children_ages, min_guest_rating (3.5+/4+/4.5+), stars (hotel rating 2-5), amenities (indoor_pool/outdoor_pool/pool/spa/kid_friendly/air_conditioned/ev_charger), min_price, max_price."
    )]
    async fn search_hotels(&self, params: Parameters<HotelsInput>) -> Result<String, String> {
        let input = params.0;

        let (valid_amenities, invalid_amenities): (Vec<_>, Vec<_>) = input
            .amenities
            .iter()
            .partition(|a| Amenity::from_str_name(a).is_some());

        let mut warnings: Vec<String> = Vec::new();
        if !invalid_amenities.is_empty() {
            let valid_list = [
                "indoor_pool",
                "outdoor_pool",
                "pool",
                "spa",
                "kid_friendly",
                "air_conditioned",
                "ev_charger",
            ]
            .join(", ");
            warnings.push(format!(
                "Unknown amenity(s): {}. Valid amenities: {}.",
                invalid_amenities
                    .iter()
                    .map(|s| s.as_str())
                    .collect::<Vec<_>>()
                    .join(", "),
                valid_list
            ));
        }

        let amenities: Vec<Amenity> = valid_amenities
            .iter()
            .filter_map(|a| Amenity::from_str_name(a))
            .collect();
        let params = HotelSearchParams {
            version: 1,
            adults: input.adults,
            children_ages: input.children_ages,
            loc_q_search: input.location,
            loc_ts_name: String::new(),
            loc_ts_id: String::new(),
            loc_ts_coords: String::new(),
            checkin_date: input.checkin_date,
            checkout_date: input.checkout_date,
            nights: 0,
            used_guests_dropdown: 0,
            currency: "USD".to_string(),
            sort_order: None,
            min_guest_rating: input.min_guest_rating,
            hotel_stars: input.stars,
            amenities,
            min_price: input.min_price,
            max_price: input.max_price,
        };

        let result = self
            .hotels_client
            .search_hotels(&params)
            .await
            .map_err(|e| format!("Hotel search failed: {e}"))?;

        let search_url = params.get_search_url();
        serde_json::to_string(&result.to_mcp_api_response(
            params.loc_q_search,
            params.checkin_date,
            params.checkout_date,
            params.currency,
            search_url,
            warnings,
        ))
        .map_err(|e| e.to_string())
    }
}

impl_server_handler!(TravelAgentServer);
