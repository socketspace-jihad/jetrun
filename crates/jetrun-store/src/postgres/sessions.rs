use async_trait::async_trait;
use uuid::Uuid;

use jetrun_common::models::Session;

use super::PgStore;
use crate::error::StoreError;
use crate::traits::SessionRepo;

#[async_trait]
impl SessionRepo for PgStore {
    async fn create_session(&self, s: &Session) -> Result<(), StoreError> {
        sqlx::query(
            "INSERT INTO sessions (id, user_id, refresh_token_hash, user_agent, ip_address, expires_at, created_at, last_used_at) VALUES ($1,$2,$3,$4,$5,$6,$7,$8)"
        ).bind(s.id).bind(s.user_id).bind(&s.refresh_token_hash).bind(&s.user_agent).bind(&s.ip_address)
        .bind(s.expires_at).bind(s.created_at).bind(s.last_used_at)
        .execute(&self.pool).await?;
        Ok(())
    }

    async fn find_by_refresh_hash(&self, hash: &str) -> Result<Option<Session>, StoreError> {
        let row = sqlx::query_as::<_, SessionRow>(
            "SELECT id, user_id, refresh_token_hash, user_agent, ip_address, expires_at, created_at, last_used_at FROM sessions WHERE refresh_token_hash = $1 AND expires_at > now()"
        ).bind(hash).fetch_optional(&self.pool).await?;
        Ok(row.map(|r| r.into()))
    }

    async fn update_session(&self, s: &Session) -> Result<(), StoreError> {
        sqlx::query("UPDATE sessions SET refresh_token_hash = $2, expires_at = $3, last_used_at = $4 WHERE id = $1")
            .bind(s.id).bind(&s.refresh_token_hash).bind(s.expires_at).bind(s.last_used_at)
            .execute(&self.pool).await?;
        Ok(())
    }

    async fn delete_session(&self, id: Uuid) -> Result<bool, StoreError> {
        let r = sqlx::query("DELETE FROM sessions WHERE id = $1").bind(id).execute(&self.pool).await?;
        Ok(r.rows_affected() > 0)
    }

    async fn list_user_sessions(&self, user_id: Uuid) -> Result<Vec<Session>, StoreError> {
        let rows = sqlx::query_as::<_, SessionRow>(
            "SELECT id, user_id, refresh_token_hash, user_agent, ip_address, expires_at, created_at, last_used_at FROM sessions WHERE user_id = $1 AND expires_at > now() ORDER BY last_used_at DESC"
        ).bind(user_id).fetch_all(&self.pool).await?;
        Ok(rows.into_iter().map(|r| r.into()).collect())
    }

    async fn delete_all_user_sessions(&self, user_id: Uuid) -> Result<u64, StoreError> {
        let r = sqlx::query("DELETE FROM sessions WHERE user_id = $1").bind(user_id).execute(&self.pool).await?;
        Ok(r.rows_affected())
    }

    async fn delete_expired(&self) -> Result<u64, StoreError> {
        let r = sqlx::query("DELETE FROM sessions WHERE expires_at < now()").execute(&self.pool).await?;
        Ok(r.rows_affected())
    }
}

#[derive(sqlx::FromRow)]
struct SessionRow {
    id: Uuid, user_id: Uuid, refresh_token_hash: String,
    user_agent: Option<String>, ip_address: Option<String>,
    expires_at: chrono::DateTime<chrono::Utc>,
    created_at: chrono::DateTime<chrono::Utc>,
    last_used_at: chrono::DateTime<chrono::Utc>,
}

impl From<SessionRow> for Session {
    fn from(r: SessionRow) -> Self {
        Session { id: r.id, user_id: r.user_id, refresh_token_hash: r.refresh_token_hash, user_agent: r.user_agent, ip_address: r.ip_address, expires_at: r.expires_at, created_at: r.created_at, last_used_at: r.last_used_at }
    }
}
