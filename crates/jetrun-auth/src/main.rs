use mimalloc::MiMalloc;
#[global_allocator]
static GLOBAL: MiMalloc = MiMalloc;

use std::sync::Arc;

use axum::{middleware as axum_mw, Router};
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
use middleware::extract::auth_middleware;
use state::AppState;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive("info".parse()?))
        .json()
        .init();

    let config = AuthServiceConfig::from_env();
    let addr = format!("{}:{}", config.host, config.port);

    let database_url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://jetrun:jetrun_dev@localhost:5432/jetrun".into());

    let store = Arc::new(
        jetrun_store::PgStore::connect(&database_url).await?
    );

    let state = AppState::new(store, config);

    seed::seed(&state).await;
    routes::sso::log_enabled_providers();

    // Protected routes get auth middleware that injects AuthUser into extensions
    let protected = Router::new()
        .nest("/auth", routes::protected_routes())
        .layer(axum_mw::from_fn_with_state(state.clone(), auth_middleware));

    let app = Router::new()
        // Public auth routes (setup, login, register, SSO)
        .nest("/api/v1/auth", routes::public_routes())
        // Protected routes (me, users, api-keys, roles, orgs, teams)
        .nest("/api/v1", protected)
        .route("/health", axum::routing::get(|| async { "ok" }))
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http())
        .with_state(state);

    tracing::info!("jetrun-auth listening on {}", addr);

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
