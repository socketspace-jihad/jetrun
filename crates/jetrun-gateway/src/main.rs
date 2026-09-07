use mimalloc::MiMalloc;
#[global_allocator]
static GLOBAL: MiMalloc = MiMalloc;

use axum::Router;
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;
use tracing_subscriber::EnvFilter;

mod auth;
mod routes;
mod state;

use state::AppState;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive("info".parse()?))
        .json()
        .init();

    let state = AppState::new();
    routes::webhooks::log_enabled_providers();

    let app = Router::new()
        .route("/", axum::routing::get(|| async {
            axum::Json(serde_json::json!({
                "name": "jetrun",
                "version": env!("CARGO_PKG_VERSION"),
                "status": "running",
                "docs": "/api/v1",
                "health": "/health"
            }))
        }))
        .nest("/api/v1", routes::api_routes())
        .route("/health", axum::routing::get(|| async { "ok" }))
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http())
        .with_state(state);

    let addr = "0.0.0.0:8080";
    tracing::info!("jetrun-gateway listening on {}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
