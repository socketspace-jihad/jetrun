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

use crate::services::{jwt, password};
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
    if state.find_user_by_email(&req.email).is_some() {
        return Ok(Json(json!({ "error": "Email already registered" })));
    }
    if state.find_user_by_username(&req.username).is_some() {
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
    state.inner.users.insert(user_id, user.clone());

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
    state.inner.organizations.insert(org_id, org.clone());

    // 3. Add user as admin of their org
    let admin_role = state.find_builtin_role("admin")
        .ok_or(StatusCode::INTERNAL_SERVER_ERROR)?;
    state.add_org_member(org_id, user_id, admin_role.id);

    // 4. Build auth context and issue tokens
    let auth_user = state.build_auth_user(user_id, Some(org_id))
        .ok_or(StatusCode::INTERNAL_SERVER_ERROR)?;

    let access_token = jwt::create_access_token(
        &auth_user,
        &state.config.jwt_secret,
        state.config.jwt_access_ttl_secs,
    ).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let (refresh_token, _) = state.inner.session_store.create_session(
        user_id,
        state.config.jwt_refresh_ttl_days,
        None,
        None,
    );

    let orgs = build_org_list(&state, user_id);

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
        .find_user_by_email(&req.email)
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
    if let Some(mut u) = state.inner.users.get_mut(&user.id) {
        u.last_login_at = Some(Utc::now());
    }

    // Resolve which org to log into
    let user_orgs = state.list_user_orgs(user.id);
    let org_id = if let Some(requested_org) = req.org_id {
        // Verify user is a member of the requested org (or is super admin)
        if state.is_org_member(user.id, requested_org) || state.is_super_admin(user.id) {
            Some(requested_org)
        } else {
            return Ok(Json(json!({ "error": "Not a member of this organization" })));
        }
    } else if let Some((first_org, _)) = user_orgs.first() {
        Some(first_org.id)
    } else if state.is_super_admin(user.id) {
        // Super admin with no org — platform context
        None
    } else {
        return Ok(Json(json!({ "error": "User has no organization. Contact an admin." })));
    };

    let auth_user = state.build_auth_user(user.id, org_id)
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

    let orgs = build_org_list(&state, user.id);

    let org_response = org_id.and_then(|oid| {
        state.inner.organizations.get(&oid).map(|o| OrgResponse {
            id: o.id,
            name: o.name.clone(),
            slug: o.slug.clone(),
        })
    });

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
    let session = state
        .inner
        .session_store
        .validate_refresh_token(&req.refresh_token)
        .ok_or(StatusCode::UNAUTHORIZED)?;

    let user = state
        .inner
        .users
        .get(&session.user_id)
        .map(|u| u.clone())
        .ok_or(StatusCode::UNAUTHORIZED)?;

    if !user.is_active {
        return Err(StatusCode::UNAUTHORIZED);
    }

    // Verify membership in target org (or super admin)
    if !state.is_org_member(user.id, req.org_id) && !state.is_super_admin(user.id) {
        return Ok(Json(json!({ "error": "Not a member of this organization" })));
    }

    // Issue new token with new org context
    let auth_user = state.build_auth_user(user.id, Some(req.org_id))
        .ok_or(StatusCode::INTERNAL_SERVER_ERROR)?;

    let access_token = jwt::create_access_token(
        &auth_user,
        &state.config.jwt_secret,
        state.config.jwt_access_ttl_secs,
    ).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Rotate refresh token
    let (new_refresh_token, _) = state
        .inner
        .session_store
        .rotate_refresh_token(&req.refresh_token, state.config.jwt_refresh_ttl_days)
        .ok_or(StatusCode::UNAUTHORIZED)?;

    let org = state.inner.organizations.get(&req.org_id)
        .map(|o| OrgResponse { id: o.id, name: o.name.clone(), slug: o.slug.clone() });

    let orgs = build_org_list(&state, user.id);

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
    let (new_refresh_token, session) = state
        .inner
        .session_store
        .rotate_refresh_token(&req.refresh_token, state.config.jwt_refresh_ttl_days)
        .ok_or(StatusCode::UNAUTHORIZED)?;

    let user = state
        .inner
        .users
        .get(&session.user_id)
        .map(|u| u.clone())
        .ok_or(StatusCode::UNAUTHORIZED)?;

    if !user.is_active {
        return Err(StatusCode::UNAUTHORIZED);
    }

    // Use requested org_id, or fall back to first org
    let org_id = if let Some(oid) = req.org_id {
        if state.is_org_member(user.id, oid) || state.is_super_admin(user.id) {
            Some(oid)
        } else {
            return Err(StatusCode::FORBIDDEN);
        }
    } else {
        state.list_user_orgs(user.id).first().map(|(org, _)| org.id)
    };

    let auth_user = state.build_auth_user(user.id, org_id)
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
    if let Some(session) = state.inner.session_store.validate_refresh_token(&req.refresh_token) {
        state.inner.session_store.revoke_session(session.id);
    }
    Json(json!({ "logged_out": true }))
}

// ── Helpers ──

fn build_org_list(state: &AppState, user_id: Uuid) -> Vec<OrgListItem> {
    state
        .list_user_orgs(user_id)
        .into_iter()
        .map(|(org, role)| OrgListItem {
            id: org.id,
            name: org.name,
            slug: org.slug,
            role: role.display_name,
        })
        .collect()
}
