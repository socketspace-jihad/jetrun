use axum::{
    extract::{Extension, Path, State},
    routing::{delete, get},
    Json, Router,
};
use chrono::Utc;
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

use jetrun_common::models::{AuthUser, Team, TeamMember};

use crate::state::AppState;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/groups", get(list_groups).post(create_group))
        .route("/groups/{id}", get(get_group).delete(delete_group))
        .route("/groups/{id}/members", get(list_members).post(add_member))
        .route("/groups/{id}/members/{user_id}", delete(remove_member))
        .route("/groups/{id}/role", axum::routing::put(set_role))
}

async fn list_groups(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
) -> Json<Value> {
    let org_id = match auth_user.org_id {
        Some(id) => id,
        None => return Json(json!({ "groups": [] })),
    };

    let groups = state.store.list_groups(org_id).await.unwrap_or_default();
    let mut result = Vec::new();

    for g in groups {
        let role = state.store.get_group_role(g.id).await.ok().flatten();
        let member_count = state.store.list_group_members(g.id).await.map(|m| m.len()).unwrap_or(0);
        result.push(json!({
            "id": g.id,
            "name": g.name,
            "slug": g.slug,
            "description": g.description,
            "role": role.map(|r| json!({ "id": r.id, "name": r.name, "display_name": r.display_name })),
            "member_count": member_count,
            "created_at": g.created_at,
        }));
    }

    Json(json!({ "groups": result }))
}

#[derive(Debug, Deserialize)]
struct CreateGroupRequest {
    name: String,
    description: Option<String>,
    role_id: Option<Uuid>,
}

async fn create_group(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Json(req): Json<CreateGroupRequest>,
) -> Result<Json<Value>, axum::http::StatusCode> {
    let org_id = auth_user.org_id.ok_or(axum::http::StatusCode::BAD_REQUEST)?;

    let slug = req.name.to_lowercase().replace(|c: char| !c.is_alphanumeric() && c != '-', "-");
    let group = Team {
        id: Uuid::new_v4(),
        org_id,
        name: req.name,
        slug,
        description: req.description,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };

    let group_id = group.id;
    if let Err(_) = state.store.create_group(&group).await {
        return Ok(Json(json!({ "error": "Group name already exists" })));
    }

    if let Some(role_id) = req.role_id {
        let _ = state.store.set_group_role(group_id, role_id).await;
    }

    Ok(Json(json!({ "id": group_id, "created": true })))
}

async fn get_group(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Json<Value> {
    match state.store.find_group_by_id(id).await.ok().flatten() {
        Some(g) => {
            let role = state.store.get_group_role(g.id).await.ok().flatten();
            let members = state.store.list_group_members(g.id).await.unwrap_or_default();
            let member_list: Vec<Value> = members.into_iter().map(|(u, m)| json!({
                "id": m.id, "user_id": u.id, "email": u.email, "username": u.username,
                "display_name": u.display_name, "added_at": m.added_at,
            })).collect();

            Json(json!({
                "id": g.id, "name": g.name, "slug": g.slug, "description": g.description,
                "role": role.map(|r| json!({ "id": r.id, "name": r.name, "display_name": r.display_name })),
                "members": member_list,
            }))
        }
        None => Json(json!({ "error": "Group not found" })),
    }
}

async fn delete_group(State(state): State<AppState>, Path(id): Path<Uuid>) -> Json<Value> {
    match state.store.delete_group(id).await {
        Ok(true) => Json(json!({ "deleted": true })),
        _ => Json(json!({ "error": "Group not found" })),
    }
}

async fn list_members(State(state): State<AppState>, Path(id): Path<Uuid>) -> Json<Value> {
    let members = state.store.list_group_members(id).await.unwrap_or_default();
    let list: Vec<Value> = members.into_iter().map(|(u, m)| json!({
        "id": m.id, "user_id": u.id, "email": u.email, "username": u.username,
        "display_name": u.display_name, "added_at": m.added_at,
    })).collect();
    Json(json!({ "members": list }))
}

#[derive(Debug, Deserialize)]
struct AddMemberRequest { email: String }

async fn add_member(
    State(state): State<AppState>,
    Path(group_id): Path<Uuid>,
    Json(req): Json<AddMemberRequest>,
) -> Json<Value> {
    let user = match state.find_user_by_email(&req.email).await {
        Some(u) => u,
        None => return Json(json!({ "error": "User not found. They must register first." })),
    };

    let member = TeamMember { id: Uuid::new_v4(), team_id: group_id, user_id: user.id, added_at: Utc::now() };
    match state.store.add_group_member(&member).await {
        Ok(()) => Json(json!({ "added": true, "user_id": user.id })),
        Err(_) => Json(json!({ "error": "User already in this group" })),
    }
}

async fn remove_member(
    State(state): State<AppState>,
    Path((group_id, user_id)): Path<(Uuid, Uuid)>,
) -> Json<Value> {
    match state.store.remove_group_member(group_id, user_id).await {
        Ok(true) => Json(json!({ "removed": true })),
        _ => Json(json!({ "error": "Member not found" })),
    }
}

#[derive(Debug, Deserialize)]
struct SetRoleRequest { role_id: Uuid }

async fn set_role(
    State(state): State<AppState>,
    Path(group_id): Path<Uuid>,
    Json(req): Json<SetRoleRequest>,
) -> Json<Value> {
    match state.store.set_group_role(group_id, req.role_id).await {
        Ok(()) => Json(json!({ "updated": true })),
        Err(_) => Json(json!({ "error": "Failed to set role" })),
    }
}
