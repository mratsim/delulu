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

//! CLI tool for searching Google Flights.
//!
//! Usage:
//!   delulu-flights --from SFO --to LHR --date 2026-04-06
//!   delulu-flights --from JFK --to LAX --date 2026-05-15 --cabin business --currency EUR
//!

use std::cmp::max;
use std::fmt;

use anyhow::{Context, Result};
use chrono::NaiveDate;
use clap::{Parser as ClapParser, ValueEnum};
use serde_json;

use delulu_travel_agent::{
    CabinClass, FlightSearchConfig, FlightSearchResult, FlightSegment, GoogleFlightsClient,
    Itinerary, PassengerType, Tfs, TripType,
};

/// Cabin class options for flights.
#[derive(Copy, Clone, Debug, PartialEq, Eq, ValueEnum)]
enum CabinArg {
    Economy,
    PremiumEconomy,
    Business,
    First,
}

impl fmt::Display for CabinArg {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CabinArg::Economy => write!(f, "economy"),
            CabinArg::PremiumEconomy => write!(f, "premium_economy"),
            CabinArg::Business => write!(f, "business"),
            CabinArg::First => write!(f, "first"),
        }
    }
}

impl From<CabinArg> for CabinClass {
    fn from(val: CabinArg) -> Self {
        match val {
            CabinArg::Economy => CabinClass::Economy,
            CabinArg::PremiumEconomy => CabinClass::PremiumEconomy,
            CabinArg::Business => CabinClass::Business,
            CabinArg::First => CabinClass::First,
        }
    }
}

/// Trip type options for flights.
#[derive(Copy, Clone, Debug, PartialEq, Eq, ValueEnum)]
enum TripArg {
    #[value(name = "one-way")]
    OneWay,
    #[value(name = "round-trip", alias = "roundtrip")]
    RoundTrip,
}

impl fmt::Display for TripArg {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TripArg::OneWay => write!(f, "one-way"),
            TripArg::RoundTrip => write!(f, "round-trip"),
        }
    }
}

impl From<TripArg> for TripType {
    fn from(val: TripArg) -> Self {
        match val {
            TripArg::OneWay => TripType::OneWay,
            TripArg::RoundTrip => TripType::RoundTrip,
        }
    }
}

#[derive(Clone, Debug, ValueEnum)]
enum OutputFormat {
    /// Compact summary (best price, flight count)
    Summary,
    /// Detailed table view
    Table,
    /// JSON output for scripting
    Json,
    /// Minimal single-line output
    Compact,
}

impl fmt::Display for OutputFormat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            OutputFormat::Summary => write!(f, "summary"),
            OutputFormat::Table => write!(f, "table"),
            OutputFormat::Json => write!(f, "json"),
            OutputFormat::Compact => write!(f, "compact"),
        }
    }
}

#[derive(ClapParser, Debug)]
#[command(name = "delulu-flights")]
#[command(version = "0.1.0")]
#[command(author = "mratsim")]
#[command(about = "Search Google Flights from the command line")]
struct CliArgs {
    /// Origin airport code (e.g., SFO, JFK, LHR)
    #[arg(short, long)]
    from: String,

    /// Destination airport code (e.g., LHR, DXB, SIN)
    #[arg(short, long)]
    to: String,

    /// Departure date in YYYY-MM-DD format
    #[arg(short, long, value_parser = parse_date_arg)]
    date: NaiveDate,

    /// Currency for prices (USD, EUR, GBP, etc.)
    #[arg(long, default_value = "USD")]
    currency: String,

    /// Language for results (en, de, fr, es, etc.)
    #[arg(long, default_value = "en")]
    language: String,

    /// Cabin class
    #[arg(long, default_value = "economy")]
    cabin: CabinArg,

    /// Trip type (one-way or round-trip)
    #[arg(long, default_value = "round-trip")]
    trip: TripArg,

    /// Number of adult passengers
    #[arg(long, default_value = "1")]
    adults: u32,

