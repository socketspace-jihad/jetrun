use async_trait::async_trait;
use uuid::Uuid;

use jetrun_common::models::Project;

use super::PgStore;
use crate::error::StoreError;
use crate::traits::ProjectRepo;

#[async_trait]
impl ProjectRepo for PgStore {
    async fn create_project(&self, p: &Project) -> Result<(), StoreError> {
        sqlx::query(
            "INSERT INTO projects (id, org_id, name, slug, repo_url, default_branch, webhook_secret, config_path, created_at, updated_at)
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10)"
        )
        .bind(p.id).bind(p.org_id).bind(&p.name).bind(&p.slug)
        .bind(&p.repo_url).bind(&p.default_branch).bind(&p.webhook_secret)
        .bind(&p.config_path).bind(p.created_at).bind(p.updated_at)
        .execute(&self.pool).await?;
        Ok(())
    }

    async fn find_project_by_id(&self, id: Uuid) -> Result<Option<Project>, StoreError> {
        let row = sqlx::query_as::<_, ProjectRow>(
            "SELECT id, org_id, name, slug, repo_url, default_branch, webhook_secret, config_path, created_at, updated_at FROM projects WHERE id = $1"
        ).bind(id).fetch_optional(&self.pool).await?;
        Ok(row.map(|r| r.into()))
    }

    async fn list_projects_by_org(&self, org_id: Uuid) -> Result<Vec<Project>, StoreError> {
        let rows = sqlx::query_as::<_, ProjectRow>(
            "SELECT id, org_id, name, slug, repo_url, default_branch, webhook_secret, config_path, created_at, updated_at FROM projects WHERE org_id = $1 ORDER BY updated_at DESC"
        ).bind(org_id).fetch_all(&self.pool).await?;
        Ok(rows.into_iter().map(|r| r.into()).collect())
    }

    async fn list_all_projects(&self) -> Result<Vec<Project>, StoreError> {
        let rows = sqlx::query_as::<_, ProjectRow>(
            "SELECT id, org_id, name, slug, repo_url, default_branch, webhook_secret, config_path, created_at, updated_at FROM projects ORDER BY updated_at DESC"
        ).fetch_all(&self.pool).await?;
        Ok(rows.into_iter().map(|r| r.into()).collect())
    }

    async fn update_project(&self, p: &Project) -> Result<(), StoreError> {
        sqlx::query(
            "UPDATE projects SET name=$2, repo_url=$3, default_branch=$4, config_path=$5, updated_at=$6 WHERE id=$1"
        )
        .bind(p.id).bind(&p.name).bind(&p.repo_url).bind(&p.default_branch)
        .bind(&p.config_path).bind(p.updated_at)
        .execute(&self.pool).await?;
        Ok(())
    }

    async fn delete_project(&self, id: Uuid) -> Result<bool, StoreError> {
        let r = sqlx::query("DELETE FROM projects WHERE id = $1").bind(id).execute(&self.pool).await?;
        Ok(r.rows_affected() > 0)
    }
}

#[derive(sqlx::FromRow)]
struct ProjectRow {
    id: Uuid, org_id: Option<Uuid>, name: String, slug: String,
    repo_url: String, default_branch: String, webhook_secret: Option<String>,
    config_path: String,
    created_at: chrono::DateTime<chrono::Utc>, updated_at: chrono::DateTime<chrono::Utc>,
}

impl From<ProjectRow> for Project {
    fn from(r: ProjectRow) -> Self {
        Project { id: r.id, org_id: r.org_id, name: r.name, slug: r.slug, repo_url: r.repo_url, default_branch: r.default_branch, webhook_secret: r.webhook_secret, config_path: r.config_path, created_at: r.created_at, updated_at: r.updated_at }
    }
}
