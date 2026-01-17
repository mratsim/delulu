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

//! # Unified MCP Server Entry Point
//!
//! Supports stdio transport via subcommand.

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use delulu_travel_agent::{GoogleFlightsClient, GoogleHotelsClient, FlightSearchParams, HotelSearchParams, CabinClass as FlightsCabinClass, TripType as FlightsTripType};
use rmcp::handler::server::{wrapper::Parameters, ServerHandler};
use rmcp::service::serve_server;
use rmcp::tool;
use rmcp::tool_router;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::sync::Arc;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

#[derive(Parser, Debug)]
#[command(name = "travel-mcp")]
#[command(author, version, about = "MCP server for travel search (flights & hotels)")]
struct Args {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Run MCP server over stdio (for Claude Desktop, etc.)
    Stdio,

    /// Run MCP server over HTTP
    Http {
        #[arg(long, default_value = "0.0.0.0")]
        host: String,

        #[arg(long, default_value = "8080")]
        port: u16,
    },
}

#[derive(Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct FlightsInput {
    pub from_airport: String,
    pub to_airport: String,
    pub depart_date: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub return_date: Option<String>,
    #[serde(default = "default_cabin_class")]
    pub cabin_class: CabinClass,
    pub adults: u32,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub children_ages: Vec<i32>,
    #[serde(default = "default_trip_type")]
    pub trip_type: TripType,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_stops: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preferred_airlines: Option<Vec<String>>,
}

#[derive(Serialize, Deserialize, JsonSchema, Copy, Clone)]
#[serde(rename_all = "snake_case")]
pub enum CabinClass {
    #[serde(alias = "economy")]
    Economy,
    #[serde(alias = "premium_economy")]
    PremiumEconomy,
    #[serde(alias = "business")]
    Business,
    #[serde(alias = "first")]
    First,
}

#[derive(Serialize, Deserialize, JsonSchema, Copy, Clone)]
#[serde(rename_all = "snake_case")]
pub enum TripType {
    #[serde(alias = "roundtrip")]
    RoundTrip,
    #[serde(alias = "oneway")]
    OneWay,
}

fn default_cabin_class() -> CabinClass {
    CabinClass::Economy
}

fn default_trip_type() -> TripType {
    TripType::RoundTrip
}

#[derive(Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct HotelsInput {
    pub location: String,
    pub checkin_date: String,
    pub checkout_date: String,
    pub adults: u32,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub children_ages: Vec<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub currency: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_guest_rating: Option<f64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hotel_stars: Vec<i32>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub amenities: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_price: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_price: Option<i32>,
}

#[derive(Clone)]
pub struct TravelAgentServer {
    flights_client: Arc<GoogleFlightsClient>,
    hotels_client: Arc<GoogleHotelsClient>,
}

impl TravelAgentServer {
    pub fn new(flights_client: Arc<GoogleFlightsClient>, hotels_client: Arc<GoogleHotelsClient>) -> Self {
        Self {
            flights_client,
            hotels_client,
        }
    }
}

#[tool_router]
impl TravelAgentServer {
    #[tool(
        name = "search_flights",
        description = "Search for flights using Google Flights. Parameters: from_airport (IATA), to_airport (IATA), depart_date (YYYY-MM-DD), optional return_date, cabin_class (economy/premium_economy/business/first), adults (1+), children_ages (1-17), trip_type (roundtrip/oneway), max_stops, preferred_airlines."
    )]
    async fn search_flights(&self, params: Parameters<FlightsInput>) -> Result<String, String> {
        let input = params.0;
        let cabin_class: FlightsCabinClass = match input.cabin_class {
            CabinClass::Economy => FlightsCabinClass::Economy,
            CabinClass::PremiumEconomy => FlightsCabinClass::PremiumEconomy,
            CabinClass::Business => FlightsCabinClass::Business,
            CabinClass::First => FlightsCabinClass::First,
        };
        let trip_type: FlightsTripType = match input.trip_type {
            TripType::RoundTrip => FlightsTripType::RoundTrip,
            TripType::OneWay => FlightsTripType::OneWay,
        };
        let params = FlightSearchParams {
            from_airport: input.from_airport,
            to_airport: input.to_airport,
            depart_date: input.depart_date,
            return_date: input.return_date,
            cabin_class,
            adults: input.adults,
            children_ages: input.children_ages,
            trip_type,
            max_stops: input.max_stops,
            preferred_airlines: input.preferred_airlines,
        };

        let result = self.flights_client.search_flights(&params)
            .await
            .map_err(|e| e.to_string())?;

        serde_json::to_string(&result).map_err(|e| e.to_string())
    }

    #[tool(
        name = "search_hotels",
        description = "Search for hotels using Google Hotels. Parameters: location, checkin_date (YYYY-MM-DD), checkout_date, adults (1+), children_ages, currency, min_guest_rating, hotel_stars, amenities, min_price, max_price."
    )]
    async fn search_hotels(&self, params: Parameters<HotelsInput>) -> Result<String, String> {
        let input = params.0;
        let params = HotelSearchParams {
            version: 1,
            adults: input.adults,
            children_ages: input.children_ages,
            loc_q_search: input.location.clone(),
            loc_ts_name: input.location,
            loc_ts_id: String::new(),
            loc_ts_coords: String::new(),
            checkin_date: input.checkin_date,
            checkout_date: input.checkout_date,
            nights: 0,
            used_guests_dropdown: 0,
            currency: input.currency.unwrap_or_default(),
            sort_order: None,
            min_guest_rating: input.min_guest_rating,
            hotel_stars: input.hotel_stars,
            amenities: Vec::new(),
            min_price: input.min_price,
            max_price: input.max_price,
        };

        let result = self.hotels_client.search_hotels(&params)
            .await
            .map_err(|e| e.to_string())?;

        serde_json::to_string(&result).map_err(|e| e.to_string())
    }
}

impl ServerHandler for TravelAgentServer {}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::registry()
        .with(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".to_string().into()))
        .with(tracing_subscriber::fmt::layer())
        .init();

    let args = Args::parse();

    let flights_client = Arc::new(
        GoogleFlightsClient::new("en".into(), "USD".into())
            .context("Failed to create flights client")?,
    );
    let hotels_client = Arc::new(
        GoogleHotelsClient::new(4)
            .context("Failed to create hotels client")?,
    );

    match args.command {
        Command::Stdio => {
            tracing::info!("Starting MCP server over stdio...");
            let server = TravelAgentServer::new(flights_client, hotels_client);
            serve_server(Arc::new(server), (std::io::stdin(), std::io::stdout())).await?;
        }
        Command::Http { host, port } => {
            let addr: SocketAddr = format!("{}:{}", host, port).parse()
                .context("Invalid host:port")?;
            tracing::info!("Starting MCP server over HTTP on {}", addr);
            tracing::warn!("HTTP transport not yet implemented");
            anyhow::bail!("HTTP transport not yet implemented");
        }
    }

    Ok(())
}
