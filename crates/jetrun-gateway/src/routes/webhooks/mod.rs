#[cfg(feature = "webhook-github")]
pub mod github;
#[cfg(feature = "webhook-gitlab")]
pub mod gitlab;
#[cfg(feature = "webhook-bitbucket")]
pub mod bitbucket;
pub mod generic;

mod verify;

use axum::Router;

use crate::state::AppState;

/// Build webhook routes — only includes providers enabled at compile time.
/// Compile with `--features webhook-github,webhook-gitlab,webhook-bitbucket`
/// or `--features webhook-all` to include all providers.
pub fn routes() -> Router<AppState> {
    let router = Router::new()
        .nest("/generic", generic::routes());

    // Conditionally mount provider routes at compile time
    #[cfg(feature = "webhook-github")]
    let router = router.nest("/github", github::routes());

    #[cfg(feature = "webhook-gitlab")]
    let router = router.nest("/gitlab", gitlab::routes());

    #[cfg(feature = "webhook-bitbucket")]
    let router = router.nest("/bitbucket", bitbucket::routes());

    router
}

/// Log which webhook providers are compiled in (called at startup)
pub fn log_enabled_providers() {
    let mut providers = vec!["generic"];

    #[cfg(feature = "webhook-github")]
    providers.push("github");

    #[cfg(feature = "webhook-gitlab")]
    providers.push("gitlab");

    #[cfg(feature = "webhook-bitbucket")]
    providers.push("bitbucket");

    tracing::info!(
        providers = ?providers,
        "webhook providers enabled (compile-time selection)"
    );
}
