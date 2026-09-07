#[cfg(any(
    feature = "sso-google",
    feature = "sso-github",
    feature = "sso-gitlab",
    feature = "sso-bitbucket",
    feature = "sso-apple"
))]
pub mod oauth2;

use axum::Router;

use crate::state::AppState;

pub fn routes() -> Router<AppState> {
    let router = Router::new();

    #[cfg(any(
        feature = "sso-google",
        feature = "sso-github",
        feature = "sso-gitlab",
        feature = "sso-bitbucket",
        feature = "sso-apple"
    ))]
    let router = router.nest("/oauth2", oauth2::routes());

    router
}

pub fn log_enabled_providers() {
    let mut providers: Vec<&str> = Vec::new();

    #[cfg(feature = "sso-google")]
    providers.push("google");

    #[cfg(feature = "sso-github")]
    providers.push("github");

    #[cfg(feature = "sso-gitlab")]
    providers.push("gitlab");

    #[cfg(feature = "sso-bitbucket")]
    providers.push("bitbucket");

    #[cfg(feature = "sso-apple")]
    providers.push("apple");

    if providers.is_empty() {
        tracing::info!("SSO providers: none (local auth only, enable with --features sso-all)");
    } else {
        tracing::info!(providers = ?providers, "SSO providers enabled");
    }
}
