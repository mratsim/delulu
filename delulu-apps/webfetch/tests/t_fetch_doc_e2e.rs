//! End-to-end tests for fetch_doc with real PDF fixtures served from a real HTTP server.
//!
//! Starts an axum server serving compressed PDF fixtures, then calls fetch_doc
//! and verifies the markdown output contains expected content from the real PDFs.

use std::sync::Arc;

use anyhow::{Context, Result};
use delulu_rate_limited_crawler::RateLimitedCrawler;
use delulu_webfetch::fetch_doc;
use std::time::Duration;

/// Start a local axum server that serves a PDF fixture at /test.pdf.
/// Returns the fixture URL and a shutdown sender.
async fn serve_pdf_fixture(
    zst_path: &str,
) -> Result<(String, tokio::sync::oneshot::Sender<()>), anyhow::Error> {
    let data =
        std::fs::read(zst_path).with_context(|| format!("Failed to read fixture: {zst_path}"))?;
    let pdf_bytes =
        Arc::new(zstd::decode_all(data.as_slice()).context("Failed to decompress fixture")?);

    let app = axum::Router::new().route(
        "/test.pdf",
        axum::routing::get(move || {
            let pdf = pdf_bytes.clone();
            async move {
                axum::response::Response::builder()
                    .header("content-type", "application/pdf")
                    .body(axum::body::Body::from(pdf.to_vec()))
                    .unwrap()
            }
        }),
    );

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let fixture_url = format!("http://{}/test.pdf", addr);

    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
    tokio::spawn(async move {
        let server = axum::serve(listener, app);
        let _ = server
            .with_graceful_shutdown(async {
                shutdown_rx.await.ok();
            })
            .await;
    });

    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    Ok((fixture_url, shutdown_tx))
}

fn fixture_path(name: &str) -> String {
    let manifest = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest
        .join("tests/fixtures-webfetch/pdf")
        .join(name)
        .to_string_lossy()
        .to_string()
}

fn test_crawler() -> RateLimitedCrawler {
    RateLimitedCrawler::builder()
        .with_qps(100)
        .with_timeout(Duration::from_secs(30))
        .with_connect_timeout(Duration::from_secs(5))
        .build()
        .expect("Failed to create test crawler")
}

#[tokio::test]
async fn test_fetch_doc_iacr_2010_354() -> Result<()> {
    let (url, _shutdown) = serve_pdf_fixture(&fixture_path("iacr-2010-354.pdf.zst")).await?;
    let crawler = test_crawler();
    let result = fetch_doc(&url, &crawler).await?;

    match result {
        delulu_webfetch::ExtractionResult::GenericHtml { content_md, .. } => {
            assert!(
                content_md.body.len() > 100,
                "Output too short: {} chars",
                content_md.body.len()
            );
            assert!(content_md.frontmatter.contains("source_type: document"));
            assert!(
                content_md.body.contains("efficient")
                    || content_md.body.contains("protocol")
                    || content_md.body.contains("scheme")
                    || content_md.body.contains("signature"),
                "Expected paper content in output:\n{}",
                &content_md.body[..300.min(content_md.body.len())]
            );
        }
        _ => panic!("Expected GenericHtml result"),
    }
    Ok(())
}

#[tokio::test]
async fn test_fetch_doc_iacr_2023_kzg() -> Result<()> {
    let (url, _shutdown) = serve_pdf_fixture(&fixture_path("iacr-2023-033-kzg.pdf.zst")).await?;
    let crawler = test_crawler();
    let result = fetch_doc(&url, &crawler).await?;

    match result {
        delulu_webfetch::ExtractionResult::GenericHtml { content_md, .. } => {
            assert!(
                content_md.body.len() > 200,
                "Output too short: {} chars",
                content_md.body.len()
            );
            assert!(
                content_md.body.contains("KZG")
                    || content_md.body.contains("proof")
                    || content_md.body.contains("commitment"),
                "Expected KZG paper content:\n{}",
                &content_md.body[..300.min(content_md.body.len())]
            );
        }
        _ => panic!("Expected GenericHtml result"),
    }
    Ok(())
}

#[tokio::test]
async fn test_fetch_doc_iacr_2023_das() -> Result<()> {
    let (url, _shutdown) = serve_pdf_fixture(&fixture_path("iacr-2023-1079-das.pdf.zst")).await?;
    let crawler = test_crawler();
    let result = fetch_doc(&url, &crawler).await?;

    match result {
        delulu_webfetch::ExtractionResult::GenericHtml { content_md, .. } => {
            assert!(
                content_md.body.len() > 200,
                "Output too short: {} chars",
                content_md.body.len()
            );
            assert!(
                content_md.body.contains("Data Availability")
                    || content_md.body.contains("sampling")
                    || content_md.body.contains("DA")
                    || content_md.body.contains("availability"),
                "Expected DAS paper content:\n{}",
                &content_md.body[..300.min(content_md.body.len())]
            );
        }
        _ => panic!("Expected GenericHtml result"),
    }
    Ok(())
}

#[tokio::test]
async fn test_fetch_doc_pubmed_alphafold3() -> Result<()> {
    let (url, _shutdown) =
        serve_pdf_fixture(&fixture_path("pubmed-2024-alphafold3.pdf.zst")).await?;
    let crawler = test_crawler();
    let result = fetch_doc(&url, &crawler).await?;

    match result {
        delulu_webfetch::ExtractionResult::GenericHtml { content_md, .. } => {
            assert!(
                content_md.body.len() > 200,
                "Output too short: {} chars",
                content_md.body.len()
            );
            assert!(
                content_md.body.contains("AlphaFold") || content_md.body.contains("protein"),
                "Expected AlphaFold paper content:\n{}",
                &content_md.body[..300.min(content_md.body.len())]
            );
        }
        _ => panic!("Expected GenericHtml result"),
    }
    Ok(())
}
