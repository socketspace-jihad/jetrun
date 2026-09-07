use chrono::{Duration, Utc};
use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use jetrun_common::models::AuthUser;

/// JWT claims embedded in the access token
#[derive(Debug, Serialize, Deserialize)]
pub struct Claims {
    pub sub: String,       // user_id
    pub email: String,
    pub username: String,
    pub org_id: Option<String>,
    pub role: String,
    pub permissions: Vec<String>,
    pub exp: i64,          // expiry timestamp
    pub iat: i64,          // issued at
    pub jti: String,       // unique token ID
}

/// Create a signed JWT access token
pub fn create_access_token(
    user: &AuthUser,
    secret: &str,
    ttl_secs: u64,
) -> Result<String, jsonwebtoken::errors::Error> {
    let now = Utc::now();
    let exp = now + Duration::seconds(ttl_secs as i64);

    let claims = Claims {
        sub: user.user_id.to_string(),
        email: user.email.clone(),
        username: user.username.clone(),
        org_id: user.org_id.map(|id| id.to_string()),
        role: user.role.clone(),
        permissions: user.permissions.clone(),
        exp: exp.timestamp(),
        iat: now.timestamp(),
        jti: Uuid::new_v4().to_string(),
    };

    encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(secret.as_bytes()),
    )
}

/// Validate and decode a JWT access token
pub fn validate_access_token(
    token: &str,
    secret: &str,
) -> Result<AuthUser, jsonwebtoken::errors::Error> {
    let token_data = decode::<Claims>(
        token,
        &DecodingKey::from_secret(secret.as_bytes()),
        &Validation::default(),
    )?;

    let claims = token_data.claims;

    Ok(AuthUser {
        user_id: Uuid::parse_str(&claims.sub).unwrap_or_default(),
        email: claims.email,
        username: claims.username,
        org_id: claims.org_id.and_then(|id| Uuid::parse_str(&id).ok()),
        role: claims.role,
        permissions: claims.permissions,
    })
}

/// Generate a random opaque refresh token (base64url-encoded)
pub fn generate_refresh_token() -> String {
    use base64::Engine;
    let mut bytes = [0u8; 32];
    rand::fill(&mut bytes);
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

/// Hash a refresh token for storage (using blake3)
pub fn hash_refresh_token(token: &str) -> String {
    blake3::hash(token.as_bytes()).to_hex().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_user() -> AuthUser {
        AuthUser {
            user_id: Uuid::new_v4(),
            email: "test@example.com".into(),
            username: "testuser".into(),
            org_id: Some(Uuid::new_v4()),
            role: "developer".into(),
            permissions: vec!["build:read".into(), "build:trigger".into()],
        }
    }

    #[test]
    fn test_create_and_validate_token() {
        let user = test_user();
        let secret = "test-secret-key-for-jwt";
        let token = create_access_token(&user, secret, 900).unwrap();

        let decoded = validate_access_token(&token, secret).unwrap();
        assert_eq!(decoded.user_id, user.user_id);
        assert_eq!(decoded.email, user.email);
        assert_eq!(decoded.role, user.role);
        assert_eq!(decoded.permissions, user.permissions);
    }

    #[test]
    fn test_invalid_secret_fails() {
        let user = test_user();
        let token = create_access_token(&user, "correct-secret", 900).unwrap();
        assert!(validate_access_token(&token, "wrong-secret").is_err());
    }

    #[test]
    fn test_expired_token_fails() {
        let user = test_user();
        let secret = "test-secret";
        // Build a token with exp in the past manually
        let claims = Claims {
            sub: user.user_id.to_string(),
            email: user.email,
            username: user.username,
            org_id: None,
            role: user.role,
            permissions: user.permissions,
            exp: (Utc::now() - Duration::seconds(300)).timestamp(),
            iat: (Utc::now() - Duration::seconds(600)).timestamp(),
            jti: Uuid::new_v4().to_string(),
        };
        let token = encode(
            &Header::default(),
            &claims,
            &EncodingKey::from_secret(secret.as_bytes()),
        )
        .unwrap();
        assert!(validate_access_token(&token, secret).is_err());
    }

    #[test]
    fn test_refresh_token_generation() {
        let t1 = generate_refresh_token();
        let t2 = generate_refresh_token();
        assert_ne!(t1, t2);
        assert!(t1.len() >= 32);
    }

    #[test]
    fn test_refresh_token_hash() {
        let token = generate_refresh_token();
        let hash1 = hash_refresh_token(&token);
        let hash2 = hash_refresh_token(&token);
        assert_eq!(hash1, hash2); // Deterministic
        assert_ne!(hash_refresh_token("other"), hash1);
    }
}
