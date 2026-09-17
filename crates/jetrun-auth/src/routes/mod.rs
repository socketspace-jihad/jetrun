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

/// Public routes — no auth required (setup, login, register, SSO)
pub fn public_routes() -> Router<AppState> {
    Router::new()
        .merge(setup::routes())
        .merge(auth::routes())
        .merge(sso::routes())
}

/// Protected routes — require valid JWT or API key
/// Auth middleware must be applied by the caller
pub fn protected_routes() -> Router<AppState> {
    Router::new()
        .merge(users::authenticated_routes())
        .nest("/users", users::admin_routes())
        .nest("/api-keys", api_keys::routes())
        .nest("/roles", roles::routes())
        .nest("/orgs", orgs::routes())
        .merge(teams::routes())
}
