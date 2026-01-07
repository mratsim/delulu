//! # Prost Abstraction Layer
//!
//! Wrapper around prost-generated protobuf code for Google Flights.
//! Provides encoding functions that accept primitives directly.

use anyhow::{anyhow, Result};
use prost::Message;

// Pull in the generated protobuf code.
// It triggers clippy
#[allow(clippy::enum_variant_names)]
pub mod google_flights {
    include!(concat!(env!("OUT_DIR"), "/google_flights.rs"));
}
pub use google_flights::*;

// =============================================================================
// High-level encoding API
// =============================================================================

/// Encode flight search parameters to protobuf bytes.
#[allow(clippy::too_many_arguments)]
pub(crate) fn encode_flight_config(
    from_airport: &str,
    to_airport: &str,
    date: chrono::NaiveDate,
    cabin_class: i32,
    trip_type: i32,
    passengers: &[(i32, u32)],
    max_stops: Option<i32>,
    preferred_airlines: Option<&[String]>,
) -> Result<Vec<u8>> {
    let info = Info {
        data: vec![FlightData {
            date: format_date(date),
            max_stops: max_stops.filter(|&v| v != 0), // Skip zero (proto3 omits defaults)
            airlines: preferred_airlines
                .map(|arr| arr.to_vec())
                .unwrap_or_default(),
            from_flight: Some(Airport {
                airport: from_airport.to_string(),
            }),
            to_flight: Some(Airport {
                airport: to_airport.to_string(),
            }),
        }],
        passengers: passengers
            .iter()
            .flat_map(|(ptype, count)| std::iter::repeat_n(*ptype, *count as usize))
            .collect(),
        seat: Some(cabin_class), // Wrap in Some for optional field
        trip: Some(trip_type),   // Wrap in Some for optional field
    };

    let mut buf = Vec::new();
    info.encode(&mut buf)
        .map_err(|e| anyhow!("Encode failed: {}", e))?;
    Ok(buf)
}

fn format_date(date: chrono::NaiveDate) -> String {
    date.format("%Y-%m-%d").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decode_info(data: &[u8]) -> Result<Info> {
        Info::decode(data).map_err(|e| anyhow!("Decode failed: {}", e))
    }

    #[test]
    fn roundtrip_simple() {
        let encoded = encode_flight_config(
            "SFO",
            "JFK",
            chrono::NaiveDate::from_ymd_opt(2025, 7, 15).unwrap(),
            1,         // Economy
            2,         // OneWay
            &[(1, 1)], // 1 Adult
            Some(0),
            None,
        )
        .unwrap();

        let decoded = decode_info(&encoded).unwrap();
        assert_eq!(decoded.data.len(), 1);
        assert_eq!(decoded.data[0].date, "2025-07-15");
        assert_eq!(decoded.data[0].from_flight.as_ref().unwrap().airport, "SFO");
        assert_eq!(decoded.data[0].to_flight.as_ref().unwrap().airport, "JFK");
    }

    #[test]
    fn roundtrip_multi_passenger() {
        let encoded = encode_flight_config(
            "LAX",
            "CDG",
            chrono::NaiveDate::from_ymd_opt(2025, 8, 1).unwrap(),
            3,                 // Business
            1,                 // RoundTrip
            &[(1, 2), (2, 1)], // 2 Adults, 1 Child
            Some(1),
            Some(&["AF".into(), "DL".into()]),
        )
        .unwrap();

        let decoded = decode_info(&encoded).unwrap();
        assert_eq!(decoded.passengers.len(), 3);
        assert_eq!(decoded.passengers[0] as i32, 1); // Adult
        assert_eq!(decoded.passengers[1] as i32, 1); // Adult
        assert_eq!(decoded.passengers[2] as i32, 2); // Child
    }
}
