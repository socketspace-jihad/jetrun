use chrono::{Duration, Utc};
use dashmap::DashMap;
use uuid::Uuid;

use jetrun_common::models::Session;

use super::jwt;

/// In-memory session store (will be backed by DB in production)
pub struct SessionStore {
    sessions: DashMap<Uuid, Session>,
}

impl SessionStore {
    pub fn new() -> Self {
        Self {
            sessions: DashMap::new(),
        }
    }

    /// Create a new session with a refresh token.
    /// Returns (refresh_token, session).
    pub fn create_session(
        &self,
        user_id: Uuid,
        refresh_ttl_days: u64,
        user_agent: Option<String>,
        ip_address: Option<String>,
    ) -> (String, Session) {
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

        self.sessions.insert(session.id, session.clone());
        (refresh_token, session)
    }

    /// Validate a refresh token and return the session if valid
    pub fn validate_refresh_token(&self, refresh_token: &str) -> Option<Session> {
        let hash = jwt::hash_refresh_token(refresh_token);

        self.sessions.iter().find_map(|entry| {
            let session = entry.value();
            if session.refresh_token_hash == hash && session.expires_at > Utc::now() {
                Some(session.clone())
            } else {
                None
            }
        })
    }

    /// Rotate a refresh token: invalidate old, create new.
    /// Returns (new_refresh_token, updated_session).
    pub fn rotate_refresh_token(
        &self,
        old_refresh_token: &str,
        refresh_ttl_days: u64,
    ) -> Option<(String, Session)> {
        let session = self.validate_refresh_token(old_refresh_token)?;

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

        self.sessions.insert(updated.id, updated.clone());
        Some((new_refresh_token, updated))
    }

    /// Get all sessions for a user
    pub fn get_user_sessions(&self, user_id: Uuid) -> Vec<Session> {
        self.sessions
            .iter()
            .filter(|e| e.value().user_id == user_id)
            .map(|e| e.value().clone())
            .collect()
    }

    /// Revoke a specific session
    pub fn revoke_session(&self, session_id: Uuid) -> bool {
        self.sessions.remove(&session_id).is_some()
    }

    /// Revoke all sessions for a user
    pub fn revoke_all_user_sessions(&self, user_id: Uuid) -> u64 {
        let to_remove: Vec<Uuid> = self
            .sessions
            .iter()
            .filter(|e| e.value().user_id == user_id)
            .map(|e| *e.key())
            .collect();

        let count = to_remove.len() as u64;
        for id in to_remove {
            self.sessions.remove(&id);
        }
        count
    }

    /// Remove expired sessions
    pub fn cleanup_expired(&self) -> u64 {
        let now = Utc::now();
        let expired: Vec<Uuid> = self
            .sessions
            .iter()
            .filter(|e| e.value().expires_at < now)
            .map(|e| *e.key())
            .collect();

        let count = expired.len() as u64;
        for id in expired {
            self.sessions.remove(&id);
        }
        count
    }
}

impl Default for SessionStore {
    fn default() -> Self {
        Self::new()
    }
}
