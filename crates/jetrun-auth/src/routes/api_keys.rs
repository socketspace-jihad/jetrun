use axum::{
    extract::{Extension, Path, State},
    routing::{delete, get, post},
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
    let keys: Vec<Value> = state
        .inner
        .api_keys
        .iter()
        .filter(|e| e.value().user_id == auth_user.user_id)
        .map(|e| {
            let k = e.value();
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
    state.inner.api_keys.insert(key_id, api_key);

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
    match state.inner.api_keys.get_mut(&key_id) {
        Some(mut key) => {
            if key.user_id != auth_user.user_id {
                return Json(json!({ "error": "Not your API key" }));
            }
            key.revoked_at = Some(Utc::now());
            Json(json!({ "revoked": true }))
        }
        None => Json(json!({ "error": "API key not found" })),
    }
}
