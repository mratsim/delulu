//! Redirect-policy tests (SEC-B-001). Unit tests exercise `validate_redirect`
//! and `is_private_ip` directly; integration tests use a live HTTP server that
//! responds with `302 Location: <target>` and verify the crawler's validating
//! redirect policy blocks private/reserved targets, credentials and scheme
//! downgrades — and still follows legitimate domain redirects.
//!
//! The "blocked" integration tests use the builder-default crawler (which now
//! installs the validating policy) — the policy rejects the hop before any
//! follow-up request is sent, so wreq's Safari emulation (absolute-form URIs
//! that confuse wiremock) is irrelevant there. The "followed" tests use a raw
//! `wreq::Client` configured with the same validating policy to keep the
//! follow-up requests in origin-form for wiremock.

use super::*;
use crate::{CrawlerError, RateLimitedCrawler};
use std::time::Duration;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// Builder-default crawler: exercises the validating policy installed by
/// `CrawlerBuilder::init_builder` (option a — policy is the default).
fn default_crawler() -> RateLimitedCrawler {
    RateLimitedCrawler::builder()
        .with_qps(10_000)
        .build()
        .expect("default crawler should build")
}

/// Crawler backed by a raw wreq client with the same validating policy but no
/// Safari emulation, so follow-up (redirected) requests stay in origin-form
/// and wiremock can match them.
fn raw_client_crawler() -> RateLimitedCrawler {
    let raw_client = wreq::Client::builder()
        .redirect(validating_redirect_policy())
        .timeout(Duration::from_secs(30))
        .build()
        .expect("raw wreq client with validating redirect policy should build");
    RateLimitedCrawler::builder()
        .with_client(raw_client)
        .with_qps(10_000)
        .build()
        .expect("crawler with raw client should build")
}

/// Mount a redirect endpoint: `/start` responds `302 Location: <target>`.
async fn mount_redirect(mock: &MockServer, target: &str) {
    Mock::given(method("GET"))
        .and(path("/start"))
        .respond_with(ResponseTemplate::new(302).insert_header("Location", target))
        .mount(mock)
        .await;
}

// ---------------------------------------------------------------------------
// Private / reserved redirect targets are rejected
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_redirect_to_loopback_is_rejected() {
    let mock_server = MockServer::start().await;
    // The policy must reject BEFORE connecting — target port 1 is not
    // listening, and the request must never reach it.
    mount_redirect(&mock_server, "http://127.0.0.1:1/private").await;

    let crawler = default_crawler();
    let url = format!("{}/start", mock_server.uri());

    let err = crawler.get(&url).send().await.expect_err("must fail");
    assert!(
        matches!(err, CrawlerError::Http(_)),
        "blocked redirect must surface as an HTTP error, got {err:?}"
    );
    assert!(
        err.to_string().contains("private/reserved"),
        "error should describe the block, got: {err}"
    );
}

#[tokio::test]
async fn test_redirect_to_cloud_metadata_is_rejected() {
    let mock_server = MockServer::start().await;
    mount_redirect(&mock_server, "http://169.254.169.254/latest/meta-data/").await;

    let crawler = default_crawler();
    let url = format!("{}/start", mock_server.uri());

    let err = crawler.get(&url).send().await.expect_err("must fail");
    assert!(
        err.to_string().contains("private/reserved"),
        "cloud-metadata redirect must be blocked, got: {err}"
    );
}

#[tokio::test]
async fn test_redirect_with_userinfo_credentials_is_rejected() {
    let mock_server = MockServer::start().await;
    mount_redirect(&mock_server, "http://user:pass@example.com/").await;

    let crawler = default_crawler();
    let url = format!("{}/start", mock_server.uri());

    let err = crawler.get(&url).send().await.expect_err("must fail");
    assert!(
        err.to_string().contains("credentials"),
        "credential-bearing redirect must be blocked, got: {err}"
    );
}

// ---------------------------------------------------------------------------
// Legitimate redirects still work
// ---------------------------------------------------------------------------

/// Domain redirect targets cannot be resolved synchronously inside the policy,
/// so they are followed. This pins the documented SEC-B-002 residual risk
/// (a domain that resolves to a private IP is out of scope for this fix).
#[tokio::test]
async fn test_redirect_to_domain_is_followed() {
    let mock_server = MockServer::start().await;
    let port = mock_server.address().port();
    mount_redirect(&mock_server, &format!("http://localhost:{port}/final")).await;
    Mock::given(method("GET"))
        .and(path("/final"))
        .respond_with(ResponseTemplate::new(200).set_body_string("final-ok"))
        .mount(&mock_server)
        .await;

    let crawler = raw_client_crawler();
    let url = format!("{}/start", mock_server.uri());

    let resp = crawler
        .get(&url)
        .send()
        .await
        .expect("redirect should be followed");
    assert_eq!(resp.status().as_u16(), 200);
    let body = resp.text().await.expect("body should be readable");
    assert_eq!(
        body, "final-ok",
        "redirect chain must land on the final page"
    );
}

