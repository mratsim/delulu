use super::*;
use delulu_webfetch::core::types::MarkdownDocument;

fn generic_html(frontmatter: &str, body: &str) -> ExtractionResult {
    ExtractionResult::GenericHtml {
        content_md: MarkdownDocument {
            frontmatter: frontmatter.to_string(),
            body: body.to_string(),
        },
        raw_html_len: 0,
        filtered_html_len: 0,
    }
}

// ── enrich_date_of_retrieval ───────────────────────────────────────────

#[test]
fn test_enrich_date_of_retrieval_replaces_placeholder() {
    let mut out = "---\ntitle: Test\ndate_of_retrieval: N/A\n---\n\nBody".to_string();
    let now = "2026-01-15T10:00:00+00:00";
    enrich_date_of_retrieval(&mut out, now);
    assert!(out.contains(&format!("date_of_retrieval: {}", now)));
    assert!(!out.contains("date_of_retrieval: N/A"));
}

#[test]
fn test_enrich_date_of_retrieval_inside_frontmatter_block() {
    // Non-tautological: asserts the `date_of_retrieval` line lands INSIDE the
    // frontmatter block — after the opening `---` delimiter and before the
    // closing `---` delimiter — and that the body never carries it. If
    // `enrich_date_of_retrieval` were to append the date to the body (the old
    // buggy behavior) instead of inside the frontmatter, this test fails.
    //
    // Uses GenericHtml (not Reddit) so the frontmatter genuinely LACKS the
    // field: the Reddit arm always emits `date_of_retrieval: N/A` and therefore
    // exercises the `replace` path, never the insert-before-closing path that
    // this test guards.
    let result = ExtractionResult::GenericHtml {
        content_md: MarkdownDocument {
            frontmatter: "title: Test\n".to_string(),
            body: "Post body".to_string(),
        },
        raw_html_len: 0,
        filtered_html_len: 0,
    };
    let out = md_doc_to_string(result);

    // Opening frontmatter delimiter must be at the very start.
    assert!(
        out.starts_with("---\n"),
        "frontmatter must open the document, got:\n{out}"
    );
    // Closing delimiter is the first `\n---` after the opening.
    let closing = out.find("\n---").expect("closing --- delimiter must exist");
    let fm_region = &out[..closing];
    // date_of_retrieval must be strictly INSIDE the block (after opening ---,
    // before the closing ---).
    assert!(
        fm_region.starts_with("---\n"),
        "date_of_retrieval must come after the opening ---, got:\n{out}"
    );
    assert!(
        fm_region.contains("date_of_retrieval:"),
        "date_of_retrieval must be inside the frontmatter block, got:\n{out}"
    );
    // The body (after the closing `---\n\n`) must NOT carry the date.
    let body_start = closing + "\n---\n\n".len();
    let body = &out[body_start..];
    assert!(
        !body.contains("date_of_retrieval:"),
        "body must not carry date_of_retrieval, got:\n{out}"
    );
}

#[test]
fn test_enrich_date_of_retrieval_no_frontmatter() {
    let mut out = "Just body text with no frontmatter".to_string();
    let now = "2026-01-15T10:00:00+00:00";
    enrich_date_of_retrieval(&mut out, now);
    // A `---` frontmatter must always be created, with the field inside it.
    assert!(out.starts_with("---\n"));
    assert!(out.contains(&format!("date_of_retrieval: {}", now)));
    // The body must not carry a stray date_of_retrieval line.
    let body_start = out.find("Just body text").unwrap();
    let body = &out[body_start..];
    assert!(!body.contains("date_of_retrieval:"));
    assert!(body.ends_with("text with no frontmatter"));
}

