use async_trait::async_trait;
use uuid::Uuid;

use jetrun_common::models::ApiKey;

use super::PgStore;
use crate::error::StoreError;
use crate::traits::ApiKeyRepo;

#[async_trait]
impl ApiKeyRepo for PgStore {
    async fn create_api_key(&self, k: &ApiKey) -> Result<(), StoreError> {
        let scopes_json = serde_json::to_value(&k.scopes).unwrap_or_default();
        sqlx::query(
            "INSERT INTO api_keys (id, user_id, org_id, name, prefix, key_hash, scopes, expires_at, created_at) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9)"
        ).bind(k.id).bind(k.user_id).bind(k.org_id).bind(&k.name).bind(&k.prefix).bind(&k.key_hash)
        .bind(scopes_json).bind(k.expires_at).bind(k.created_at)
        .execute(&self.pool).await?;
        Ok(())
    }

    async fn find_by_prefix(&self, prefix: &str) -> Result<Option<ApiKey>, StoreError> {
        let row = sqlx::query_as::<_, ApiKeyRow>(
            "SELECT id, user_id, org_id, name, prefix, key_hash, scopes, last_used_at, expires_at, created_at, revoked_at FROM api_keys WHERE prefix = $1"
        ).bind(prefix).fetch_optional(&self.pool).await?;
        Ok(row.map(|r| r.into()))
    }

    async fn revoke_key(&self, id: Uuid) -> Result<bool, StoreError> {
        let r = sqlx::query("UPDATE api_keys SET revoked_at = now() WHERE id = $1 AND revoked_at IS NULL")
            .bind(id).execute(&self.pool).await?;
        Ok(r.rows_affected() > 0)
    }

    async fn list_user_keys(&self, user_id: Uuid) -> Result<Vec<ApiKey>, StoreError> {
        let rows = sqlx::query_as::<_, ApiKeyRow>(
            "SELECT id, user_id, org_id, name, prefix, key_hash, scopes, last_used_at, expires_at, created_at, revoked_at FROM api_keys WHERE user_id = $1 ORDER BY created_at DESC"
        ).bind(user_id).fetch_all(&self.pool).await?;
        Ok(rows.into_iter().map(|r| r.into()).collect())
    }
}

#[derive(sqlx::FromRow)]
struct ApiKeyRow {
    id: Uuid, user_id: Uuid, org_id: Option<Uuid>, name: String, prefix: String, key_hash: String,
    scopes: serde_json::Value,
    last_used_at: Option<chrono::DateTime<chrono::Utc>>,
    expires_at: Option<chrono::DateTime<chrono::Utc>>,
    created_at: chrono::DateTime<chrono::Utc>,
    revoked_at: Option<chrono::DateTime<chrono::Utc>>,
}

impl From<ApiKeyRow> for ApiKey {
    fn from(r: ApiKeyRow) -> Self {
        let scopes: Vec<String> = serde_json::from_value(r.scopes).unwrap_or_default();
        ApiKey { id: r.id, user_id: r.user_id, org_id: r.org_id, name: r.name, prefix: r.prefix, key_hash: r.key_hash, scopes, last_used_at: r.last_used_at, expires_at: r.expires_at, created_at: r.created_at, revoked_at: r.revoked_at }
    }
}
