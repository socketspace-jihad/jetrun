use async_trait::async_trait;
use uuid::Uuid;

use jetrun_common::models::{Build, BuildStage, BuildStatus, BuildTrigger};

use super::PgStore;
use crate::error::StoreError;
use crate::traits::BuildRepo;

#[async_trait]
impl BuildRepo for PgStore {
    async fn create_build(&self, b: &Build) -> Result<(), StoreError> {
        let stages_json = serde_json::to_value(&b.stages).unwrap_or_default();
        let matrix_json = b.matrix_values.as_ref().map(|m| serde_json::to_value(m).unwrap_or_default());
        sqlx::query(
            "INSERT INTO builds (id, pipeline_id, number, status, trigger, commit_sha, branch, matrix_values, stages, started_at, finished_at, created_at)
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12)"
        )
        .bind(b.id).bind(b.pipeline_id).bind(b.number as i64)
        .bind(status_to_str(b.status)).bind(trigger_to_str(&b.trigger))
        .bind(&b.commit_sha).bind(&b.branch).bind(matrix_json).bind(stages_json)
        .bind(b.started_at).bind(b.finished_at).bind(b.created_at)
        .execute(&self.pool).await?;
        Ok(())
    }

    async fn find_build_by_id(&self, id: Uuid) -> Result<Option<Build>, StoreError> {
        let row = sqlx::query_as::<_, BuildRow>(
            "SELECT id, pipeline_id, number, status, trigger, commit_sha, branch, matrix_values, stages, started_at, finished_at, created_at FROM builds WHERE id = $1"
        ).bind(id).fetch_optional(&self.pool).await?;
        Ok(row.map(|r| r.into()))
    }

    async fn list_builds(&self, limit: i64) -> Result<Vec<Build>, StoreError> {
        let rows = sqlx::query_as::<_, BuildRow>(
            "SELECT id, pipeline_id, number, status, trigger, commit_sha, branch, matrix_values, stages, started_at, finished_at, created_at FROM builds ORDER BY created_at DESC LIMIT $1"
        ).bind(limit).fetch_all(&self.pool).await?;
        Ok(rows.into_iter().map(|r| r.into()).collect())
    }

    async fn update_build_status(&self, id: Uuid, status: BuildStatus, finished_at: Option<chrono::DateTime<chrono::Utc>>) -> Result<(), StoreError> {
        sqlx::query("UPDATE builds SET status = $2, finished_at = $3 WHERE id = $1")
            .bind(id).bind(status_to_str(status)).bind(finished_at)
            .execute(&self.pool).await?;
        Ok(())
    }

    async fn update_build_stages(&self, id: Uuid, stages: &[jetrun_common::models::BuildStage]) -> Result<(), StoreError> {
        let stages_json = serde_json::to_value(stages).unwrap_or_default();
        sqlx::query("UPDATE builds SET stages = $2 WHERE id = $1")
            .bind(id).bind(stages_json)
            .execute(&self.pool).await?;
        Ok(())
    }

    async fn check_fingerprint(&self, project_id: Uuid, hash: &str) -> Result<bool, StoreError> {
        let row = sqlx::query_scalar::<_, i64>(
            "SELECT 1 FROM fingerprint_cache WHERE hash = $1 AND project_id = $2 LIMIT 1"
        ).bind(hash).bind(project_id).fetch_optional(&self.pool).await?;
        Ok(row.is_some())
    }

    async fn store_fingerprint(&self, project_id: Uuid, hash: &str, step_name: &str) -> Result<(), StoreError> {
        sqlx::query(
            "INSERT INTO fingerprint_cache (hash, project_id, step_name) VALUES ($1, $2, $3) ON CONFLICT (hash) DO NOTHING"
        ).bind(hash).bind(project_id).bind(step_name)
        .execute(&self.pool).await?;
        Ok(())
    }
}

#[derive(sqlx::FromRow)]
struct BuildRow {
    id: Uuid, pipeline_id: Uuid, number: i64,
    status: String, trigger: String,
    commit_sha: Option<String>, branch: Option<String>,
    matrix_values: Option<serde_json::Value>,
    stages: serde_json::Value,
    started_at: Option<chrono::DateTime<chrono::Utc>>,
    finished_at: Option<chrono::DateTime<chrono::Utc>>,
    created_at: chrono::DateTime<chrono::Utc>,
}

impl From<BuildRow> for Build {
    fn from(r: BuildRow) -> Self {
        let stages: Vec<BuildStage> = serde_json::from_value(r.stages).unwrap_or_default();
        let matrix_values = r.matrix_values.and_then(|v| serde_json::from_value(v).ok());
        Build {
            id: r.id, pipeline_id: r.pipeline_id, number: r.number as u64,
            status: str_to_status(&r.status), trigger: str_to_trigger(&r.trigger),
            commit_sha: r.commit_sha, branch: r.branch, matrix_values, stages,
            started_at: r.started_at, finished_at: r.finished_at, created_at: r.created_at,
        }
    }
}

fn status_to_str(s: BuildStatus) -> &'static str {
    match s {
        BuildStatus::Queued => "queued", BuildStatus::Running => "running",
        BuildStatus::Success => "success", BuildStatus::Failed => "failed",
        BuildStatus::Cancelled => "cancelled", BuildStatus::Skipped => "skipped",
    }
}

fn str_to_status(s: &str) -> BuildStatus {
    match s {
        "running" => BuildStatus::Running, "success" => BuildStatus::Success,
        "failed" => BuildStatus::Failed, "cancelled" => BuildStatus::Cancelled,
        "skipped" => BuildStatus::Skipped, _ => BuildStatus::Queued,
    }
}

fn trigger_to_str(t: &BuildTrigger) -> &'static str {
    match t {
        BuildTrigger::Push => "push", BuildTrigger::PullRequest => "pull_request",
        BuildTrigger::Webhook => "webhook", BuildTrigger::Manual => "manual",
        BuildTrigger::Schedule => "schedule",
    }
}

fn str_to_trigger(s: &str) -> BuildTrigger {
    match s {
        "pull_request" => BuildTrigger::PullRequest, "webhook" => BuildTrigger::Webhook,
        "manual" => BuildTrigger::Manual, "schedule" => BuildTrigger::Schedule,
        _ => BuildTrigger::Push,
    }
}
