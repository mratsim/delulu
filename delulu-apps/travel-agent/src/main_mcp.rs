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

use anyhow::{Context, Error, Result};
use clap::{Parser, Subcommand};
use delulu_travel_agent::{
    Amenity, FlightSearchParams, GoogleFlightsClient, GoogleHotelsClient, HotelSearchParams, Seat,
    Trip,
};
use rmcp::handler::server::{ServerHandler, tool::ToolRouter, wrapper::Parameters};
use rmcp::service::serve_server;
use rmcp::tool;
use rmcp::tool_router;
use rmcp::transport::streamable_http_server::{
    StreamableHttpServerConfig, StreamableHttpService, session::local::LocalSessionManager,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::sync::Arc;
use tracing_subscriber::{EnvFilter, layer::SubscriberExt, util::SubscriberInitExt};

#[derive(Parser, Debug)]
#[command(name = "travel-mcp")]
#[command(
    author,
    version,
    about = "MCP server for travel search (flights & hotels)"
)]
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

#[derive(Serialize, Deserialize, JsonSchema, Default)]
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

#[derive(Clone)]
pub struct TravelAgentServer {
    flights_client: Arc<GoogleFlightsClient>,
    hotels_client: Arc<GoogleHotelsClient>,
    tool_router: ToolRouter<Self>,
}

impl TravelAgentServer {
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
        let mut passengers = vec![(delulu_travel_agent::Passenger::Adult, input.adults)];
        if !input.children_ages.is_empty() {
            passengers.push((
                delulu_travel_agent::Passenger::Child,
                input.children_ages.len() as u32,
            ));
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
            let valid_list = ["indoor_pool", "outdoor_pool", "pool", "spa", "kid_friendly", "air_conditioned", "ev_charger"]
                .join(", ");
            warnings.push(format!(
                "Unknown amenity(s): {}. Valid amenities: {}.",
                invalid_amenities.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", "),
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

impl ServerHandler for TravelAgentServer {
    fn list_tools(
        &self,
        _request: Option<rmcp::model::PaginatedRequestParam>,
        _context: rmcp::service::RequestContext<rmcp::RoleServer>,
    ) -> impl Future<Output = Result<rmcp::model::ListToolsResult, rmcp::ErrorData>> + Send + '_
    {
        tracing::debug!(
            "list_tools called, tools count: {}",
            self.tool_router.list_all().len()
        );
        Box::pin(async move {
            let tools = self.tool_router.list_all();
            tracing::debug!("Returning {} tools", tools.len());
            Ok(rmcp::model::ListToolsResult::with_all_items(tools))
        })
    }

    fn call_tool(
        &self,
        request: rmcp::model::CallToolRequestParam,
        context: rmcp::service::RequestContext<rmcp::RoleServer>,
    ) -> impl Future<Output = Result<rmcp::model::CallToolResult, rmcp::ErrorData>> + Send + '_
    {
        let router = self.tool_router.clone();
        let self_clone = self.clone();
        Box::pin(async move {
            let context =
                rmcp::handler::server::tool::ToolCallContext::new(&self_clone, request, context);
            router.call(context).await
        })
    }

    fn get_info(&self) -> rmcp::model::ServerInfo {
        rmcp::model::ServerInfo {
            protocol_version: rmcp::model::ProtocolVersion::V_2025_03_26,
            capabilities: rmcp::model::ServerCapabilities {
                tools: Some(rmcp::model::ToolsCapability::default()),
                ..Default::default()
            },
            server_info: rmcp::model::Implementation::from_build_env(),
            instructions: None,
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Error> {
    tracing_subscriber::registry()
        .with(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".to_string().into()))
        .with(
            tracing_subscriber::fmt::layer()
                .with_timer(tracing_subscriber::fmt::time::ChronoUtc::rfc_3339())
                .with_writer(std::io::stderr),
        )
        .init();

    tracing::debug!("Parsing arguments...");
    let args = Args::parse();
    tracing::debug!("Parsed args: {:?}", args);

    tracing::debug!("Creating flights client...");
    let flights_client = Arc::new(
        GoogleFlightsClient::new("en".into(), "USD".into())
            .context("Failed to create flights client")?,
    );
    tracing::debug!("Creating hotels client...");
    let hotels_client =
        Arc::new(GoogleHotelsClient::new(4).context("Failed to create hotels client")?);
    tracing::debug!("Clients created");

    match args.command {
        Command::Stdio => {
            eprintln!("Starting MCP server over stdio...");
            let server = TravelAgentServer::new(flights_client, hotels_client);
            let (stdin, stdout) = rmcp::transport::io::stdio();
            tracing::debug!("Starting MCP server on stdio transport...");
            let _running = serve_server(Arc::new(server), (stdin, stdout))
                .await
                .map_err(|e| anyhow::anyhow!("Server error: {}", e))?;
            tracing::debug!("Server running. Press Ctrl+C to stop.");
            std::future::pending::<()>().await;
        }
        Command::Http { host, port } => {
            let addr: SocketAddr = format!("{}:{}", host, port)
                .parse()
                .context("Invalid host:port")?;
            tracing::info!("Starting MCP server over HTTP on {}", addr);
            let server = TravelAgentServer::new(flights_client, hotels_client);
            let session_manager = Arc::new(LocalSessionManager::default());
            let config = StreamableHttpServerConfig {
                stateful_mode: true,
                ..Default::default()
            };
            let service =
                StreamableHttpService::new(move || Ok(server.clone()), session_manager, config);
            let app = axum::Router::new().nest_service("/mcp", service);
            let listener = tokio::net::TcpListener::bind(addr)
                .await
                .context("Failed to bind to address")?;
            tracing::debug!("Listening on {}", addr);
            axum::serve(listener, app)
                .await
                .context("HTTP server error")?;
        }
    }

    Ok(())
}
