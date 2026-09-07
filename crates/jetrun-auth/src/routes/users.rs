use axum::{
    extract::{Extension, Path, State},
    http::StatusCode,
    routing::{delete, get, patch},
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

use jetrun_common::models::AuthUser;

use crate::state::AppState;

/// Routes for the logged-in user's own profile
pub fn authenticated_routes() -> Router<AppState> {
    Router::new()
        .route("/me", get(get_me).patch(update_me))
        .route("/me/password", axum::routing::put(change_password))
        .route("/me/sessions", get(list_my_sessions))
        .route("/me/sessions/{id}", delete(revoke_my_session))
}

/// Admin routes — user management scoped to the current org
pub fn admin_routes() -> Router<AppState> {
    Router::new()
        .route("/", get(list_users))
        .route("/{id}", get(get_user).delete(deactivate_user))
        .route("/{id}/role", patch(change_user_role))
}

// ── Profile routes (own user) ──

async fn get_me(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
) -> Json<Value> {
    let user = match state.inner.users.get(&auth_user.user_id) {
        Some(u) => u.clone(),
        None => return Json(json!({ "error": "User not found" })),
    };

    let orgs: Vec<Value> = state
        .list_user_orgs(auth_user.user_id)
        .into_iter()
        .map(|(org, role)| {
            json!({
                "id": org.id,
                "name": org.name,
                "slug": org.slug,
                "role": role.display_name,
            })
        })
        .collect();

    Json(json!({
        "id": user.id,
        "email": user.email,
        "username": user.username,
        "display_name": user.display_name,
        "avatar_url": user.avatar_url,
        "auth_provider": user.auth_provider,
        "email_verified": user.email_verified,
        "role": auth_user.role,
        "permissions": auth_user.permissions,
        "org_id": auth_user.org_id,
        "orgs": orgs,
        "last_login_at": user.last_login_at,
        "created_at": user.created_at,
    }))
}

#[derive(Debug, Deserialize)]
struct UpdateProfileRequest {
    display_name: Option<String>,
    avatar_url: Option<String>,
}

async fn update_me(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Json(req): Json<UpdateProfileRequest>,
) -> Json<Value> {
    if let Some(mut user) = state.inner.users.get_mut(&auth_user.user_id) {
        if let Some(name) = req.display_name {
            user.display_name = Some(name);
        }
        if let Some(url) = req.avatar_url {
            user.avatar_url = Some(url);
        }
        user.updated_at = chrono::Utc::now();
        Json(json!({ "updated": true }))
    } else {
        Json(json!({ "error": "User not found" }))
    }
}

#[derive(Debug, Deserialize)]
struct ChangePasswordRequest {
    current_password: String,
    new_password: String,
}

async fn change_password(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Json(req): Json<ChangePasswordRequest>,
) -> Json<Value> {
    let user = match state.inner.users.get(&auth_user.user_id) {
        Some(u) => u.clone(),
        None => return Json(json!({ "error": "User not found" })),
    };

    let hash = match &user.password_hash {
        Some(h) => h,
        None => return Json(json!({ "error": "User has no password (SSO account)" })),
    };

    match crate::services::password::verify_password(&req.current_password, hash) {
        Ok(true) => {}
        _ => return Json(json!({ "error": "Current password is incorrect" })),
    }

    let new_hash = match crate::services::password::hash_password(&req.new_password) {
        Ok(h) => h,
        Err(_) => return Json(json!({ "error": "Failed to hash password" })),
    };

    if let Some(mut u) = state.inner.users.get_mut(&auth_user.user_id) {
        u.password_hash = Some(new_hash);
        u.updated_at = chrono::Utc::now();
    }

    state
        .inner
        .session_store
        .revoke_all_user_sessions(auth_user.user_id);

    Json(json!({ "updated": true }))
}

async fn list_my_sessions(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
) -> Json<Value> {
    let sessions: Vec<Value> = state
        .inner
        .session_store
        .get_user_sessions(auth_user.user_id)
        .iter()
        .map(|s| {
            json!({
                "id": s.id,
                "user_agent": s.user_agent,
                "ip_address": s.ip_address,
                "created_at": s.created_at,
                "last_used_at": s.last_used_at,
                "expires_at": s.expires_at,
            })
        })
        .collect();

    Json(json!({ "sessions": sessions }))
}

async fn revoke_my_session(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Path(session_id): Path<Uuid>,
) -> Json<Value> {
    let sessions = state
        .inner
        .session_store
        .get_user_sessions(auth_user.user_id);

    if sessions.iter().any(|s| s.id == session_id) {
        state.inner.session_store.revoke_session(session_id);
        Json(json!({ "revoked": true }))
    } else {
        Json(json!({ "error": "Session not found" }))
    }
}

// ── Admin routes (org-scoped) ──

/// List users in the current org (not all platform users)
async fn list_users(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
) -> Result<Json<Value>, StatusCode> {
    if !auth_user.has_permission("user:read") && !state.is_super_admin(auth_user.user_id) {
        return Err(StatusCode::FORBIDDEN);
    }

    let org_id = auth_user.org_id.ok_or(StatusCode::BAD_REQUEST)?;

    let users: Vec<Value> = state
        .list_org_members(org_id)
        .into_iter()
        .map(|(user, role, member)| {
            json!({
                "id": user.id,
                "email": user.email,
                "username": user.username,
                "display_name": user.display_name,
                "auth_provider": user.auth_provider,
                "is_active": user.is_active,
                "role": role.display_name,
                "role_id": role.id,
                "joined_at": member.joined_at,
                "last_login_at": user.last_login_at,
            })
        })
        .collect();

    Ok(Json(json!({ "users": users })))
}

async fn get_user(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, StatusCode> {
    if !auth_user.has_permission("user:read") && !state.is_super_admin(auth_user.user_id) {
        return Err(StatusCode::FORBIDDEN);
    }

    // Verify target user is in the same org
    let org_id = auth_user.org_id.ok_or(StatusCode::BAD_REQUEST)?;
    if !state.is_org_member(id, org_id) && !state.is_super_admin(auth_user.user_id) {
        return Err(StatusCode::NOT_FOUND);
    }

    match state.inner.users.get(&id) {
        Some(u) => {
            let role = state.get_user_role_in_org(id, org_id);
            Ok(Json(json!({
                "id": u.id,
                "email": u.email,
                "username": u.username,
                "display_name": u.display_name,
                "auth_provider": u.auth_provider,
                "is_active": u.is_active,
                "role": role.map(|r| r.display_name),
                "created_at": u.created_at,
            })))
        }
        None => Err(StatusCode::NOT_FOUND),
    }
}

#[derive(Debug, Deserialize)]
struct ChangeRoleRequest {
    role_id: Uuid,
}

/// Change a user's role within the current org
async fn change_user_role(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Path(user_id): Path<Uuid>,
    Json(req): Json<ChangeRoleRequest>,
) -> Result<Json<Value>, StatusCode> {
    if !auth_user.has_permission("user:manage") && !state.is_super_admin(auth_user.user_id) {
        return Err(StatusCode::FORBIDDEN);
    }

    let org_id = auth_user.org_id.ok_or(StatusCode::BAD_REQUEST)?;

    // Can't change own role (prevents locking yourself out)
    if user_id == auth_user.user_id {
        return Ok(Json(json!({ "error": "Cannot change your own role" })));
    }

    // Verify target user is in this org
    if !state.is_org_member(user_id, org_id) {
        return Ok(Json(json!({ "error": "User is not a member of this organization" })));
    }

    // Verify role exists
    if !state.inner.roles.contains_key(&req.role_id) {
        return Ok(Json(json!({ "error": "Role not found" })));
    }

    // Remove old membership, add new one with new role
    state.remove_org_member(org_id, user_id);
    state.add_org_member(org_id, user_id, req.role_id);

    Ok(Json(json!({ "updated": true })))
}

/// Deactivate a user (platform-wide, requires user:delete)
async fn deactivate_user(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, StatusCode> {
    if !auth_user.has_permission("user:delete") && !state.is_super_admin(auth_user.user_id) {
        return Err(StatusCode::FORBIDDEN);
    }

    // Can't deactivate yourself
    if id == auth_user.user_id {
        return Ok(Json(json!({ "error": "Cannot deactivate yourself" })));
    }

    if let Some(mut user) = state.inner.users.get_mut(&id) {
        user.is_active = false;
        user.updated_at = chrono::Utc::now();
        state.inner.session_store.revoke_all_user_sessions(id);
        Ok(Json(json!({ "deactivated": true })))
    } else {
        Err(StatusCode::NOT_FOUND)
    }
}
