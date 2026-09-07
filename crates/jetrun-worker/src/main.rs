use mimalloc::MiMalloc;
#[global_allocator]
static GLOBAL: MiMalloc = MiMalloc;

use axum::Router;
use tracing_subscriber::EnvFilter;

mod executor;
mod log_stream;
mod state;

use state::AppState;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive("info".parse()?))
        .json()
        .init();

    let state = AppState::new();

    let app = Router::new()
        .route("/health", axum::routing::get(|| async { "ok" }))
        .with_state(state);

    let addr = "0.0.0.0:9002";
    tracing::info!("jetrun-worker listening on {}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
