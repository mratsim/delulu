//! CLI entry point for delulu-fetch.
//!
//! Usage:
//! ```
//! delulu-fetch -u <URL>
//! delulu-fetch -i <FILE>
//! delulu-fetch -u <URL> --output-format json
//! delulu-fetch -i <FILE> --output-format html
//! delulu-fetch -u <URL> --timeout 120
//! echo '<html>...</html>' | delulu-fetch -i -
//! ```

use anyhow::{Context, Error, Result};
use clap::Parser;
use delulu_webfetch::{
    ExtractionResult, MarkdownDocument, RedditComment, WebbfetchClient, fetch_and_extract,
};
use std::io::Read;
use tracing_subscriber::{EnvFilter, layer::SubscriberExt, util::SubscriberInitExt};

// ---------------------------------------------------------------------------
// Args
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

    /// Output format: markdown (default), html, or json.
    #[arg(long)]
    output_format: Option<String>,

    /// [deprecated] Output raw JSON instead of Markdown. Use --output-format json.
    #[arg(long)]
    raw: bool,

    /// Pipeline to use: "rd" (default, Mozilla Readability) or "tf" (Trafilatura).
    #[arg(long, default_value = "rd")]
    pipeline: String,
}

// ---------------------------------------------------------------------------
// Output formatting
// ---------------------------------------------------------------------------

/// Convert an `ExtractionResult` into a Markdown string with YAML frontmatter.
fn md_doc_to_string(result: ExtractionResult) -> String {
    match result {
        ExtractionResult::GenericHtml { content_md } => {
            let mut out = String::new();
            if !content_md.frontmatter.is_empty() {
                out.push_str("---\n");
                out.push_str(&content_md.frontmatter);
                if !content_md.frontmatter.ends_with('\n') {
                    out.push('\n');
                }
                out.push_str("---\n\n");
            }
            out.push_str(&content_md.body);
            out
        }
        ExtractionResult::Reddit {
            title,
            selftext,
            author,
            score,
            permalink,
            comments,
        } => {
            let frontmatter = format!(
                "title: {}\nauthor: {}\nscore: {}\nsource_type: reddit\npermalink: {}",
                title, author, score, permalink
            );
            let mut out = format!("---\n{frontmatter}\n---\n\n");
            out.push_str(&selftext);
            out.push('\n');
            for comment in &comments {
                format_reddit_comment(&mut out, comment, 0);
            }
            out
        }
        ExtractionResult::Discourse {
            title,
            topic_id,
            posts,
        } => {
            let frontmatter = format!(
                "title: {}\ntopic_id: {}\nsource_type: discourse",
                title, topic_id
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
            out
        }
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

/// Select pipeline based on CLI argument.
fn select_pipeline(
    name: &str,
) -> &'static [delulu_webfetch::pipeline::PassFn] {
    match name {
        "rd" | "" => &[delulu_webfetch::pipeline::mozilla_readability::filter_mozilla_readability],
        "tf" => &[delulu_webfetch::pipeline::trafilatura::filter_trafilatura],
        _ => {
            tracing::warn!("unknown pipeline '{}', falling back to default", name);
            &[delulu_webfetch::pipeline::mozilla_readability::filter_mozilla_readability]
        }
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

    let args = Args::parse();

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

        let mut dom = delulu_webfetch::pipeline::parse_html(&html)
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
            Some("json") | None if args.raw => {
                let result = delulu_webfetch::ExtractionResult::GenericHtml {
                    content_md: MarkdownDocument {
                        frontmatter: String::new(),
                        body: delulu_webfetch::generators::gen_md::MarkdownLowerer::lower(
                            &dom, None,
                        ),
                    },
                };
                println!("{}", serde_json::to_string_pretty(&result)?);
            }
            _ => {
                let result = delulu_webfetch::ExtractionResult::GenericHtml {
                    content_md: MarkdownDocument {
                        frontmatter: String::new(),
                        body: delulu_webfetch::generators::gen_md::MarkdownLowerer::lower(
                            &dom, None,
                        ),
                    },
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

        let client = WebbfetchClient::new(args.timeout);

        match args.output_format.as_deref() {
            Some("html") => {
                let fetch_result = client
                    .fetch(url)
                    .await
                    .map_err(|e| anyhow::anyhow!("Fetch error: {e}"))?;
                let body = match &fetch_result.content {
                    ExtractionResult::GenericHtml { content_md } => content_md.body.clone(),
                    _ => anyhow::bail!("Unexpected content type from fetch"),
                };
                let mut dom = delulu_webfetch::pipeline::parse_html(&body)
                    .map_err(|e| anyhow::anyhow!("Parse error: {e}"))?;
                let pipeline = select_pipeline(&args.pipeline);
                for pass in pipeline {
                    pass(&mut dom);
                }
                let out_html = delulu_webfetch::generators::gen_html::dom_nodes_to_html(&dom);
                println!("{out_html}");
            }
            Some("json") | None if args.raw => {
                let result = fetch_and_extract(url, &client, select_pipeline(&args.pipeline))
                    .await
                    .context("Fetch and extract failed")?;
                println!("{}", serde_json::to_string_pretty(&result)?);
            }
            _ => {
                let result = fetch_and_extract(url, &client, select_pipeline(&args.pipeline))
                    .await
                    .context("Fetch and extract failed")?;
                let md = md_doc_to_string(result);
                println!("{md}");
            }
        }
    }

    Ok(())
}
