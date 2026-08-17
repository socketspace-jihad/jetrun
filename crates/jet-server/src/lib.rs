//! The jetrun control plane.
//!
//! Two listeners, deliberately on two ports:
//!
//! * **HTTP/JSON** ([`http`]) -- the public surface. VCS webhooks, the API, and
//!   eventually the web UI. Must face the internet, so it is written to fail
//!   closed.
//! * **JRP** ([`jrp`]) -- the internal surface. The `jet` CLI and workers, over
//!   the custom TCP protocol in `jet-proto`. Should be reachable only from trusted
//!   networks, which is far easier to enforce as a separate port than as a path.
//!
//! Splitting them is a security boundary, not organization: the two have
//! completely different exposure and completely different authentication models.

pub mod http;
pub mod jrp;
pub mod state;
pub mod webhook;

pub use state::{ServerState, SharedState, WebhookSecrets};

/// Run both listeners until shutdown.
pub async fn serve(
    state: SharedState,
    http_addr: std::net::SocketAddr,
    jrp_addr: std::net::SocketAddr,
    shutdown: tokio::sync::watch::Receiver<bool>,
) -> std::io::Result<()> {
    let http_listener = tokio::net::TcpListener::bind(http_addr).await?;
    let jrp_listener = tokio::net::TcpListener::bind(jrp_addr).await?;

    tracing::info!(%http_addr, %jrp_addr, "jetrun server starting");

    let app = http::router(std::sync::Arc::clone(&state));
    let mut http_shutdown = shutdown.clone();

    let http_task = tokio::spawn(async move {
        axum::serve(http_listener, app)
            .with_graceful_shutdown(async move {
                // Ignore a closed channel: the sender going away is itself a
                // shutdown signal.
                while http_shutdown.changed().await.is_ok() {
                    if *http_shutdown.borrow() {
                        break;
                    }
                }
            })
            .await
    });

    let jrp_task = tokio::spawn(jrp::serve(jrp_listener, state, shutdown));

    // Either listener failing takes the process down: running the control plane
    // with half its surface silently missing is worse than exiting, because a
    // dead JRP port looks like every worker having network trouble.
    let (h, j) = tokio::join!(http_task, jrp_task);
    h.map_err(std::io::Error::other)??;
    j.map_err(std::io::Error::other)??;
    Ok(())
}
