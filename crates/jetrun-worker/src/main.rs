use mimalloc::MiMalloc;
#[global_allocator]
static GLOBAL: MiMalloc = MiMalloc;

use std::sync::Arc;
use std::collections::HashMap;
use std::time::Instant;

use tracing_subscriber::EnvFilter;
use tokio::process::Command;
use tokio::io::{AsyncBufReadExt, BufReader};

use jetrun_broker::traits::MessageBroker;
use jetrun_broker::types::{BuildJob, BuildStepJob, SUBJECT_BUILD_EXECUTE};
use jetrun_common::models::BuildStatus;
use jetrun_store::traits::Store;

mod executor;
mod log_stream;
mod state;

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
        .map_err(|e| anyhow::anyhow!("NATS: {}", e))?);

    tracing::info!("jetrun-worker starting, subscribing to build jobs");

    let mut stream = broker
        .subscribe(SUBJECT_BUILD_EXECUTE)
        .await
        .map_err(|e| anyhow::anyhow!("subscribe: {}", e))?;

    loop {
        let msg = match stream.next().await {
            Some(m) => m,
            None => { tokio::time::sleep(std::time::Duration::from_secs(5)).await; continue; }
        };

        let job = match BuildJob::from_bytes(&msg.payload) {
            Ok(j) => j,
            Err(e) => { tracing::error!(error = %e, "bad build job"); let _ = broker.ack(&msg).await; continue; }
        };

        tracing::info!(build_id = %job.build_id, stages = job.stages.len(), "executing build");

        // Mark running
        let _ = store.update_build_status(job.build_id, BuildStatus::Running, None).await;

        let result = execute_build(&store, &job).await;
        let (status, finished) = match &result {
            Ok(()) => (BuildStatus::Success, Some(chrono::Utc::now())),
            Err(_) => (BuildStatus::Failed, Some(chrono::Utc::now())),
        };

        let _ = store.update_build_status(job.build_id, status, finished).await;

        match result {
            Ok(()) => tracing::info!(build_id = %job.build_id, "build succeeded"),
            Err(e) => tracing::error!(build_id = %job.build_id, error = %e, "build failed"),
        }

        let _ = broker.ack(&msg).await;
    }
}

/// Execute build: stages in DAG order, update stage/step statuses in DB as they progress
async fn execute_build(store: &Arc<dyn Store>, job: &BuildJob) -> anyhow::Result<()> {
    use jetrun_common::models::{BuildStage, BuildStep};

    let stage_idx: HashMap<&str, usize> = job.stages.iter().enumerate()
        .map(|(i, s)| (s.name.as_str(), i))
        .collect();

    // Build mutable stage tracking from job
    let mut stages: Vec<BuildStage> = job.stages.iter().map(|s| BuildStage {
        id: s.stage_id,
        build_id: job.build_id,
        name: s.name.clone(),
        status: BuildStatus::Queued,
        steps: s.steps.iter().map(|step| BuildStep {
            id: step.step_id, stage_id: s.stage_id, name: step.name.clone(),
            status: BuildStatus::Queued, exit_code: None, log_url: None,
            started_at: None, finished_at: None, duration_ms: None,
            cache_hit: false, fingerprint: None,
        }).collect(),
        started_at: None, finished_at: None,
    }).collect();

    let mut done = vec![false; stages.len()];
    let mut progress = true;

    while progress {
        progress = false;
        for i in 0..stages.len() {
            if done[i] { continue; }

            let ready = job.stages[i].depends_on.iter()
                .all(|d| stage_idx.get(d.as_str()).map_or(true, |&j| done[j]));
            if !ready { continue; }

            // Mark stage running
            stages[i].status = BuildStatus::Running;
            stages[i].started_at = Some(chrono::Utc::now());
            let _ = store.update_build_stages(job.build_id, &stages).await;

            let mut stage_failed = false;
            for (j, step_job) in job.stages[i].steps.iter().enumerate() {
                // Mark step running
                stages[i].steps[j].status = BuildStatus::Running;
                stages[i].steps[j].started_at = Some(chrono::Utc::now());
                let _ = store.update_build_stages(job.build_id, &stages).await;

                let start = Instant::now();
                let result = execute_step(step_job, &job.repo_path).await;
                let duration = start.elapsed().as_millis() as u64;

                match result {
                    Ok(code) => {
                        stages[i].steps[j].exit_code = Some(code);
                        stages[i].steps[j].duration_ms = Some(duration);
                        stages[i].steps[j].finished_at = Some(chrono::Utc::now());
                        stages[i].steps[j].status = if code == 0 { BuildStatus::Success } else { BuildStatus::Failed };
                        let _ = store.update_build_stages(job.build_id, &stages).await;

                        if code != 0 {
                            stage_failed = true;
                            break;
                        }
                    }
                    Err(e) => {
                        stages[i].steps[j].status = BuildStatus::Failed;
                        stages[i].steps[j].finished_at = Some(chrono::Utc::now());
                        stages[i].steps[j].duration_ms = Some(duration);
                        let _ = store.update_build_stages(job.build_id, &stages).await;
                        anyhow::bail!("step '{}': {}", step_job.name, e);
                    }
                }
            }

            stages[i].finished_at = Some(chrono::Utc::now());
            stages[i].status = if stage_failed { BuildStatus::Failed } else { BuildStatus::Success };
            let _ = store.update_build_stages(job.build_id, &stages).await;

            if stage_failed {
                anyhow::bail!("stage '{}' failed", stages[i].name);
            }

            done[i] = true;
            progress = true;
            tracing::info!(stage = %stages[i].name, "✓ stage done");
        }
    }

    anyhow::ensure!(done.iter().all(|&d| d), "circular dependency in stages");
    Ok(())
}

