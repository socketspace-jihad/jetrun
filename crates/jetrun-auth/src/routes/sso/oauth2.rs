use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::Redirect,
    routing::get,
    Json, Router,
};
use chrono::Utc;
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

use jetrun_common::models::{AuthProvider, Organization, User};

use crate::services::jwt;
use crate::state::AppState;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/{provider}/authorize", get(authorize))
        .route("/{provider}/callback", get(callback))
        .route("/providers", get(list_providers))
}

/// Returns which SSO providers are compiled in and configured
async fn list_providers() -> Json<Value> {
    let mut providers = Vec::new();

    #[cfg(feature = "sso-google")]
    if std::env::var("GOOGLE_CLIENT_ID").is_ok() {
        providers.push(json!({ "id": "google", "name": "Google" }));
    }

    #[cfg(feature = "sso-github")]
    if std::env::var("GITHUB_CLIENT_ID").is_ok() {
        providers.push(json!({ "id": "github", "name": "GitHub" }));
    }

    #[cfg(feature = "sso-gitlab")]
    if std::env::var("GITLAB_CLIENT_ID").is_ok() {
        providers.push(json!({ "id": "gitlab", "name": "GitLab" }));
    }

    #[cfg(feature = "sso-bitbucket")]
    if std::env::var("BITBUCKET_CLIENT_ID").is_ok() {
        providers.push(json!({ "id": "bitbucket", "name": "Bitbucket" }));
    }

    #[cfg(feature = "sso-apple")]
    if std::env::var("APPLE_CLIENT_ID").is_ok() {
        providers.push(json!({ "id": "apple", "name": "Apple" }));
    }

    Json(json!({ "providers": providers }))
}

/// Redirect user to OAuth2 provider's authorization page
async fn authorize(
    Path(provider): Path<String>,
) -> Result<Redirect, StatusCode> {
    let auth_url = match provider.as_str() {
        #[cfg(feature = "sso-google")]
        "google" => build_auth_url(
            "https://accounts.google.com/o/oauth2/v2/auth",
            "GOOGLE_CLIENT_ID",
            "GOOGLE_REDIRECT_URI",
            "google",
            "openid email profile",
        ),
        #[cfg(feature = "sso-github")]
        "github" => build_auth_url(
            "https://github.com/login/oauth/authorize",
            "GITHUB_CLIENT_ID",
            "GITHUB_REDIRECT_URI",
            "github",
            "user:email read:user",
        ),
        #[cfg(feature = "sso-gitlab")]
        "gitlab" => {
            let base = std::env::var("GITLAB_URL").unwrap_or_else(|_| "https://gitlab.com".into());
            build_auth_url(
                &format!("{}/oauth/authorize", base),
                "GITLAB_CLIENT_ID",
                "GITLAB_REDIRECT_URI",
                "gitlab",
                "openid email profile",
            )
        }
        #[cfg(feature = "sso-bitbucket")]
        "bitbucket" => build_auth_url(
            "https://bitbucket.org/site/oauth2/authorize",
            "BITBUCKET_CLIENT_ID",
            "BITBUCKET_REDIRECT_URI",
            "bitbucket",
            "",
        ),
        #[cfg(feature = "sso-apple")]
        "apple" => build_auth_url(
            "https://appleid.apple.com/auth/authorize",
            "APPLE_CLIENT_ID",
            "APPLE_REDIRECT_URI",
            "apple",
            "name email",
        ),
        _ => return Err(StatusCode::NOT_FOUND),
    };

    match auth_url {
        Some(url) => Ok(Redirect::temporary(&url)),
        None => Err(StatusCode::NOT_IMPLEMENTED),
    }
}

#[derive(Debug, Deserialize)]
struct CallbackParams {
    code: String,
    #[allow(dead_code)]
    state: Option<String>,
}

