//! Shared Markdown output formatting for the CLI and MCP binaries.
//!
//! `md_doc_to_string` renders an [`ExtractionResult`] into a Markdown string
//! with a YAML frontmatter block, and `enrich_date_of_retrieval` stamps the
//! current retrieval timestamp into that frontmatter. Both binaries include
//! this single module (via `#[path]`) so they share one implementation and one
//! test suite — there is no per-binary copy.
//!
//! The binaries (`delulu-fetch`, `delulu-webfetch-mcp`) are separate crates
//! from the library, so this module is included into each via `#[path]` rather
//! than being registered in `core/mod.rs`. Types are referenced through the
//! absolute crate name (`delulu_webfetch::`), which resolves the library from
//! inside either binary.

use chrono::Utc;
use delulu_webfetch::core::types::{ExtractionResult, RedditComment};

/// Convert an `ExtractionResult` into a Markdown string with YAML frontmatter.
pub fn md_doc_to_string(result: ExtractionResult) -> String {
    let now_iso = Utc::now().to_rfc3339();
    match result {
        ExtractionResult::GenericHtml { content_md, .. } => {
            let mut out = String::new();
            // Always emit a YAML frontmatter block so `date_of_retrieval`
            // always has a home, even when the source produced an empty
            // frontmatter string.
            out.push_str("---\n");
            out.push_str(&content_md.frontmatter);
            if !content_md.frontmatter.is_empty() && !content_md.frontmatter.ends_with('\n') {
                out.push('\n');
            }
            out.push_str("---\n\n");
            out.push_str(&content_md.body);
            enrich_date_of_retrieval(&mut out, &now_iso);
            out
        }
        ExtractionResult::Reddit {
            title,
            selftext,
            author,
            score,
            permalink,
            source_url,
            comment_count,
            comments,
            ..
        } => {
            // TODO: date_of_publication should be extracted from created_utc (thread
            // it through ExtractionResult::Reddit) instead of hardcoded N/A.
            let frontmatter = format!(
                "title: {}\nauthor: {}\nscore: {}\nsource_type: reddit\npermalink: {}\nsource_url: {}\ndate_of_publication: N/A\ndate_of_retrieval: N/A\ncomment_count: {}",
                title, author, score, permalink, source_url, comment_count
            );
            let mut out = format!("---\n{frontmatter}\n---\n\n");
            out.push_str(&selftext);
            out.push('\n');
            for comment in &comments {
                format_reddit_comment(&mut out, comment, 0);
            }
            enrich_date_of_retrieval(&mut out, &now_iso);
            out
        }
        ExtractionResult::Discourse {
            title,
            topic_id,
            posts,
            post_count,
            posts_returned,
            ..
        } => {
            // TODO: date_of_publication should be extracted from the first post's
            // created_at (thread through ExtractionResult::Discourse) instead of N/A.
            let frontmatter = format!(
                "title: {}\ntopic_id: {}\nsource_type: discourse\nsource_url: N/A\ndate_of_publication: N/A\ndate_of_retrieval: N/A\npost_count: {}\nposts_returned: {}",
                title, topic_id, post_count, posts_returned
            );
            let mut out = format!("---\n{frontmatter}\n---\n\n");
            for (i, post) in posts.iter().enumerate() {
                if i > 0 {
                    out.push_str("---\n\n");
                }
                out.push_str(&format!(
                    "**{}** (post #{}):\n\n{}\n\n",
                    post.username, post.post_number, post.raw
                ));
            }
            enrich_date_of_retrieval(&mut out, &now_iso);
            out
        }
    }
}

/// Replace `date_of_retrieval: N/A` with the actual ISO 8601 timestamp.
///
/// Operates ONLY on the YAML frontmatter block at the top of the document.
/// Guarantees the output always has a proper `---` frontmatter containing a
/// `date_of_retrieval` field: if the document has no frontmatter, one is
/// created; if the frontmatter lacks the field, it is inserted inside the
/// block — never appended to the body and never spliced at an arbitrary
/// `---` mid-document.
pub fn enrich_date_of_retrieval(out: &mut String, now_iso: &str) {
    let replacement = format!("date_of_retrieval: {}", now_iso);

    // The frontmatter block is at the very start: opening `---\n`, then its
    // closing `\n---` (the first `---` delimiter). The body (and any
    // `\n---` horizontal rules / fenced-code delimiters in it) only ever
    // follows the closing delimiter, so the first `\n---` is the frontmatter's.
    let fm_end = out.find("\n---").unwrap_or(out.len());
    let fm_region = &out[..fm_end];

    if fm_region.contains("date_of_retrieval:") {
        // Field already present — only rewrite the exact `N/A` placeholder.
        *out = out.replace("date_of_retrieval: N/A", &replacement);
        return;
    }

    if fm_region.starts_with("---") {
        // Frontmatter exists but lacks the field: insert it inside the block,
        // just before the closing `---` delimiter.
        out.insert_str(fm_end, &format!("\n{}", replacement));
        return;
    }

    // No frontmatter at all: create one that carries the field.
    *out = format!("---\n{}\n---\n\n{}", replacement, out);
}

/// Format a reddit comment recursively into markdown.
fn format_reddit_comment(out: &mut String, comment: &RedditComment, depth: u32) {
    let prefix = "> ".repeat(depth as usize);
    out.push_str(&format!(
        "{}**{}** (score: {}): {}\n\n",
        prefix, comment.author, comment.score, comment.body
    ));
    for reply in &comment.replies {
        format_reddit_comment(out, reply, depth + 1);
    }
}

#[cfg(test)]
mod tests {
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
    fn test_enrich_date_of_retrieval_appends_before_closing() {
        let mut out = "---\ntitle: Test\n---\n\nBody".to_string();
        let now = "2026-01-15T10:00:00+00:00";
        enrich_date_of_retrieval(&mut out, now);
        assert!(out.contains(&format!("date_of_retrieval: {}", now)));
        assert!(out.starts_with("---"));
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
        let mut out = "---\ntitle: Test\ndate_of_retrieval: 2025-12-01T00:00:00+00:00\n---\n\nBody"
            .to_string();
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
        };
        let out = md_doc_to_string(result);
        assert!(
            out.starts_with("---\ntitle: My Post\n"),
            "reddit arm must keep its frontmatter, got:\n{out}"
        );
        assert!(out.contains("source_type: reddit"), "got:\n{out}");
        assert!(out.contains("**bob** (score: 2): A comment"), "got:\n{out}");
        assert!(out.contains("date_of_retrieval: "), "got:\n{out}");
    }
}
