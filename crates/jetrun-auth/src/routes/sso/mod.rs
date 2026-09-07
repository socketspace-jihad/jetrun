#[cfg(any(feature = "sso-google", feature = "sso-github", feature = "sso-gitlab"))]
pub mod oauth2;

use axum::Router;

use crate::state::AppState;

pub fn routes() -> Router<AppState> {
    let router = Router::new();

    #[cfg(any(feature = "sso-google", feature = "sso-github", feature = "sso-gitlab"))]
    let router = router.nest("/oauth2", oauth2::routes());

    router
}

/// Log which SSO providers are compiled in
pub fn log_enabled_providers() {
    let mut providers: Vec<&str> = Vec::new();

    #[cfg(feature = "sso-google")]
    providers.push("google");

    #[cfg(feature = "sso-github")]
    providers.push("github");

    #[cfg(feature = "sso-gitlab")]
    providers.push("gitlab");

    if providers.is_empty() {
        tracing::info!("SSO providers: none (local auth only)");
    } else {
        tracing::info!(providers = ?providers, "SSO providers enabled");
    }
}
