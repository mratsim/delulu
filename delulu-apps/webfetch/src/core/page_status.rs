//! Page-status classification shared by every webfetch pipeline.
//!
//! This module defines the `PageStatus` enum (whose JSON serialization is added
//! as a sibling `page_status` key in the `webfetch_raw` MCP tool) and the
//! token-anchored detection functions that drive it. It is deliberately not tied
//! to any single pipeline: every pipeline calls `classify_page` with the raw
//! HTML body and the extracted markdown length.
//!
//! Detection reuses the existing `is_bot_detected` / `BOT_DETECTION_PATTERNS`
//! machinery in `core/detect.rs` as the single source of truth for "is this page
//! bot-blocked", rather than building a second, divergent detector.

use serde::Serialize;

use crate::core::detect::is_bot_detected;

/// Minimum visible-text byte length (of `visible_len`, measured pre-pipeline)
/// that counts as "meaningful content" for the Article gate. Measured against
/// pre-pipeline visible text (excludes script/style/etc.), not extracted
/// markdown bytes. The value stays 200.
pub const MEANINGFUL_CONTENT_THRESHOLD: usize = 200;

/// Minimum number of `<img>` tags (case-insensitive) for a thin page to be
/// classified as a gallery.
pub const GALLERY_IMG_THRESHOLD: usize = 8;

// ---------------------------------------------------------------------------
// PageStatus
// ---------------------------------------------------------------------------

/// Classification of a fetched page, used to explain *why* a page came back
/// empty, thin, or blocked so a calling LLM can decide how to proceed.
///
/// **Enum-level recoverability contract:** *all* variants are recoverable
/// statuses — they describe the page, not a fatal error. `Blocked` (and only
/// `Blocked`) additionally maps to `Err(BLOCKED_MSG)` in `fetch_and_extract`
/// for backward compatibility; `Partial`/`JSHeavy`/`Gallery`/`Empty` always
/// yield `Ok`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum PageStatus {
    /// Meaningful content was extracted (≥ `MEANINGFUL_CONTENT_THRESHOLD` bytes
    /// of markdown). Occurs on a normal readable article. **Checked FIRST and
    /// outranks every `Blocked`/`Partial`/`JSHeavy`/`Gallery` signal.**
    Article,
    /// Content is restricted behind a paywall/meter/gate. Occurs when a
    /// token-anchored gate marker is present **and** content is thin. Bare unit
    /// variant (no reason field).
    Partial,
    /// Thin content dominated by JavaScript. Fires via any of three trigger
    /// classes: (1) **script-dominance** — `script_len > visible_len` with
    /// `visible_len < MEANINGFUL_CONTENT_THRESHOLD` (measurement-based, e.g. a
    /// JS-enforcement interstitial whose escaped script dwarfs its visible
    /// text); (2) a **SPA-shell root marker** (`<div id="root">`-style) on a
    /// client-rendered app with little static text; (3) a **JS-enforcement
    /// interstitial marker** (token-anchored `enablejs` /
    /// `httpservice/retry/enablejs`). This advisory status (never a fatal
    /// error) aggregates blank/JS-failed/
    /// SPA/JS-gated pages and is retryable; a more precise
    /// `JSHeavy { kind: ... }` discriminator is escalated future debt.
    #[serde(rename = "js_heavy")]
    JSHeavy,
    /// Thin content dominated by many images. Occurs on a photo gallery.
    Gallery,
    /// The body matched an anti-bot **or consent-wall** signal **and content is
    /// not found**. Occurs on a Cloudflare/CAPTCHA/Anubis challenge page or a
    /// cookie-consent wall.
    Blocked {
        by: BlockedBy,
    },
    /// Nothing recognizable. Occurs on a blank/JS-failed/otherwise-empty page.
    /// Documented catch-all that intentionally aggregates genuinely-blank,
    /// JS-failed, short-but-real, auth-required, and geo-blocked pages. This is
    /// an advisory status, not a fatal error.
    Empty,
}

// ---------------------------------------------------------------------------
// BlockedBy
// ---------------------------------------------------------------------------

/// The cause of a `PageStatus::Blocked` classification.
///
/// Exactly four variants this release. `CloudflareTurnstile` doubles as the
/// "unknown/unclassified" default catch-all: a future
/// non-Cloudflare `BOT_DETECTION_PATTERNS` entry would be mislabeled here — a
/// documented, latent time-bomb, not a live bug (all 6 current patterns are
/// Cloudflare-family). Do not branch a Cloudflare-specific retry on it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BlockedBy {
    /// Cloudflare-turnstile family (and the legacy "unknown/unclassified"
    /// default catch-all for unrecognized `BOT_DETECTION_PATTERNS` matches).
    CloudflareTurnstile,
    /// A CAPTCHA challenge (reCAPTCHA/hCaptcha).
    Captcha,
    /// An Anubis challenge page.
    Anubis,
    /// A cookie-consent **wall** (e.g. `consent.google.com`, EU consent
    /// interstitial). Approximation note: the marker set is also present in
    /// ordinary footer banners; the variant fires only when content is missing.
    CookieConsent,
}

