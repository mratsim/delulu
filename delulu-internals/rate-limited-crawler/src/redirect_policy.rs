//! SSRF-safe redirect policy (SEC-B-001).
//!
//! The application layer (`delulu-webfetch`) validates the *initial* URL
//! against private/reserved IP ranges before fetching, but redirects are
//! followed by the HTTP client and the redirect target is never re-validated.
//! An attacker-controlled public endpoint could answer `302 Location:
//! http://169.254.169.254/latest/meta-data/` and the crawler would happily
//! return the cloud-metadata body. This module installs a redirect policy
//! that re-validates **every** redirect hop synchronously.
//!
//! The policy rejects a redirect when:
//! - the hop budget is exhausted (mirrors the previous `Policy::limited(5)`),
//! - the target is a private/reserved IP literal (loopback, RFC1918,
//!   link-local incl. cloud metadata, IPv6 equivalents, IPv4-mapped forms),
//! - the target carries embedded userinfo credentials (`http://user:pass@host/`),
//! - the scheme downgrades from `https` to `http`.
//!
//! The policy closure is synchronous: no DNS resolution happens inside it.
//! Redirect targets that are *domain names* are therefore allowed through
//! (only the checks above apply); a domain that resolves to a private IP is a
//! DNS-rebinding style residual risk, tracked separately as SEC-B-002 (out of
//! scope for this fix).
//!
//! NOTE: this policy is installed as the crawler-wide default, so it also
//! applies when `delulu-webfetch` runs with `--expose-local-networks` — that
//! flag only relaxes the *initial* URL check in `validate_url`, not redirect
//! hops. A private redirect target is blocked even in that mode; a deliberate
//! conservative trade-off (blocking is always safe).

use std::fmt;
use std::net::IpAddr;

/// Maximum number of redirect hops to follow per request.
///
/// Kept at 5 to match the previous `Policy::limited(5)` default.
pub const MAX_REDIRECT_HOPS: usize = 5;

/// Build the default SSRF-validating redirect policy for the crawler.
///
/// The closure receives each redirect attempt and decides follow/stop/error
/// synchronously. Errors surface as `CrawlerError::Http(wreq::Error)` on the
/// request — the MCP layer must render those to clients generically (see
/// `delulu-webfetch` lib_mcp error arms).
pub fn validating_redirect_policy() -> wreq::redirect::Policy {
    wreq::redirect::Policy::custom(|attempt: wreq::redirect::Attempt| {
        match validate_redirect(&attempt.previous, &attempt.uri) {
            Ok(()) => attempt.follow(),
            Err(reason) => attempt.error(RedirectBlocked(reason)),
        }
    })
}

/// Evaluate one redirect hop synchronously.
///
/// `previous` is the list of URIs already requested in this chain (the first
/// entry is the initial request URI; the last is the URI the redirect came
/// from). `target` is the already-resolved, absolute redirect target (wreq
/// resolves relative `Location` headers against the current URI before
/// invoking the policy).
///
/// Returns `Ok(())` when the hop may be followed, or a static reason string
/// when it must be rejected. No I/O, no DNS — safe to call from the
/// synchronous policy closure.
pub fn validate_redirect(previous: &[http::Uri], target: &http::Uri) -> Result<(), &'static str> {
    // Hop budget. `previous` includes the initial request URI, so `len()` is
    // the number of redirects followed so far plus one. `Policy::limited(5)`
    // rejected when `previous.len() > 5`; keep identical semantics.
    if previous.len() > MAX_REDIRECT_HOPS {
        return Err("too many redirects");
    }

    let target_str = target.to_string();
    let parsed =
        url::Url::parse(&target_str).map_err(|_| "redirect target is not a parseable URL")?;
    let scheme = parsed.scheme();
    if scheme != "http" && scheme != "https" {
        return Err("redirect target uses a non-http(s) scheme");
    }

    // Credential-bearing redirect: `http://user:pass@host/` — never follow.
    if !parsed.username().is_empty() || parsed.password().is_some() {
        return Err("redirect target carries embedded credentials");
    }

    // Scheme downgrade https -> http relative to the URI we are leaving.
    let leaving_https = previous
        .last()
        .and_then(|u| u.scheme_str())
        .is_some_and(|s| s.eq_ignore_ascii_case("https"));
    if leaving_https && scheme.eq_ignore_ascii_case("http") {
        return Err("redirect downgrades https to http");
    }

    // Private/reserved IP literals. Domain names cannot be resolved
    // synchronously and are allowed here (SEC-B-002 residual risk).
    match parsed.host() {
        Some(url::Host::Ipv4(ip)) if is_private_ip(&IpAddr::V4(ip)) => {
            Err("redirect target resolves to a private/reserved IP address")
        }
        Some(url::Host::Ipv6(ip)) if is_private_ip(&IpAddr::V6(ip)) => {
            Err("redirect target resolves to a private/reserved IP address")
        }
        Some(url::Host::Domain(_)) | Some(url::Host::Ipv4(_)) | Some(url::Host::Ipv6(_)) => Ok(()),
        None => Err("redirect target has no host"),
    }
}

