pub mod api_keys;
pub mod auth;
pub mod orgs;
pub mod roles;
pub mod setup;
pub mod sso;
pub mod teams;
pub mod users;

use axum::Router;

use crate::state::AppState;

pub fn api_routes() -> Router<AppState> {
    // Merge all /auth routes into a single router to avoid axum nest conflicts
    let auth_router = Router::new()
        .merge(setup::routes())
        .merge(auth::routes())
        .merge(users::authenticated_routes())
        .merge(sso::routes());

    Router::new()
        .nest("/auth", auth_router)
        .nest("/auth/users", users::admin_routes())
        .nest("/auth/api-keys", api_keys::routes())
        .nest("/auth/roles", roles::routes())
        .nest("/auth/orgs", orgs::routes())
        .nest("/auth", teams::routes())
}
