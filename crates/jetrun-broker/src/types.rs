use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Subjects/queues used in the system
pub const SUBJECT_REPO_SYNC: &str = "jetrun.repo.sync";
pub const SUBJECT_REPO_DELETE: &str = "jetrun.repo.delete";

/// Jobs published by jetrun-repo, consumed by jetrun-repo-controller
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RepoJob {
    /// Clone/pull repo, read .jetrun/pipeline.yaml, create build
    Sync {
        project_id: Uuid,
        repo_url: String,
        branch: String,
        commit_sha: Option<String>,
        trigger: String,
        triggered_by: Option<Uuid>,
    },
    /// Clean up cached repo on project deletion
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
