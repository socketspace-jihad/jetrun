use axum::{
    extract::State,
    http::StatusCode,
    routing::post,
    Json, Router,
};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use uuid::Uuid;

use jetrun_common::models::{AuthProvider, AuthUser, User};

use crate::services::{jwt, password};
use crate::state::AppState;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/register", post(register))
        .route("/login", post(login))
        .route("/refresh", post(refresh))
        .route("/logout", post(logout))
}

#[derive(Debug, Deserialize)]
struct RegisterRequest {
    email: String,
    username: String,
    password: String,
    display_name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct LoginRequest {
    email: String,
    password: String,
}

#[derive(Debug, Deserialize)]
struct RefreshRequest {
    refresh_token: String,
}

#[derive(Debug, Serialize)]
struct AuthResponse {
    access_token: String,
    refresh_token: String,
    user: UserResponse,
}

#[derive(Debug, Serialize)]
struct UserResponse {
    id: Uuid,
    email: String,
    username: String,
    display_name: Option<String>,
    avatar_url: Option<String>,
    role: String,
    permissions: Vec<String>,
}

async fn register(
    State(state): State<AppState>,
    Json(req): Json<RegisterRequest>,
) -> Result<Json<Value>, StatusCode> {
    // Check if email already exists
    if state.find_user_by_email(&req.email).is_some() {
        return Ok(Json(json!({ "error": "Email already registered" })));
    }

    // Check if username already exists
    if state.find_user_by_username(&req.username).is_some() {
        return Ok(Json(json!({ "error": "Username already taken" })));
    }

    // Hash password
    let password_hash = password::hash_password(&req.password)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let user = User {
        id: Uuid::new_v4(),
        email: req.email,
        username: req.username,
        display_name: req.display_name,
        avatar_url: None,
        password_hash: Some(password_hash),
        auth_provider: AuthProvider::Local,
        provider_id: None,
        is_active: true,
        email_verified: false,
        last_login_at: None,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };

    let user_id = user.id;
    state.inner.users.insert(user_id, user.clone());

    // Default role: developer
    let role = state
        .inner
        .roles
        .iter()
        .find(|r| r.value().name == "developer" && r.value().is_builtin)
        .map(|r| r.value().clone());

    let (role_name, permissions) = if let Some(role) = &role {
        let perms = state.get_role_permission_names(role.id);
        (role.name.clone(), perms)
    } else {
        ("developer".into(), vec![])
    };

    let auth_user = AuthUser {
        user_id,
        email: user.email.clone(),
        username: user.username.clone(),
        org_id: None,
        role: role_name.clone(),
        permissions: permissions.clone(),
    };

    // Create tokens
    let access_token = jwt::create_access_token(
        &auth_user,
        &state.config.jwt_secret,
        state.config.jwt_access_ttl_secs,
    )
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let (refresh_token, _session) = state.inner.session_store.create_session(
        user_id,
        state.config.jwt_refresh_ttl_days,
        None,
        None,
    );

    Ok(Json(json!(AuthResponse {
        access_token,
        refresh_token,
        user: UserResponse {
            id: user_id,
            email: user.email,
            username: user.username,
            display_name: user.display_name,
            avatar_url: None,
            role: role_name,
            permissions,
        },
    })))
}

async fn login(
    State(state): State<AppState>,
    Json(req): Json<LoginRequest>,
) -> Result<Json<Value>, StatusCode> {
    // Find user by email
    let user = state
        .find_user_by_email(&req.email)
        .ok_or(StatusCode::UNAUTHORIZED)?;

    if !user.is_active {
        return Err(StatusCode::UNAUTHORIZED);
    }

    // Verify password
    let password_hash = user.password_hash.as_ref().ok_or(StatusCode::UNAUTHORIZED)?;
    let valid = password::verify_password(&req.password, password_hash)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    if !valid {
        return Err(StatusCode::UNAUTHORIZED);
    }

    // Update last login
    if let Some(mut u) = state.inner.users.get_mut(&user.id) {
        u.last_login_at = Some(Utc::now());
    }

    // Resolve role and permissions
    let role = state.get_user_role(user.id);
    let (role_name, permissions) = if let Some(role) = &role {
        let perms = state.get_role_permission_names(role.id);
        (role.name.clone(), perms)
    } else {
        ("viewer".into(), vec![])
    };

    let auth_user = AuthUser {
        user_id: user.id,
        email: user.email.clone(),
        username: user.username.clone(),
        org_id: None,
        role: role_name.clone(),
        permissions: permissions.clone(),
    };

    let access_token = jwt::create_access_token(
        &auth_user,
        &state.config.jwt_secret,
        state.config.jwt_access_ttl_secs,
    )
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let (refresh_token, _session) = state.inner.session_store.create_session(
        user.id,
        state.config.jwt_refresh_ttl_days,
        None,
        None,
    );

    Ok(Json(json!(AuthResponse {
        access_token,
        refresh_token,
        user: UserResponse {
            id: user.id,
            email: user.email,
            username: user.username,
            display_name: user.display_name,
            avatar_url: user.avatar_url,
            role: role_name,
            permissions,
        },
    })))
}

async fn refresh(
    State(state): State<AppState>,
    Json(req): Json<RefreshRequest>,
) -> Result<Json<Value>, StatusCode> {
    // Rotate the refresh token
    let (new_refresh_token, session) = state
        .inner
        .session_store
        .rotate_refresh_token(&req.refresh_token, state.config.jwt_refresh_ttl_days)
        .ok_or(StatusCode::UNAUTHORIZED)?;

    // Get user
    let user = state
        .inner
        .users
        .get(&session.user_id)
        .map(|u| u.value().clone())
        .ok_or(StatusCode::UNAUTHORIZED)?;

    if !user.is_active {
        return Err(StatusCode::UNAUTHORIZED);
    }

    // Resolve role
    let role = state.get_user_role(user.id);
    let (role_name, permissions) = if let Some(role) = &role {
        (role.name.clone(), state.get_role_permission_names(role.id))
    } else {
        ("viewer".into(), vec![])
    };

    let auth_user = AuthUser {
        user_id: user.id,
        email: user.email.clone(),
        username: user.username.clone(),
        org_id: None,
        role: role_name,
        permissions,
    };

    let access_token = jwt::create_access_token(
        &auth_user,
        &state.config.jwt_secret,
        state.config.jwt_access_ttl_secs,
    )
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(json!({
        "access_token": access_token,
        "refresh_token": new_refresh_token,
    })))
}

async fn logout(
    State(state): State<AppState>,
    Json(req): Json<RefreshRequest>,
) -> Json<Value> {
    // Find and revoke the session
    if let Some(session) = state.inner.session_store.validate_refresh_token(&req.refresh_token) {
        state.inner.session_store.revoke_session(session.id);
    }
    Json(json!({ "logged_out": true }))
}
