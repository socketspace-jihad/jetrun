use axum::{
    extract::{Path, State},
    routing::get,
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

use crate::state::AppState;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/{id}", get(get_org).patch(update_org))
        .route("/{id}/members", get(list_org_members))
}

async fn get_org(State(state): State<AppState>, Path(id): Path<Uuid>) -> Json<Value> {
    match state.inner.organizations.get(&id) {
        Some(org) => Json(json!({
            "id": org.id,
            "name": org.name,
            "slug": org.slug,
            "owner_id": org.owner_id,
            "created_at": org.created_at,
        })),
        None => Json(json!({ "error": "Organization not found" })),
    }
}

#[derive(Debug, Deserialize)]
struct UpdateOrgRequest {
    name: Option<String>,
}

async fn update_org(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(req): Json<UpdateOrgRequest>,
) -> Json<Value> {
    if let Some(mut org) = state.inner.organizations.get_mut(&id) {
        if let Some(name) = req.name {
            org.name = name;
        }
        org.updated_at = chrono::Utc::now();
        Json(json!({ "updated": true }))
    } else {
        Json(json!({ "error": "Organization not found" }))
    }
}

async fn list_org_members(
    State(state): State<AppState>,
    Path(org_id): Path<Uuid>,
) -> Json<Value> {
    let members: Vec<Value> = state
        .inner
        .org_members
        .iter()
        .filter(|e| e.value().org_id == org_id)
        .filter_map(|e| {
            let member = e.value();
            let user = state.inner.users.get(&member.user_id)?;
            let role = state.inner.roles.get(&member.role_id)?;
            Some(json!({
                "id": member.id,
                "user_id": user.id,
                "email": user.email,
                "username": user.username,
                "display_name": user.display_name,
                "role": role.display_name,
                "joined_at": member.joined_at,
            }))
        })
        .collect();

    Json(json!({ "members": members }))
}
