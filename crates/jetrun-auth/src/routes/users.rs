use axum::{
    extract::{Extension, Path, State},
    routing::{get, patch, delete},
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

use jetrun_common::models::AuthUser;

use crate::state::AppState;

/// Routes that require authentication (for the logged-in user)
pub fn authenticated_routes() -> Router<AppState> {
    Router::new()
        .route("/me", get(get_me).patch(update_me))
        .route("/me/password", axum::routing::put(change_password))
        .route("/me/sessions", get(list_my_sessions))
        .route("/me/sessions/{id}", delete(revoke_my_session))
}

/// Admin routes for user management
pub fn admin_routes() -> Router<AppState> {
    Router::new()
        .route("/", get(list_users))
        .route("/{id}", get(get_user).delete(deactivate_user))
        .route("/{id}/role", patch(change_user_role))
}

async fn get_me(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
) -> Json<Value> {
    match state.inner.users.get(&auth_user.user_id) {
        Some(user) => Json(json!({
            "id": user.id,
            "email": user.email,
            "username": user.username,
            "display_name": user.display_name,
            "avatar_url": user.avatar_url,
            "auth_provider": user.auth_provider,
            "email_verified": user.email_verified,
            "role": auth_user.role,
            "permissions": auth_user.permissions,
            "last_login_at": user.last_login_at,
            "created_at": user.created_at,
        })),
        None => Json(json!({ "error": "User not found" })),
    }
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
        Some(u) => u.value().clone(),
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

    // Revoke all other sessions
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
    let sessions = state
        .inner
        .session_store
        .get_user_sessions(auth_user.user_id);

    let sessions: Vec<Value> = sessions
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
    // Verify the session belongs to the user
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

// --- Admin endpoints ---

async fn list_users(State(state): State<AppState>) -> Json<Value> {
    let users: Vec<Value> = state
        .inner
        .users
        .iter()
        .map(|entry| {
            let u = entry.value();
            json!({
                "id": u.id,
                "email": u.email,
                "username": u.username,
                "display_name": u.display_name,
                "auth_provider": u.auth_provider,
                "is_active": u.is_active,
                "last_login_at": u.last_login_at,
                "created_at": u.created_at,
            })
        })
        .collect();

    Json(json!({ "users": users }))
}

async fn get_user(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Json<Value> {
    match state.inner.users.get(&id) {
        Some(u) => Json(json!({
            "id": u.id,
            "email": u.email,
            "username": u.username,
            "display_name": u.display_name,
            "auth_provider": u.auth_provider,
            "is_active": u.is_active,
            "created_at": u.created_at,
        })),
        None => Json(json!({ "error": "User not found" })),
    }
}

#[derive(Debug, Deserialize)]
struct ChangeRoleRequest {
    role_id: Uuid,
}

async fn change_user_role(
    State(state): State<AppState>,
    Path(user_id): Path<Uuid>,
    Json(req): Json<ChangeRoleRequest>,
) -> Json<Value> {
    // Verify role exists
    if !state.inner.roles.contains_key(&req.role_id) {
        return Json(json!({ "error": "Role not found" }));
    }

    // Update org membership role (or create one)
    let existing = state
        .inner
        .org_members
        .iter()
        .find(|e| e.value().user_id == user_id)
        .map(|e| e.key().clone());

    if let Some(member_id) = existing {
        if let Some(mut member) = state.inner.org_members.get_mut(&member_id) {
            member.role_id = req.role_id;
        }
    }

    Json(json!({ "updated": true }))
}

async fn deactivate_user(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Json<Value> {
    if let Some(mut user) = state.inner.users.get_mut(&id) {
        user.is_active = false;
        user.updated_at = chrono::Utc::now();
        // Revoke all sessions
        state.inner.session_store.revoke_all_user_sessions(id);
        Json(json!({ "deactivated": true }))
    } else {
        Json(json!({ "error": "User not found" }))
    }
}
