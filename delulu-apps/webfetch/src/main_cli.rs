//! CLI entry point for delulu-fetch.
//!
//! Usage:
//! ```
//! delulu-fetch -u <URL>
//! delulu-fetch -i <FILE>
//! delulu-fetch -u <URL> --output-format html
//! delulu-fetch -i <FILE> --output-format html
//! delulu-fetch -u <URL> --timeout 120
//! echo '<html>...</html>' | delulu-fetch -i -
//! delulu-fetch doc <URL>
//! delulu-fetch doc <URL> --output-format html
//! ```

use anyhow::{Context, Error, Result};
use chrono::Utc;
use clap::{Parser, Subcommand};
use delulu_rate_limited_crawler::RateLimitedCrawler;
use delulu_webfetch::{
    ExtractionResult, MAX_BODY_SIZE, MarkdownDocument, RedditComment, fetch_and_extract,
    fetch_raw_html,
};
use std::io::Read;
use std::time::Duration;
use tracing_subscriber::{EnvFilter, layer::SubscriberExt, util::SubscriberInitExt};

// ---------------------------------------------------------------------------
// Subcommand enum
// ---------------------------------------------------------------------------

#[derive(Subcommand, Debug)]
enum Command {
    /// Fetch a document (PDF, Word, etc.) and convert to markdown/html.
    Doc(DocArgs),
}

#[derive(Parser, Debug)]
#[command(name = "delulu-fetch")]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

// ---------------------------------------------------------------------------
// Args (flat, backward-compatible with prior versions)
// ---------------------------------------------------------------------------
#[derive(Parser, Debug)]
#[command(name = "delulu-fetch")]
struct Args {
    /// Input file path (raw HTML). Use "-" for stdin.
    #[arg(short = 'i', long, conflicts_with_all = &["url"])]
    input_file: Option<String>,

    /// URL to fetch and extract content from.
    #[arg(short = 'u', long, conflicts_with_all = &["input_file"])]
    url: Option<String>,

    /// Total fetch timeout in seconds (URL mode only).
    #[arg(long, default_value = "60")]
    timeout: u64,

    /// Queries per second rate limit (URL mode only).
    #[arg(long, default_value = "2")]
    qps: u64,

    /// Output format: markdown (default) or html.
    #[arg(long)]
    output_format: Option<String>,

    /// [deprecated] Output raw JSON instead of Markdown.
    #[arg(long)]
    raw: bool,

    /// Pipeline to use: "tf" (default, Trafilatura) or "rd" (Mozilla Readability).
    #[arg(long, default_value = "tf", value_parser = ["tf", "rd"])]
    pipeline: String,
}

// ---------------------------------------------------------------------------
// Doc subcommand args
// ---------------------------------------------------------------------------

#[derive(Parser, Debug)]
#[command(name = "doc")]
struct DocArgs {
    /// URL of the document to fetch
    url: String,
    /// Output format: markdown (default) or html
    #[arg(long)]
    output_format: Option<String>,
    /// Fetch timeout in seconds
    #[arg(long, default_value = "120")]
    timeout: u64,
}

// ---------------------------------------------------------------------------
// Output formatting
// ---------------------------------------------------------------------------

