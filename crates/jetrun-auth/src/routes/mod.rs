pub mod api_keys;
pub mod auth;
pub mod orgs;
pub mod roles;
pub mod sso;
pub mod teams;
pub mod users;

use axum::Router;

use crate::state::AppState;

pub fn api_routes() -> Router<AppState> {
    Router::new()
        .nest("/auth", auth::routes())
        .nest("/auth", users::authenticated_routes())
        .nest("/auth/users", users::admin_routes())
        .nest("/auth/api-keys", api_keys::routes())
        .nest("/auth/roles", roles::routes())
        .nest("/auth/orgs", orgs::routes())
        .nest("/auth/sso", sso::routes())
}
