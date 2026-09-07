use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BuildStatus {
    Queued,
    Running,
    Success,
    Failed,
    Cancelled,
    Skipped,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Build {
    pub id: Uuid,
    pub pipeline_id: Uuid,
    pub number: u64,
    pub status: BuildStatus,
    pub trigger: BuildTrigger,
    pub commit_sha: Option<String>,
    pub branch: Option<String>,
    pub matrix_values: Option<HashMap<String, String>>,
    pub stages: Vec<BuildStage>,
    pub started_at: Option<DateTime<Utc>>,
    pub finished_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BuildTrigger {
    Push,
    PullRequest,
    Webhook,
    Manual,
    Schedule,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuildStage {
    pub id: Uuid,
    pub build_id: Uuid,
    pub name: String,
    pub status: BuildStatus,
    pub steps: Vec<BuildStep>,
    pub started_at: Option<DateTime<Utc>>,
    pub finished_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuildStep {
    pub id: Uuid,
    pub stage_id: Uuid,
    pub name: String,
    pub status: BuildStatus,
    pub exit_code: Option<i32>,
    pub log_url: Option<String>,
    pub started_at: Option<DateTime<Utc>>,
    pub finished_at: Option<DateTime<Utc>>,
    pub duration_ms: Option<u64>,
    pub cache_hit: bool,
    pub fingerprint: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuildLog {
    pub step_id: Uuid,
    pub line_number: u64,
    pub timestamp: DateTime<Utc>,
    pub stream: LogStream,
    pub content: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LogStream {
    Stdout,
    Stderr,
    System,
}
