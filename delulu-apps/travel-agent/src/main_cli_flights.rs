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

//! CLI for Google Flights search.

use anyhow::{Context, Result};
use chrono::NaiveDate;
use clap::Parser;
use delulu_travel_agent::{
    FlightSearchParams, FlightSearchResult, GoogleFlightsClient, Passenger, Seat, Trip,
};
use std::cmp::max;
use term_size;

/// CLI arguments
#[derive(Parser, Debug)]
#[command(name = "delulu-flights")]
#[command(author, version, about, long_about = None)]
struct CliArgs {
    /// Origin airport code (e.g., SFO, LAX)
    #[arg(short, long)]
    from: String,

    /// Destination airport code (e.g., JFK, LHR)
    #[arg(short, long)]
    to: String,

    /// Departure date (YYYY-MM-DD or YYYY/MM/DD)
    #[arg(short, long)]
    date: String,

    /// Return date for round trips (YYYY-MM-DD or YYYY/MM/DD)
    #[arg(short = 'R', long)]
    return_date: Option<String>,

    /// Cabin class: economy, premium_economy, business, first
    #[arg(short, long, default_value = "economy")]
    cabin: String,

    /// Number of passengers (adults)
    #[arg(short, long, default_value = "1")]
    passengers: u32,

    /// Trip type: roundtrip, oneway
    #[arg(short, long, default_value = "roundtrip")]
    trip: String,

    /// Maximum number of stops (0 = nonstop only)
    #[arg(long)]
    max_stops: Option<i32>,

    /// Preferred airlines (comma-separated, e.g., "AA,DL,UA")
    #[arg(long)]
    preferred_airlines: Option<String>,

    /// Verbose output
    #[arg(short, long, default_value = "false")]
    verbose: bool,

    /// Save raw HTML response to file for debugging
    #[arg(long)]
    save_html: bool,
}

/// Configure logging based on verbosity level
fn setup_logging(verbose: bool) {
    let level = if verbose {
        tracing::Level::DEBUG
    } else {
        tracing::Level::INFO
    };
    tracing_subscriber::fmt().with_max_level(level).init();
}

/// Parse cabin class string to Seat enum
fn parse_cabin(s: &str) -> Result<Seat> {
    match s.to_lowercase().as_str() {
        "economy" | "e" => Ok(Seat::Economy),
        "premium_economy" | "premium" | "pe" => Ok(Seat::PremiumEconomy),
        "business" | "b" => Ok(Seat::Business),
        "first" | "f" => Ok(Seat::First),
        _ => anyhow::bail!(
            "Invalid cabin class: {}. Use: economy, premium_economy, business, first",
            s
        ),
    }
}

/// Parse trip type string to Trip enum
fn parse_trip(s: &str) -> Result<Trip> {
    match s.to_lowercase().as_str() {
        "roundtrip" | "round" | "rt" => Ok(Trip::RoundTrip),
        "oneway" | "one" | "ow" => Ok(Trip::OneWay),
        _ => anyhow::bail!("Invalid trip type: {}. Use: roundtrip, oneway", s),
    }
}

/// Parse date string to NaiveDate
fn parse_date(s: &str) -> Result<NaiveDate> {
    NaiveDate::parse_from_str(s, "%Y-%m-%d")
        .or_else(|_| NaiveDate::parse_from_str(s, "%Y/%m/%d"))
        .context(format!(
            "Invalid date format: {}. Use YYYY-MM-DD or YYYY/MM/DD",
            s
        ))
}

/// Helper: convert Option<String> to display string
fn opt_display(opt: &Option<String>, default: &str) -> String {
    opt.as_deref().unwrap_or(default).to_string()
}

/// Helper: convert Option<i32> to i32 with default
fn opt_i32(opt: &Option<i32>, default: i32) -> i32 {
    opt.unwrap_or(default)
}

/// Format duration in hours/minutes.
fn fmt_duration(minutes: i32) -> String {
    let hrs = minutes / 60;
    let mins = minutes % 60;
    if mins == 0 {
        format!("{}h", hrs)
    } else if hrs == 0 {
        format!("{}m", mins)
    } else {
        format!("{}h {:02}m", hrs, mins)
    }
}

