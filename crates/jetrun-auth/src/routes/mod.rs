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
    Router::new()
        // Setup (first deploy onboarding — public, one-time)
        .nest("/auth", setup::routes())
        // Public auth endpoints (register, login, refresh, logout, switch-org)
        .nest("/auth", auth::routes())
        // Authenticated user profile (/me, /me/password, /me/sessions)
        .nest("/auth", users::authenticated_routes())
        // Admin user management (org-scoped)
        .nest("/auth/users", users::admin_routes())
        // API key management
        .nest("/auth/api-keys", api_keys::routes())
        // Role + permission listing
        .nest("/auth/roles", roles::routes())
        // Organization management
        .nest("/auth/orgs", orgs::routes())
        // Team management (org-scoped)
        .nest("/auth", teams::routes())
        // SSO (feature-gated)
        .nest("/auth/sso", sso::routes())
}
