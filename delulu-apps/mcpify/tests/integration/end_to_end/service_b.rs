use axum::{Json, Router, routing::get};
use tokio::net::TcpListener;
use tokio::sync::oneshot;
use tokio::task::JoinHandle;

/// Start a mock service B that serves `GET /items` returning `[{"id":1,"item":"Widget"}]`.
/// Returns `(server_task, shutdown_sender, port)`.
pub async fn start_service_b() -> (JoinHandle<()>, oneshot::Sender<()>, u16) {
    let app = Router::new()
        .route(
            "/items",
            get(|| async { Json(serde_json::json!([{"id":1,"item":"Widget"}])) }),
        )
        .route("/health", get(|| async { "ok" }));
    let l = TcpListener::bind("127.0.0.1:0").await.expect("bind B");
    let p = l.local_addr().expect("addr").port();
    let (tx, rx) = oneshot::channel();
    (
        tokio::spawn(async move {
            axum::serve(l, app)
                .with_graceful_shutdown(async {
                    let _ = rx.await;
                })
                .await
                .ok();
        }),
        tx,
        p,
    )
}
