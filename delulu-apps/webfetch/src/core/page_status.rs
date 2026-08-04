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

use regex::Regex;
use std::sync::LazyLock;

use crate::core::detect::is_bot_detected;

/// Minimum visible-text byte length (of `visible_len`, measured pre-pipeline)
/// that counts as "meaningful content" for the Article gate. Measured against
/// pre-pipeline visible text (excludes script/style/etc.), not extracted
/// markdown bytes. The value stays 200.
///
/// **Byte-based approximation (deliberate, not a bug):** the gate is applied to
/// the **byte length** of the pre-pipeline visible text, never its character
/// count. For CJK/multibyte content this overestimates the character count
/// (e.g. 200 bytes ≈ 66 CJK chars, since each CJK char is 3 UTF-8 bytes), so a
/// short CJK page can be classified `Article` while a Latin page of the same
/// character count would not. This tradeoff is intentional and documented — the
/// gate stays byte-based because pre-pipeline `visible_len` is measured in
/// bytes and byte comparison is cheap. It is pinned by the CJK unit test in
/// `page_status_test.rs` (`classify_cjk_short_text_counts_bytes_not_chars_is_article`).
pub const MEANINGFUL_CONTENT_THRESHOLD: usize = 200;

/// Minimum number of `<img>` tags (case-insensitive) for a thin page to be
/// classified as a gallery.
pub const GALLERY_IMG_THRESHOLD: usize = 8;

/// Matches a lowercase `<img` element opening: `<img` immediately followed by
/// whitespace, `>`, or `/` (self-closing). This excludes `<imgsrc=...>` (no
/// space), `<image>`, and `<img` embedded inside `<script>` strings or
/// attribute values — all of which the old raw `"<img"` substring count
/// mistakenly tallied as gallery images.
static IMG_OPEN_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"<img[\s/>]").expect("valid regex: img element opening"));

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
    Blocked { by: BlockedBy },
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
/// Five variants this release. `Unknown` is the catch-all for unrecognized
/// anti-bot patterns: a future non-Cloudflare `BOT_DETECTION_PATTERNS` entry
/// (generic CAPTCHA, Anubis, etc.) that is not matched by a specific vendor
/// check lands here instead of being mislabeled as a Cloudflare-specific cause.
/// Do not branch a Cloudflare-specific retry on `Unknown`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BlockedBy {
    /// Cloudflare-turnstile family (cf-turnstile, cf-browser-verification,
    /// data-sitekey, challenge-platform, "just a moment...", and bare `turnstile`
    /// anchored to Cloudflare context).
    CloudflareTurnstile,
    /// A CAPTCHA challenge (reCAPTCHA/hCaptcha).
    Captcha,
    /// An Anubis challenge page.
    Anubis,
    /// A cookie-consent **wall** (e.g. `consent.google.com`, EU consent
    /// interstitial). Approximation note: the marker set is also present in
    /// ordinary footer banners; the variant fires only when content is missing.
    CookieConsent,
    /// Catch-all for unrecognized anti-bot patterns: `is_bot_detected` matched
    /// but no specific vendor (Cloudflare/CAPTCHA/Anubis) check fired.
    Unknown,
}

// ---------------------------------------------------------------------------
// Detection
// ---------------------------------------------------------------------------

