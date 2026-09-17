use async_trait::async_trait;
use uuid::Uuid;

use jetrun_common::models::{Permission, Role};

use super::PgStore;
use crate::error::StoreError;
use crate::traits::RoleRepo;

#[async_trait]
impl RoleRepo for PgStore {
    async fn create_role(&self, role: &Role) -> Result<(), StoreError> {
        sqlx::query(
            "INSERT INTO roles (id, name, display_name, description, is_builtin, org_id, created_at, updated_at) VALUES ($1,$2,$3,$4,$5,$6,$7,$8) ON CONFLICT DO NOTHING"
        ).bind(role.id).bind(&role.name).bind(&role.display_name).bind(&role.description)
        .bind(role.is_builtin).bind(role.org_id).bind(role.created_at).bind(role.updated_at)
        .execute(&self.pool).await?;
        Ok(())
    }

    async fn find_role_by_id(&self, id: Uuid) -> Result<Option<Role>, StoreError> {
        let row = sqlx::query_as::<_, RoleRow>("SELECT id, name, display_name, description, is_builtin, org_id, created_at, updated_at FROM roles WHERE id = $1")
            .bind(id).fetch_optional(&self.pool).await?;
        Ok(row.map(|r| r.into()))
    }

    async fn find_builtin_role(&self, name: &str) -> Result<Option<Role>, StoreError> {
        let row = sqlx::query_as::<_, RoleRow>("SELECT id, name, display_name, description, is_builtin, org_id, created_at, updated_at FROM roles WHERE name = $1 AND is_builtin = true")
            .bind(name).fetch_optional(&self.pool).await?;
        Ok(row.map(|r| r.into()))
    }

    async fn list_roles(&self) -> Result<Vec<Role>, StoreError> {
        let rows = sqlx::query_as::<_, RoleRow>("SELECT id, name, display_name, description, is_builtin, org_id, created_at, updated_at FROM roles ORDER BY is_builtin DESC, name")
            .fetch_all(&self.pool).await?;
        Ok(rows.into_iter().map(|r| r.into()).collect())
    }

    async fn update_role(&self, role: &Role) -> Result<(), StoreError> {
        sqlx::query("UPDATE roles SET display_name = $2, description = $3, updated_at = $4 WHERE id = $1")
            .bind(role.id).bind(&role.display_name).bind(&role.description).bind(role.updated_at)
            .execute(&self.pool).await?;
        Ok(())
    }

    async fn delete_role(&self, id: Uuid) -> Result<bool, StoreError> {
        let result = sqlx::query("DELETE FROM roles WHERE id = $1 AND is_builtin = false")
            .bind(id).execute(&self.pool).await?;
        Ok(result.rows_affected() > 0)
    }

    async fn create_permission(&self, perm: &Permission) -> Result<(), StoreError> {
        sqlx::query("INSERT INTO permissions (id, name, description, resource, action) VALUES ($1,$2,$3,$4,$5) ON CONFLICT (name) DO NOTHING")
            .bind(perm.id).bind(&perm.name).bind(&perm.description).bind(&perm.resource).bind(&perm.action)
            .execute(&self.pool).await?;
        Ok(())
    }

    async fn find_permission_by_name(&self, name: &str) -> Result<Option<Permission>, StoreError> {
        let row = sqlx::query_as::<_, PermRow>("SELECT id, name, description, resource, action FROM permissions WHERE name = $1")
            .bind(name).fetch_optional(&self.pool).await?;
        Ok(row.map(|r| Permission { id: r.id, name: r.name, description: r.description, resource: r.resource, action: r.action }))
    }

    async fn list_permissions(&self) -> Result<Vec<Permission>, StoreError> {
        let rows = sqlx::query_as::<_, PermRow>("SELECT id, name, description, resource, action FROM permissions ORDER BY resource, action")
            .fetch_all(&self.pool).await?;
        Ok(rows.into_iter().map(|r| Permission { id: r.id, name: r.name, description: r.description, resource: r.resource, action: r.action }).collect())
    }

    async fn set_role_permissions(&self, role_id: Uuid, perm_ids: &[Uuid]) -> Result<(), StoreError> {
        sqlx::query("DELETE FROM role_permissions WHERE role_id = $1").bind(role_id).execute(&self.pool).await?;
        for pid in perm_ids {
            sqlx::query("INSERT INTO role_permissions (role_id, permission_id) VALUES ($1, $2)")
                .bind(role_id).bind(pid).execute(&self.pool).await?;
        }
        Ok(())
    }

    async fn get_role_permission_names(&self, role_id: Uuid) -> Result<Vec<String>, StoreError> {
        let rows: Vec<(String,)> = sqlx::query_as(
            "SELECT p.name FROM role_permissions rp JOIN permissions p ON p.id = rp.permission_id WHERE rp.role_id = $1"
        ).bind(role_id).fetch_all(&self.pool).await?;
        Ok(rows.into_iter().map(|r| r.0).collect())
    }
}

#[derive(sqlx::FromRow)]
struct RoleRow {
    id: Uuid, name: String, display_name: String, description: Option<String>,
    is_builtin: bool, org_id: Option<Uuid>,
    created_at: chrono::DateTime<chrono::Utc>, updated_at: chrono::DateTime<chrono::Utc>,
}

impl From<RoleRow> for Role {
    fn from(r: RoleRow) -> Self {
        Role { id: r.id, name: r.name, display_name: r.display_name, description: r.description, is_builtin: r.is_builtin, org_id: r.org_id, created_at: r.created_at, updated_at: r.updated_at }
    }
}

#[derive(sqlx::FromRow)]
struct PermRow { id: Uuid, name: String, description: String, resource: String, action: String }
