use chrono::{Duration, Utc};
use uuid::Uuid;

use jetrun_common::models::Session;
use jetrun_store::traits::Store;

use super::jwt;

/// Session management functions that operate on the Store.
/// All methods are async and delegate to the SessionRepo trait.

/// Create a new session with a refresh token.
/// Returns (refresh_token, session).
pub async fn create_session(
    store: &dyn Store,
    user_id: Uuid,
    refresh_ttl_days: u64,
    user_agent: Option<String>,
    ip_address: Option<String>,
) -> Option<(String, Session)> {
    let refresh_token = jwt::generate_refresh_token();
    let refresh_token_hash = jwt::hash_refresh_token(&refresh_token);

    let now = Utc::now();
    let session = Session {
        id: Uuid::new_v4(),
        user_id,
        refresh_token_hash,
        user_agent,
        ip_address,
        expires_at: now + Duration::days(refresh_ttl_days as i64),
        created_at: now,
        last_used_at: now,
    };

    match store.create_session(&session).await {
        Ok(()) => Some((refresh_token, session)),
        Err(e) => {
            tracing::error!(error = %e, "failed to create session");
            None
        }
    }
}

/// Validate a refresh token and return the session if valid
pub async fn validate_refresh_token(
    store: &dyn Store,
    refresh_token: &str,
) -> Option<Session> {
    let hash = jwt::hash_refresh_token(refresh_token);

    match store.find_by_refresh_hash(&hash).await {
        Ok(Some(session)) if session.expires_at > Utc::now() => Some(session),
        _ => None,
    }
}

/// Rotate a refresh token: invalidate old, create new.
/// Returns (new_refresh_token, updated_session).
pub async fn rotate_refresh_token(
    store: &dyn Store,
    old_refresh_token: &str,
    refresh_ttl_days: u64,
) -> Option<(String, Session)> {
    let session = validate_refresh_token(store, old_refresh_token).await?;

    // Generate new refresh token
    let new_refresh_token = jwt::generate_refresh_token();
    let new_hash = jwt::hash_refresh_token(&new_refresh_token);

    let now = Utc::now();
    let updated = Session {
        refresh_token_hash: new_hash,
        expires_at: now + Duration::days(refresh_ttl_days as i64),
        last_used_at: now,
        ..session
    };

    match store.update_session(&updated).await {
        Ok(()) => Some((new_refresh_token, updated)),
        Err(e) => {
            tracing::error!(error = %e, "failed to rotate refresh token");
            None
        }
    }
}

/// Get all sessions for a user
pub async fn get_user_sessions(
    store: &dyn Store,
    user_id: Uuid,
) -> Vec<Session> {
    store.list_user_sessions(user_id).await.unwrap_or_default()
}

/// Revoke a specific session
pub async fn revoke_session(
    store: &dyn Store,
    session_id: Uuid,
) -> bool {
    store.delete_session(session_id).await.unwrap_or(false)
}

/// Revoke all sessions for a user
pub async fn revoke_all_user_sessions(
    store: &dyn Store,
    user_id: Uuid,
) -> u64 {
    store.delete_all_user_sessions(user_id).await.unwrap_or(0)
}

/// Remove expired sessions
pub async fn cleanup_expired(store: &dyn Store) -> u64 {
    store.delete_expired().await.unwrap_or(0)
}
