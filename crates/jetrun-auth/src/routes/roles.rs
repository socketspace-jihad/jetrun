use axum::{
    extract::{Path, State},
    routing::{delete, get, post, patch},
    Json, Router,
};
use chrono::Utc;
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

use jetrun_common::models::{Permission, Role, RolePermission};

use crate::state::AppState;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/", get(list_roles).post(create_role))
        .route("/{id}", patch(update_role).delete(delete_role))
        .route("/permissions", get(list_permissions))
}

async fn list_roles(State(state): State<AppState>) -> Json<Value> {
    let roles: Vec<Value> = state
        .inner
        .roles
        .iter()
        .map(|entry| {
            let r = entry.value();
            let perms = state.get_role_permission_names(r.id);
            json!({
                "id": r.id,
                "name": r.name,
                "display_name": r.display_name,
                "description": r.description,
                "is_builtin": r.is_builtin,
                "permissions": perms,
                "created_at": r.created_at,
            })
        })
        .collect();

    Json(json!({ "roles": roles }))
}

async fn list_permissions(State(state): State<AppState>) -> Json<Value> {
    let perms: Vec<Value> = state
        .inner
        .permissions
        .iter()
        .map(|entry| {
            let p = entry.value();
            json!({
                "id": p.id,
                "name": p.name,
                "description": p.description,
                "resource": p.resource,
                "action": p.action,
            })
        })
        .collect();

    Json(json!({ "permissions": perms }))
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
    let role_perms: Vec<RolePermission> = req
        .permissions
        .iter()
        .filter_map(|perm_name| {
            state
                .inner
                .permissions
                .get(perm_name.as_str())
                .map(|p| RolePermission {
                    role_id: role.id,
                    permission_id: p.id,
                })
        })
        .collect();

    let role_id = role.id;
    state.inner.roles.insert(role_id, role);
    state.inner.role_permissions.insert(role_id, role_perms);

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
    let role = match state.inner.roles.get(&role_id) {
        Some(r) => r.value().clone(),
        None => return Json(json!({ "error": "Role not found" })),
    };

    if role.is_builtin {
        return Json(json!({ "error": "Cannot modify built-in roles" }));
    }

    if let Some(mut r) = state.inner.roles.get_mut(&role_id) {
        if let Some(name) = req.display_name {
            r.display_name = name;
        }
        if let Some(desc) = req.description {
            r.description = Some(desc);
        }
        r.updated_at = Utc::now();
    }

    // Update permissions if provided
    if let Some(perm_names) = req.permissions {
        let role_perms: Vec<RolePermission> = perm_names
            .iter()
            .filter_map(|name| {
                state
                    .inner
                    .permissions
                    .get(name.as_str())
                    .map(|p| RolePermission {
                        role_id,
                        permission_id: p.id,
                    })
            })
            .collect();
        state.inner.role_permissions.insert(role_id, role_perms);
    }

    Json(json!({ "updated": true }))
}

async fn delete_role(
    State(state): State<AppState>,
    Path(role_id): Path<Uuid>,
) -> Json<Value> {
    let role = match state.inner.roles.get(&role_id) {
        Some(r) => r.value().clone(),
        None => return Json(json!({ "error": "Role not found" })),
    };

    if role.is_builtin {
        return Json(json!({ "error": "Cannot delete built-in roles" }));
    }

    state.inner.roles.remove(&role_id);
    state.inner.role_permissions.remove(&role_id);

    Json(json!({ "deleted": true }))
}
