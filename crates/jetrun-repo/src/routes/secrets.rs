use axum::{
    extract::{Path, Query, State},
    routing::{delete, get, post},
    Json, Router,
};
use chrono::Utc;
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

use jetrun_common::models::{Secret, SecretType};

use super::AppState;
use crate::crypto;

#[derive(Debug, Deserialize)]
struct OrgQuery {
    org_id: Option<Uuid>,
}

pub fn admin_routes() -> Router<AppState> {
    Router::new()
        .route("/", get(list_secrets).post(create_secret))
        .route("/generate-key", post(generate_ssh_key))
        .route("/{id}", get(get_secret).delete(delete_secret))
}

/// Developer+ route — only returns names for dropdown
pub fn names_route() -> Router<AppState> {
    Router::new().route("/names", get(list_secret_names))
}

// ── Admin endpoints ──

async fn list_secrets(State(state): State<AppState>, Query(q): Query<OrgQuery>) -> Json<Value> {
    let org_id = q.org_id.unwrap_or(Uuid::nil());
    let secrets = if org_id == Uuid::nil() {
        // No org filter — list all (admin view)
        state.store.list_secrets_by_org(org_id).await.unwrap_or_default()
    } else {
        state.store.list_secrets_by_org(org_id).await.unwrap_or_default()
    };

    let list: Vec<Value> = secrets.into_iter().map(|s| json!({
        "id": s.id,
        "name": s.name,
        "description": s.description,
        "secret_type": s.secret_type,
        "ssh_public_key": s.ssh_public_key,
        "created_at": s.created_at,
        // NEVER include encrypted_value
    })).collect();

    Json(json!({ "secrets": list }))
}

#[derive(Debug, Deserialize)]
struct CreateSecretRequest {
    name: String,
    description: Option<String>,
    secret_type: String,
    /// For token/password: the actual value to encrypt
    value: Option<String>,
    /// For ssh_key: auto-generate if true
    #[serde(default)]
    generate: bool,
    org_id: Option<Uuid>,
    created_by: Option<Uuid>,
}

async fn create_secret(
    State(state): State<AppState>,
    Json(req): Json<CreateSecretRequest>,
) -> Json<Value> {
    let key = crypto::get_encryption_key();

    let secret_type = match req.secret_type.as_str() {
        "ssh_key" => SecretType::SshKey,
        "token" => SecretType::Token,
        "password" => SecretType::Password,
        _ => return Json(json!({ "error": "Invalid secret_type. Use: ssh_key, token, password" })),
    };

    let (encrypted_value, ssh_public_key) = match secret_type {
        SecretType::SshKey => {
            if req.generate {
                // Auto-generate SSH keypair
                match crypto::generate_ssh_keypair() {
                    Ok((private_key, public_key)) => {
                        let encrypted = match crypto::encrypt(&private_key, &key) {
                            Ok(e) => e,
                            Err(e) => return Json(json!({ "error": format!("Encryption failed: {}", e) })),
                        };
                        (encrypted, Some(public_key))
                    }
                    Err(e) => return Json(json!({ "error": format!("Key generation failed: {}", e) })),
                }
            } else {
                // User uploads their own private key
                let value = match req.value {
                    Some(v) => v,
                    None => return Json(json!({ "error": "Provide 'value' (private key) or set 'generate': true" })),
                };
                let encrypted = match crypto::encrypt(&value, &key) {
                    Ok(e) => e,
                    Err(e) => return Json(json!({ "error": format!("Encryption failed: {}", e) })),
                };
                (encrypted, None)
            }
        }
        SecretType::Token | SecretType::Password => {
            let value = match req.value {
                Some(v) => v,
                None => return Json(json!({ "error": "Provide 'value' for token/password secrets" })),
            };
            let encrypted = match crypto::encrypt(&value, &key) {
                Ok(e) => e,
                Err(e) => return Json(json!({ "error": format!("Encryption failed: {}", e) })),
            };
            (encrypted, None)
        }
    };

    let org_id = match req.org_id {
        Some(id) if id != Uuid::nil() => id,
        _ => {
            tracing::warn!(org_id = ?req.org_id, created_by = ?req.created_by, "missing org_id in create secret request");
            return Json(json!({ "error": "org_id is required. Please re-login and try again." }));
        }
    };

    let created_by = match req.created_by {
        Some(id) if id != Uuid::nil() => id,
        _ => {
            return Json(json!({ "error": "created_by (user_id) is required. Please re-login." }));
        }
    };

    let secret = Secret {
        id: Uuid::new_v4(),
        org_id,
        name: req.name.clone(),
        description: req.description,
        secret_type,
        encrypted_value,
        ssh_public_key: ssh_public_key.clone(),
        created_by,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };

    let secret_id = secret.id;
    if let Err(e) = state.store.create_secret(&secret).await {
        tracing::error!(error = %e, "failed to create secret");
        return Json(json!({ "error": "Secret name already exists in this org" }));
    }

    Json(json!({
        "id": secret_id,
        "name": req.name,
        "secret_type": secret_type,
        "ssh_public_key": ssh_public_key,
        "created": true,
    }))
}

async fn generate_ssh_key(State(state): State<AppState>) -> Json<Value> {
    // Convenience endpoint: generate + store in one call
    let key = crypto::get_encryption_key();

    match crypto::generate_ssh_keypair() {
        Ok((private_key, public_key)) => {
            let encrypted = match crypto::encrypt(&private_key, &key) {
                Ok(e) => e,
                Err(e) => return Json(json!({ "error": format!("Encryption failed: {}", e) })),
            };

            Json(json!({
                "private_key_encrypted": true,
                "public_key": public_key,
                "message": "Add this public key as a Deploy Key in your repository settings."
            }))
        }
        Err(e) => Json(json!({ "error": format!("Key generation failed: {}", e) })),
    }
}

async fn get_secret(State(state): State<AppState>, Path(id): Path<Uuid>) -> Json<Value> {
    match state.store.find_secret_by_id(id).await.ok().flatten() {
        Some(s) => Json(json!({
            "id": s.id,
            "name": s.name,
            "description": s.description,
            "secret_type": s.secret_type,
            "ssh_public_key": s.ssh_public_key,
            "created_at": s.created_at,
            // NEVER include encrypted_value
        })),
        None => Json(json!({ "error": "Secret not found" })),
    }
}

async fn delete_secret(State(state): State<AppState>, Path(id): Path<Uuid>) -> Json<Value> {
    match state.store.delete_secret(id).await {
        Ok(true) => Json(json!({ "deleted": true })),
        _ => Json(json!({ "error": "Secret not found" })),
    }
}

// ── Developer endpoint (names only for dropdown) ──

async fn list_secret_names(State(state): State<AppState>, Query(q): Query<OrgQuery>) -> Json<Value> {
    let org_id = q.org_id.unwrap_or(Uuid::nil());
    let names = state.store.list_secret_names_by_org(org_id).await.unwrap_or_default();

    let list: Vec<Value> = names.into_iter().map(|(id, name, secret_type)| json!({
        "id": id,
        "name": name,
        "type": secret_type,
    })).collect();

    Json(json!({ "secrets": list }))
}
