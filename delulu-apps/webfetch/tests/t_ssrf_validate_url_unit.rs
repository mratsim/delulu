//! SSRF `validate_url` unit tests.
//!
//! Copyright (C) 2026  Mamy Ratsimbazafy
//!
//! This program is free software: you can redistribute it and/or modify
//! it under the terms of the GNU Affero General Public License as published by
//! the Free Software Foundation, either version 3 of the License, or
//! (at your option) any later version.
//!
//! This program is distributed in the hope that it will be useful,
//! but WITHOUT ANY WARRANTY; without even the implied warranty of
//! MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
//! GNU Affero General Public License for more details.
//!
//! You should have received a copy of the GNU Affero General Public License
//! along with this program.  If not, see <http://www.gnu.org/licenses/>.
//!
//! # Test Pattern
//!
//! Calls `delulu_webfetch::lib_mcp::validate_url` directly with synthetic
//! socket addresses (no network, no DNS — all cases use IP-literal or
//! malformed URLs). Mirrors the e2e contract (INV-007): the detailed
//! private-IP message for stdio/same-subnet requestors, the generic message
//! for external requestors, and the bypass when `expose_local_networks` is set.

use delulu_webfetch::lib_mcp::validate_url;
use std::net::SocketAddr;

/// Exact private-IP rejection string (INV-007) — matches the e2e pin.
const DETAILED: &str = "URL resolves to a private IP address which is blocked by default. Use --expose-local-networks to allow fetching from local/private networks.";

/// Exact generic rejection string (INV-007) — matches the e2e pin.
const GENERIC: &str = "DNS resolution failed";

fn addr(ip: &str, port: u16) -> SocketAddr {
    format!("{ip}:{port}").parse().unwrap()
}

/// stdio (peer None) requestor: loopback is rejected with the detailed message.
#[tokio::test]
async fn peer_none_loopback_is_detailed() {
    let err = validate_url("http://127.0.0.1/", false, None, None)
        .await
        .unwrap_err();
    assert_eq!(err, DETAILED);
}

/// HTTP requestor on the same /16 subnet as the server: detailed message.
#[tokio::test]
async fn same_subnet_peer_gets_detailed() {
    let peer = addr("127.0.0.2", 40000);
    let local = addr("127.0.0.1", 8080);
    let err = validate_url("http://127.0.0.1/", false, Some(peer), Some(local))
        .await
        .unwrap_err();
    assert_eq!(err, DETAILED);
}

/// HTTP requestor from a different subnet: generic message (no oracle leak).
#[tokio::test]
async fn different_subnet_peer_gets_generic() {
    let peer = addr("198.51.100.1", 40000);
    let local = addr("127.0.0.1", 8080);
    let err = validate_url("http://10.0.0.5/", false, Some(peer), Some(local))
        .await
        .unwrap_err();
    assert_eq!(err, GENERIC);
}

/// expose_local_networks=true bypasses validation entirely.
#[tokio::test]
async fn expose_local_networks_bypasses() {
    let result = validate_url("http://127.0.0.1/", true, None, None).await;
    assert!(result.is_ok());
}

/// Cloud metadata endpoint is blocked with the detailed message (stdio).
#[tokio::test]
async fn cloud_metadata_is_detailed() {
    let err = validate_url("http://169.254.169.254/", false, None, None)
        .await
        .unwrap_err();
    assert_eq!(err, DETAILED);
}

/// Malformed URL: generic message (parse failure is indistinguishable from
/// DNS failure to avoid leaking details).
#[tokio::test]
async fn malformed_url_is_generic() {
    let err = validate_url("not a url", false, None, None)
        .await
        .unwrap_err();
    assert_eq!(err, GENERIC);
}