/// Convert an `ExtractionResult` into a Markdown string with YAML frontmatter.
fn md_doc_to_string(result: ExtractionResult) -> String {
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
fn enrich_date_of_retrieval(out: &mut String, now_iso: &str) {
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

/// Select pipeline based on CLI argument.
fn select_pipeline(name: &str) -> &'static [delulu_webfetch::pipelines::PassFn] {
    match name {
        "tf" => &[delulu_webfetch::pipelines::trafilatura::filter_trafilatura],
        "rd" => &[delulu_webfetch::pipelines::mozilla_readability::filter_mozilla_readability],
        // Defensive branch: unreachable because `--pipeline` is restricted by
        // Clap's `value_parser` to exactly "tf" / "rd".
        _ => unreachable!("pipeline '{}' should have been rejected by Clap", name),
    }
}

// ---------------------------------------------------------------------------
// main
// ---------------------------------------------------------------------------

#[tokio::main]
async fn main() -> Result<(), Error> {
    tracing_subscriber::registry()
        .with(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".to_string().into()))
        .with(
            tracing_subscriber::fmt::layer()
                .with_timer(tracing_subscriber::fmt::time::ChronoUtc::rfc_3339())
                .with_writer(std::io::stderr),
        )
        .init();

    // Try clap subcommand parsing first (for "doc" subcommand)
    if let Ok(cli) = Cli::try_parse()
        && let Some(Command::Doc(doc_args)) = cli.command
    {
        return run_doc(doc_args).await;
    }

    // Fallback to flat args (backward-compatible with prior versions)
    let args = Args::parse();
    run_fetch(args).await
}

async fn run_fetch(args: Args) -> Result<(), Error> {
    if let Some(file) = &args.input_file {
        // File/stdin mode
        let html = if file == "-" {
            let mut buf = String::new();
            std::io::stdin()
                .read_to_string(&mut buf)
                .context("Failed to read from stdin")?;
            buf
        } else {
            std::fs::read_to_string(file).context(format!("Failed to read file '{file}'"))?
        };

        let mut dom = delulu_webfetch::pipelines::parse_html(&html)
            .map_err(|e| anyhow::anyhow!("Parse error: {e}"))?;
        let pipeline = select_pipeline(&args.pipeline);
        for pass in pipeline {
            pass(&mut dom);
        }

        match args.output_format.as_deref() {
            Some("html") => {
                let out_html = delulu_webfetch::generators::gen_html::dom_nodes_to_html(&dom);
                println!("{out_html}");
            }
            None if args.raw => {
                let body = delulu_webfetch::generators::gen_md::MarkdownLowerer::lower(&dom, None);
                let filtered_html_len = body.len();
                let result = delulu_webfetch::ExtractionResult::GenericHtml {
                    content_md: MarkdownDocument {
                        frontmatter: String::new(),
                        body,
                    },
                    raw_html_len: html.len(),
                    filtered_html_len,
                };
                println!("{}", serde_json::to_string_pretty(&result)?);
            }
            Some("markdown") | None => {
                let body = delulu_webfetch::generators::gen_md::MarkdownLowerer::lower(&dom, None);
                let filtered_html_len = body.len();
                let result = delulu_webfetch::ExtractionResult::GenericHtml {
                    content_md: MarkdownDocument {
                        frontmatter: String::new(),
                        body,
                    },
                    raw_html_len: html.len(),
                    filtered_html_len,
                };
                let md = md_doc_to_string(result);
                println!("{md}");
            }
            _ => {
                tracing::warn!("unknown output format, falling back to markdown");
                let body = delulu_webfetch::generators::gen_md::MarkdownLowerer::lower(&dom, None);
                let filtered_html_len = body.len();
                let result = delulu_webfetch::ExtractionResult::GenericHtml {
                    content_md: MarkdownDocument {
                        frontmatter: String::new(),
                        body,
                    },
                    raw_html_len: html.len(),
                    filtered_html_len,
                };
                let md = md_doc_to_string(result);
                println!("{md}");
            }
        }
    } else {
        // URL mode
        let url = args
            .url
            .as_deref()
            .context("Either -u <URL> or -i <FILE> is required")?;

        let crawler = RateLimitedCrawler::builder()
            .with_qps(args.qps)
            .with_max_resp_size(MAX_BODY_SIZE)
            .with_timeout(Duration::from_secs(args.timeout))
            .with_connect_timeout(Duration::from_secs(args.timeout))
            .build()
            .context("Failed to create rate-limited crawler")?;

        match args.output_format.as_deref() {
            Some("html") => {
                let raw_html = fetch_raw_html(url, &crawler)
                    .await
                    .context("Fetch raw HTML failed")?;
                println!("{raw_html}");
            }
            None if args.raw => {
                let result = fetch_and_extract(url, &crawler, select_pipeline(&args.pipeline))
                    .await
                    .context("Fetch and extract failed")?;
                println!("{}", serde_json::to_string_pretty(&result)?);
            }
            Some("markdown") | None => {
                let result = fetch_and_extract(url, &crawler, select_pipeline(&args.pipeline))
                    .await
                    .context("Fetch and extract failed")?;
                let md = md_doc_to_string(result);
                println!("{md}");
            }
            _ => {
                tracing::warn!("unknown output format, falling back to markdown");
                let result = fetch_and_extract(url, &crawler, select_pipeline(&args.pipeline))
                    .await
                    .context("Fetch and extract failed")?;
                let md = md_doc_to_string(result);
                println!("{md}");
            }
        }
    }

    Ok(())
}

async fn run_doc(args: DocArgs) -> Result<(), Error> {
    let crawler = RateLimitedCrawler::builder()
        .with_qps(2)
        .with_max_resp_size(MAX_BODY_SIZE)
        .with_timeout(Duration::from_secs(args.timeout))
        .with_connect_timeout(Duration::from_secs(args.timeout))
        .build()
        .context("Failed to create rate-limited crawler")?;

    match args.output_format.as_deref() {
        Some("html") => {
            let html = delulu_webfetch::fetch_doc_as_html(&args.url, &crawler)
                .await
                .context("Document fetch failed")?;
            println!("{}", html);
        }
        _ => {
            let result = delulu_webfetch::fetch_doc(&args.url, &crawler)
                .await
                .context("Document fetch failed")?;
            let md = md_doc_to_string(result);
            println!("{md}");
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[test]
    fn pipeline_rejects_unknown_value() {
        // Non-tautological: before the `value_parser = ["tf", "rd"]` fix,
        // `--pipeline=banana` parsed OK and silently fell back to trafilatura.
        let err = Args::try_parse_from(["delulu-fetch", "-i", "-", "--pipeline=banana"]);
        assert!(
            err.is_err(),
            "unknown pipeline must be rejected at parse time"
        );
    }

    #[test]
    fn pipeline_accepts_valid_values() {
        for valid in ["tf", "rd"] {
            let args = Args::try_parse_from(["delulu-fetch", "-i", "-", "--pipeline", valid]);
            assert!(args.is_ok(), "valid pipeline '{valid}' should parse");
            assert_eq!(args.unwrap().pipeline, valid);
        }
    }

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
}
