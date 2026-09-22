use async_trait::async_trait;
use std::collections::HashMap;

use super::PgStore;
use crate::error::StoreError;
use crate::traits::SettingsRepo;

#[async_trait]
impl SettingsRepo for PgStore {
    async fn get_setting(&self, key: &str) -> Result<Option<String>, StoreError> {
        let row = sqlx::query_scalar::<_, String>(
            "SELECT value FROM system_settings WHERE key = $1"
        ).bind(key).fetch_optional(&self.pool).await?;
        Ok(row)
    }

    async fn set_setting(&self, key: &str, value: &str) -> Result<(), StoreError> {
        sqlx::query(
            "INSERT INTO system_settings (key, value, updated_at) VALUES ($1, $2, NOW()) ON CONFLICT (key) DO UPDATE SET value = $2, updated_at = NOW()"
        ).bind(key).bind(value)
        .execute(&self.pool).await?;
        Ok(())
    }

    async fn get_all_settings(&self, prefix: &str) -> Result<HashMap<String, String>, StoreError> {
        let pattern = format!("{}%", prefix);
        let rows = sqlx::query_as::<_, (String, String)>(
            "SELECT key, value FROM system_settings WHERE key LIKE $1"
        ).bind(pattern).fetch_all(&self.pool).await?;
        Ok(rows.into_iter().collect())
    }
}