/// Detect an anti-bot blocking cause from a raw HTML body.
///
/// Returns one of the **four anti-bot** causes (`CloudflareTurnstile` /
/// `Captcha` / `Anubis` / `Unknown`) and **never** `CookieConsent` (consent is
/// the separate concern of [`detect_cookie_consent`]). Reuses
/// `BOT_DETECTION_PATTERNS` / `is_bot_detected` as the single source of truth
/// and is a **superset** of it: for every input where `is_bot_detected(html)`
/// is true, this returns `Some(...)`. Matching is case-insensitive and
/// first-match-in-document wins.
///
/// Vendor classification order:
/// 1. `Captcha` ← token-anchored `g-recaptcha`, `h-captcha`, `recaptcha`.
/// 2. `CloudflareTurnstile` ← `cf-turnstile`, `cf-browser-verification`,
///    `cf-challenge`, `__cf_chl_opt`, `challenge-platform`, `data-sitekey`,
///    and `just a moment...`. Bare `turnstile` counts only when anchored to
///    Cloudflare context (e.g. `cf-turnstile` / `data-sitekey`); a bare
///    `turnstile` in prose does NOT match here.
/// 3. `Anubis` ← token-anchored only: `id="anubis"`, `class="anubis"`,
///    `data-anubis`, `anubis.js`. Bare `anubis` in prose does NOT match.
/// 4. **Unknown catch-all:** if none of the above matched but
///    `is_bot_detected(html)` is true, return `Some(BlockedBy::Unknown)`.
///
/// Caller must pass the HTML already lowercased (see `classify_page`).
pub fn detect_anti_bot(lower_html: &str) -> Option<BlockedBy> {
    // 1. Captcha (checked first; first-match-in-document wins).
    if lower_html.contains("g-recaptcha")
        || lower_html.contains("h-captcha")
        || lower_html.contains("recaptcha")
    {
        return Some(BlockedBy::Captcha);
    }

    // 2. Cloudflare-turnstile family. Bare `turnstile` is deliberately NOT
    //    matched here (it must be anchored to a Cloudflare marker such as
    //    `cf-turnstile`); an unanchored `turnstile` falls through to the
    //    `is_bot_detected` catch-all below as `BlockedBy::Unknown`.
    if lower_html.contains("cf-turnstile")
        || lower_html.contains("cf-browser-verification")
        || lower_html.contains("cf-challenge")
        || lower_html.contains("__cf_chl_opt")
        || lower_html.contains("challenge-platform")
        || lower_html.contains("data-sitekey")
        || lower_html.contains("just a moment...")
    {
        return Some(BlockedBy::CloudflareTurnstile);
    }

    // 3. Anubis (token-anchored only).
    if lower_html.contains(r#"id="anubis""#)
        || lower_html.contains(r#"class="anubis""#)
        || lower_html.contains("data-anubis")
        || lower_html.contains("anubis.js")
    {
        return Some(BlockedBy::Anubis);
    }

    // 4. Unknown catch-all: any legacy pattern not matched by a vendor check.
    if is_bot_detected(lower_html) {
        return Some(BlockedBy::Unknown);
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
///
/// Caller must pass the HTML already lowercased (see `classify_page`).
pub fn detect_cookie_consent(lower_html: &str) -> bool {
    // Multi-token / exact anchored markers.
    if lower_html.contains("consent.google.com") || lower_html.contains("consent.google") {
        return true;
    }
    if lower_html.contains("__tcfapi") {
        return true;
    }
    if lower_html.contains("onetrust-consent-sdk") {
        return true;
    }
    if lower_html.contains(r#"id="cmp""#)
        || lower_html.contains(r#"class="cmp""#)
        || lower_html.contains("data-cmp")
    {
        return true;
    }

    // Single-token vendor markers — only in attribute-value context.
    if attr_token_present(lower_html, "onetrust")
        || attr_token_present(lower_html, "didomi")
        || attr_token_present(lower_html, "cookiebot")
        || attr_token_present(lower_html, "consentmanager")
    {
        return true;
    }

    false
}

/// Detect a paywall/meter/gate marker from a raw HTML body.
///
/// Case-insensitive, token-anchored matching (NOT bare-word substrings). Bare
/// `paywall`/`metered`/`premium`/`subscription` words in prose do NOT match.
///
/// Caller must pass the HTML already lowercased (see `classify_page`).
pub fn detect_paywall(lower_html: &str) -> bool {
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
    MARKERS.iter().any(|m| lower_html.contains(m))
}

/// True when any SPA-shell root marker is present, case-insensitively.
///
/// Caller must pass the HTML already lowercased (see `classify_page`).
fn has_spa_shell_marker(lower_html: &str) -> bool {
    const MARKERS: &[&str] = &[
        r#"<div id="root">"#,
        r#"<div id="app">"#,
        r#"<div id="__next">"#,
        r#"<div id="__nuxt">"#,
        r#"<div id="app-root">"#,
    ];
    MARKERS.iter().any(|m| lower_html.contains(m))
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
///
/// Caller must pass the HTML already lowercased (see `classify_page`).
fn has_js_enforcement_marker(lower_html: &str) -> bool {
    lower_html.contains("httpservice/retry/enablejs")
        || token_anchored(lower_html, "enablejs")
        || token_anchored(lower_html, "enable-js")
}

/// True when a page is JS-heavy: script-dominance (measurement-based, primary)
/// OR an SPA-shell root marker OR a JS-enforcement interstitial marker.
///
/// Caller must pass the HTML already lowercased (see `classify_page`).
fn is_js_heavy(lower_html: &str, visible_len: usize, script_len: usize) -> bool {
    // 1. Script dominance (primary, measurement-based).
    if script_len > visible_len && visible_len < MEANINGFUL_CONTENT_THRESHOLD {
        return true;
    }
    // 2. SPA-shell root markers (additional signal).
    if has_spa_shell_marker(lower_html) {
        return true;
    }
    // 3. JS-enforcement interstitial markers (additional signal).
    has_js_enforcement_marker(lower_html)
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
///   a. `detect_anti_bot(html)` → `Blocked { by }`;
///   b. `detect_cookie_consent(html)` → `Blocked { by: CookieConsent }`;
///   c. `detect_paywall(html)` → `Partial`;
///   d. JS-heavy (script-dominance / SPA shell / JS-enforcement interstitial)
///     → `JSHeavy`;
///   e. `>= GALLERY_IMG_THRESHOLD` `<img>` element openings → `Gallery`;
///   f. else → `Empty`.
///
/// The gallery check is a substring approximation: `classify_page` has only the
/// raw HTML (no parsed DOM), so it counts `<img` openings via `IMG_OPEN_RE`
/// (`<img` followed by whitespace/`>`/`/`) rather than real parsed `<img>`
/// elements.
#[allow(clippy::doc_overindented_list_items, clippy::doc_lazy_continuation)]
pub fn classify_page(html: &str, visible_len: usize, script_len: usize) -> PageStatus {
    if visible_len >= MEANINGFUL_CONTENT_THRESHOLD {
        return PageStatus::Article;
    }

    // Lower-case the HTML ONCE for the entire detection pass. Every detector
    // below (and the gallery `<img>` count) consumes this single pre-lowered
    // string, avoiding ~5-6 full-body allocations per thin page.
    let lower = html.to_lowercase();

    if let Some(by) = detect_anti_bot(&lower) {
        return PageStatus::Blocked { by };
    }
    if detect_cookie_consent(&lower) {
        return PageStatus::Blocked {
            by: BlockedBy::CookieConsent,
        };
    }
    if detect_paywall(&lower) {
        return PageStatus::Partial;
    }
    if is_js_heavy(&lower, visible_len, script_len) {
        return PageStatus::JSHeavy;
    }
    if IMG_OPEN_RE.find_iter(&lower).count() >= GALLERY_IMG_THRESHOLD {
        return PageStatus::Gallery;
    }
    PageStatus::Empty
}

#[cfg(test)]
#[path = "../../tests/unit/core/page_status_test.rs"]
mod tests;
