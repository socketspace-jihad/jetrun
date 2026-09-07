use thiserror::Error;

#[derive(Debug, Error)]
pub enum JetrunError {
    #[error("Pipeline not found: {0}")]
    PipelineNotFound(String),

    #[error("Build not found: {0}")]
    BuildNotFound(String),

    #[error("Project not found: {0}")]
    ProjectNotFound(String),

    #[error("Invalid pipeline config: {0}")]
    InvalidConfig(String),

    #[error("YAML parse error: {0}")]
    YamlParse(#[from] serde_yaml::Error),

    #[error("Cache error: {0}")]
    Cache(String),

    #[error("Worker error: {0}")]
    Worker(String),

    #[error("Step execution failed: exit code {exit_code}")]
    StepFailed { step_id: String, exit_code: i32 },

    #[error("Step timed out after {timeout_seconds}s")]
    StepTimeout {
        step_id: String,
        timeout_seconds: u32,
    },

    #[error("Build cancelled")]
    BuildCancelled,

    #[error("Internal error: {0}")]
    Internal(String),
}

impl JetrunError {
    pub fn status_code(&self) -> u16 {
        match self {
            Self::PipelineNotFound(_) | Self::BuildNotFound(_) | Self::ProjectNotFound(_) => 404,
            Self::InvalidConfig(_) | Self::YamlParse(_) => 400,
            Self::BuildCancelled => 409,
            _ => 500,
        }
    }
}