/// The hop budget (5, matching the old `Policy::limited(5)`) is enforced even
/// when every hop is an allowed domain redirect: the 6th redirect attempt is
/// rejected. Chain uses the `localhost` domain so hops are not blocked by the
/// private-IP check.
#[tokio::test]
async fn test_redirect_chain_hops_limited() {
    let mock_server = MockServer::start().await;
    let port = mock_server.address().port();

    // /r1 -> /r2 -> ... -> /r6, each responding 302 to the next. The 6th
    // redirect attempt (previous.len() == 6) must be rejected before /r7 is
    // requested.
    for i in 1..=6 {
        let target = format!("http://localhost:{port}/r{}", i + 1);
        Mock::given(method("GET"))
            .and(path(format!("/r{i}")))
            .respond_with(ResponseTemplate::new(302).insert_header("Location", target))
            .mount(&mock_server)
            .await;
    }

    let crawler = raw_client_crawler();
    let url = format!("{}/r1", mock_server.uri());

    let err = crawler.get(&url).send().await.expect_err("must fail");
    assert!(
        err.to_string().contains("too many redirects"),
        "6th redirect hop must be rejected by the hop budget, got: {err}"
    );
}

// ---------------------------------------------------------------------------
// Unit tests: validate_redirect / is_private_ip (no network)
// ---------------------------------------------------------------------------

use std::net::IpAddr;
use std::str::FromStr;

fn uri(s: &str) -> http::Uri {
    http::Uri::from_str(s).unwrap_or_else(|_| {
        // http::Uri's parser only accepts http(s) schemes, so strings like
        // "ftp://..." or "not a url" cannot be represented as a parsed
        // Uri. Such inputs never reach `validate_redirect` through wreq in
        // production either; the closest representable stand-ins below
        // still exercise the url::Url defense-in-depth gates inside
        // `validate_redirect`.
        if let Some((scheme, _rest)) = s.split_once("://") {
            http::Uri::builder()
                .scheme(scheme)
                .authority("stand-in.invalid")
                .path_and_query("/")
                .build()
                .expect("builder-constructed test URI must parse")
        } else {
            http::Uri::from_static("/stand-in")
        }
    })
}

fn prev(scheme: &str) -> Vec<http::Uri> {
    vec![uri(&format!("{scheme}://public.example/"))]
}

fn assert_blocked(target: &str, prev_scheme: &str) {
    let result = validate_redirect(&prev(prev_scheme), &uri(target));
    assert!(
        result.is_err(),
        "expected redirect to {target} to be rejected, got Ok"
    );
}

fn assert_allowed(target: &str, prev_scheme: &str) {
    let result = validate_redirect(&prev(prev_scheme), &uri(target));
    assert!(
        result.is_ok(),
        "expected redirect to {target} to be allowed, got Err({})",
        result.unwrap_err()
    );
}

// -----------------------------------------------------------------------
// Private/reserved IP literals
// -----------------------------------------------------------------------

#[test]
fn blocks_loopback_v4() {
    assert_blocked("http://127.0.0.1/", "http");
    assert_blocked("http://127.8.8.8:8080/x", "http");
}

#[test]
fn blocks_cloud_metadata() {
    assert_blocked("http://169.254.169.254/latest/meta-data/", "http");
}

#[test]
fn blocks_rfc1918_ranges() {
    assert_blocked("http://10.0.0.1/", "http");
    assert_blocked("http://10.255.255.255/", "http");
    assert_blocked("http://172.16.0.1/", "http");
    assert_blocked("http://172.31.255.254/", "http");
    assert_blocked("http://192.168.0.1/", "http");
    assert_blocked("http://192.168.255.254/", "http");
}

#[test]
fn blocks_link_local_v4() {
    assert_blocked("http://169.254.0.1/", "http");
}

#[test]
fn blocks_private_v6() {
    assert_blocked("http://[::1]/", "http"); // loopback
    assert_blocked("http://[::]/", "http"); // unspecified
    assert_blocked("http://[fc00::1]/", "http"); // ULA fc00::/7
    assert_blocked("http://[fd12:3456::1]/", "http"); // ULA fd00::/8
    assert_blocked("http://[fe80::1]/", "http"); // link-local
    assert_blocked("http://[2001:db8::1]/", "http"); // documentation
}

