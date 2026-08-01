use super::*;

// ---------------------------------------------------------------------------
// exact JSON serialization forms
// ---------------------------------------------------------------------------



#[test]
fn page_status_article_serializes_exact() {
    assert_eq!(serde_json::to_string(&PageStatus::Article).unwrap(), r#"{"status":"article"}"#);
}

#[test]
fn page_status_partial_serializes_exact() {
    assert_eq!(serde_json::to_string(&PageStatus::Partial).unwrap(), r#"{"status":"partial"}"#);
}

#[test]
fn page_status_js_heavy_serializes_exact() {
    assert_eq!(serde_json::to_string(&PageStatus::JSHeavy).unwrap(), r#"{"status":"js_heavy"}"#);
}

#[test]
fn page_status_gallery_serializes_exact() {
    assert_eq!(serde_json::to_string(&PageStatus::Gallery).unwrap(), r#"{"status":"gallery"}"#);
}

#[test]
fn page_status_empty_serializes_exact() {
    assert_eq!(serde_json::to_string(&PageStatus::Empty).unwrap(), r#"{"status":"empty"}"#);
}

#[test]
fn page_status_blocked_cloudflare_turnstile_serializes_exact() {
    assert_eq!(
        serde_json::to_string(&PageStatus::Blocked {
            by: BlockedBy::CloudflareTurnstile
        })
        .unwrap(),
        r#"{"status":"blocked","by":"cloudflare_turnstile"}"#
    );
}

#[test]
fn page_status_blocked_captcha_serializes_exact() {
    assert_eq!(
        serde_json::to_string(&PageStatus::Blocked {
            by: BlockedBy::Captcha
        })
        .unwrap(),
        r#"{"status":"blocked","by":"captcha"}"#
    );
}

#[test]
fn page_status_blocked_anubis_serializes_exact() {
    assert_eq!(
        serde_json::to_string(&PageStatus::Blocked {
            by: BlockedBy::Anubis
        })
        .unwrap(),
        r#"{"status":"blocked","by":"anubis"}"#
    );
}

#[test]
fn page_status_blocked_cookie_consent_serializes_exact() {
    // CookieConsent is a 4th BlockedBy cause that serializes to "cookie_consent".
    assert_eq!(
        serde_json::to_string(&PageStatus::Blocked {
            by: BlockedBy::CookieConsent
        })
        .unwrap(),
        r#"{"status":"blocked","by":"cookie_consent"}"#
    );
}

#[test]
fn blocked_by_has_exactly_four_variants() {
    // Compile-time assertion via a non-exhaustive match over all four variants.
    let all = [
        BlockedBy::CloudflareTurnstile,
        BlockedBy::Captcha,
        BlockedBy::Anubis,
        BlockedBy::CookieConsent,
    ];
    for b in all {
        let _ = match b {
            BlockedBy::CloudflareTurnstile => 0,
            BlockedBy::Captcha => 1,
            BlockedBy::Anubis => 2,
            BlockedBy::CookieConsent => 3,
        };
    }
}

// ---------------------------------------------------------------------------
// detect_anti_bot
// ---------------------------------------------------------------------------

#[test]
fn detect_anti_bot_clean_page_is_none() {
    let html = "<html><body><p>Ordinary readable content here</p></body></html>";
    assert_eq!(detect_anti_bot(html), None);
}

#[test]
fn detect_anti_bot_captcha_vendor() {
    let html = r#"<div class="g-recaptcha" data-sitekey="abc"></div>"#;
    assert_eq!(detect_anti_bot(html), Some(BlockedBy::Captcha));
    let html = r#"<div class="h-captcha"></div>"#;
    assert_eq!(detect_anti_bot(html), Some(BlockedBy::Captcha));
    let html = r#"<div data-recaptcha="1"></div>"#;
    assert_eq!(detect_anti_bot(html), Some(BlockedBy::Captcha));
}

#[test]
fn detect_anti_bot_cloudflare_vendor() {
    let html = r#"<div class="cf-turnstile"></div>"#;
    assert_eq!(detect_anti_bot(html), Some(BlockedBy::CloudflareTurnstile));
    let html = r#"<div id="cf-browser-verification"></div>"#;
    assert_eq!(detect_anti_bot(html), Some(BlockedBy::CloudflareTurnstile));
    let html = r#"<div class="cf-challenge"></div>"#;
    assert_eq!(detect_anti_bot(html), Some(BlockedBy::CloudflareTurnstile));
    let html = r#"<div id="__cf_chl_opt"></div>"#;
    assert_eq!(detect_anti_bot(html), Some(BlockedBy::CloudflareTurnstile));
    let html = r#"<div id="challenge-platform"></div>"#;
    assert_eq!(detect_anti_bot(html), Some(BlockedBy::CloudflareTurnstile));
}

#[test]
fn detect_anti_bot_anubis_vendor() {
    let html = r#"<div id="anubis"></div>"#;
    assert_eq!(detect_anti_bot(html), Some(BlockedBy::Anubis));
    let html = r#"<div class="anubis"></div>"#;
    assert_eq!(detect_anti_bot(html), Some(BlockedBy::Anubis));
    let html = r#"<div data-anubis="1"></div>"#;
    assert_eq!(detect_anti_bot(html), Some(BlockedBy::Anubis));
    let html = r#"<script src="anubis.js"></script>"#;
    assert_eq!(detect_anti_bot(html), Some(BlockedBy::Anubis));
}

#[test]
fn detect_anti_bot_never_returns_cookie_consent() {
    // CookieConsent is the separate concern of detect_cookie_consent.
    let html = r#"<div id="cmp"></div><script>__tcfapi</script>"#;
    let result = detect_anti_bot(html);
    assert!(result.is_none() || result != Some(BlockedBy::CookieConsent));
}

#[test]
fn detect_anti_bot_data_sitekey_only_maps_to_cloudflare() {
    // data-sitekey is a generic attribute used by reCAPTCHA v2, hCaptcha,
    // and Turnstile; it deliberately maps to CloudflareTurnstile (legacy group).
    let html = r#"<div data-sitekey="6Lc12345"></div>"#;
    assert_eq!(
        detect_anti_bot(html),
        Some(BlockedBy::CloudflareTurnstile),
        "data-sitekey-only must deterministically map to CloudflareTurnstile"
    );
}

#[test]
fn detect_anti_bot_vendor_determinism_captcha_wins() {
    // g-recaptcha AND data-sitekey both present → Captcha (checked first).
    let html = r#"<div class="g-recaptcha" data-sitekey="abc"></div>"#;
    assert_eq!(detect_anti_bot(html), Some(BlockedBy::Captcha));
}

#[test]
fn detect_anti_bot_false_positive_pins() {
    // Known false-positive class (per the legacy superset): these DO match.
    let html = "Just a moment... checking your browser before accessing.";
    assert_eq!(detect_anti_bot(html), Some(BlockedBy::CloudflareTurnstile));
    let html = "Please solve the turnstile to continue.";
    assert_eq!(detect_anti_bot(html), Some(BlockedBy::CloudflareTurnstile));
}

#[test]
fn detect_anti_bot_benign_prose_is_none() {
    let html = concat!(
        "We value your consent and take your privacy seriously. ",
        "No captcha needed here. Our premium subscription includes perks. ",
        "Read our anubis-themed blog post."
    );
    assert_eq!(detect_anti_bot(html), None);
}

#[test]
fn detect_anti_bot_bare_anubis_in_prose_is_none() {
    let html = "The ancient Egyptian god Anubis guarded the underworld.";
    assert_eq!(detect_anti_bot(html), None);
}

// -- superset equivalence -----------------------

#[test]
fn detect_anti_bot_is_superset_of_bot_detection_patterns() {
    // Mechanically iterate BOT_DETECTION_PATTERNS (single source of truth).
    use crate::core::detect::{BOT_DETECTION_PATTERNS, is_bot_detected};
    for pattern in BOT_DETECTION_PATTERNS.iter() {
        let html = format!("<html><body>{pattern}</body></html>");
        assert!(
            is_bot_detected(&html),
            "pattern '{pattern}' must satisfy is_bot_detected"
        );
        assert!(
            detect_anti_bot(&html).is_some(),
            "detect_anti_bot must be a superset: pattern '{pattern}' yielded None"
        );
    }
}

#[test]
fn detect_anti_bot_label_correctness_for_known_vendor_patterns() {
    // Assert the *label* (not just is_some) for known vendor patterns so
    // the step-4 catch-all mislabel is CI-visible.
    use crate::core::detect::BOT_DETECTION_PATTERNS;
    for pattern in BOT_DETECTION_PATTERNS.iter() {
        let html = format!("<html><body>{pattern}</body></html>");
        let expected = if pattern.to_lowercase().contains("g-recaptcha")
            || pattern.to_lowercase().contains("recaptcha")
        {
            BlockedBy::Captcha
        } else {
            BlockedBy::CloudflareTurnstile
        };
        assert_eq!(
            detect_anti_bot(&html),
            Some(expected),
            "pattern {pattern} should map to the expected label"
        );
    }
}

#[test]
fn detect_anti_bot_differential_is_bot_detected_implies_some() {
    use crate::core::detect::BOT_DETECTION_PATTERNS;
    for pattern in BOT_DETECTION_PATTERNS.iter() {
        let html = format!("<html><body>{pattern}</body></html>");
        assert!(
            is_bot_detected(&html),
            "precondition: is_bot_detected must be true for '{pattern}'"
        );
        assert!(
            detect_anti_bot(&html).is_some(),
            "is_bot_detected ⇒ detect_anti_bot.is_some() failed for '{pattern}'"
        );
    }
}

// ---------------------------------------------------------------------------
// detect_cookie_consent
// ---------------------------------------------------------------------------

#[test]
fn cookie_consent_consent_google_com_true() {
    assert!(detect_cookie_consent(r#"<script src="https://consent.google.com/..."></script>"#));
    assert!(detect_cookie_consent(r#"<script src="https://consent.google/..."></script>"#));
}

#[test]
fn cookie_consent_tcfapi_true() {
    assert!(detect_cookie_consent(r#"<script>__tcfapi('addEventListener')</script>"#));
}

#[test]
fn cookie_consent_id_cmp_true() {
    assert!(detect_cookie_consent(r#"<div id="cmp"></div>"#));
    assert!(detect_cookie_consent(r#"<div class="cmp"></div>"#));
    assert!(detect_cookie_consent(r#"<div data-cmp="1"></div>"#));
}

#[test]
fn cookie_consent_onetrust_attr_true_bare_prose_false() {
    assert!(detect_cookie_consent(r#"<div id="onetrust-consent-sdk"></div>"#));
    assert!(detect_cookie_consent(r#"<div id="onetrust-banner-sdk"></div>"#));
    assert!(!detect_cookie_consent("we use onetrust to manage cookies"));
}

#[test]
fn cookie_consent_bare_vendor_prose_false() {
    assert!(!detect_cookie_consent("Our vendor is didomi."));
    assert!(!detect_cookie_consent("We integrate cookiebot for analytics."));
    assert!(!detect_cookie_consent("Powered by consentmanager."));
}

#[test]
fn cookie_consent_bare_consent_prose_false() {
    assert!(!detect_cookie_consent("By continuing you agree to our cookie policy."));
    assert!(!detect_cookie_consent("We use cookies, see our consent policy."));
}

#[test]
fn cookie_consent_clean_false() {
    assert!(!detect_cookie_consent("<html><body><p>Normal page</p></body></html>"));
}

// ---------------------------------------------------------------------------
// detect_paywall
// ---------------------------------------------------------------------------

#[test]
fn paywall_token_anchored_true() {
    assert!(detect_paywall(r#"<div class="paywall"></div>"#));
    assert!(detect_paywall(r#"<div id="paywall"></div>"#));
    assert!(detect_paywall(r#"<div data-paywall="1"></div>"#));
    assert!(detect_paywall(r#"<div class="paywall-container"></div>"#));
    assert!(detect_paywall(r#"<div id="metered-content"></div>"#));
    assert!(detect_paywall(r#"<div class="metered-content"></div>"#));
    assert!(detect_paywall(r#"<div data-metered="1"></div>"#));
    assert!(detect_paywall(r#"<div class="subscription-gate"></div>"#));
    assert!(detect_paywall(r#"<div class="premium-gate"></div>"#));
    assert!(detect_paywall(r#"<div data-premium="1"></div>"#));
    assert!(detect_paywall(r#"<div id="subscription"></div>"#));
    assert!(detect_paywall(r#"<div class="subscription-gate-outer"></div>"#));
}

#[test]
fn paywall_prose_only_false() {
    assert!(!detect_paywall("We offer a premium subscription to all readers."));
    assert!(!detect_paywall("This article is behind a metered paywall."));
}

#[test]
fn paywall_clean_false() {
    assert!(!detect_paywall("<html><body><p>Free content</p></body></html>"));
}

// ---------------------------------------------------------------------------
// classify_page — Article-first priority
// ---------------------------------------------------------------------------

fn classify(html: &str, visible_len: usize, script_len: usize) -> PageStatus {
    classify_page(html, visible_len, script_len)
}

#[test]
fn classify_full_length_with_cloudflare_marker_is_article() {
    let html = "<html><body><div class=\"cf-turnstile\"></div>".to_string()
        + &"x".repeat(250)
        + "</body></html>";
    assert_eq!(classify(&html, 250, 0), PageStatus::Article);
}

#[test]
fn classify_full_length_with_consent_wall_marker_is_article() {
    let html = "<html><body><script src=\"https://consent.google.com/x\"></script>"
        .to_string()
        + &"y".repeat(250)
        + "</body></html>";
    assert_eq!(classify(&html, 250, 0), PageStatus::Article);
}

#[test]
fn classify_full_length_with_cmp_footer_banner_is_article() {
    // The readable-article-with-CMP-banner case: Article-first beats the marker.
    for marker in [
        "__tcfapi",
        r#"id="onetrust-consent-sdk""#,
        r#"id="cmp""#,
    ] {
        let html = format!("<html><body><div {marker}></div>{}...</body></html>", "z".repeat(220));
        assert_eq!(classify(&html, 220, 0), PageStatus::Article, "marker: {marker}");
    }
}

#[test]
fn classify_full_length_with_gate_words_is_article() {
    let html = "<html><body>premium subscription consent text</body></html>";
    assert_eq!(classify(html, 200, 0), PageStatus::Article);
}

#[test]
fn classify_thin_consent_wall_is_blocked_cookie_consent() {
    // User scenario: thin page with a consent-wall marker.
    let html = r#"<html><body><script src="https://consent.google.com/x"></script></body></html>"#;
    assert_eq!(
        classify(html, 199, 0),
        PageStatus::Blocked {
            by: BlockedBy::CookieConsent
        }
    );
}

#[test]
fn classify_thin_readable_with_cmp_footer_banner_is_blocked_cookie_consent() {
    // Mandatory thin-page-with-CMP-banner residual false-positive pin.
    for marker in ["__tcfapi", r#"id="onetrust-consent-sdk""#, r#"id="cmp""#] {
        let html = format!("<html><body><div {marker}></div>short</body></html>");
        assert_eq!(
            classify(&html, 199, 0),
            PageStatus::Blocked {
                by: BlockedBy::CookieConsent
            },
            "marker: {marker}"
        );
    }
}

#[test]
fn classify_thin_cloudflare_is_blocked_cloudflare() {
    let html = r#"<html><body><div class="cf-turnstile"></div></body></html>"#;
    assert_eq!(
        classify(html, 199, 0),
        PageStatus::Blocked {
            by: BlockedBy::CloudflareTurnstile
        }
    );
}

#[test]
fn classify_thin_both_cloudflare_and_consent_is_cloudflare() {
    // anti-bot outranks consent.
    let html = r#"<html><body><div class="cf-turnstile"></div><script src="https://consent.google.com/x"></script></body></html>"#;
    assert_eq!(
        classify(html, 199, 0),
        PageStatus::Blocked {
            by: BlockedBy::CloudflareTurnstile
        }
    );
}

#[test]
fn classify_thin_paywall_is_partial() {
    let html = r#"<html><body><div class="paywall"></div></body></html>"#;
    assert_eq!(classify(html, 199, 0), PageStatus::Partial);
}

#[test]
fn classify_thin_spa_is_js_heavy() {
    let html = r#"<html><body><div id="root"></div></body></html>"#;
    assert_eq!(classify(html, 199, 0), PageStatus::JSHeavy);
    let html = r#"<html><body><div id="__next"></div></body></html>"#;
    assert_eq!(classify(html, 199, 0), PageStatus::JSHeavy);
}

#[test]
fn classify_thin_image_dominant_is_gallery() {
    let imgs: String = (0..8).map(|i| format!(r#"<img src="img{i}.jpg" />"#)).collect();
    let html = format!("<html><body>{imgs}</body></html>");
    assert_eq!(classify(&html, 199, 0), PageStatus::Gallery);
}

#[test]
fn classify_thin_clean_is_empty() {
    let html = "<html><body></body></html>";
    assert_eq!(classify(html, 0, 0), PageStatus::Empty);
    assert_eq!(classify(html, 199, 0), PageStatus::Empty);
}

#[test]
fn classify_boundary_199_never_article_200_article() {
    let html = "<html><body>".to_string() + &"a".repeat(1000) + "</body></html>";
    assert_eq!(classify(&html, 199, 0), PageStatus::Empty);
    assert_eq!(classify(&html, 200, 0), PageStatus::Article);
}

// ---------------------------------------------------------------------------
// classify_page — measurement-based JSHeavy
// ---------------------------------------------------------------------------
// The Article gate uses `visible_len` (pre-pipeline visible text); JSHeavy is
// primarily script-dominance (`script_len > visible_len` with
// `visible_len < MEANINGFUL_CONTENT_THRESHOLD`), with SPA-shell and
// enable-js interstitial markers as additional signals.

#[test]
fn classify_script_dominant_marker_free_is_js_heavy() {
    // Marker-free body: no SPA shell, no enable-js marker. JSHeavy fires by
    // script-dominance measurement alone — no marker required.
    let html = r#"<html><body><script>var big = "x".repeat(5000);</script><p>hi</p></body></html>"#;
    assert_eq!(
        classify(html, 50, 5000),
        PageStatus::JSHeavy,
        "script-dominant -> JSHeavy"
    );
    // Same body, no script dominance -> Empty (not script-dominant).
    assert_eq!(
        classify(html, 50, 0),
        PageStatus::Empty,
        "not script-dominant -> Empty"
    );
    // Boundary: visible_len == 200 -> Article-first, never JSHeavy.
    assert_eq!(
        classify(html, 200, 5000),
        PageStatus::Article,
        "Article-first at 200"
    );
}

#[test]
fn classify_style_only_thin_page_is_empty() {
    // A style-only thin page (CSS, no script, no marker) is NOT script-dominant.
    let html = r#"<html><head><style>body{color:red}</style></head><body></body></html>"#;
    assert_eq!(classify(html, 10, 0), PageStatus::Empty);
}

#[test]
fn classify_enablejs_path_marker_is_js_heavy() {
    let html = r#"<script src="https://www.google.com/httpservice/retry/enablejs"></script>"#;
    assert_eq!(classify(html, 50, 0), PageStatus::JSHeavy);
}

#[test]
fn classify_token_anchored_enablejs_is_js_heavy() {
    // Token-anchored `enablejs` (attribute-value context) triggers JSHeavy.
    let html = r#"<div data-x="enablejs"></div>"#;
    assert_eq!(classify(html, 50, 0), PageStatus::JSHeavy);
    // Hyphenated `enable-js` standalone also triggers.
    let html2 = r#"<div data-x="enable-js"></div>"#;
    assert_eq!(classify(html2, 50, 0), PageStatus::JSHeavy);
}

#[test]
fn classify_bare_or_embedded_js_prose_not_js_heavy() {
    // Bare prose words and tokens embedded in a larger identifier must NOT
    // trigger JSHeavy when the page is not script-dominant.
    let cases = [
        "please enable javascript to continue",
        "we use js and enable features for you",
        "enablejsx is a token",
        "enablejson config",
        "enable-jsx variant",
        r#"<script>var x=1;</script><p>enable</p>"#,
    ];
    for html in cases {
        assert_eq!(
            classify(html, 50, 0),
            PageStatus::Empty,
            "bare/embedded JS prose must not be JSHeavy: {html}"
        );
    }
}

#[test]
fn classify_readable_article_outranks_js_markers() {
    // A readable article (visible_len >= 200) that is also
    // script-dominant and/or carries an enable-js marker is still Article.
    let html = r#"<html><body><div id="root"></div><script>var big = "x".repeat(9000);</script><p>meaningful article content that exceeds the threshold</p></body></html>"#;
    assert_eq!(classify(html, 500, 9000), PageStatus::Article);
    let html2 = r#"<html><body><div data-x="httpservice/retry/enablejs"></div><p>substantial readable body text</p></body></html>"#;
    assert_eq!(classify(html2, 300, 0), PageStatus::Article);
}