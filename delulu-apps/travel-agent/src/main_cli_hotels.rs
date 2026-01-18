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
//!
//! # Examples
//!
//! ## Basic search
//!
//! ```bash
//! delulu-hotels -L "Tokyo, Japan" -i 2026-02-15 -o 2026-02-20
//! ```
//!
//! ## Search with filters
//!
//! ```bash
//! # 4-5 star hotels with pool and spa amenities, under €200/night
//! delulu-hotels -L "Paris" -i 2026-03-01 -o 2026-03-05 -s 4,5 -m pool,spa -p 200
//! ```
//!
//! ## Luxury with high rating
//!
//! ```bash
//! # 5-star hotels with 4.5+ rating, sorted by highest rating
//! delulu-hotels -L "London" -i 2026-04-10 -o 2026-04-15 --rating 4.5 -s 5 --sort highest_rating
//! ```
//!
//! ## Family-friendly with children
//!
//! ```bash
//! delulu-hotels -L "New York" -i 2026-06-01 -o 2026-06-07 -a 2 -c 5,10 -m kid-friendly
//! ```
//!
//! ## Dry run (show URL only)
//!
//! ```bash
//! delulu-hotels -L "Tokyo" -i 2026-02-15 -o 2026-02-20 --dry-run
//! ```
//!
//! # Output
//!
//! The tool prints a summary of the search parameters followed by matching hotels with:
//! - Name and star rating
//! - Price per night
//! - Guest rating and review count
//! - Available amenities

use anyhow::Result;
use clap::{Parser, ValueEnum};
use delulu_travel_agent::{Amenity, GoogleHotelsClient, HotelSearchParams};

#[derive(Parser, Debug)]
#[command(name = "delulu-hotels")]
#[command(version = "0.1.0")]
#[command(about = "Search hotels via Google Hotels API")]
struct Args {
    #[arg(short = 'L', long)]
    location: String,
    #[arg(short = 'i', long)]
    checkin: String,
    #[arg(short = 'o', long)]
    checkout: String,
    #[arg(short = 'a', long, default_value = "2")]
    adults: u32,
    #[arg(
        short = 'c',
        long,
        help = "Children ages (comma-separated, e.g., 5,10)"
    )]
    children: Option<String>,
    #[arg(short = 'C', long, default_value = "EUR")]
    currency: String,
    #[arg(long, help = "Minimum guest rating (3.5, 4.0, 4.5)")]
    rating: Option<f64>,
    #[arg(short = 's', long, help = "Star ratings (comma-separated, e.g., 4,5)")]
    stars: Option<String>,
    #[arg(
        short = 'm',
        long,
        help = "Amenities (comma-separated, e.g., spa,pool,kid-friendly)"
    )]
    amenities: Option<String>,
    #[arg(long, help = "Minimum price per night")]
    min_price: Option<f64>,
    #[arg(short = 'p', long, help = "Maximum price per night")]
    max_price: Option<f64>,
    #[arg(short = 'S', long, value_enum, help = "Sort by")]
    sort: Option<SortOption>,
    #[arg(short = 'n', long, default_value = "10")]
    limit: usize,
    #[arg(long, help = "Show search URL without making request")]
    dry_run: bool,
}

#[derive(Clone, Debug, ValueEnum)]
enum SortOption {
    #[clap(name = "relevance")]
    Relevance,
    #[clap(name = "lowest_price")]
    LowestPrice,
    #[clap(name = "highest_rating")]
    HighestRating,
    #[clap(name = "most_reviewed")]
    MostReviewed,
}

impl std::fmt::Display for SortOption {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SortOption::Relevance => write!(f, "relevance"),
            SortOption::LowestPrice => write!(f, "lowest_price"),
            SortOption::HighestRating => write!(f, "highest_rating"),
            SortOption::MostReviewed => write!(f, "most_reviewed"),
        }
    }
}

fn parse_date(s: &str) -> Result<chrono::NaiveDate> {
    chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d")
        .map_err(|_| anyhow::anyhow!("Invalid date: {}", s))
}

fn parse_children_ages(s: &str) -> Result<Vec<i32>, std::num::ParseIntError> {
    if s.is_empty() {
        return Ok(Vec::new());
    }
    // Children of age 0~1 are set to 1 in the UI
    s.split(',')
        .map(|age| {
            let parsed: Result<i32, _> = age.trim().parse();
            parsed.map(|a| if a == 0 { 1 } else { a })
        })
        .collect()
}

fn parse_amenities(s: &str) -> (Vec<Amenity>, Vec<String>) {
    if s.is_empty() {
        return (Vec::new(), Vec::new());
    }
    let (valid, invalid): (Vec<_>, Vec<_>) = s
        .split(',')
        .map(|a| a.trim().to_uppercase().replace('-', "_"))
        .partition(|a| Amenity::from_str_name(a).is_some());
    let valid: Vec<Amenity> = valid
        .into_iter()
        .filter_map(|a| Amenity::from_str_name(&a))
        .collect();
    (valid, invalid)
}

