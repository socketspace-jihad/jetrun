use async_trait::async_trait;
use uuid::Uuid;

use jetrun_common::models::{Pipeline, PipelineConfig};

use super::PgStore;
use crate::error::StoreError;
use crate::traits::PipelineRepo;

#[async_trait]
impl PipelineRepo for PgStore {
    async fn create_pipeline(&self, p: &Pipeline) -> Result<(), StoreError> {
        let config_json = serde_json::to_value(&p.config).unwrap_or_default();
        sqlx::query(
            "INSERT INTO pipelines (id, project_id, name, description, config_path, config, active, created_at, updated_at) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9)"
        ).bind(p.id).bind(p.project_id).bind(&p.name).bind(&p.description).bind(&p.config_path)
        .bind(config_json).bind(p.active).bind(p.created_at).bind(p.updated_at)
        .execute(&self.pool).await?;
        Ok(())
    }

    async fn find_pipeline_by_id(&self, id: Uuid) -> Result<Option<Pipeline>, StoreError> {
        let row = sqlx::query_as::<_, PipelineRow>(
            "SELECT id, project_id, name, description, config_path, config, active, created_at, updated_at FROM pipelines WHERE id = $1"
        ).bind(id).fetch_optional(&self.pool).await?;
        Ok(row.map(|r| r.into()))
    }

    async fn list_pipelines(&self) -> Result<Vec<Pipeline>, StoreError> {
        let rows = sqlx::query_as::<_, PipelineRow>(
            "SELECT id, project_id, name, description, config_path, config, active, created_at, updated_at FROM pipelines ORDER BY updated_at DESC"
        ).fetch_all(&self.pool).await?;
        Ok(rows.into_iter().map(|r| r.into()).collect())
    }

    async fn delete_pipeline(&self, id: Uuid) -> Result<bool, StoreError> {
        let r = sqlx::query("DELETE FROM pipelines WHERE id = $1").bind(id).execute(&self.pool).await?;
        Ok(r.rows_affected() > 0)
    }
}

#[derive(sqlx::FromRow)]
struct PipelineRow {
    id: Uuid, project_id: Uuid, name: String, description: Option<String>,
    config_path: String, config: serde_json::Value, active: bool,
    created_at: chrono::DateTime<chrono::Utc>, updated_at: chrono::DateTime<chrono::Utc>,
}

impl From<PipelineRow> for Pipeline {
    fn from(r: PipelineRow) -> Self {
        let config: PipelineConfig = serde_json::from_value(r.config).unwrap_or_else(|_| PipelineConfig {
            name: r.name.clone(), description: None,
            on: jetrun_common::models::TriggerConfig { push: None, pull_request: None, webhook: false, schedule: None },
            env: Default::default(), stages: vec![], cache: None,
        });
        Pipeline { id: r.id, project_id: r.project_id, name: r.name, description: r.description, config_path: r.config_path, config, active: r.active, created_at: r.created_at, updated_at: r.updated_at }
    }
}