    /// Maximum number of concurrent requests
    #[arg(long, default_value = "4")]
    max_concurrent: u64,

    /// Output format
    #[arg(long, default_value = "summary")]
    format: OutputFormat,

    /// Show debug URL without making request
    #[arg(long)]
    dry_run: bool,

    /// Increase verbosity
    #[arg(short, long, action = clap::ArgAction::Count)]
    verbose: u8,
}

/// Helper to unwrap or display a fallback
fn opt_display<T: fmt::Display>(opt: &Option<T>, fallback: &str) -> String {
    opt.as_ref()
        .map(|v| v.to_string())
        .unwrap_or_else(|| fallback.to_string())
}

/// Helper to unwrap i32 option
fn opt_i32(opt: &Option<i32>, fallback: i32) -> i32 {
    opt.unwrap_or(fallback)
}

/// Parse date from YYYY-MM-DD string.
fn parse_date_arg(s: &str) -> Result<NaiveDate, chrono::ParseError> {
    NaiveDate::parse_from_str(s, "%Y-%m-%d")
}

/// Configure tracing based on verbosity level.
fn setup_logging(verbose: u8) {
    let level = match verbose {
        0 => "warn",
        1 => "info",
        2 => "debug",
        _ => "trace",
    };

    let level = level.parse().unwrap_or(tracing::Level::INFO);
    tracing_subscriber::fmt().with_max_level(level).init();
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

/// Detect suspicious layover claims:
/// Flags flights claiming "direct/nonstop" but with physically impossible durations.
fn analyze_route_quality(stops: Option<i32>, duration_minutes: i32) -> (&'static str, bool) {
    match stops {
        Some(0) => {
            // Intercontinental "nonstop" should realistically be 6-18h max
            // Any claimed nonstop exceeding 18h almost certainly has connections
            if duration_minutes > 1080 {
                // 18 hours
                ("⚠️ SUSPICIOUS", true)
            } else {
                ("direct", false)
            }
        }
        Some(1) => ("1 stop", false),
        Some(n) => {
            let label = if n == 2 { "2 stops" } else { "multiple" };
            (label, false)
        }
        None => ("unknown", false),
    }
}

/// Get first flight segment
#[allow(clippy::needless_lifetimes)] // Clippy is wrong about lifetimes
fn first_seg<'a>(itin: &'a Itinerary) -> Option<&'a FlightSegment> {
    itin.flights.first()
}

/// Calculate terminal-aware column widths
fn calc_column_widths(
    itins: &[Itinerary],
    show_rank: bool,
) -> (usize, usize, usize, usize, usize, usize) {
    let mut max_airline = 7;
    let mut max_flight = 6;
    let mut max_times = 15;
    let mut max_duration = 10;
    let mut max_via = 12;
    let min_rank = 4;

    for itin in itins {
        if let Some(seg) = first_seg(itin) {
            max_airline = max(max_airline, opt_display(&seg.airline, "??").len());
            max_flight = max(max_flight, opt_display(&seg.flight_number, "????").len());
            max_times = max(
                max_times,
                fmt_times(&seg.departure_time, &seg.arrival_time).len(),
            );
            max_duration = max(
                max_duration,
                fmt_duration(opt_i32(&itin.duration_minutes, 0)).len(),
            );

            let (via, _) = analyze_route_quality(itin.stops, opt_i32(&itin.duration_minutes, 0));
            max_via = max(max_via, via.len());
        }
    }

    let overhead = if show_rank { 10 } else { 0 };
    let price_space = 12;
    let total_needs =
        max_airline + max_flight + max_times + max_duration + max_via + overhead + price_space;

    let term_w = get_terminal_width();

    if total_needs > term_w {
        let shrink = (term_w as f64) / (total_needs as f64);
        (
            min_rank,
            (max_airline as f64 * shrink).ceil() as usize,
            (max_flight as f64 * shrink).ceil() as usize,
            (max_times as f64 * shrink).ceil() as usize,
            (max_duration as f64 * shrink).ceil() as usize,
            (max_via as f64 * shrink).ceil() as usize,
        )
    } else {
        (
            min_rank,
            max_airline,
            max_flight,
            max_times,
            max_duration,
            max_via,
        )
    }
}