/// Handle OAuth2 callback — exchange code for token, fetch user info, create/login user
async fn callback(
    State(state): State<AppState>,
    Path(provider): Path<String>,
    Query(params): Query<CallbackParams>,
) -> Result<Json<Value>, StatusCode> {
    // 1. Exchange authorization code for access token
    let token_response = exchange_code(&provider, &params.code).await
        .map_err(|e| {
            tracing::error!(provider = %provider, error = %e, "OAuth2 code exchange failed");
            StatusCode::BAD_GATEWAY
        })?;

    // 2. Fetch user info from provider
    let sso_user = fetch_user_info(&provider, &token_response.access_token).await
        .map_err(|e| {
            tracing::error!(provider = %provider, error = %e, "OAuth2 user info fetch failed");
            StatusCode::BAD_GATEWAY
        })?;

    let auth_provider = match provider.as_str() {
        "google" => AuthProvider::Google,
        "github" => AuthProvider::Github,
        "gitlab" => AuthProvider::Gitlab,
        "bitbucket" => AuthProvider::Bitbucket,
        "apple" => AuthProvider::Apple,
        _ => return Err(StatusCode::BAD_REQUEST),
    };

    // 3. Find or create user
    let user = find_or_create_sso_user(&state, &sso_user, auth_provider);

    // 4. Update last login
    if let Some(mut u) = state.inner.users.get_mut(&user.id) {
        u.last_login_at = Some(Utc::now());
    }

    // 5. Resolve org (use first org, or create default)
    let user_orgs = state.list_user_orgs(user.id);
    let org_id = if let Some((org, _)) = user_orgs.first() {
        org.id
    } else {
        // First SSO login — create a default org
        let org = Organization {
            id: Uuid::new_v4(),
            name: format!("{}'s Org", sso_user.name.as_deref().unwrap_or(&user.username)),
            slug: format!("{}-org", user.username),
            owner_id: user.id,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };
        let oid = org.id;
        state.inner.organizations.insert(oid, org);
        let admin_role = state.find_builtin_role("admin")
            .ok_or(StatusCode::INTERNAL_SERVER_ERROR)?;
        state.add_org_member(oid, user.id, admin_role.id);
        oid
    };

    // 6. Build auth context and issue tokens
    let auth_user = state.build_auth_user(user.id, Some(org_id))
        .ok_or(StatusCode::INTERNAL_SERVER_ERROR)?;

    let access_token = jwt::create_access_token(
        &auth_user,
        &state.config.jwt_secret,
        state.config.jwt_access_ttl_secs,
    ).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let (refresh_token, _) = state.inner.session_store.create_session(
        user.id,
        state.config.jwt_refresh_ttl_days,
        None,
        None,
    );

    let orgs: Vec<Value> = state
        .list_user_orgs(user.id)
        .into_iter()
        .map(|(org, role)| json!({ "id": org.id, "name": org.name, "slug": org.slug, "role": role.display_name }))
        .collect();

    tracing::info!(
        provider = %provider,
        email = %user.email,
        user_id = %user.id,
        "SSO login successful"
    );

    Ok(Json(json!({
        "access_token": access_token,
        "refresh_token": refresh_token,
        "user": {
            "id": user.id,
            "email": user.email,
            "username": user.username,
            "display_name": user.display_name,
            "avatar_url": user.avatar_url,
            "auth_provider": auth_provider,
            "role": auth_user.role,
            "permissions": auth_user.permissions,
        },
        "org": {
            "id": org_id,
        },
        "orgs": orgs,
    })))
}

// ── OAuth2 token exchange ──

#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: String,
    #[allow(dead_code)]
    token_type: Option<String>,
}

async fn exchange_code(provider: &str, code: &str) -> anyhow::Result<TokenResponse> {
    let client = reqwest::Client::new();

    let (token_url, client_id_env, client_secret_env, redirect_uri_env) = match provider {
        "google" => (
            "https://oauth2.googleapis.com/token",
            "GOOGLE_CLIENT_ID", "GOOGLE_CLIENT_SECRET", "GOOGLE_REDIRECT_URI",
        ),
        "github" => (
            "https://github.com/login/oauth/access_token",
            "GITHUB_CLIENT_ID", "GITHUB_CLIENT_SECRET", "GITHUB_REDIRECT_URI",
        ),
        "gitlab" => {
            let base = std::env::var("GITLAB_URL").unwrap_or_else(|_| "https://gitlab.com".into());
            // Leak the string to get a static reference (only called once per callback)
            let url = Box::leak(format!("{}/oauth/token", base).into_boxed_str());
            (url as &str, "GITLAB_CLIENT_ID", "GITLAB_CLIENT_SECRET", "GITLAB_REDIRECT_URI")
        }
        "bitbucket" => (
            "https://bitbucket.org/site/oauth2/access_token",
            "BITBUCKET_CLIENT_ID", "BITBUCKET_CLIENT_SECRET", "BITBUCKET_REDIRECT_URI",
        ),
        "apple" => (
            "https://appleid.apple.com/auth/token",
            "APPLE_CLIENT_ID", "APPLE_CLIENT_SECRET", "APPLE_REDIRECT_URI",
        ),
        _ => anyhow::bail!("unknown provider: {}", provider),
    };

    let client_id = std::env::var(client_id_env)?;
    let client_secret = std::env::var(client_secret_env)?;
    let redirect_uri = std::env::var(redirect_uri_env)
        .unwrap_or_else(|_| format!("http://localhost:9004/api/v1/auth/sso/oauth2/{}/callback", provider));

    let mut req = client
        .post(token_url)
        .form(&[
            ("grant_type", "authorization_code"),
            ("code", code),
            ("client_id", &client_id),
            ("client_secret", &client_secret),
            ("redirect_uri", &redirect_uri),
        ]);

    // GitHub needs Accept: application/json
    if provider == "github" {
        req = req.header("Accept", "application/json");
    }

    let resp = req.send().await?.json::<TokenResponse>().await?;
    Ok(resp)
}

