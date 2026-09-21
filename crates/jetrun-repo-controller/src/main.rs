use mimalloc::MiMalloc;
#[global_allocator]
static GLOBAL: MiMalloc = MiMalloc;

use std::sync::Arc;
use std::path::PathBuf;

use tracing_subscriber::EnvFilter;

use jetrun_broker::traits::MessageBroker;
use jetrun_broker::types::{BuildJob, BuildStageJob, BuildStepJob, RepoJob, SUBJECT_BUILD_EXECUTE, SUBJECT_REPO_SYNC};
use jetrun_common::models::PipelineConfig;
use jetrun_store::traits::Store;

mod git;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive("info".parse()?))
        .json()
        .init();

    let database_url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://jetrun:jetrun_dev@localhost:5432/jetrun".into());
    let store: Arc<dyn Store> = Arc::new(jetrun_store::PgStore::connect(&database_url).await?);

    let nats_url = std::env::var("NATS_URL").unwrap_or_else(|_| "nats://localhost:4222".into());
    let broker = Arc::new(jetrun_broker::NatsBroker::connect(&nats_url).await
        .map_err(|e| anyhow::anyhow!("NATS connection failed: {}", e))?);

    let repos_dir = PathBuf::from(
        std::env::var("REPOS_DIR").unwrap_or_else(|_| "/opt/jetrun/data/repos".into()),
    );
    tokio::fs::create_dir_all(&repos_dir).await?;

    tracing::info!("jetrun-repo-controller starting, subscribing to sync jobs");

    let mut stream = broker
        .subscribe(SUBJECT_REPO_SYNC)
        .await
        .map_err(|e| anyhow::anyhow!("subscribe failed: {}", e))?;

    loop {
        let msg = match stream.next().await {
            Some(m) => m,
            None => {
                tracing::warn!("message stream ended, reconnecting...");
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                continue;
            }
        };

        let job = match RepoJob::from_bytes(&msg.payload) {
            Ok(j) => j,
            Err(e) => {
                tracing::error!(error = %e, "bad job payload");
                let _ = broker.ack(&msg).await;
                continue;
            }
        };

        match &job {
            RepoJob::Sync { project_id, repo_url, branch, commit_sha, trigger, .. } => {
                let actual_url = match repo_url.is_empty() {
                    true => match store.find_project_by_id(*project_id).await {
                        Ok(Some(p)) => p.repo_url,
                        _ => { tracing::error!(project_id = %project_id, "project not found"); let _ = broker.ack(&msg).await; continue; }
                    },
                    false => repo_url.clone(),
                };

                tracing::info!(project_id = %project_id, repo = %actual_url, branch = %branch, "sync started");

                match process_sync(&store, &repos_dir, *project_id, &actual_url, branch, commit_sha.as_deref(), trigger).await {
                    Ok(build_job) => {
                        // Publish build execution job to worker
                        match build_job.to_bytes() {
                            Ok(payload) => match broker.publish(SUBJECT_BUILD_EXECUTE, &payload).await {
                                Ok(()) => tracing::info!(build_id = %build_job.build_id, "build job dispatched to worker"),
                                Err(e) => tracing::error!(error = %e, "failed to dispatch build job"),
                            },
                            Err(e) => tracing::error!(error = %e, "failed to serialize build job"),
                        }
                    }
                    Err(e) => tracing::error!(project_id = %project_id, error = %e, "sync failed"),
                }
            }
            RepoJob::Delete { project_id } => {
                let repo_path = repos_dir.join(project_id.to_string());
                if repo_path.exists() {
                    let _ = tokio::fs::remove_dir_all(&repo_path).await;
                    tracing::info!(project_id = %project_id, "repo cache deleted");
                }
            }
        }

        let _ = broker.ack(&msg).await;
    }
}