#[test]
fn test_enrich_date_of_retrieval_body_horizontal_rule_not_spliced() {
    // A `\n---` in the BODY (horizontal rule / fenced code) must not be
    // used as the frontmatter's closing delimiter.
    let mut out = "Some intro.\n\n---\n\nMore text after a horizontal rule.".to_string();
    let now = "2026-01-15T10:00:00+00:00";
    enrich_date_of_retrieval(&mut out, now);
    assert!(out.starts_with("---\n"));
    assert!(out.contains(&format!("date_of_retrieval: {}", now)));
    assert!(out.contains("\n\n---\n\nMore text after a horizontal rule."));
    let body_start = out.find("Some intro.").unwrap();
    let body = &out[body_start..];
    assert!(!body.contains("date_of_retrieval:"));
}

#[test]
fn test_enrich_date_of_retrieval_preserves_existing_timestamp() {
    let mut out =
        "---\ntitle: Test\ndate_of_retrieval: 2025-12-01T00:00:00+00:00\n---\n\nBody".to_string();
    let now = "2026-01-15T10:00:00+00:00";
    enrich_date_of_retrieval(&mut out, now);
    // Only replaces exact "N/A" placeholder, not existing timestamps
    assert!(out.contains("date_of_retrieval: 2025-12-01T00:00:00+00:00"));
}

// ── md_doc_to_string frontmatter shape (Issue 3) ───────────────────────

#[test]
fn test_md_doc_to_string_generic_html_with_frontmatter() {
    // Non-tautological: asserts the actual frontmatter BLOCK structure, not
    // merely that the body survives.
    let out = md_doc_to_string(generic_html("title: Hello World", "Body text here"));
    // Opens with the frontmatter block containing the source frontmatter.
    assert!(
        out.starts_with("---\ntitle: Hello World\n"),
        "frontmatter block must open with the source frontmatter, got:\n{out}"
    );
    // date_of_retrieval is inserted INSIDE the frontmatter block.
    assert!(
        out.contains("\ndate_of_retrieval: "),
        "date_of_retrieval must be inserted into the frontmatter block, got:\n{out}"
    );
    // Closing delimiter followed by the preserved body.
    assert!(
        out.contains("\n---\n\nBody text here"),
        "body must follow the closing frontmatter delimiter, got:\n{out}"
    );
}

#[test]
fn test_md_doc_to_string_generic_html_empty_frontmatter() {
    // Non-tautological: even an empty source frontmatter must produce a
    // well-formed `---` block (the always-frontmatter fix), with the body
    // preserved after the closing delimiter.
    let out = md_doc_to_string(generic_html("", "Body text here"));
    assert!(
        out.starts_with("---\n"),
        "must always open a block, got:\n{out}"
    );
    assert!(
        out.contains("date_of_retrieval: "),
        "date_of_retrieval must be present in the empty-frontmatter block, got:\n{out}"
    );
    assert!(
        out.contains("\n---\n\nBody text here"),
        "body must be preserved after the closing delimiter, got:\n{out}"
    );
}

#[test]
fn test_md_doc_to_string_reddit_arm() {
    // Optional reddit-arm regression: frontmatter + comments still emitted.
    let result = ExtractionResult::Reddit {
        title: "My Post".to_string(),
        selftext: "Post body".to_string(),
        author: "alice".to_string(),
        score: 5,
        permalink: "/r/test/comments/abc".to_string(),
        source_url: "https://reddit.com/r/test/comments/abc".to_string(),
        comments: vec![RedditComment {
            author: "bob".to_string(),
            body: "A comment".to_string(),
            score: 2,
            depth: 0,
            replies: Vec::new(),
        }],
        comment_count: 1,
        comments_truncated: false,
    };
    let out = md_doc_to_string(result);
    assert!(
        out.starts_with("---\ntitle: My Post\n"),
        "reddit arm must keep its frontmatter, got:\n{out}"
    );
    assert!(out.contains("source_type: reddit"), "got:\n{out}");
    assert!(out.contains("**bob** (score: 2): A comment"), "got:\n{out}");
    assert!(out.contains("date_of_retrieval: "), "got:\n{out}");
    assert!(out.contains("comments_truncated: false"), "got:\n{out}");
}

