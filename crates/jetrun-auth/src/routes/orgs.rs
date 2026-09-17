use axum::{
    extract::{Extension, Path, State},
    http::StatusCode,
    routing::{delete, get, post},
    Json, Router,
};
use chrono::Utc;
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

use jetrun_common::models::{AuthUser, Organization};

use crate::state::AppState;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/", post(create_org))
        .route("/me", get(list_my_orgs))
        .route("/{id}", get(get_org).patch(update_org))
        .route("/{id}/members", get(list_org_members).post(invite_member))
        .route("/{id}/members/{user_id}", delete(remove_member))
}

// ── Create organization ──

#[derive(Debug, Deserialize)]
struct CreateOrgRequest {
    name: String,
    slug: String,
}

async fn create_org(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Json(req): Json<CreateOrgRequest>,
) -> Result<Json<Value>, StatusCode> {
    // Check slug uniqueness — try finding by slug (no dedicated method, but we can use org listing or just try create)
    // For now, we attempt creation and rely on DB uniqueness constraint, or check via user orgs.
    // A simpler approach: just create and let DB reject if slug is duplicate.

    let org = Organization {
        id: Uuid::new_v4(),
        name: req.name,
        slug: req.slug,
        owner_id: auth_user.user_id,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };

    let org_id = org.id;
    if let Err(_) = state.store.create_org(&org).await {
        return Ok(Json(json!({ "error": "Organization slug already taken" })));
    }

    // Creator becomes admin of the new org
    let admin_role = state
        .find_builtin_role("admin").await
        .ok_or(StatusCode::INTERNAL_SERVER_ERROR)?;
    state.add_org_member(org_id, auth_user.user_id, admin_role.id).await;

    Ok(Json(json!({
        "id": org.id,
        "name": org.name,
        "slug": org.slug,
        "created": true,
    })))
}

// ── List my organizations ──

async fn list_my_orgs(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
) -> Json<Value> {
    let orgs: Vec<Value> = state
        .list_user_orgs(auth_user.user_id).await
        .into_iter()
        .map(|(org, role)| {
            json!({
                "id": org.id,
                "name": org.name,
                "slug": org.slug,
                "role": role.display_name,
                "is_owner": org.owner_id == auth_user.user_id,
            })
        })
        .collect();

    Json(json!({ "orgs": orgs }))
}

// ── Get org details ──

async fn get_org(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, StatusCode> {
    // Must be a member or super admin
    if !state.is_org_member(auth_user.user_id, id).await && !state.is_super_admin(auth_user.user_id).await {
        return Err(StatusCode::FORBIDDEN);
    }

    match state.store.find_org_by_id(id).await.ok().flatten() {
        Some(org) => Ok(Json(json!({
            "id": org.id,
            "name": org.name,
            "slug": org.slug,
            "owner_id": org.owner_id,
            "created_at": org.created_at,
        }))),
        None => Err(StatusCode::NOT_FOUND),
    }
}

// ── Update org ──

#[derive(Debug, Deserialize)]
struct UpdateOrgRequest {
    name: Option<String>,
}

async fn update_org(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Path(id): Path<Uuid>,
    Json(req): Json<UpdateOrgRequest>,
) -> Result<Json<Value>, StatusCode> {
    // Require org:update permission
    if !auth_user.has_permission("org:update") && !state.is_super_admin(auth_user.user_id).await {
        return Err(StatusCode::FORBIDDEN);
    }

    match state.store.find_org_by_id(id).await.ok().flatten() {
        Some(mut org) => {
            if let Some(name) = req.name {
                org.name = name;
            }
            org.updated_at = Utc::now();
            let _ = state.store.update_org(&org).await;
            Ok(Json(json!({ "updated": true })))
        }
        None => Err(StatusCode::NOT_FOUND),
    }
}

// ── List org members ──

async fn list_org_members(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Path(org_id): Path<Uuid>,
) -> Result<Json<Value>, StatusCode> {
    if !state.is_org_member(auth_user.user_id, org_id).await && !state.is_super_admin(auth_user.user_id).await {
        return Err(StatusCode::FORBIDDEN);
    }

    let members: Vec<Value> = state
        .list_org_members(org_id).await
        .into_iter()
        .map(|(user, role, member)| {
            json!({
                "id": member.id,
                "user_id": user.id,
                "email": user.email,
                "username": user.username,
                "display_name": user.display_name,
                "role": role.display_name,
                "role_id": role.id,
                "joined_at": member.joined_at,
            })
        })
        .collect();

    Ok(Json(json!({ "members": members })))
}

// ── Invite member to org ──

#[derive(Debug, Deserialize)]
struct InviteMemberRequest {
    /// Email of the user to invite (must already have an account)
    email: String,
    /// Role to assign. Defaults to "developer" if omitted.
    role_id: Option<Uuid>,
}

async fn invite_member(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Path(org_id): Path<Uuid>,
    Json(req): Json<InviteMemberRequest>,
) -> Result<Json<Value>, StatusCode> {
    // Require user:manage permission
    if !auth_user.has_permission("user:manage") && !state.is_super_admin(auth_user.user_id).await {
        return Err(StatusCode::FORBIDDEN);
    }

    // Find the user by email
    let target_user = state
        .find_user_by_email(&req.email).await
        .ok_or(StatusCode::NOT_FOUND)?;

    // Check not already a member
    if state.is_org_member(target_user.id, org_id).await {
        return Ok(Json(json!({ "error": "User is already a member of this organization" })));
    }

    // Determine role
    let role_id = match req.role_id {
        Some(rid) => rid,
        None => state
            .find_builtin_role("developer").await
            .map(|r| r.id)
            .unwrap_or_default(),
    };

    // Verify role exists
    if state.store.find_role_by_id(role_id).await.ok().flatten().is_none() {
        return Ok(Json(json!({ "error": "Role not found" })));
    }

    let member = state.add_org_member(org_id, target_user.id, role_id).await;

    Ok(Json(json!({
        "invited": true,
        "member_id": member.id,
        "user_id": target_user.id,
        "email": target_user.email,
    })))
}

// ── Remove member from org ──

async fn remove_member(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Path((org_id, user_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<Value>, StatusCode> {
    // Require user:manage permission
    if !auth_user.has_permission("user:manage") && !state.is_super_admin(auth_user.user_id).await {
        return Err(StatusCode::FORBIDDEN);
    }

    // Can't remove the org owner
    if let Some(org) = state.store.find_org_by_id(org_id).await.ok().flatten() {
        if org.owner_id == user_id {
            return Ok(Json(json!({ "error": "Cannot remove the organization owner" })));
        }
    }

    if state.remove_org_member(org_id, user_id).await {
        Ok(Json(json!({ "removed": true })))
    } else {
        Ok(Json(json!({ "error": "Member not found" })))
    }
}
