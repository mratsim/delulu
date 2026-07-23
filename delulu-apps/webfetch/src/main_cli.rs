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
use clap::Parser;
use delulu_rate_limited_crawler::RateLimitedCrawler;
use delulu_webfetch::{
    ExtractionResult, MarkdownDocument, RedditComment, MAX_BODY_SIZE, fetch_and_extract,
};
use std::io::Read;
use std::time::Duration;
use tracing_subscriber::{EnvFilter, layer::SubscriberExt, util::SubscriberInitExt};

// ---------------------------------------------------------------------------
// Args (flat, backward-compatible)
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

    /// Pipeline to use: "rd" (default, Mozilla Readability) or "tf" (Trafilatura).
    #[arg(long, default_value = "rd")]
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
fn select_pipeline(name: &str) -> &'static [delulu_webfetch::pipelines::PassFn] {
    match name {
        "rd" | "" => &[delulu_webfetch::pipelines::mozilla_readability::filter_mozilla_readability],
        "tf" => &[delulu_webfetch::pipelines::trafilatura::filter_trafilatura],
        _ => {
            tracing::warn!("unknown pipeline '{}', falling back to default", name);
            &[delulu_webfetch::pipelines::mozilla_readability::filter_mozilla_readability]
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

    // Check if the first argument is "doc" to dispatch to doc subcommand
    let args: Vec<String> = std::env::args().collect();
    let is_doc = args.get(1).map(|s| s == "doc").unwrap_or(false);

    if is_doc {
        // Strip the "doc" subcommand name — clap's parse_from expects
        // [program_name, url, ...], not [program_name, subcommand, url, ...]
        let mut doc_argv = vec![args[0].clone()]; // program name
        doc_argv.extend(args.iter().skip(2).cloned()); // skip "doc"
        let doc_args = DocArgs::parse_from(&doc_argv);
        return run_doc(doc_args).await;
    } else {
        // Use original flat-args parsing (backward compatible)
        let args = Args::parse();
        run_fetch(args).await
    }
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
            Some("markdown") | None => {
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
            _ => {
                tracing::warn!("unknown output format, falling back to markdown");
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

        let crawler = RateLimitedCrawler::builder()
            .with_qps(args.qps)
            .with_max_resp_size(MAX_BODY_SIZE)
            .with_timeout(Duration::from_secs(args.timeout))
            .with_connect_timeout(Duration::from_secs(args.timeout))
            .build()
            .context("Failed to create rate-limited crawler")?;

        match args.output_format.as_deref() {
            Some("html") => {
                let fetch_result = fetch_and_extract(url, &crawler, select_pipeline(&args.pipeline))
                    .await
                    .context("Fetch and extract failed")?;
                let body = match &fetch_result {
                    ExtractionResult::GenericHtml { content_md } => content_md.body.clone(),
                    ExtractionResult::Reddit { selftext, .. } => selftext.clone(),
                    ExtractionResult::Discourse { .. } => {
                        anyhow::bail!("HTML output not supported for Discourse results")
                    }
                };
                let mut dom = delulu_webfetch::pipelines::parse_html(&body)
                    .map_err(|e| anyhow::anyhow!("Parse error: {e}"))?;
                let pipeline = select_pipeline(&args.pipeline);
                for pass in pipeline {
                    pass(&mut dom);
                }
                let out_html = delulu_webfetch::generators::gen_html::dom_nodes_to_html(&dom);
                println!("{out_html}");
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

    let result = delulu_webfetch::fetch_doc(&args.url, &crawler)
        .await
        .context("Document fetch failed")?;

    match args.output_format.as_deref() {
        Some("html") => {
            if let ExtractionResult::GenericHtml { content_md } = &result {
                println!("{}", content_md.body);
            }
        }
        _ => {
            let md = md_doc_to_string(result);
            println!("{md}");
        }
    }

    Ok(())
}