// ── Fetch user info from provider ──

#[derive(Debug, Default)]
struct SsoUserInfo {
    provider_id: String,
    email: Option<String>,
    name: Option<String>,
    avatar_url: Option<String>,
    username: Option<String>,
}

async fn fetch_user_info(provider: &str, access_token: &str) -> anyhow::Result<SsoUserInfo> {
    let client = reqwest::Client::new();

    match provider {
        "google" => {
            #[derive(Deserialize)]
            struct GoogleUser { sub: String, email: Option<String>, name: Option<String>, picture: Option<String> }
            let u: GoogleUser = client
                .get("https://www.googleapis.com/oauth2/v3/userinfo")
                .bearer_auth(access_token)
                .send().await?.json().await?;
            Ok(SsoUserInfo { provider_id: u.sub, email: u.email, name: u.name, avatar_url: u.picture, username: None })
        }
        "github" => {
            #[derive(Deserialize)]
            struct GithubUser { id: u64, login: String, email: Option<String>, name: Option<String>, avatar_url: Option<String> }
            let u: GithubUser = client
                .get("https://api.github.com/user")
                .bearer_auth(access_token)
                .header("User-Agent", "jetrun")
                .send().await?.json().await?;

            // GitHub may not return email in profile — fetch from emails endpoint
            let email = if u.email.is_some() {
                u.email
            } else {
                #[derive(Deserialize)]
                struct GithubEmail { email: String, primary: bool }
                let emails: Vec<GithubEmail> = client
                    .get("https://api.github.com/user/emails")
                    .bearer_auth(access_token)
                    .header("User-Agent", "jetrun")
                    .send().await?.json().await?;
                emails.into_iter().find(|e| e.primary).map(|e| e.email)
            };

            Ok(SsoUserInfo { provider_id: u.id.to_string(), email, name: u.name, avatar_url: u.avatar_url, username: Some(u.login) })
        }
        "gitlab" => {
            #[derive(Deserialize)]
            struct GitlabUser { id: u64, username: String, email: Option<String>, name: Option<String>, avatar_url: Option<String> }
            let u: GitlabUser = client
                .get(&format!("{}/api/v4/user", std::env::var("GITLAB_URL").unwrap_or_else(|_| "https://gitlab.com".into())))
                .bearer_auth(access_token)
                .send().await?.json().await?;
            Ok(SsoUserInfo { provider_id: u.id.to_string(), email: u.email, name: u.name, avatar_url: u.avatar_url, username: Some(u.username) })
        }
        "bitbucket" => {
            #[derive(Deserialize)]
            struct BitbucketUser { uuid: String, username: String, display_name: Option<String> }
            #[derive(Deserialize)]
            struct BitbucketEmail { email: String, is_primary: bool }
            #[derive(Deserialize)]
            struct BitbucketEmailResp { values: Vec<BitbucketEmail> }

            let u: BitbucketUser = client
                .get("https://api.bitbucket.org/2.0/user")
                .bearer_auth(access_token)
                .send().await?.json().await?;

            let emails: BitbucketEmailResp = client
                .get("https://api.bitbucket.org/2.0/user/emails")
                .bearer_auth(access_token)
                .send().await?.json().await?;

            let email = emails.values.into_iter().find(|e| e.is_primary).map(|e| e.email);
            Ok(SsoUserInfo { provider_id: u.uuid, email, name: u.display_name, avatar_url: None, username: Some(u.username) })
        }
        "apple" => {
            // Apple sends user info in the initial callback (id_token), not via a separate API.
            // The access_token here is actually the id_token for Apple.
            // Decode the JWT to extract user info (without full verification for now).
            let parts: Vec<&str> = access_token.split('.').collect();
            if parts.len() >= 2 {
                use base64::Engine;
                let payload = base64::engine::general_purpose::URL_SAFE_NO_PAD
                    .decode(parts[1])
                    .unwrap_or_default();

                #[derive(Deserialize)]
                struct AppleClaims { sub: String, email: Option<String> }

                if let Ok(claims) = serde_json::from_slice::<AppleClaims>(&payload) {
                    return Ok(SsoUserInfo {
                        provider_id: claims.sub,
                        email: claims.email.clone(),
                        name: None,
                        avatar_url: None,
                        username: claims.email,
                    });
                }
            }
            anyhow::bail!("Failed to decode Apple id_token")
        }
        _ => anyhow::bail!("unknown provider: {}", provider),
    }
}