/// Format departure/arrival times.
fn fmt_times(dep: &Option<String>, arr: &Option<String>) -> String {
    let dep_str = dep.as_deref().unwrap_or("??:??");
    let arr_str = arr.as_deref().unwrap_or("??:??");
    format!("{} → {}", dep_str, arr_str)
}

/// Get terminal width for responsive tables
fn get_terminal_width() -> usize {
    term_size::dimensions().map(|(w, _)| w).unwrap_or(100)
}

/// Get first flight segment
#[allow(clippy::needless_lifetimes)] // Clippy is wrong about lifetimes
fn first_seg<'a>(
    itin: &'a delulu_travel_agent::Itinerary,
) -> Option<&'a delulu_travel_agent::FlightSegment> {
    itin.flights.first()
}

/// Format stops and layovers combined: "2 stops: 5h09@Vancouver, 2h20@Brisbane"
fn fmt_stops_and_layovers(layovers: &[delulu_travel_agent::Layover]) -> String {
    let stops = layovers.len();
    match stops {
        0 => "direct".to_string(),
        1 => {
            if let Some(l) = layovers.first() {
                let dur = l
                    .duration_minutes
                    .map_or("??".to_string(), |m| fmt_duration(m));
                let name = l.airport_city.as_deref().unwrap_or("Unknown");
                format!("1 stop: {}@{}", dur, name)
            } else {
                "1 stop".to_string()
            }
        }
        n => {
            let parts: Vec<String> = layovers
                .iter()
                .map(|l| {
                    let dur = l
                        .duration_minutes
                        .map_or("??".to_string(), |m| fmt_duration(m));
                    let name = l.airport_city.as_deref().unwrap_or("Unknown");
                    format!("{}@{}", dur, name)
                })
                .collect();
            let layover_str = format!(": {}", parts.join(", "));
            format!("{} stops{}", n, layover_str)
        }
    }
}

/// Calculate terminal-aware column widths
fn calc_column_widths(
    itins: &[delulu_travel_agent::Itinerary],
    _show_rank: bool,
) -> (usize, usize, usize, usize, usize) {
    let mut max_airline = 7;
    let mut max_times = 15;
    let mut max_duration = 10;
    let mut max_stops = 25;

    for itin in itins {
        if let Some(seg) = first_seg(itin) {
            max_airline = max(max_airline, opt_display(&seg.airline, "??").len());
            max_times = max(
                max_times,
                fmt_times(&seg.departure_time, &seg.arrival_time).len(),
            );
            max_duration = max(
                max_duration,
                fmt_duration(opt_i32(&itin.duration_minutes, 0)).len(),
            );
            let stops_label = fmt_stops_and_layovers(&itin.layovers);
            max_stops = max(max_stops, stops_label.len());
        }
    }

    let terminal_width = get_terminal_width();
    let available_width = terminal_width.saturating_sub(25);
    let total_content = max_airline + max_times + max_duration + max_stops;

    if total_content > available_width && available_width > 50 {
        let ratio = available_width as f64 / total_content as f64;
        max_airline = (max_airline as f64 * ratio).floor() as usize;
        max_times = (max_times as f64 * ratio).floor() as usize;
        max_duration = (max_duration as f64 * ratio).floor() as usize;
        max_stops = (max_stops as f64 * ratio).floor() as usize;

        max_airline = max(max_airline, 4);
        max_times = max(max_times, 10);
        max_duration = max(max_duration, 5);
        max_stops = max(max_stops, 10);
    }

    let rank_width = 5; // Fixed width for rank column (`#1-`#5)
    (rank_width, max_airline, max_times, max_duration, max_stops)
}

