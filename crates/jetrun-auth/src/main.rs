use mimalloc::MiMalloc;
#[global_allocator]
static GLOBAL: MiMalloc = MiMalloc;

use axum::Router;
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;
use tracing_subscriber::EnvFilter;

mod config;
mod grpc;
mod middleware;
mod routes;
mod seed;
mod services;
mod state;

use config::AuthServiceConfig;
use state::AppState;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive("info".parse()?))
        .json()
        .init();

    let config = AuthServiceConfig::from_env();
    let addr = format!("{}:{}", config.host, config.port);

    let state = AppState::new(config);

    // Seed built-in roles, permissions, and super admin
    seed::seed(&state);
    routes::sso::log_enabled_providers();

    let app = Router::new()
        .nest("/api/v1", routes::api_routes())
        .route("/health", axum::routing::get(|| async { "ok" }))
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http())
        .with_state(state);

    tracing::info!("jetrun-auth listening on {}", addr);

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
