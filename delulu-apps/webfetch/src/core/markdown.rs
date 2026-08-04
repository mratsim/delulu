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
            comments_truncated,
            comments,
            ..
        } => {
            // TODO: date_of_publication should be extracted from created_utc (thread
            // it through ExtractionResult::Reddit) instead of hardcoded N/A.
            let frontmatter = format!(
                "title: {}\nauthor: {}\nscore: {}\nsource_type: reddit\npermalink: {}\nsource_url: {}\ndate_of_publication: N/A\ndate_of_retrieval: N/A\ncomment_count: {}\ncomments_truncated: {}",
                yaml_escape(&title),
                yaml_escape(&author),
                score,
                yaml_escape(&permalink),
                yaml_escape(&source_url),
                comment_count,
                comments_truncated
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
                yaml_escape(&title),
                topic_id,
                post_count,
                posts_returned
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

/// Make a string safe to embed as a YAML scalar value in the frontmatter.
///
/// Attacker-controlled page metadata (title, author, permalink, source_url)
/// must never break out of the `---` frontmatter block or inject a new
/// `key: value` line. Embedded CR/LF are collapsed onto a single line (as a
/// literal `\n` escape), and values that would otherwise be ambiguous (a
/// leading YAML indicator char, or containing `: ` or `#`) are wrapped in
/// double quotes with internal backslashes/quotes escaped.
fn yaml_escape(value: &str) -> String {
    let single_line = value.replace('\r', " ").replace('\n', "\\n");
    let needs_quotes = single_line.starts_with(|c: char| {
        matches!(
            c,
            '-' | '?'
                | ':'
                | '{'
                | '}'
                | '['
                | ']'
                | '#'
                | '&'
                | '*'
                | '!'
                | '|'
                | '>'
                | '\''
                | '"'
                | '%'
                | '@'
                | '`'
        )
    }) || single_line.contains(": ")
        || single_line.contains('#');
    if needs_quotes {
        let escaped = single_line.replace('\\', "\\\\").replace('"', "\\\"");
        format!("\"{escaped}\"")
    } else {
        single_line
    }
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
#[path = "../../tests/unit/core/markdown_test.rs"]
mod tests;