fn parse_stars(s: &str) -> Result<Vec<i32>> {
    if s.is_empty() {
        return Ok(Vec::new());
    }
    s.split(',')
        .map(|a| {
            let v: i32 = a
                .trim()
                .parse()
                .map_err(|_| anyhow::anyhow!("Invalid star rating: {}", a.trim()))?;
            if (1..=5).contains(&v) {
                Ok(v)
            } else {
                Err(anyhow::anyhow!("Star rating out of range (1-5): {}", v))
            }
        })
        .collect()
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    let checkin = parse_date(&args.checkin)?;
    let checkout = parse_date(&args.checkout)?;

    let children_ages = args
        .children
        .as_ref()
        .map(|s| parse_children_ages(s))
        .transpose()
        .map_err(|e| anyhow::anyhow!("Failed to parse children ages: {}", e))?
        .unwrap_or_default();

    let stars_filter: Vec<i32> = args
        .stars
        .as_deref()
        .map(parse_stars)
        .transpose()
        .map_err(|e| anyhow::anyhow!("Failed to parse stars: {}", e))?
        .unwrap_or_default();
    let (amenities_filter, invalid_amenities) = args
        .amenities
        .as_deref()
        .map(parse_amenities)
        .unwrap_or_default();
    if !invalid_amenities.is_empty() {
        eprintln!(
            "Warning: Unknown amenities ignored: {}",
            invalid_amenities.join(", ")
        );
    }

    let sort_order = match args.sort {
        Some(SortOption::Relevance) => None,
        Some(SortOption::LowestPrice) => Some(delulu_travel_agent::SortType::LowestPrice),
        Some(SortOption::HighestRating) => {
            Some(delulu_travel_agent::SortType::HighestRating)
        }
        Some(SortOption::MostReviewed) => Some(delulu_travel_agent::SortType::MostReviewed),
        None => None,
    };

    let request = HotelSearchParams::builder(
        args.location.clone(),
        checkin,
        checkout,
        args.adults,
        children_ages.clone(),
    )
    .currency(args.currency)
    .min_guest_rating(args.rating.unwrap_or(0.0))
    .hotel_stars(stars_filter)
    .amenities(amenities_filter)
    .min_price(args.min_price.map(|p| p as i32))
    .max_price(args.max_price.map(|p| p as i32))
    .sort_order(sort_order)
    .build()?;

    let search_url = request.get_search_url();

    let children_count = children_ages.len() as u32;

    println!("\n🏨 Google Hotels Search");
    println!("=======================");
    println!("Location: {}", args.location);
    println!("Dates: {} to {}", checkin, checkout);
    println!(
        "Guests: {} adults, {} children",
        args.adults, children_count
    );
    if !children_ages.is_empty() {
        println!(
            "Children ages: {}",
            children_ages
                .iter()
                .map(|a| a.to_string())
                .collect::<Vec<_>>()
                .join(", ")
        );
    }
    if let Some(r) = args.rating {
        println!("Min rating: {}", r);
    }
    if let Some(s) = &args.stars {
        println!("Stars: {}", s);
    }
    if let Some(a) = &args.amenities {
        println!("Amenities: {}", a);
    }
    if let Some(so) = args.sort {
        println!("Sort: {}", so);
    }
    println!("=======================");

    if args.dry_run {
        println!("\n🔗 Search URL:\n{}", search_url);
        return Ok(());
    }

    println!("\n🔗 Search URL: {}\n", search_url);

    const MAX_CONCURRENT_REQUESTS: u64 = 4;
    let client = GoogleHotelsClient::new(MAX_CONCURRENT_REQUESTS)?;
    match client.search_hotels(&request).await {
        Ok(results) => {
            if results.hotels.is_empty() {
                println!("No hotels found.");
            } else {
                println!("Found {} hotel(s)", results.hotels.len());
                if let Some(ref lowest) = results.lowest_price {
                    println!("Lowest: {}\n", lowest);
                }
                for (i, hotel) in results.hotels().take(args.limit).enumerate() {
                    let stars = hotel.star_class.as_deref().unwrap_or("");
                    println!("{}. {}", i + 1, hotel.name);
                    if !stars.is_empty() {
                        println!("   {}", stars.trim());
                    }
                    println!("   Price: {}", hotel.price);
                    if let Some(r) = hotel.rating {
                        let reviews = hotel.reviews.unwrap_or(0);
                        println!("   Rating: {:.1} ({} reviews)", r, reviews);
                    }
                    if !hotel.amenities.is_empty() {
                        println!("   Amenities: {}", hotel.amenities.join(", "));
                    }
                    if let Some(loc) = &hotel.location_rating {
                        println!("   Location: {}", loc);
                    }
                    println!();
                }
            }
        }
        Err(e) => {
            eprintln!("Search failed: {:#}", e);
            std::process::exit(1);
        }
    }
    Ok(())
}
