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

use jetrun_common::models::{AuthProvider, Organization, User};

use crate::services::{jwt, password, session};
use crate::state::AppState;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/register", post(register))
        .route("/login", post(login))
        .route("/refresh", post(refresh))
        .route("/logout", post(logout))
        .route("/switch-org", post(switch_org))
}

// ── Request/Response types ──

#[derive(Debug, Deserialize)]
struct RegisterRequest {
    email: String,
    username: String,
    password: String,
    display_name: Option<String>,
    /// Optional: org name to create. If omitted, creates "{username}'s Org".
    org_name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct LoginRequest {
    email: String,
    password: String,
    /// Optional: select which org to log into. If omitted, uses first org.
    org_id: Option<Uuid>,
}

#[derive(Debug, Deserialize)]
struct RefreshRequest {
    refresh_token: String,
    /// Optional: switch org on refresh
    org_id: Option<Uuid>,
}

#[derive(Debug, Deserialize)]
struct SwitchOrgRequest {
    org_id: Uuid,
    refresh_token: String,
}

#[derive(Debug, Serialize)]
struct AuthResponse {
    access_token: String,
    refresh_token: String,
    user: UserResponse,
    org: OrgResponse,
    orgs: Vec<OrgListItem>,
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

#[derive(Debug, Serialize)]
struct OrgResponse {
    id: Uuid,
    name: String,
    slug: String,
}

#[derive(Debug, Serialize)]
struct OrgListItem {
    id: Uuid,
    name: String,
    slug: String,
    role: String,
}

// ── Handlers ──

async fn register(
    State(state): State<AppState>,
    Json(req): Json<RegisterRequest>,
) -> Result<Json<Value>, StatusCode> {
    if state.find_user_by_email(&req.email).await.is_some() {
        return Ok(Json(json!({ "error": "Email already registered" })));
    }
    if state.find_user_by_username(&req.username).await.is_some() {
        return Ok(Json(json!({ "error": "Username already taken" })));
    }

    let password_hash = password::hash_password(&req.password)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // 1. Create user
    let user = User {
        id: Uuid::new_v4(),
        email: req.email,
        username: req.username.clone(),
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
    state.store.create_user(&user).await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // 2. Create default org for this user
    let org_name = req.org_name.unwrap_or_else(|| format!("{}'s Org", req.username));
    let org_slug = org_name.to_lowercase().replace(' ', "-").replace('\'', "");
    let org = Organization {
        id: Uuid::new_v4(),
        name: org_name,
        slug: org_slug,
        owner_id: user_id,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };
    let org_id = org.id;
    state.store.create_org(&org).await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // 3. Add user as admin of their org
    let admin_role = state.find_builtin_role("admin").await
        .ok_or(StatusCode::INTERNAL_SERVER_ERROR)?;
    state.add_org_member(org_id, user_id, admin_role.id).await;

    // 4. Build auth context and issue tokens
    let auth_user = state.build_auth_user(user_id, Some(org_id)).await
        .ok_or(StatusCode::INTERNAL_SERVER_ERROR)?;

    let access_token = jwt::create_access_token(
        &auth_user,
        &state.config.jwt_secret,
        state.config.jwt_access_ttl_secs,
    ).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let (refresh_token, _) = session::create_session(
        state.store.as_ref(),
        user_id,
        state.config.jwt_refresh_ttl_days,
        None,
        None,
    ).await.ok_or(StatusCode::INTERNAL_SERVER_ERROR)?;

    let orgs = build_org_list(&state, user_id).await;

    Ok(Json(json!(AuthResponse {
        access_token,
        refresh_token,
        user: UserResponse {
            id: user_id,
            email: user.email,
            username: user.username,
            display_name: user.display_name,
            avatar_url: None,
            role: auth_user.role,
            permissions: auth_user.permissions,
        },
        org: OrgResponse {
            id: org.id,
            name: org.name,
            slug: org.slug,
        },
        orgs,
    })))
}

async fn login(
    State(state): State<AppState>,
    Json(req): Json<LoginRequest>,
) -> Result<Json<Value>, StatusCode> {
    let user = state
        .find_user_by_email(&req.email).await
        .ok_or(StatusCode::UNAUTHORIZED)?;

    if !user.is_active {
        return Err(StatusCode::UNAUTHORIZED);
    }

    let password_hash = user.password_hash.as_ref().ok_or(StatusCode::UNAUTHORIZED)?;
    let valid = password::verify_password(&req.password, password_hash)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    if !valid {
        return Err(StatusCode::UNAUTHORIZED);
    }

    // Update last login
    let mut updated_user = user.clone();
    updated_user.last_login_at = Some(Utc::now());
    let _ = state.store.update_user(&updated_user).await;

    // Resolve which org to log into
    let user_orgs = state.list_user_orgs(user.id).await;
    let org_id = if let Some(requested_org) = req.org_id {
        // Verify user is a member of the requested org (or is super admin)
        if state.is_org_member(user.id, requested_org).await || state.is_super_admin(user.id).await {
            Some(requested_org)
        } else {
            return Ok(Json(json!({ "error": "Not a member of this organization" })));
        }
    } else if let Some((first_org, _)) = user_orgs.first() {
        Some(first_org.id)
    } else if state.is_super_admin(user.id).await {
        // Super admin with no org — platform context
        None
    } else {
        return Ok(Json(json!({ "error": "User has no organization. Contact an admin." })));
    };

    let auth_user = state.build_auth_user(user.id, org_id).await
        .ok_or(StatusCode::INTERNAL_SERVER_ERROR)?;

    let access_token = jwt::create_access_token(
        &auth_user,
        &state.config.jwt_secret,
        state.config.jwt_access_ttl_secs,
    ).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let (refresh_token, _) = session::create_session(
        state.store.as_ref(),
        user.id,
        state.config.jwt_refresh_ttl_days,
        None,
        None,
    ).await.ok_or(StatusCode::INTERNAL_SERVER_ERROR)?;

    let orgs = build_org_list(&state, user.id).await;

    let org_response = match org_id {
        Some(oid) => state.store.find_org_by_id(oid).await.ok().flatten().map(|o| OrgResponse {
            id: o.id,
            name: o.name,
            slug: o.slug,
        }),
        None => None,
    };

    Ok(Json(json!({
        "access_token": access_token,
        "refresh_token": refresh_token,
        "user": UserResponse {
            id: user.id,
            email: user.email,
            username: user.username,
            display_name: user.display_name,
            avatar_url: user.avatar_url,
            role: auth_user.role,
            permissions: auth_user.permissions,
        },
        "org": org_response,
        "orgs": orgs,
    })))
}

async fn switch_org(
    State(state): State<AppState>,
    Json(req): Json<SwitchOrgRequest>,
) -> Result<Json<Value>, StatusCode> {
    // Validate refresh token to get user
    let sess = session::validate_refresh_token(state.store.as_ref(), &req.refresh_token)
        .await
        .ok_or(StatusCode::UNAUTHORIZED)?;

    let user = state.store
        .find_user_by_id(sess.user_id).await
        .ok()
        .flatten()
        .ok_or(StatusCode::UNAUTHORIZED)?;

    if !user.is_active {
        return Err(StatusCode::UNAUTHORIZED);
    }

    // Verify membership in target org (or super admin)
    if !state.is_org_member(user.id, req.org_id).await && !state.is_super_admin(user.id).await {
        return Ok(Json(json!({ "error": "Not a member of this organization" })));
    }

    // Issue new token with new org context
    let auth_user = state.build_auth_user(user.id, Some(req.org_id)).await
        .ok_or(StatusCode::INTERNAL_SERVER_ERROR)?;

    let access_token = jwt::create_access_token(
        &auth_user,
        &state.config.jwt_secret,
        state.config.jwt_access_ttl_secs,
    ).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Rotate refresh token
    let (new_refresh_token, _) = session::rotate_refresh_token(
        state.store.as_ref(),
        &req.refresh_token,
        state.config.jwt_refresh_ttl_days,
    ).await.ok_or(StatusCode::UNAUTHORIZED)?;

    let org = state.store.find_org_by_id(req.org_id).await.ok().flatten()
        .map(|o| OrgResponse { id: o.id, name: o.name, slug: o.slug });

    let orgs = build_org_list(&state, user.id).await;

    Ok(Json(json!({
        "access_token": access_token,
        "refresh_token": new_refresh_token,
        "user": UserResponse {
            id: user.id,
            email: user.email,
            username: user.username,
            display_name: user.display_name,
            avatar_url: user.avatar_url,
            role: auth_user.role,
            permissions: auth_user.permissions,
        },
        "org": org,
        "orgs": orgs,
    })))
}

async fn refresh(
    State(state): State<AppState>,
    Json(req): Json<RefreshRequest>,
) -> Result<Json<Value>, StatusCode> {
    let (new_refresh_token, sess) = session::rotate_refresh_token(
        state.store.as_ref(),
        &req.refresh_token,
        state.config.jwt_refresh_ttl_days,
    ).await.ok_or(StatusCode::UNAUTHORIZED)?;

    let user = state.store
        .find_user_by_id(sess.user_id).await
        .ok()
        .flatten()
        .ok_or(StatusCode::UNAUTHORIZED)?;

    if !user.is_active {
        return Err(StatusCode::UNAUTHORIZED);
    }

    // Use requested org_id, or fall back to first org
    let org_id = if let Some(oid) = req.org_id {
        if state.is_org_member(user.id, oid).await || state.is_super_admin(user.id).await {
            Some(oid)
        } else {
            return Err(StatusCode::FORBIDDEN);
        }
    } else {
        state.list_user_orgs(user.id).await.first().map(|(org, _)| org.id)
    };

    let auth_user = state.build_auth_user(user.id, org_id).await
        .ok_or(StatusCode::INTERNAL_SERVER_ERROR)?;

    let access_token = jwt::create_access_token(
        &auth_user,
        &state.config.jwt_secret,
        state.config.jwt_access_ttl_secs,
    ).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(json!({
        "access_token": access_token,
        "refresh_token": new_refresh_token,
    })))
}

async fn logout(
    State(state): State<AppState>,
    Json(req): Json<RefreshRequest>,
) -> Json<Value> {
    if let Some(sess) = session::validate_refresh_token(state.store.as_ref(), &req.refresh_token).await {
        session::revoke_session(state.store.as_ref(), sess.id).await;
    }
    Json(json!({ "logged_out": true }))
}

// ── Helpers ──

async fn build_org_list(state: &AppState, user_id: Uuid) -> Vec<OrgListItem> {
    state
        .list_user_orgs(user_id).await
        .into_iter()
        .map(|(org, role)| OrgListItem {
            id: org.id,
            name: org.name,
            slug: org.slug,
            role: role.display_name,
        })
        .collect()
}
