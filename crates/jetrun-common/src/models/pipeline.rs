use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

/// YAML pipeline configuration — what users write in their `.jetrun/pipeline.yml`
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PipelineConfig {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    pub on: TriggerConfig,
    #[serde(default)]
    pub env: HashMap<String, String>,
    pub stages: Vec<StageConfig>,
    #[serde(default)]
    pub cache: Option<PipelineCacheConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TriggerConfig {
    #[serde(default)]
    pub push: Option<BranchFilter>,
    #[serde(default)]
    pub pull_request: Option<BranchFilter>,
    #[serde(default)]
    pub webhook: bool,
    #[serde(default)]
    pub schedule: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BranchFilter {
    pub branches: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StageConfig {
    pub name: String,
    #[serde(default)]
    pub depends_on: Vec<String>,
    pub steps: Vec<StepConfig>,
    #[serde(default)]
    pub matrix: Option<MatrixConfig>,
    #[serde(default)]
    pub condition: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepConfig {
    pub name: String,
    #[serde(default)]
    pub image: Option<String>,
    pub run: String,
    #[serde(default)]
    pub env: HashMap<String, String>,
    #[serde(default)]
    pub timeout_minutes: Option<u32>,
    #[serde(default)]
    pub cache: Option<StepCacheConfig>,
    #[serde(default)]
    pub artifacts: Option<ArtifactConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MatrixConfig {
    pub values: HashMap<String, Vec<String>>,
    #[serde(default)]
    pub exclude: Vec<HashMap<String, String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepCacheConfig {
    pub key: String,
    pub paths: Vec<String>,
    #[serde(default)]
    pub restore_keys: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArtifactConfig {
    pub name: String,
    pub paths: Vec<String>,
    #[serde(default = "default_retention_days")]
    pub retention_days: u32,
}

fn default_retention_days() -> u32 {
    30
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PipelineCacheConfig {
    #[serde(default)]
    pub docker_layer_cache: bool,
    #[serde(default)]
    pub paths: Vec<StepCacheConfig>,
}

/// Database entity for a registered pipeline
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Pipeline {
    pub id: Uuid,
    pub project_id: Uuid,
    pub name: String,
    pub description: Option<String>,
    pub config_path: String,
    pub config: PipelineConfig,
    pub active: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