// ── Find or create user from SSO info ──

fn find_or_create_sso_user(state: &AppState, info: &SsoUserInfo, provider: AuthProvider) -> User {
    // Try to find existing user by provider_id
    let existing = state
        .inner
        .users
        .iter()
        .find(|e| {
            let u = e.value();
            u.auth_provider == provider && u.provider_id.as_deref() == Some(&info.provider_id)
        })
        .map(|e| e.value().clone());

    if let Some(mut user) = existing {
        // Update avatar/name if changed
        if let Some(mut u) = state.inner.users.get_mut(&user.id) {
            if info.avatar_url.is_some() {
                u.avatar_url = info.avatar_url.clone();
            }
            if info.name.is_some() {
                u.display_name = info.name.clone();
            }
        }
        user.avatar_url = info.avatar_url.clone().or(user.avatar_url);
        user.display_name = info.name.clone().or(user.display_name);
        return user;
    }

    // Try to find by email (link SSO to existing local account)
    if let Some(email) = &info.email {
        if let Some(existing) = state.find_user_by_email(email) {
            // Update provider info on existing account
            if let Some(mut u) = state.inner.users.get_mut(&existing.id) {
                u.auth_provider = provider;
                u.provider_id = Some(info.provider_id.clone());
                if info.avatar_url.is_some() {
                    u.avatar_url = info.avatar_url.clone();
                }
            }
            return existing;
        }
    }

    // Create new user
    let email = info.email.clone().unwrap_or_else(|| format!("{}@sso.jetrun", info.provider_id));
    let username = info.username.clone()
        .or_else(|| info.name.as_ref().map(|n| n.to_lowercase().replace(' ', "")))
        .unwrap_or_else(|| format!("user_{}", &info.provider_id[..8.min(info.provider_id.len())]));

    // Ensure username uniqueness
    let mut final_username = username.clone();
    let mut counter = 1u32;
    while state.find_user_by_username(&final_username).is_some() {
        final_username = format!("{}{}", username, counter);
        counter += 1;
    }

    let user = User {
        id: Uuid::new_v4(),
        email,
        username: final_username,
        display_name: info.name.clone(),
        avatar_url: info.avatar_url.clone(),
        password_hash: None, // SSO users have no password
        auth_provider: provider,
        provider_id: Some(info.provider_id.clone()),
        is_active: true,
        email_verified: true, // SSO-verified emails are trusted
        last_login_at: Some(Utc::now()),
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };

    state.inner.users.insert(user.id, user.clone());
    user
}

// ── Helpers ──

fn build_auth_url(
    authorize_url: &str,
    client_id_env: &str,
    redirect_uri_env: &str,
    provider: &str,
    scope: &str,
) -> Option<String> {
    let client_id = std::env::var(client_id_env).ok()?;
    let redirect_uri = std::env::var(redirect_uri_env)
        .unwrap_or_else(|_| format!("http://localhost:9004/api/v1/auth/sso/oauth2/{}/callback", provider));

    let mut url = format!(
        "{}?client_id={}&redirect_uri={}&response_type=code",
        authorize_url, client_id, redirect_uri,
    );

    if !scope.is_empty() {
        url.push_str(&format!("&scope={}", scope.replace(' ', "%20")));
    }

    // Apple requires response_mode=form_post
    if provider == "apple" {
        url.push_str("&response_mode=form_post");
    }

    Some(url)
}
