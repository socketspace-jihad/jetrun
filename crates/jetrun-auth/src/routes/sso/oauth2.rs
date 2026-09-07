use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::Redirect,
    routing::get,
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::state::AppState;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/{provider}/authorize", get(authorize))
        .route("/{provider}/callback", get(callback))
        .route("/providers", get(list_providers))
}

/// Returns which OAuth2 providers are available
async fn list_providers() -> Json<Value> {
    let mut providers = Vec::new();

    #[cfg(feature = "sso-google")]
    providers.push("google");

    #[cfg(feature = "sso-github")]
    providers.push("github");

    #[cfg(feature = "sso-gitlab")]
    providers.push("gitlab");

    Json(json!({ "providers": providers }))
}

/// Redirect user to OAuth2 provider's authorization page
async fn authorize(
    State(_state): State<AppState>,
    Path(provider): Path<String>,
) -> Result<Redirect, StatusCode> {
    let auth_url = match provider.as_str() {
        #[cfg(feature = "sso-google")]
        "google" => build_google_auth_url(),
        #[cfg(feature = "sso-github")]
        "github" => build_github_auth_url(),
        #[cfg(feature = "sso-gitlab")]
        "gitlab" => build_gitlab_auth_url(),
        _ => return Err(StatusCode::NOT_FOUND),
    };

    match auth_url {
        Some(url) => Ok(Redirect::temporary(&url)),
        None => {
            tracing::warn!(provider = %provider, "SSO provider not configured");
            Err(StatusCode::NOT_IMPLEMENTED)
        }
    }
}

#[derive(Debug, Deserialize)]
struct CallbackParams {
    code: String,
    #[allow(dead_code)]
    state: Option<String>,
}

/// Handle OAuth2 callback — exchange code for token, create/login user
async fn callback(
    State(_state): State<AppState>,
    Path(provider): Path<String>,
    Query(params): Query<CallbackParams>,
) -> Json<Value> {
    // Stub: In production, this would:
    // 1. Exchange the authorization code for an access token
    // 2. Fetch user info from the provider
    // 3. Find or create user in our DB
    // 4. Issue JWT + refresh token
    tracing::info!(
        provider = %provider,
        code_len = params.code.len(),
        "OAuth2 callback received — user creation/login not yet wired"
    );

    Json(json!({
        "status": "callback_received",
        "provider": provider,
        "message": "OAuth2 flow not yet fully implemented — configure provider credentials"
    }))
}

// --- Provider-specific URL builders ---

#[cfg(feature = "sso-google")]
fn build_google_auth_url() -> Option<String> {
    let client_id = std::env::var("GOOGLE_CLIENT_ID").ok()?;
    let redirect_uri = std::env::var("GOOGLE_REDIRECT_URI")
        .unwrap_or_else(|_| "http://localhost:9004/auth/sso/oauth2/google/callback".into());

    Some(format!(
        "https://accounts.google.com/o/oauth2/v2/auth?client_id={}&redirect_uri={}&response_type=code&scope=openid%20email%20profile",
        client_id, redirect_uri
    ))
}

#[cfg(feature = "sso-github")]
fn build_github_auth_url() -> Option<String> {
    let client_id = std::env::var("GITHUB_CLIENT_ID").ok()?;
    let redirect_uri = std::env::var("GITHUB_REDIRECT_URI")
        .unwrap_or_else(|_| "http://localhost:9004/auth/sso/oauth2/github/callback".into());

    Some(format!(
        "https://github.com/login/oauth/authorize?client_id={}&redirect_uri={}&scope=user:email",
        client_id, redirect_uri
    ))
}

#[cfg(feature = "sso-gitlab")]
fn build_gitlab_auth_url() -> Option<String> {
    let client_id = std::env::var("GITLAB_CLIENT_ID").ok()?;
    let gitlab_url = std::env::var("GITLAB_URL").unwrap_or_else(|_| "https://gitlab.com".into());
    let redirect_uri = std::env::var("GITLAB_REDIRECT_URI")
        .unwrap_or_else(|_| "http://localhost:9004/auth/sso/oauth2/gitlab/callback".into());

    Some(format!(
        "{}/oauth/authorize?client_id={}&redirect_uri={}&response_type=code&scope=openid+email+profile",
        gitlab_url, client_id, redirect_uri
    ))
}
