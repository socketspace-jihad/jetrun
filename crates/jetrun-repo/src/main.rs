use mimalloc::MiMalloc;
#[global_allocator]
static GLOBAL: MiMalloc = MiMalloc;

use std::sync::Arc;

use axum::Router;
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;
use tracing_subscriber::EnvFilter;

mod routes;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive("info".parse()?))
        .json()
        .init();

    // Connect to database
    let database_url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://jetrun:jetrun_dev@localhost:5432/jetrun".into());
    let store = Arc::new(jetrun_store::PgStore::connect(&database_url).await?);

    // Connect to NATS
    let nats_url = std::env::var("NATS_URL").unwrap_or_else(|_| "nats://localhost:4222".into());
    let broker = Arc::new(jetrun_broker::NatsBroker::connect(&nats_url).await
        .map_err(|e| anyhow::anyhow!("NATS connection failed: {}. Start NATS with: docker run -d -p 4222:4222 nats:latest -js", e))?);

    let state = routes::AppState { store, broker };

    let app = Router::new()
        .nest("/api/v1", routes::api_routes())
        .route("/health", axum::routing::get(|| async { "ok" }))
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http())
        .with_state(state);

    let addr = "0.0.0.0:9005";
    tracing::info!("jetrun-repo listening on {}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