async fn process_sync(
    store: &Arc<dyn Store>,
    repos_dir: &std::path::Path,
    project_id: uuid::Uuid,
    repo_url: &str,
    branch: &str,
    commit_sha: Option<&str>,
    trigger: &str,
) -> anyhow::Result<BuildJob> {
    use jetrun_common::models::*;

    let repo_path = repos_dir.join(project_id.to_string());
    let now = chrono::Utc::now();

    // 1. Clone or pull
    match repo_path.exists() {
        true => git::pull(&repo_path, branch).await?,
        false => git::clone(repo_url, branch, &repo_path).await?,
    }

    // 2. Read + parse
    let config_path = repo_path.join(".jetrun/pipeline.yaml");
    anyhow::ensure!(config_path.exists(), ".jetrun/pipeline.yaml not found");

    let config: PipelineConfig = serde_yaml::from_str(
        &tokio::fs::read_to_string(&config_path).await?
    )?;

    tracing::info!(pipeline = %config.name, stages = config.stages.len(), "pipeline parsed");

    // 3. Upsert pipeline
    let hash = blake3::hash(format!("{}:{}", project_id, config.name).as_bytes());
    let mut id_bytes = [0u8; 16];
    id_bytes.copy_from_slice(&hash.as_bytes()[..16]);
    let pipeline_id = uuid::Uuid::from_bytes(id_bytes);

    let _ = store.create_pipeline(&Pipeline {
        id: pipeline_id, project_id,
        name: config.name.clone(), description: config.description.clone(),
        config_path: ".jetrun/pipeline.yaml".into(), config: config.clone(),
        active: true, created_at: now, updated_at: now,
    }).await;

    // 4. Create build with stages/steps
    let build_id = uuid::Uuid::new_v4();
    let mut stage_jobs = Vec::with_capacity(config.stages.len());

    let stages: Vec<BuildStage> = config.stages.iter().map(|s| {
        let stage_id = uuid::Uuid::new_v4();
        let step_jobs: Vec<BuildStepJob> = s.steps.iter().map(|step| {
            let step_id = uuid::Uuid::new_v4();
            BuildStepJob {
                step_id,
                name: step.name.clone(),
                command: step.run.clone(),
                image: step.image.clone(),
                env: step.env.iter().map(|(k, v)| (k.clone(), v.clone())).collect(),
                timeout_secs: step.timeout_minutes.map(|m| m * 60),
            }
        }).collect();

        stage_jobs.push(BuildStageJob {
            stage_id,
            name: s.name.clone(),
            depends_on: s.depends_on.clone(),
            steps: step_jobs,
        });

        BuildStage {
            id: stage_id, build_id, name: s.name.clone(), status: BuildStatus::Queued,
            steps: s.steps.iter().map(|step| BuildStep {
                id: uuid::Uuid::new_v4(), stage_id, name: step.name.clone(),
                status: BuildStatus::Queued, exit_code: None, log_url: None,
                started_at: None, finished_at: None, duration_ms: None,
                cache_hit: false, fingerprint: None,
            }).collect(),
            started_at: None, finished_at: None,
        }
    }).collect();

    let build = Build {
        id: build_id, pipeline_id, number: now.timestamp_millis() as u64,
        status: BuildStatus::Queued,
        trigger: match trigger {
            "push" => BuildTrigger::Push, "pull_request" => BuildTrigger::PullRequest,
            "webhook" => BuildTrigger::Webhook, _ => BuildTrigger::Manual,
        },
        commit_sha: commit_sha.map(String::from), branch: Some(branch.to_string()),
        matrix_values: None, stages, started_at: None, finished_at: None, created_at: now,
    };

    store.create_build(&build).await.map_err(|e| anyhow::anyhow!("create build: {}", e))?;
    tracing::info!(build_id = %build_id, pipeline = %config.name, "build created");

    Ok(BuildJob {
        build_id,
        pipeline_id,
        project_id,
        repo_path: repo_path.to_string_lossy().to_string(),
        branch: branch.to_string(),
        commit_sha: commit_sha.map(String::from),
        stages: stage_jobs,
    })
}
