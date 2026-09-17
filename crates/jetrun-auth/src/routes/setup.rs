use axum::{
    extract::State,
    http::StatusCode,
    routing::{get, post},
    Json, Router,
};
use chrono::Utc;
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

use jetrun_common::models::{AuthProvider, Organization, User};

use crate::services::{jwt, password, session};
use crate::state::AppState;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/setup/status", get(setup_status))
        .route("/setup", post(initial_setup))
}

/// Check if the platform has been set up.
/// Setup is complete when at least one organization exists
/// (the seeded super admin user doesn't count — an org means someone completed onboarding).
async fn setup_status(State(state): State<AppState>) -> Json<Value> {
    let has_orgs = state.store.has_any_org().await.unwrap_or(false);

    Json(json!({
        "setup_completed": has_orgs,
    }))
}

#[derive(Debug, Deserialize)]
struct SetupRequest {
    org_name: String,
    email: String,
    password: String,
}

/// One-time initial setup. Creates the first org + admin user.
/// Rejects if any org already exists (setup already done).
async fn initial_setup(
    State(state): State<AppState>,
    Json(req): Json<SetupRequest>,
) -> Result<Json<Value>, StatusCode> {
    // Guard: only works when no org exists (fresh deploy or reset)
    if state.store.has_any_org().await.unwrap_or(false) {
        return Ok(Json(json!({
            "error": "Setup already completed. Use /auth/register or /auth/login instead."
        })));
    }

    if req.password.len() < 8 {
        return Ok(Json(json!({ "error": "Password must be at least 8 characters" })));
    }

    let password_hash = password::hash_password(&req.password)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Derive username from email
    let username = req.email
        .split('@')
        .next()
        .unwrap_or("admin")
        .to_lowercase()
        .replace(|c: char| !c.is_alphanumeric() && c != '_' && c != '-', "");

    // 1. Create user
    let user = User {
        id: Uuid::new_v4(),
        email: req.email,
        username,
        display_name: None,
        avatar_url: None,
        password_hash: Some(password_hash),
        auth_provider: AuthProvider::Local,
        provider_id: None,
        is_active: true,
        email_verified: true, // first user is trusted
        last_login_at: Some(Utc::now()),
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };
    let user_id = user.id;
    state.store.create_user(&user).await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // 2. Create org
    let org_slug = req.org_name
        .to_lowercase()
        .replace(|c: char| !c.is_alphanumeric() && c != '-', "-")
        .trim_matches('-')
        .to_string();

    let org = Organization {
        id: Uuid::new_v4(),
        name: req.org_name,
        slug: org_slug,
        owner_id: user_id,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };
    let org_id = org.id;
    state.store.create_org(&org).await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // 3. Assign admin role (not just developer — this is the platform owner)
    let admin_role = state
        .find_builtin_role("admin").await
        .ok_or(StatusCode::INTERNAL_SERVER_ERROR)?;
    state.add_org_member(org_id, user_id, admin_role.id).await;

    // 4. Issue tokens
    let auth_user = state
        .build_auth_user(user_id, Some(org_id)).await
        .ok_or(StatusCode::INTERNAL_SERVER_ERROR)?;

    let access_token = jwt::create_access_token(
        &auth_user,
        &state.config.jwt_secret,
        state.config.jwt_access_ttl_secs,
    )
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let (refresh_token, _) = session::create_session(
        state.store.as_ref(),
        user_id,
        state.config.jwt_refresh_ttl_days,
        None,
        None,
    ).await.ok_or(StatusCode::INTERNAL_SERVER_ERROR)?;

    tracing::info!(
        email = %user.email,
        org = %org.name,
        "initial setup completed — first admin created"
    );

    Ok(Json(json!({
        "setup_completed": true,
        "access_token": access_token,
        "refresh_token": refresh_token,
        "user": {
            "id": user.id,
            "email": user.email,
            "username": user.username,
            "role": auth_user.role,
            "permissions": auth_user.permissions,
        },
        "org": {
            "id": org.id,
            "name": org.name,
            "slug": org.slug,
        },
    })))
}
