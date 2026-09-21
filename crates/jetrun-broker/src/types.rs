use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub const SUBJECT_REPO_SYNC: &str = "jetrun.repo.sync";
pub const SUBJECT_REPO_DELETE: &str = "jetrun.repo.delete";
pub const SUBJECT_BUILD_EXECUTE: &str = "jetrun.build.execute";

/// Jobs published by jetrun-repo, consumed by jetrun-repo-controller
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RepoJob {
    Sync {
        project_id: Uuid,
        repo_url: String,
        branch: String,
        commit_sha: Option<String>,
        trigger: String,
        triggered_by: Option<Uuid>,
    },
    Delete {
        project_id: Uuid,
    },
}

impl RepoJob {
    pub fn subject(&self) -> &'static str {
        match self {
            RepoJob::Sync { .. } => SUBJECT_REPO_SYNC,
            RepoJob::Delete { .. } => SUBJECT_REPO_DELETE,
        }
    }

    pub fn to_bytes(&self) -> Result<Vec<u8>, serde_json::Error> {
        serde_json::to_vec(self)
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self, serde_json::Error> {
        serde_json::from_slice(bytes)
    }
}

/// Job published by repo-controller after creating a build, consumed by jetrun-worker
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuildJob {
    pub build_id: Uuid,
    pub pipeline_id: Uuid,
    pub project_id: Uuid,
    pub repo_path: String,
    pub branch: String,
    pub commit_sha: Option<String>,
    pub stages: Vec<BuildStageJob>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuildStageJob {
    pub stage_id: Uuid,
    pub name: String,
    pub depends_on: Vec<String>,
    pub steps: Vec<BuildStepJob>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuildStepJob {
    pub step_id: Uuid,
    pub name: String,
    pub command: String,
    pub image: Option<String>,
    pub env: Vec<(String, String)>,
    pub timeout_secs: Option<u32>,
}

impl BuildJob {
    pub fn to_bytes(&self) -> Result<Vec<u8>, serde_json::Error> {
        serde_json::to_vec(self)
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self, serde_json::Error> {
        serde_json::from_slice(bytes)
    }
}
