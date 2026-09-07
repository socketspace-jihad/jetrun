use axum::{
    extract::{Request, State},
    http::StatusCode,
    middleware::Next,
    response::Response,
};

use jetrun_common::models::AuthUser;

use crate::services::jwt;
use crate::state::AppState;

/// Middleware that extracts and validates auth credentials from the request.
/// Supports JWT Bearer tokens and API keys.
/// Injects `AuthUser` into request extensions on success.
pub async fn auth_middleware(
    State(state): State<AppState>,
    mut request: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    let auth_header = request
        .headers()
        .get("authorization")
        .and_then(|v| v.to_str().ok());

    let api_key_header = request
        .headers()
        .get("x-api-key")
        .and_then(|v| v.to_str().ok());

    let auth_user = if let Some(header) = auth_header {
        // JWT Bearer token
        if let Some(token) = header.strip_prefix("Bearer ") {
            jwt::validate_access_token(token, &state.config.jwt_secret)
                .map_err(|_| StatusCode::UNAUTHORIZED)?
        } else {
            return Err(StatusCode::UNAUTHORIZED);
        }
    } else if let Some(raw_key) = api_key_header {
        // API Key authentication
        validate_api_key_auth(&state, raw_key)?
    } else {
        return Err(StatusCode::UNAUTHORIZED);
    };

    request.extensions_mut().insert(auth_user);
    Ok(next.run(request).await)
}

fn validate_api_key_auth(state: &AppState, raw_key: &str) -> Result<AuthUser, StatusCode> {
    // Extract prefix for lookup
    let prefix = if raw_key.len() >= 16 {
        &raw_key[..16]
    } else {
        return Err(StatusCode::UNAUTHORIZED);
    };

    let api_key = state
        .find_api_key_by_prefix(prefix)
        .ok_or(StatusCode::UNAUTHORIZED)?;

    if !api_key.is_valid() {
        return Err(StatusCode::UNAUTHORIZED);
    }

    // Verify the full key hash
    if !crate::services::api_key::validate_api_key(raw_key, &api_key.key_hash) {
        return Err(StatusCode::UNAUTHORIZED);
    }

    // Get the user
    let user = state
        .inner
        .users
        .get(&api_key.user_id)
        .map(|u| u.value().clone())
        .ok_or(StatusCode::UNAUTHORIZED)?;

    if !user.is_active {
        return Err(StatusCode::UNAUTHORIZED);
    }

    // API key scopes override role permissions
    Ok(AuthUser {
        user_id: user.id,
        email: user.email,
        username: user.username,
        org_id: api_key.org_id,
        role: "api_key".into(),
        permissions: api_key.scopes,
    })
}

/// Axum extractor for requiring specific permissions.
/// Use in route handlers: `RequirePermission<"pipeline:create">`
pub async fn require_permission(
    auth_user: &AuthUser,
    permission: &str,
) -> Result<(), StatusCode> {
    if auth_user.has_permission(permission) {
        Ok(())
    } else {
        Err(StatusCode::FORBIDDEN)
    }
}