// ---------------------------------------------------------------------------
// Detection
// ---------------------------------------------------------------------------

/// Detect an anti-bot blocking cause from a raw HTML body.
///
/// Returns one of the **three anti-bot** causes (`CloudflareTurnstile` /
/// `Captcha` / `Anubis`) and **never** `CookieConsent` (consent is the separate
/// concern of [`detect_cookie_consent`]). Reuses `BOT_DETECTION_PATTERNS` /
/// `is_bot_detected` as the single source of truth and is a **superset** of it:
/// for every input where `is_bot_detected(html)` is true, this returns
/// `Some(...)`. Matching is case-insensitive and first-match-in-document wins.
///
/// Vendor classification order:
/// 1. `Captcha` ← token-anchored `g-recaptcha`, `h-captcha`, `recaptcha`.
/// 2. `CloudflareTurnstile` ← `cf-turnstile`, `cf-browser-verification`,
///    `cf-challenge`, `__cf_chl_opt`, `challenge-platform`, `data-sitekey`,
///    bare `turnstile`, and `just a moment...` (the last group covers the
///    legacy `BOT_DETECTION_PATTERNS` entries and must remain a superset).
/// 3. `Anubis` ← token-anchored only: `id="anubis"`, `class="anubis"`,
///    `data-anubis`, `anubis.js`. Bare `anubis` in prose does NOT match.
/// 4. **Superset safety net:** if none of the above matched but
///    `is_bot_detected(html)` is true, return `Some(CloudflareTurnstile)`.
pub fn detect_anti_bot(html: &str) -> Option<BlockedBy> {
    let lower = html.to_lowercase();

    // 1. Captcha (checked first; first-match-in-document wins).
    if lower.contains("g-recaptcha") || lower.contains("h-captcha") || lower.contains("recaptcha") {
        return Some(BlockedBy::Captcha);
    }

    // 2. Cloudflare-turnstile family (including legacy superset markers).
    if lower.contains("cf-turnstile")
        || lower.contains("cf-browser-verification")
        || lower.contains("cf-challenge")
        || lower.contains("__cf_chl_opt")
        || lower.contains("challenge-platform")
        || lower.contains("data-sitekey")
        || lower.contains("turnstile")
        || lower.contains("just a moment...")
    {
        return Some(BlockedBy::CloudflareTurnstile);
    }

    // 3. Anubis (token-anchored only).
    if lower.contains(r#"id="anubis""#)
        || lower.contains(r#"class="anubis""#)
        || lower.contains("data-anubis")
        || lower.contains("anubis.js")
    {
        return Some(BlockedBy::Anubis);
    }

    // 4. Superset safety net: any legacy pattern that wasn't matched above.
    if is_bot_detected(html) {
        return Some(BlockedBy::CloudflareTurnstile);
    }

    None
}

/// Return true when a single-token CMP vendor name appears inside an HTML
/// attribute value (quoted) or as part of an identifier token (e.g.
/// `data-<token>` or `<token>-`), never as a bare word in prose.
fn attr_token_present(lower: &str, token: &str) -> bool {
    let double_quoted = format!("\"{token}\"");
    if lower.contains(&double_quoted) {
        return true;
    }
    let single_quoted = format!("'{token}'");
    if lower.contains(&single_quoted) {
        return true;
    }
    let data_attr = format!("data-{token}");
    if lower.contains(&data_attr) {
        return true;
    }
    let hyphenated = format!("{token}-");
    if lower.contains(&hyphenated) {
        return true;
    }
    false
}

/// Detect a cookie-consent **wall** marker from a raw HTML body.
///
/// Case-insensitive, token-anchored matching (NOT bare-word substrings).
/// Returns `true` when a CMP marker is present. **Honest framing:** this is NOT
/// a wall-vs-banner classifier — the marker set is an *approximation*
/// ("CMP-present"); the `Article`-first gate in [`classify_page`] is the real
/// guard, and the residual false-positive fires on thin pages.
///
/// Single-token vendor markers (`onetrust`, `didomi`, `cookiebot`,
/// `consentmanager`) match only when they appear as a CMP SDK token inside an
/// HTML attribute value (quoted) or identifier token — never as a bare word in
/// prose ("we use onetrust" → `false`).
pub fn detect_cookie_consent(html: &str) -> bool {
    let lower = html.to_lowercase();

    // Multi-token / exact anchored markers.
    if lower.contains("consent.google.com") || lower.contains("consent.google") {
        return true;
    }
    if lower.contains("__tcfapi") {
        return true;
    }
    if lower.contains("onetrust-consent-sdk") {
        return true;
    }
    if lower.contains(r#"id="cmp""#)
        || lower.contains(r#"class="cmp""#)
        || lower.contains("data-cmp")
    {
        return true;
    }

    // Single-token vendor markers — only in attribute-value context.
    if attr_token_present(&lower, "onetrust")
        || attr_token_present(&lower, "didomi")
        || attr_token_present(&lower, "cookiebot")
        || attr_token_present(&lower, "consentmanager")
    {
        return true;
    }

    false
}

/// Detect a paywall/meter/gate marker from a raw HTML body.
///
/// Case-insensitive, token-anchored matching (NOT bare-word substrings). Bare
/// `paywall`/`metered`/`premium`/`subscription` words in prose do NOT match.
pub fn detect_paywall(html: &str) -> bool {
    let lower = html.to_lowercase();
    const MARKERS: &[&str] = &[
        r#"class="paywall""#,
        r#"id="paywall""#,
        "data-paywall",
        "paywall-",
        "metered-content",
        r#"id="metered-content""#,
        r#"class="metered-content""#,
        "data-metered",
        "subscription-gate",
        "premium-gate",
        "data-premium",
        r#"id="subscription""#,
        r#"class="subscription-gate""#,
    ];
    MARKERS.iter().any(|m| lower.contains(m))
}

/// True when any SPA-shell root marker is present, case-insensitively.
fn has_spa_shell_marker(html: &str) -> bool {
    let lower = html.to_lowercase();
    const MARKERS: &[&str] = &[
        r#"<div id="root">"#,
        r#"<div id="app">"#,
        r#"<div id="__next">"#,
        r#"<div id="__nuxt">"#,
        r#"<div id="app-root">"#,
    ];
    MARKERS.iter().any(|m| lower.contains(m))
}

/// True when `token` appears in `lower` as a standalone token — bounded by
/// non-alphanumeric characters or string edges. Prevents bare prose words and
/// tokens embedded in a larger identifier (e.g. `enablejsx`) from matching.
fn token_anchored(lower: &str, token: &str) -> bool {
    let bytes = lower.as_bytes();
    let t = token.as_bytes();
    let n = bytes.len();
    if t.is_empty() || t.len() > n {
        return false;
    }
    let mut i = 0;
    while i + t.len() <= n {
        if &bytes[i..i + t.len()] == t {
            let prev_ok = i == 0 || !bytes[i - 1].is_ascii_alphanumeric();
            let next = i + t.len();
            let next_ok = next == n || !bytes[next].is_ascii_alphanumeric();
            if prev_ok && next_ok {
                return true;
            }
        }
        i += 1;
    }
    false
}

/// True when a JS-enforcement interstitial marker is present: the plain
/// `httpservice/retry/enablejs` path, or token-anchored `enablejs` /
/// `enable-js`. Bare prose `javascript`/`enable`/`js` never matches.
fn has_js_enforcement_marker(html: &str) -> bool {
    let lower = html.to_lowercase();
    lower.contains("httpservice/retry/enablejs")
        || token_anchored(&lower, "enablejs")
        || token_anchored(&lower, "enable-js")
}

/// True when a page is JS-heavy: script-dominance (measurement-based, primary)
/// OR an SPA-shell root marker OR a JS-enforcement interstitial marker.
fn is_js_heavy(html: &str, visible_len: usize, script_len: usize) -> bool {
    // 1. Script dominance (primary, measurement-based).
    if script_len > visible_len && visible_len < MEANINGFUL_CONTENT_THRESHOLD {
        return true;
    }
    // 2. SPA-shell root markers (additional signal).
    if has_spa_shell_marker(html) {
        return true;
    }
    // 3. JS-enforcement interstitial markers (additional signal).
    has_js_enforcement_marker(html)
}

/// Classify a page from its raw HTML body and visible/script text lengths.
///
/// `visible_len` is the **pre-pipeline visible-text byte length** (excludes
/// `<script>`/`<style>`/etc.). `script_len` is the **pre-pipeline text byte
/// length inside `<script>` elements**. Deterministic priority, first hit
/// wins, with `Article` FIRST:
///
/// 1. `visible_len >= MEANINGFUL_CONTENT_THRESHOLD` → `Article` (content found
///    outranks ALL `Blocked`/`Partial`/`JSHeavy`/`Gallery` signals).
/// 2. else (content missing), in order:
///    a. `detect_anti_bot(html)` → `Blocked { by }`;
///    b. `detect_cookie_consent(html)` → `Blocked { by: CookieConsent }`;
///    c. `detect_paywall(html)` → `Partial`;
///    d. JS-heavy (script-dominance / SPA shell / JS-enforcement interstitial)
///       → `JSHeavy`;
///    e. `>= GALLERY_IMG_THRESHOLD` `<img>` tags → `Gallery`;
///    f. else → `Empty`.
pub fn classify_page(html: &str, visible_len: usize, script_len: usize) -> PageStatus {
    if visible_len >= MEANINGFUL_CONTENT_THRESHOLD {
        return PageStatus::Article;
    }

    if let Some(by) = detect_anti_bot(html) {
        return PageStatus::Blocked { by };
    }
    if detect_cookie_consent(html) {
        return PageStatus::Blocked {
            by: BlockedBy::CookieConsent,
        };
    }
    if detect_paywall(html) {
        return PageStatus::Partial;
    }
    if is_js_heavy(html, visible_len, script_len) {
        return PageStatus::JSHeavy;
    }
    if html.to_lowercase().matches("<img").count() >= GALLERY_IMG_THRESHOLD {
        return PageStatus::Gallery;
    }
    PageStatus::Empty
}

#[cfg(test)]
#[path = "../../tests/unit/core/page_status_test.rs"]
mod tests;
