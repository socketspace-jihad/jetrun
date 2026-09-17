use mimalloc::MiMalloc;
#[global_allocator]
static GLOBAL: MiMalloc = MiMalloc;

use std::sync::Arc;
use std::path::PathBuf;

use tracing_subscriber::EnvFilter;

use jetrun_broker::traits::{MessageBroker, MessageStream};
use jetrun_broker::types::{RepoJob, SUBJECT_REPO_SYNC};
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
    let store: Arc<dyn jetrun_store::traits::Store> = Arc::new(jetrun_store::PgStore::connect(&database_url).await?);

    let nats_url = std::env::var("NATS_URL").unwrap_or_else(|_| "nats://localhost:4222".into());
    let broker = Arc::new(jetrun_broker::NatsBroker::connect(&nats_url).await
        .map_err(|e| anyhow::anyhow!("NATS connection failed: {}", e))?);

    let repos_dir = PathBuf::from(
        std::env::var("REPOS_DIR").unwrap_or_else(|_| "/opt/jetrun/data/repos".into()),
    );
    tokio::fs::create_dir_all(&repos_dir).await?;

    tracing::info!("jetrun-repo-controller starting, subscribing to sync jobs");

    // Subscribe to repo sync jobs
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
                tracing::error!(error = %e, "failed to deserialize job");
                let _ = broker.ack(&msg).await;
                continue;
            }
        };

        match &job {
            RepoJob::Sync { project_id, repo_url, branch, commit_sha, trigger, .. } => {
                tracing::info!(
                    project_id = %project_id,
                    repo_url = %repo_url,
                    branch = %branch,
                    trigger = %trigger,
                    "processing sync job"
                );

                let result = process_sync(
                    &store,
                    &repos_dir,
                    *project_id,
                    repo_url,
                    branch,
                    commit_sha.as_deref(),
                    trigger,
                )
                .await;

                match result {
                    Ok(()) => tracing::info!(project_id = %project_id, "sync completed"),
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
) -> anyhow::Result<()> {
    let repo_path = repos_dir.join(project_id.to_string());

    // 1. Clone or pull
    if repo_path.exists() {
        git::pull(&repo_path, branch).await?;
    } else {
        git::clone(repo_url, branch, &repo_path).await?;
    }

    // 2. Read .jetrun/pipeline.yaml
    let config_path = repo_path.join(".jetrun/pipeline.yaml");
    if !config_path.exists() {
        anyhow::bail!(".jetrun/pipeline.yaml not found in repo");
    }

    let yaml_content = tokio::fs::read_to_string(&config_path).await?;

    // 3. Parse YAML
    let config: PipelineConfig = serde_yaml::from_str(&yaml_content)?;

    tracing::info!(
        pipeline = %config.name,
        stages = config.stages.len(),
        "pipeline config parsed"
    );

    // 4. Create build record
    let build = jetrun_common::models::Build {
        id: uuid::Uuid::new_v4(),
        pipeline_id: project_id, // using project_id as pipeline_id for now
        number: 1, // TODO: auto-increment per pipeline
        status: jetrun_common::models::BuildStatus::Queued,
        trigger: match trigger {
            "push" => jetrun_common::models::BuildTrigger::Push,
            "pull_request" => jetrun_common::models::BuildTrigger::PullRequest,
            "webhook" => jetrun_common::models::BuildTrigger::Webhook,
            _ => jetrun_common::models::BuildTrigger::Manual,
        },
        commit_sha: commit_sha.map(String::from),
        branch: Some(branch.to_string()),
        matrix_values: None,
        stages: vec![],
        started_at: None,
        finished_at: None,
        created_at: chrono::Utc::now(),
    };

    store.create_build(&build).await
        .map_err(|e| anyhow::anyhow!("failed to create build: {}", e))?;

    tracing::info!(build_id = %build.id, "build created");

    Ok(())
}
