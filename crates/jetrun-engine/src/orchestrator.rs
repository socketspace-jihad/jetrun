use uuid::Uuid;

use jetrun_common::error::JetrunError;
use jetrun_common::models::{Build, BuildStage, BuildStatus, BuildTrigger, PipelineConfig};

use crate::scheduler::dag::DagScheduler;
use crate::scheduler::matrix::expand_matrix;

/// Orchestrates the lifecycle of a build: parsing, scheduling, and tracking.
pub struct Orchestrator;

impl Orchestrator {
    /// Create a new build from a pipeline config.
    pub fn create_build(
        pipeline_id: Uuid,
        build_number: u64,
        config: &PipelineConfig,
        trigger: BuildTrigger,
        commit_sha: Option<String>,
        branch: Option<String>,
    ) -> Result<Build, JetrunError> {
        // Expand matrix stages
        let mut all_stages = Vec::new();
        for stage_config in &config.stages {
            let expanded = expand_matrix(stage_config);
            for (expanded_stage, _matrix_values) in expanded {
                all_stages.push(expanded_stage);
            }
        }

        // Validate the DAG
        let _scheduler = DagScheduler::new(&all_stages)?;

        // Create build stages
        let build_stages: Vec<BuildStage> = all_stages
            .iter()
            .map(|stage| BuildStage {
                id: Uuid::new_v4(),
                build_id: Uuid::nil(), // Will be set below
                name: stage.name.clone(),
                status: BuildStatus::Queued,
                steps: stage
                    .steps
                    .iter()
                    .map(|step| jetrun_common::models::BuildStep {
                        id: Uuid::new_v4(),
                        stage_id: Uuid::nil(),
                        name: step.name.clone(),
                        status: BuildStatus::Queued,
                        exit_code: None,
                        log_url: None,
                        started_at: None,
                        finished_at: None,
                        duration_ms: None,
                        cache_hit: false,
                        fingerprint: None,
                    })
                    .collect(),
                started_at: None,
                finished_at: None,
            })
            .collect();

        let build_id = Uuid::new_v4();
        let build = Build {
            id: build_id,
            pipeline_id,
            number: build_number,
            status: BuildStatus::Queued,
            trigger,
            commit_sha,
            branch,
            matrix_values: None,
            stages: build_stages,
            started_at: None,
            finished_at: None,
            created_at: chrono::Utc::now(),
        };

        Ok(build)
    }
}
