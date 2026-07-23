//! Shared test utilities for paper-search integration tests.
//!
//! Provides fixture loading (`.zst` decompression) and a local HTTP server
//! that serves fixture content at configurable paths.

use std::path::PathBuf;
use std::sync::Arc;

use axum::http::StatusCode;
use axum::routing::get;
use axum::Router;

/// Find the workspace root by walking up from the manifest dir
/// until we find a Cargo.toml containing `[workspace]`.
pub fn workspace_root() -> PathBuf {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let mut dir = Some(manifest.as_path());
    while let Some(d) = dir {
        let cargo_toml = d.join("Cargo.toml");
        if cargo_toml.exists() {
            if let Ok(content) = std::fs::read_to_string(&cargo_toml) {
                if content.contains("[workspace]") {
                    return d.to_path_buf();
                }
            }
        }
        dir = d.parent();
    }
    manifest
}

/// Path to a crate's fixture directory: `delulu-apps/<crate>/tests/fixtures/`
pub fn fixture_dir(crate_name: &str) -> PathBuf {
    workspace_root()
        .join("delulu-apps")
        .join(crate_name)
        .join("tests")
        .join("fixtures")
}

/// Return the path to a `.zst` fixture file for the given crate.
pub fn fixture_path(crate_name: &str, name: &str) -> PathBuf {
    fixture_dir(crate_name).join(name)
}

/// Spawn a local HTTP server that serves a `.zst` fixture at a given route path.
/// Decompresses on each request — no pre-decompressed content held in memory.
pub async fn serve_fixture(route: &str, zst_path: PathBuf) -> (String, tokio::sync::oneshot::Sender<()>) {
    let zst_path = Arc::new(zst_path);
    let route_str = route.to_string();

    let app = Router::new().route(
        &route_str,
        get(move || {
            let zst_path = zst_path.clone();
            async move {
                match std::fs::read(&*zst_path) {
                    Ok(compressed) => {
                        match zstd::decode_all(compressed.as_slice()) {
                            Ok(decompressed) => {
                                match String::from_utf8(decompressed) {
                                    Ok(body) => (StatusCode::OK, body),
                                    Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, format!("fixture not UTF-8: {e}")),
                                }
                            }
                            Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, format!("zstd error: {e}")),
                        }
                    }
                    Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, format!("read error: {e}")),
                }
            }
        }),
    );

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("failed to bind test server");
    let addr = listener.local_addr().unwrap();
    let url = format!("http://{}", addr);

    let (tx, rx) = tokio::sync::oneshot::channel::<()>();

    tokio::spawn(async move {
        axum::serve(listener, app)
            .with_graceful_shutdown(async { rx.await.ok(); })
            .await
            .ok();
    });

    (url, tx)
}
