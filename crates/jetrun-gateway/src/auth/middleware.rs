use axum::{
    extract::Request,
    http::StatusCode,
    middleware::Next,
    response::Response,
};
use jsonwebtoken::{decode, DecodingKey, Validation};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use jetrun_common::models::AuthUser;

/// JWT claims — must match the structure created by jetrun-auth
#[derive(Debug, Serialize, Deserialize)]
struct Claims {
    sub: String,
    email: String,
    username: String,
    org_id: Option<String>,
    role: String,
    permissions: Vec<String>,
    exp: i64,
    iat: i64,
    jti: String,
}

/// Authentication middleware — validates JWT Bearer tokens or API keys.
/// Injects `AuthUser` into request extensions on success.
/// Skips auth for webhook routes (they use signature verification).
pub async fn auth_middleware(request: Request, next: Next) -> Result<Response, StatusCode> {
    let jwt_secret = std::env::var("JWT_SECRET").unwrap_or_else(|_| "dev-secret-change-in-production".into());

    let auth_header = request
        .headers()
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .map(String::from);

    let mut request = request;

    if let Some(header) = auth_header {
        if let Some(token) = header.strip_prefix("Bearer ") {
            // Validate JWT locally — no network call
            match decode::<Claims>(
                token,
                &DecodingKey::from_secret(jwt_secret.as_bytes()),
                &Validation::default(),
            ) {
                Ok(token_data) => {
                    let claims = token_data.claims;
                    let auth_user = AuthUser {
                        user_id: Uuid::parse_str(&claims.sub).unwrap_or_default(),
                        email: claims.email,
                        username: claims.username,
                        org_id: claims.org_id.and_then(|id| Uuid::parse_str(&id).ok()),
                        role: claims.role,
                        permissions: claims.permissions,
                    };
                    request.extensions_mut().insert(auth_user);
                }
                Err(e) => {
                    tracing::debug!(error = %e, "JWT validation failed");
                    return Err(StatusCode::UNAUTHORIZED);
                }
            }
        }
    }

    // If no auth header, proceed without AuthUser in extensions.
    // Individual route handlers can check for AuthUser and return 401 if needed.
    Ok(next.run(request).await)
}
