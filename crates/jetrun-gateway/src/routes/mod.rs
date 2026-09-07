pub mod builds;
pub mod pipelines;
pub mod webhooks;
pub mod ws;

use axum::Router;

use crate::state::AppState;

pub fn api_routes() -> Router<AppState> {
    Router::new()
        .nest("/pipelines", pipelines::routes())
        .nest("/builds", builds::routes())
        .nest("/webhooks", webhooks::routes())
        .nest("/ws", ws::routes())
}