#[test]
fn test_md_doc_to_string_reddit_escapes_frontmatter_metadata() {
    // Non-tautological: a title containing a newline, a `---` line, and a
    // `malicious: true`-style payload must stay INSIDE the frontmatter as a
    // single escaped scalar. Before the fix this payload was injected as its
    // own frontmatter line (breaking the `---` delimiters and moving every
    // subsequent field into the body).
    let result = ExtractionResult::Reddit {
        title: "hi\n---\nmalicious: true".to_string(),
        selftext: "Body".to_string(),
        author: "alice".to_string(),
        score: 1,
        permalink: "/r/x".to_string(),
        source_url: "https://reddit.com/r/x".to_string(),
        comments: vec![],
        comment_count: 0,
        comments_truncated: false,
    };
    let out = md_doc_to_string(result);
    // The frontmatter still opens with `---`.
    assert!(out.starts_with("---\n"), "got:\n{out}");
    // The closing `---` is immediately followed by the intact body — this
    // fails before the fix because the injected `---` closed the block early.
    assert!(
        out.contains("\n---\n\nBody"),
        "body must follow a single closing delimiter, got:\n{out}"
    );
    // The frontmatter block (up to the first `---`) still contains all the
    // real fields — before the fix the injected `---` pushed `author:` etc.
    // into the body.
    let fm_end = out.find("\n---").unwrap();
    let fm = &out[..fm_end];
    assert!(
        fm.contains("author: alice"),
        "author escaped frontmatter:\n{fm}"
    );
    assert!(fm.contains("comment_count: 0"), "got:\n{fm}");
    // The injected payload is NOT its own frontmatter line.
    assert!(
        !fm.contains("\nmalicious: true\n"),
        "payload injected a key:\n{fm}"
    );
}

#[test]
fn test_yaml_escape_neutralizes_delimiter_and_key_injection() {
    // A2 regression: page-controlled metadata must never break out of the YAML
    // frontmatter or inject a key. yaml_escape collapses newlines (so a `---`
    // cannot close the block) and quotes ambiguous values.
    let payload = "hi\n---\nmalicious: true";
    let escaped = delulu_webfetch::core::yaml::yaml_escape(payload);
    // No literal newline survives (the `---` and `malicious:` are on one line,
    // inside quotes) — so it cannot close the block or inject a bare key line.
    assert!(
        !escaped.contains('\n'),
        "escaped value must be single-line: {escaped:?}"
    );
    assert!(
        escaped.starts_with('"'),
        "ambiguous value must be quoted: {escaped:?}"
    );
    // Embedding it in a frontmatter line must keep the block intact.
    assert!(
        !escaped.contains("\n---"),
        "injected --- must not form a bare line: {escaped:?}"
    );
    // Embedding it in a frontmatter block keeps the block intact: the injected
    // `---` is now inside the quoted value, so the real closing `---` follows
    // `source_type:` and the body survives.
    let frontmatter = format!("title: {escaped}\nsource_type: generic_html\n");
    let out = format!("---\n{frontmatter}---\n\nBody");
    assert!(out.starts_with("---\n"));
    assert!(
        out.contains("\nsource_type: generic_html\n---\n\nBody"),
        "block must close after source_type with body intact, got: {out}"
    );
}

#[test]
fn test_enrich_date_of_retrieval_only_rewrites_frontmatter_placeholder() {
    // A3 regression: the exact `date_of_retrieval: N/A` placeholder must be
    // rewritten ONLY within the frontmatter region — a body occurrence must
    // never be touched.
    let mut out =
        "---\ntitle: T\ndate_of_retrieval: N/A\n---\n\nBody has date_of_retrieval: N/A inside it."
            .to_string();
    let now = "2026-01-15T10:00:00+00:00";
    enrich_date_of_retrieval(&mut out, now);
    // Frontmatter occurrence replaced:
    assert!(out.starts_with(&format!("---\ntitle: T\ndate_of_retrieval: {now}\n---\n\n")));
    // Body occurrence untouched:
    assert!(
        out.contains("Body has date_of_retrieval: N/A inside it."),
        "body placeholder must not be rewritten, got: {out}"
    );
}
