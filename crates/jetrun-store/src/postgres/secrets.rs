use async_trait::async_trait;
use uuid::Uuid;

use jetrun_common::models::{Secret, SecretType};

use super::PgStore;
use crate::error::StoreError;
use crate::traits::SecretRepo;

#[async_trait]
impl SecretRepo for PgStore {
    async fn create_secret(&self, s: &Secret) -> Result<(), StoreError> {
        sqlx::query(
            "INSERT INTO secrets (id, org_id, name, description, secret_type, encrypted_value, ssh_public_key, created_by, created_at, updated_at)
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10)"
        )
        .bind(s.id).bind(s.org_id).bind(&s.name).bind(&s.description)
        .bind(s.secret_type.to_string()).bind(&s.encrypted_value)
        .bind(&s.ssh_public_key).bind(s.created_by)
        .bind(s.created_at).bind(s.updated_at)
        .execute(&self.pool).await?;
        Ok(())
    }

    async fn find_secret_by_id(&self, id: Uuid) -> Result<Option<Secret>, StoreError> {
        let row = sqlx::query_as::<_, SecretRow>(
            "SELECT id, org_id, name, description, secret_type, encrypted_value, ssh_public_key, created_by, created_at, updated_at FROM secrets WHERE id = $1"
        ).bind(id).fetch_optional(&self.pool).await?;
        Ok(row.map(|r| r.into()))
    }

    async fn list_secrets_by_org(&self, org_id: Uuid) -> Result<Vec<Secret>, StoreError> {
        let rows = sqlx::query_as::<_, SecretRow>(
            "SELECT id, org_id, name, description, secret_type, encrypted_value, ssh_public_key, created_by, created_at, updated_at FROM secrets WHERE org_id = $1 ORDER BY name"
        ).bind(org_id).fetch_all(&self.pool).await?;
        Ok(rows.into_iter().map(|r| r.into()).collect())
    }

    async fn delete_secret(&self, id: Uuid) -> Result<bool, StoreError> {
        let r = sqlx::query("DELETE FROM secrets WHERE id = $1").bind(id).execute(&self.pool).await?;
        Ok(r.rows_affected() > 0)
    }

    async fn list_secret_names_by_org(&self, org_id: Uuid) -> Result<Vec<(Uuid, String, String)>, StoreError> {
        let rows: Vec<(Uuid, String, String)> = sqlx::query_as(
            "SELECT id, name, secret_type FROM secrets WHERE org_id = $1 ORDER BY name"
        ).bind(org_id).fetch_all(&self.pool).await?;
        Ok(rows)
    }
}

#[derive(sqlx::FromRow)]
struct SecretRow {
    id: Uuid, org_id: Uuid, name: String, description: Option<String>,
    secret_type: String, encrypted_value: String, ssh_public_key: Option<String>,
    created_by: Uuid,
    created_at: chrono::DateTime<chrono::Utc>, updated_at: chrono::DateTime<chrono::Utc>,
}

impl From<SecretRow> for Secret {
    fn from(r: SecretRow) -> Self {
        Secret {
            id: r.id, org_id: r.org_id, name: r.name, description: r.description,
            secret_type: match r.secret_type.as_str() {
                "token" => SecretType::Token,
                "password" => SecretType::Password,
                _ => SecretType::SshKey,
            },
            encrypted_value: r.encrypted_value,
            ssh_public_key: r.ssh_public_key,
            created_by: r.created_by,
            created_at: r.created_at, updated_at: r.updated_at,
        }
    }
}