/// Main printing function

fn print_results(
    result: FlightSearchResult,
    config: &FlightSearchConfig,
    format: OutputFormat,
    search_url: Option<&str>,
) {
    match format {
        OutputFormat::Json => {
            #[derive(serde::Serialize)]
            struct JsonOutput<'a> {
                count: usize,
                generated_at: &'a str,
                search_url: Option<&'a str>,
            }

            let output = JsonOutput {
                count: result.itineraries.len(),
                generated_at: &result.generated_at,
                search_url,
            };

            println!("{}", serde_json::to_string_pretty(&output).unwrap());
        }

        OutputFormat::Summary | OutputFormat::Compact => {
            let term_w = get_terminal_width();
            let title_sep_len = 40usize.max(term_w.saturating_sub(4));
            let title_bar = "=".repeat(title_sep_len);
            let dash_bar = "-".repeat(title_sep_len);

            println!("\n{}", title_bar);
            println!(
                "  🛫  {} → {} on {}",
                config.from_airport,
                config.to_airport,
                config.depart_date.format("%Y-%m-%d")
            );
            println!("{}\n", title_bar);

            let best_price = result
                .itineraries
                .first()
                .map(|i| opt_i32(&i.price, 0))
                .unwrap_or(0);

            println!("💰 Best Price:  ${}", best_price);
            println!("📊 Total Flights: {}", result.itineraries.len());
            println!("⏱ Generated: {}", result.generated_at);

            if let Some(url) = search_url {
                println!("\n🔗 Search URL: {}", url);
            }

            // Calculate column widths
            let (rw, aw, fw, tw, dw, vw) = calc_column_widths(&result.itineraries, true);

            println!("\n🏆 Top {} Cheapest:", 5.min(result.itineraries.len()));
            println!("{}\n", dash_bar);

            // Header with manual padding
            let h1 = format!("  {:>w$}", "#", w = rw);
            let h2 = format!("{:<w$}", "AIRLINE", w = aw);
            let h3 = format!("{:<w$}", "FLIGHT", w = fw);
            let h4 = format!("{:<w$}", "DEP → ARR", w = tw);
            let h5 = format!("{:<w$}", "DURATION", w = dw);
            let h6 = format!("{:<w$}", "STATUS", w = vw);
            println!("{}  {}  {}  {}  {}  {}   PRICE", h1, h2, h3, h4, h5, h6);
            println!("{}\n", dash_bar);

            // Data rows with individual cell formatting
            for (i, itin) in result.itineraries.iter().take(5).enumerate() {
                if let Some(seg) = first_seg(itin) {
                    let (via_label, is_suspicious) =
                        analyze_route_quality(itin.stops, opt_i32(&itin.duration_minutes, 0));
                    let price = opt_i32(&itin.price, 0);
                    let warn = if is_suspicious { " ⚠️" } else { "" };

                    let c1 = format!("  {:>w$}", i + 1, w = rw);
                    let c2 = format!("{:<w$}", opt_display(&seg.airline, "??"), w = aw);
                    let c3 = format!("{:<w$}", opt_display(&seg.flight_number, "????"), w = fw);
                    let c4 = format!(
                        "{:<w$}",
                        fmt_times(&seg.departure_time, &seg.arrival_time),
                        w = tw
                    );
                    let c5 = format!(
                        "{:<w$}",
                        fmt_duration(opt_i32(&itin.duration_minutes, 0)),
                        w = dw
                    );
                    let c6 = format!("{:<w$}", via_label, w = vw);

                    println!(
                        "{}  {}  {}  {}  {}  {}   ${}{}",
                        c1, c2, c3, c4, c5, c6, price, warn
                    );
                }
            }

            println!("\n⚠️ = Highly unlikely to be truly direct (verify with URL)");
        }

        OutputFormat::Table => {
            let (_, aw, fw, tw, dw, vw) = calc_column_widths(&result.itineraries, false);

            if let Some(url) = search_url {
                println!("\n🔗 Verification URL:\n   {}\n", url);
            }

            println!("\n FULL FLIGHTS TABLE");

            // Header with proper column widths
            let h_airline = format!("{:<w$}", "AIRLINE", w = aw);
            let h_flight = format!("{:<w$}", "FLIGHT", w = fw);
            let h_times = format!("{:<w$}", "DEP → ARR", w = tw);
            let h_dur = format!("{:<w$}", "DURATION", w = dw);
            let h_via = format!("{:<w$}", "ROUTES", w = vw);
            let header_row = format!(
                "  {}  {}  {}  {}  {}   PRICE",
                h_airline, h_flight, h_times, h_dur, h_via
            );
            let sep_len = header_row.len();
            let sep = "-".repeat(sep_len);
            println!("{}", sep);
            println!("{}", header_row);
            println!("{}\n", sep);

            // All rows with correct column widths (matching header)
            for itin in &result.itineraries {
                if let Some(seg) = first_seg(itin) {
                    let (via_label, is_suspicious) =
                        analyze_route_quality(itin.stops, opt_i32(&itin.duration_minutes, 0));
                    let price = opt_i32(&itin.price, 0);
                    let warn = if is_suspicious { "⚠️" } else { "" };

                    let cell_airline = format!("  {:<w$}", opt_display(&seg.airline, "??"), w = aw);
                    let cell_flight =
                        format!("{:<w$}", opt_display(&seg.flight_number, "????"), w = fw);
                    let cell_times = format!(
                        "{:<w$}",
                        fmt_times(&seg.departure_time, &seg.arrival_time),
                        w = tw
                    );
                    let cell_dur = format!(
                        "{:<w$}",
                        fmt_duration(opt_i32(&itin.duration_minutes, 0)),
                        w = dw
                    );
                    let cell_via = format!("{:<w$}", format!("{}{}", via_label, warn), w = vw);

                    println!(
                        "{}  {}  {}  {}  {}   ${}",
                        cell_airline, cell_flight, cell_times, cell_dur, cell_via, price
                    );
                }
            }

            println!("\n{}", sep);
            if let Some(url) = search_url {
                println!("\n🖥 Open URL for live updates: {}", url);
            }
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = CliArgs::parse();
    setup_logging(args.verbose);

    let from = args.from;
    let to = args.to;
    let depart_date = args.date;

    let flight_config = FlightSearchConfig {
        from_airport: from.to_uppercase(),
        to_airport: to.to_uppercase(),
        depart_date,
        cabin_class: args.cabin.into(),
        passengers: vec![(PassengerType::Adult, args.adults)],
        trip_type: args.trip.into(),
        max_stops: None,
        preferred_airlines: None,
    };

    // Dry run: just show URL
    if args.dry_run {
        let tfs = Tfs::from_config(&flight_config, &args.language, &args.currency)?;
        println!("Dry run - Google Flights URL:\n");
        println!("{}", tfs.get_url());
        return Ok(());
    }

    // Generate URL for display
    let tfs = Tfs::from_config(&flight_config, &args.language, &args.currency)?;
    let search_url_owned = tfs.get_url();

    let client = GoogleFlightsClient::new(
        args.language.clone(),
        args.currency.clone(),
        args.max_concurrent,
    )
    .context("Create client")?;

    tracing::info!("Fetching flights...");

    let result = match client.search(&flight_config).await {
        Ok(r) => r,
        Err(e) => {
            println!("\n🔗 Search URL used: {}", search_url_owned);
            anyhow::bail!("Search failed: {:?}", e);
        }
    };

    print_results(
        result,
        &flight_config,
        args.format,
        Some(search_url_owned.as_str()),
    );

    Ok(())
}