#[test]
fn blocks_ipv4_mapped_private() {
    assert_blocked("http://[::ffff:127.0.0.1]/", "http");
    assert_blocked("http://[::ffff:10.0.0.1]/", "http");
    assert_blocked("http://[::ffff:169.254.169.254]/", "http");
    assert_blocked("http://[::ffff:192.168.1.1]/", "http");
}

#[test]
fn allows_public_ip_literals() {
    assert_allowed("http://8.8.8.8/", "http");
    assert_allowed("http://93.184.216.34/", "http");
    assert_allowed("http://[2606:2800:220:1::1]/", "http");
}

#[test]
fn allows_domain_targets() {
    // Domains cannot be resolved synchronously — allowed here; the
    // domain-resolves-to-private case is the SEC-B-002 residual risk.
    assert_allowed("http://example.com/", "http");
    assert_allowed("https://example.com/path?q=1", "https");
}

// -----------------------------------------------------------------------
// Credentials in the redirect target
// -----------------------------------------------------------------------

#[test]
fn blocks_userinfo_credentials() {
    assert_blocked("http://user:pass@example.com/", "http");
    assert_blocked("http://user@example.com/", "http");
    assert_blocked("http://user:pass@8.8.8.8/", "http");
}

// -----------------------------------------------------------------------
// Scheme downgrade https -> http
// -----------------------------------------------------------------------

#[test]
fn blocks_https_to_http_downgrade() {
    assert_blocked("http://example.com/", "https");
    assert_blocked("http://8.8.8.8/", "https");
}

#[test]
fn allows_http_to_https_upgrade() {
    assert_allowed("https://example.com/", "http");
    assert_allowed("https://8.8.8.8/", "http");
}

#[test]
fn allows_same_scheme_redirects() {
    assert_allowed("http://example.com/", "http");
    assert_allowed("https://example.com/", "https");
}

// -----------------------------------------------------------------------
// Malformed / non-http targets
// -----------------------------------------------------------------------

#[test]
fn blocks_non_http_schemes() {
    assert_blocked("ftp://example.com/", "http");
    assert_blocked("file:///etc/passwd", "http");
    assert_blocked("javascript:alert(1)", "http");
}

#[test]
fn blocks_unparsable_or_hostless_targets() {
    // Not parseable as an absolute URL.
    assert_blocked("not a url", "http");
    // Relative-ish target with no host.
    assert_blocked("/just/a/path", "http");
}

// -----------------------------------------------------------------------
// Hop budget
// -----------------------------------------------------------------------

#[test]
fn hop_budget_allows_up_to_max_and_rejects_beyond() {
    let target = uri("http://example.com/");
    // previous.len() == 5 -> still allowed (matches limited(5): 5 hops)
    let prev5: Vec<http::Uri> = (0..5)
        .map(|i| uri(&format!("http://h{i}.example/")))
        .collect();
    assert!(validate_redirect(&prev5, &target).is_ok());
    // previous.len() == 6 -> rejected
    let prev6: Vec<http::Uri> = (0..6)
        .map(|i| uri(&format!("http://h{i}.example/")))
        .collect();
    let err = validate_redirect(&prev6, &target).unwrap_err();
    assert_eq!(err, "too many redirects");
}

// -----------------------------------------------------------------------
// is_private_ip parity with delulu-webfetch
// -----------------------------------------------------------------------

#[test]
fn is_private_ip_parity() {
    let private: Vec<&str> = vec![
        "127.0.0.1",
        "10.0.0.1",
        "172.16.0.1",
        "172.31.255.254",
        "192.168.1.1",
        "169.254.169.254",
        "::1",
        "::",
        "fc00::1",
        "fd12:3456::1",
        "fe80::1",
        "2001:db8::1",
        "::ffff:127.0.0.1",
        "::ffff:10.1.2.3",
        "::ffff:169.254.169.254",
    ];
    for s in private {
        let ip: IpAddr = s.parse().unwrap();
        assert!(
            is_private_ip(&ip),
            "{s} must be treated as private (parity with delulu-webfetch)"
        );
    }
    let public: Vec<&str> = vec![
        "8.8.8.8",
        "93.184.216.34",
        "1.1.1.1",
        "2606:2800:220:1::1",
        "2001:4860:4860::8888",
    ];
    for s in public {
        let ip: IpAddr = s.parse().unwrap();
        assert!(
            !is_private_ip(&ip),
            "{s} must be treated as public (parity with delulu-webfetch)"
        );
    }
}

#[test]
fn redirect_blocked_error_display_and_debug() {
    let e = RedirectBlocked("too many redirects");
    assert_eq!(e.to_string(), "redirect blocked: too many redirects");
    assert!(std::error::Error::source(&e).is_none());
}