/// Execute a step — selects executor based on step config:
///   image set → Docker (if compiled with --features docker)
///   no image  → Linux namespaces (if Linux + feature enabled) or bare sh -c
async fn execute_step(step: &BuildStepJob, working_dir: &str) -> anyhow::Result<i32> {
    // Docker executor: when `image:` is specified in pipeline YAML
    #[cfg(feature = "docker")]
    if let Some(image) = &step.image {
        tracing::info!(step = %step.name, image = %image, "using Docker executor");
        // TODO: wire bollard DockerExecutor here
        // For now fall through to namespace/native
    }

    // Linux namespace isolation (default on Linux)
    #[cfg(all(target_os = "linux", feature = "namespace-isolation"))]
    return executor::namespace::execute_isolated(step, working_dir).await;

    // Bare sh -c fallback (macOS dev, or no isolation features)
    #[cfg(not(all(target_os = "linux", feature = "namespace-isolation")))]
    return execute_native(step, working_dir).await;
}

/// Bare sh -c execution — no isolation, for macOS or when namespaces are disabled
#[allow(dead_code)]
pub async fn execute_native(step: &BuildStepJob, working_dir: &str) -> anyhow::Result<i32> {
    let start = Instant::now();

    let mut cmd = Command::new("sh");
    cmd.arg("-c").arg(&step.command)
        .current_dir(working_dir)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());

    for (k, v) in &step.env { cmd.env(k, v); }

    let mut child = cmd.spawn()?;
    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();

    let name1 = step.name.clone();
    let t1 = tokio::spawn(async move {
        let mut lines = BufReader::new(stdout).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            tracing::info!(step = %name1, "{}", line);
        }
    });

    let name2 = step.name.clone();
    let t2 = tokio::spawn(async move {
        let mut lines = BufReader::new(stderr).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            tracing::warn!(step = %name2, "{}", line);
        }
    });

    let status = match step.timeout_secs {
        Some(t) => tokio::time::timeout(std::time::Duration::from_secs(t as u64), child.wait())
            .await
            .map_err(|_| { let _ = child.start_kill(); anyhow::anyhow!("timeout {}s", t) })?,
        None => child.wait().await,
    }?;

    let _ = tokio::join!(t1, t2);
    let code = status.code().unwrap_or(-1);
    tracing::info!(step = %step.name, code = code, ms = start.elapsed().as_millis() as u64, "step finished (native)");
    Ok(code)
}