/// Error surfaced by the redirect policy when a hop is rejected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RedirectBlocked(pub &'static str);

impl fmt::Display for RedirectBlocked {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "redirect blocked: {}", self.0)
    }
}

impl std::error::Error for RedirectBlocked {}

/// Check if an IP address is in a private/internal/reserved range.
///
/// NOTE: intentionally duplicated from `delulu-webfetch/src/lib.rs`
/// (`is_private_ip`, INV-007/SSRF protection) because the crawler crate
/// cannot depend on the app crate (SEC-B-001). Keep both implementations in
/// sync.
pub fn is_private_ip(ip: &IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            v4.is_loopback() // 127.0.0.0/8
                || v4.is_private() // 10.0.0.0/8, 172.16.0.0/12, 192.168.0.0/16
                || v4.is_link_local() // 169.254.0.0/16 (incl. cloud metadata 169.254.169.254)
        }
        IpAddr::V6(v6) => {
            if is_ipv4_mapped(v6) {
                // Extract embedded IPv4 and check if IT is private
                let ipv4 = std::net::Ipv4Addr::from(
                    (u128::from(*v6) & 0x0000_0000_0000_0000_0000_0000_FFFF_FFFF) as u32,
                );
                return ipv4.is_loopback() || ipv4.is_private() || ipv4.is_link_local();
            }
            v6.is_loopback()
                || v6.is_unspecified()
                || is_ula(v6)
                || is_link_local(v6)
                || is_ipv4_compatible(v6)
                || is_documentation(v6)
        }
    }
}

/// Unique Local Address (fc00::/7).
fn is_ula(v6: &std::net::Ipv6Addr) -> bool {
    v6.octets()[0] & 0xfe == 0xfc
}

/// Link-local address (fe80::/10).
fn is_link_local(v6: &std::net::Ipv6Addr) -> bool {
    v6.octets()[0] == 0xfe && v6.octets()[1] & 0xc0 == 0x80
}

/// IPv4-mapped address (::ffff:0:0/96).
fn is_ipv4_mapped(v6: &std::net::Ipv6Addr) -> bool {
    (u128::from(*v6) >> 32) == 0xFFFF
}

/// IPv4-compatible address (::/96, deprecated but still reserved).
fn is_ipv4_compatible(v6: &std::net::Ipv6Addr) -> bool {
    (u128::from(*v6) >> 32) == 0
}

/// Documentation address (2001:db8::/32).
fn is_documentation(v6: &std::net::Ipv6Addr) -> bool {
    (u128::from(*v6) >> 96) == 0x2001_0DB8
}

#[cfg(test)]
#[path = "../tests/unit/redirect_policy_test.rs"]
mod redirect_policy_test;
