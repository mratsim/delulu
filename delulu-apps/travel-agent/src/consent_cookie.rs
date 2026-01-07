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

//! SOCS cookie generation for Google services.
//!
//! Generates valid SOCS cookies to bypass consent 302 redirects.
//! Used by Google Flights and Google Hotels scrapers.

use anyhow::{bail, Result};
use chrono::Datelike;
use prost::Message;

include!(concat!(env!("OUT_DIR"), "/google_flights.cookies.rs"));

// =============================================================================
// Cookie Generation
// =============================================================================

/// Generate fresh SOCS cookie with today's date-based GWS ID.
///
/// Produces base64-encoded protobuf for HTTP Cookie header.
/// Fresh cookies avoid the 302 redirect issue caused by stale static cookies.
fn generate_socs_cookie(locale: &str, gws_override: Option<&str>) -> Result<String> {
    let now = chrono::Utc::now();

    let gws = match gws_override {
        Some(v) => v.to_string(),
        None => format!(
            "gws_{:04}{:02}{:02}-0_RC2",
            now.date_naive().year(),
            now.date_naive().month(),
            now.date_naive().day()
        ),
    };

    if !gws.starts_with("gws_") || !gws.ends_with("-0_RC2") {
        bail!("Invalid GWS format: {} (expected: gws_YYYYMMDD-0_RC2)", gws);
    }

    // Build SOCS protobuf using prost
    //
    // NOTE: Timestamp truncated to u32, which overflows after Y2038.
    // This is fine for the foreseeable future as Google's SOCS cookies
    // are regenerated frequently anyway.
    let socs = Socs {
        info: Some(Information {
            gws: gws.clone(),
            locale: locale.to_string(),
        }),
        datetime: Some(Datetime {
            timestamp: now.timestamp() as u32,
        }),
    };

    // Serialize and base64 encode
    let mut buf = Vec::new();
    socs.encode(&mut buf)
        .map_err(|e| anyhow::anyhow!("SOCS encode failed: {}", e))?;

    use base64::{engine::general_purpose::STANDARD, Engine as _};
    Ok(STANDARD.encode(&buf))
}

/// Generate complete cookie header.
///
/// Returns: `CONSENT=PENDING+987; SOCS=<base64_value>`
pub(crate) fn generate_cookie_header(locale: &str, gws_override: Option<&str>) -> Result<String> {
    let socs = generate_socs_cookie(locale, gws_override)?;
    Ok(format!("CONSENT=PENDING+987; SOCS={}", socs))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Test-only parsing of SOCS cookie for verification
    fn parse_socs_for_test(b64_value: &str) -> Result<Option<Socs>> {
        use base64::{engine::general_purpose::STANDARD, Engine as _};
        match STANDARD.decode(b64_value) {
            Ok(bytes) => Socs::decode(bytes.as_slice())
                .map(Some)
                .map_err(|e| anyhow::anyhow!("Decode error: {:?}", e)),
            Err(_) => Ok(None),
        }
    }

    #[test]
    fn generate_and_parse_roundtrip() {
        let socs_cookie = generate_socs_cookie("en", None).unwrap();
        let parsed = parse_socs_for_test(&socs_cookie).unwrap().unwrap();

        // Access parsed info fields
        let info = parsed.info.as_ref().unwrap();
        assert_eq!(info.locale.as_str(), "en");
        assert!(info.gws.starts_with("gws_20"));
        let dt = parsed.datetime.as_ref().unwrap();
        assert!(dt.timestamp > 1700000000);
    }

    #[test]
    fn custom_gws_and_locale() {
        let socs_cookie = generate_socs_cookie("de", Some("gws_20241225-0_RC2")).unwrap();
        let parsed = parse_socs_for_test(&socs_cookie).unwrap().unwrap();

        let info = parsed.info.as_ref().unwrap();
        assert_eq!(info.locale.as_str(), "de");
        assert_eq!(info.gws.as_str(), "gws_20241225-0_RC2");
    }

    #[test]
    fn cookie_header_format() {
        let header = generate_cookie_header("en", None).unwrap();
        assert!(header.contains("CONSENT=PENDING+987;"));
        assert!(header.contains("SOCS="));
    }

    #[test]
    fn invalid_gws_rejected() {
        assert!(generate_socs_cookie("en", Some("bad-format")).is_err());
        assert!(generate_socs_cookie("en", Some("gws_20240101")).is_err());
    }

    #[test]
    fn garbage_input_returns_none() {
        assert!(parse_socs_for_test("!!!invalid!!!").unwrap().is_none());
    }
}