/// Render results to stdout
fn render_results(result: &delulu_travel_agent::FlightSearchResult, search_url: Option<&str>) {
    let params = &result.search_params;

    let title_bar = format!(
        "================================================================================================\n  🛫  {} → {} on {}\n================================================================================================",
        params.from_airport, params.to_airport, params.depart_date
    );
    println!("{}\n", title_bar);

    let best_price = result
        .itineraries
        .first()
        .and_then(|i| i.price)
        .unwrap_or(0);

    println!("💰 Best Price:  ${}", best_price);
    println!("📊 Total Flights: {}", result.itineraries.len());

    if let Some(url) = search_url {
        println!("\n🔗 Search URL: {}", url);
    }

    // Calculate column widths
    let (rw, aw, tw, dw, sw) = calc_column_widths(&result.itineraries, true);

    println!("\n🏆 Top {} Results:", 5.min(result.itineraries.len()));
    println!("{}\n", dash_bar());

    // Header with manual padding
    let h1 = format!("  {:>w$}", "#", w = rw);
    let h2 = format!("{:<w$}", "AIRLINE", w = aw);
    let h3 = format!("{:<w$}", "DEP → ARR", w = tw);
    let h4 = format!("{:<w$}", "DURATION", w = dw);
    let h5 = format!("{:<w$}", "LAYOVERS", w = sw);
    println!("{}  {}  {}  {}  {}   PRICE", h1, h2, h3, h4, h5);
    println!("{}\n", dash_bar());

    // Data rows with individual cell formatting
    for (i, itin) in result.itineraries.iter().take(5).enumerate() {
        if let Some(seg) = first_seg(itin) {
            let stops_label = fmt_stops_and_layovers(&itin.layovers);
            let is_suspicious =
                stops_label == "direct" && opt_i32(&itin.duration_minutes, 0) > 1080;
            let price = opt_i32(&itin.price, 0);
            let warn = if is_suspicious { " ⚠️" } else { "" };

            let c1 = format!("  {:>w$}", i + 1, w = rw);
            let c2 = format!("{:<w$}", opt_display(&seg.airline, "??"), w = aw);
            let c3 = format!(
                "{:<w$}",
                fmt_times(&seg.departure_time, &seg.arrival_time),
                w = tw
            );
            let c4 = format!(
                "{:<w$}",
                fmt_duration(opt_i32(&itin.duration_minutes, 0)),
                w = dw
            );
            let c5 = format!("{:<w$}", stops_label, w = sw);

            println!(
                "{}  {}  {}  {}  {}   ${}{}",
                c1, c2, c3, c4, c5, price, warn
            );
        }
    }
}

fn dash_bar() -> String {
    "-".repeat(get_terminal_width().min(100))
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = CliArgs::parse();
    setup_logging(args.verbose);

    tracing::info!("Starting delulu-flights CLI");
    tracing::info!("Args: {:?}", args);

    // Parse and validate inputs
    let cabin = parse_cabin(&args.cabin)?;
    let trip = parse_trip(&args.trip)?;
    let depart_date = parse_date(&args.date)?;
    let return_date = args.return_date.map(|d| parse_date(&d)).transpose()?;

    tracing::info!(
        "Parsed request: {} -> {} on {:?} ({:?}, {:?})",
        args.from,
        args.to,
        depart_date,
        cabin,
        trip
    );

    // Build search params
    let passengers = vec![(Passenger::Adult, args.passengers)];
    let mut builder = FlightSearchParams::builder(
        args.from.to_uppercase(),
        args.to.to_uppercase(),
        depart_date,
    )
    .cabin_class(cabin)
    .passengers(passengers)
    .trip_type(trip);

    if let Some(rd) = return_date {
        builder = builder.return_date(rd);
    }

    let params = builder
        .build()
        .context("Failed to build search parameters")?;

    let search_url = params.get_search_url();
    tracing::debug!("Generated search URL ({} chars)", search_url.len());

    // Create client and execute search
    let client = GoogleFlightsClient::new(
        "en".into(),
        "USD".into(),
        5, // timeout_secs
        2, // queries_per_second
    )?;

    let result = if args.save_html {
        let url = params.get_search_url();
        let html = client.fetch_raw(&url).await.context("Fetch failed")?;
        let filename = format!("debug_{}_{}.html", args.from, args.to);
        std::fs::write(&filename, &html).context("Failed to write HTML file")?;
        tracing::info!("Saved HTML to {}", filename);

        FlightSearchResult::from_html(&html, params.clone()).context("Parse failed")?
    } else {
        client
            .search_flights(&params)
            .await
            .context("Search failed")?
    };

    tracing::info!(
        "Search completed: {} itineraries found, best price: ${}",
        result.itineraries.len(),
        result
            .itineraries
            .first()
            .and_then(|i| i.price)
            .unwrap_or(0)
    );

    // Render results
    render_results(&result, Some(&search_url));

    Ok(())
}
