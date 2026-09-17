use axum::{
    extract::{Path, State},
    routing::{get, patch},
    Json, Router,
};
use chrono::Utc;
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

use jetrun_common::models::Role;

use crate::state::AppState;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/", get(list_roles).post(create_role))
        .route("/{id}", patch(update_role).delete(delete_role))
        .route("/permissions", get(list_permissions))
}

async fn list_roles(State(state): State<AppState>) -> Json<Value> {
    let roles = state.store.list_roles().await.unwrap_or_default();

    let mut role_values: Vec<Value> = Vec::with_capacity(roles.len());
    for r in &roles {
        let perms = state.get_role_permission_names(r.id).await;
        role_values.push(json!({
            "id": r.id,
            "name": r.name,
            "display_name": r.display_name,
            "description": r.description,
            "is_builtin": r.is_builtin,
            "permissions": perms,
            "created_at": r.created_at,
        }));
    }

    Json(json!({ "roles": role_values }))
}

async fn list_permissions(State(state): State<AppState>) -> Json<Value> {
    let perms = state.store.list_permissions().await.unwrap_or_default();

    let perm_values: Vec<Value> = perms
        .into_iter()
        .map(|p| {
            json!({
                "id": p.id,
                "name": p.name,
                "description": p.description,
                "resource": p.resource,
                "action": p.action,
            })
        })
        .collect();

    Json(json!({ "permissions": perm_values }))
}

#[derive(Debug, Deserialize)]
struct CreateRoleRequest {
    name: String,
    display_name: String,
    description: Option<String>,
    permissions: Vec<String>,
}

async fn create_role(
    State(state): State<AppState>,
    Json(req): Json<CreateRoleRequest>,
) -> Json<Value> {
    let role = Role {
        id: Uuid::new_v4(),
        name: req.name,
        display_name: req.display_name,
        description: req.description,
        is_builtin: false,
        org_id: None,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };

    // Map permission names to IDs
    let all_perms = state.store.list_permissions().await.unwrap_or_default();
    let perm_ids: Vec<Uuid> = req
        .permissions
        .iter()
        .filter_map(|perm_name| {
            all_perms.iter().find(|p| p.name == *perm_name).map(|p| p.id)
        })
        .collect();

    let role_id = role.id;
    if let Err(e) = state.store.create_role(&role).await {
        tracing::error!(error = %e, "failed to create role");
        return Json(json!({ "error": "Failed to create role" }));
    }
    let _ = state.store.set_role_permissions(role_id, &perm_ids).await;

    Json(json!({ "id": role_id, "created": true }))
}

#[derive(Debug, Deserialize)]
struct UpdateRoleRequest {
    display_name: Option<String>,
    description: Option<String>,
    permissions: Option<Vec<String>>,
}

async fn update_role(
    State(state): State<AppState>,
    Path(role_id): Path<Uuid>,
    Json(req): Json<UpdateRoleRequest>,
) -> Json<Value> {
    let mut role = match state.store.find_role_by_id(role_id).await.ok().flatten() {
        Some(r) => r,
        None => return Json(json!({ "error": "Role not found" })),
    };

    if role.is_builtin {
        return Json(json!({ "error": "Cannot modify built-in roles" }));
    }

    if let Some(name) = req.display_name {
        role.display_name = name;
    }
    if let Some(desc) = req.description {
        role.description = Some(desc);
    }
    role.updated_at = Utc::now();
    let _ = state.store.update_role(&role).await;

    // Update permissions if provided
    if let Some(perm_names) = req.permissions {
        let all_perms = state.store.list_permissions().await.unwrap_or_default();
        let perm_ids: Vec<Uuid> = perm_names
            .iter()
            .filter_map(|name| {
                all_perms.iter().find(|p| p.name == *name).map(|p| p.id)
            })
            .collect();
        let _ = state.store.set_role_permissions(role_id, &perm_ids).await;
    }

    Json(json!({ "updated": true }))
}

async fn delete_role(
    State(state): State<AppState>,
    Path(role_id): Path<Uuid>,
) -> Json<Value> {
    let role = match state.store.find_role_by_id(role_id).await.ok().flatten() {
        Some(r) => r,
        None => return Json(json!({ "error": "Role not found" })),
    };

    if role.is_builtin {
        return Json(json!({ "error": "Cannot delete built-in roles" }));
    }

    match state.store.delete_role(role_id).await {
        Ok(true) => Json(json!({ "deleted": true })),
        _ => Json(json!({ "error": "Failed to delete role" })),
    }
}
