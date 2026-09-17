use axum::{
    extract::{Extension, Path, State},
    routing::{delete, get},
    Json, Router,
};
use chrono::Utc;
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

use jetrun_common::models::AuthUser;

use crate::services::api_key as api_key_svc;
use crate::state::AppState;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/", get(list_api_keys).post(create_api_key))
        .route("/{id}", delete(revoke_api_key))
}

async fn list_api_keys(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
) -> Json<Value> {
    let keys: Vec<Value> = state.store
        .list_user_keys(auth_user.user_id).await
        .unwrap_or_default()
        .into_iter()
        .map(|k| {
            json!({
                "id": k.id,
                "name": k.name,
                "prefix": k.prefix,
                "scopes": k.scopes,
                "last_used_at": k.last_used_at,
                "expires_at": k.expires_at,
                "created_at": k.created_at,
                "revoked_at": k.revoked_at,
            })
        })
        .collect();

    Json(json!({ "api_keys": keys }))
}

#[derive(Debug, Deserialize)]
struct CreateApiKeyRequest {
    name: String,
    #[serde(default)]
    scopes: Vec<String>,
    /// Expiry in days (optional)
    #[serde(default)]
    expires_in_days: Option<u64>,
}

async fn create_api_key(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Json(req): Json<CreateApiKeyRequest>,
) -> Json<Value> {
    let expires_at = req
        .expires_in_days
        .map(|days| Utc::now() + chrono::Duration::days(days as i64));

    let (full_key, api_key) = api_key_svc::generate_api_key(
        auth_user.user_id,
        auth_user.org_id,
        &req.name,
        req.scopes,
        expires_at,
    );

    let key_id = api_key.id;
    if let Err(e) = state.store.create_api_key(&api_key).await {
        tracing::error!(error = %e, "failed to create API key");
        return Json(json!({ "error": "Failed to create API key" }));
    }

    // Return the full key ONCE — it cannot be retrieved later
    Json(json!({
        "id": key_id,
        "key": full_key,
        "name": req.name,
        "message": "Save this key now — it will not be shown again."
    }))
}

async fn revoke_api_key(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Path(key_id): Path<Uuid>,
) -> Json<Value> {
    // First check if the key belongs to the user by looking it up in their keys
    let user_keys = state.store.list_user_keys(auth_user.user_id).await.unwrap_or_default();

    match user_keys.iter().find(|k| k.id == key_id) {
        Some(_) => {
            match state.store.revoke_key(key_id).await {
                Ok(true) => Json(json!({ "revoked": true })),
                _ => Json(json!({ "error": "Failed to revoke API key" })),
            }
        }
        None => Json(json!({ "error": "API key not found" })),
    }
}
