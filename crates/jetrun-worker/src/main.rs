use mimalloc::MiMalloc;
#[global_allocator]
static GLOBAL: MiMalloc = MiMalloc;

use std::sync::Arc;
use std::collections::HashMap;
use std::time::Instant;

use tracing_subscriber::EnvFilter;
use tokio::process::Command;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::task::JoinSet;

use jetrun_broker::traits::MessageBroker;
use jetrun_broker::types::{BuildJob, BuildStageJob, BuildStepJob, SUBJECT_BUILD_EXECUTE};
use jetrun_common::models::{BuildStatus, BuildStage, BuildStep, StageConfig, StepConfig};
use jetrun_engine::DagScheduler;
use jetrun_store::traits::Store;

mod executor;
mod log_stream;
pub mod logs;
pub mod runtime;
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

    let log_dir = std::path::PathBuf::from(
        std::env::var("LOG_DIR").unwrap_or_else(|_| "/opt/jetrun/data/logs".into())
    );
    let data_dir = std::path::PathBuf::from(
        std::env::var("DATA_DIR").unwrap_or_else(|_| "/opt/jetrun/data".into())
    );
    tokio::fs::create_dir_all(&log_dir).await?;

    let log_store = match logs::LogStore::from_env().await {
        Ok(s) => Some(Arc::new(s)),
        Err(e) => { tracing::warn!(error = %e, "S3 log store not configured, logs stay on disk only"); None }
    };

    // Load runtime config from DB (tmpfs, dep cache, CPU pinning, concurrency)
    let rt = Arc::new(runtime::WorkerRuntime::load(&store, &data_dir).await);
    tracing::info!(
        max_parallel = rt.max_parallel,
        tmpfs = rt.tmpfs_enabled,
        dep_cache = rt.dep_cache_enabled,
        cpu_pinning = rt.cpu_pinning_enabled,
        "worker runtime config loaded"
    );

    // Setup tmpfs workspace + dependency cache
    runtime::setup_tmpfs(&rt).await?;
    runtime::setup_dep_cache(&rt).await?;

    // Create parent cgroup for jetrun builds
    #[cfg(target_os = "linux")]
    {
        let _ = tokio::fs::create_dir_all("/sys/fs/cgroup/jetrun").await;
    }

    tracing::info!("jetrun-worker starting, subscribing to build jobs");

    let mut stream = broker
        .subscribe(SUBJECT_BUILD_EXECUTE)
        .await
        .map_err(|e| anyhow::anyhow!("subscribe: {}", e))?;

    // Track active builds for CPU core allocation
    let active_builds = Arc::new(std::sync::atomic::AtomicUsize::new(0));

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

        let build_log_dir = logs::create_build_log_dir(&log_dir, job.build_id).await.ok();

        // Prepare tmpfs workspace: copy repo into RAM
        let workspace = runtime::prepare_workspace(&rt, job.build_id, &job.repo_path).await
            .unwrap_or_else(|_| std::path::PathBuf::from(&job.repo_path));
        let workspace_str = workspace.to_string_lossy().to_string();

        // Allocate CPU cores for this build
        let build_idx = active_builds.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let cpus_per_build = (rt.max_parallel).max(1);
        let cpu_start = (build_idx * cpus_per_build) % rt.max_parallel.max(1);

        // Create cgroup with CPU pinning
        let cgroup = runtime::create_build_cgroup(&rt, job.build_id, cpu_start, cpus_per_build).await
            .unwrap_or(None);

        // Inject persistent dep cache env vars into the job
        let dep_env = runtime::dep_cache_env(&rt);

        let _ = store.update_build_status(job.build_id, BuildStatus::Running, None).await;

        let result = execute_build(&store, &job, build_log_dir.as_deref(), &workspace_str, &dep_env, cgroup.as_deref()).await;
        let (status, finished) = match &result {
            Ok(()) => (BuildStatus::Success, Some(chrono::Utc::now())),
            Err(_) => (BuildStatus::Failed, Some(chrono::Utc::now())),
        };

        let _ = store.update_build_status(job.build_id, status, finished).await;

        // Cleanup: workspace + cgroup
        runtime::cleanup_workspace(&rt, job.build_id).await;
        runtime::cleanup_cgroup(job.build_id).await;
        active_builds.fetch_sub(1, std::sync::atomic::Ordering::Relaxed);

        if build_log_dir.is_some() {
            if let Some(s3) = &log_store {
                match s3.upload_build_logs(&log_dir, job.build_id).await {
                    Ok(_) => { let _ = logs::cleanup_build_logs(&log_dir, job.build_id).await; }
                    Err(e) => tracing::warn!(error = %e, "S3 upload failed, logs stay on disk"),
                }
            }
        }

        match result {
            Ok(()) => tracing::info!(build_id = %job.build_id, "build succeeded"),
            Err(e) => tracing::error!(build_id = %job.build_id, error = %e, "build failed"),
        }

        let _ = broker.ack(&msg).await;
    }
}

// ── Conversions: BuildJob types → engine StageConfig/StepConfig ──

fn stage_jobs_to_configs(stages: &[BuildStageJob]) -> Vec<StageConfig> {
    stages.iter().map(|s| StageConfig {
        name: s.name.clone(),
        depends_on: s.depends_on.clone(),
        steps: s.steps.iter().map(step_job_to_config).collect(),
        matrix: None,
        condition: None,
    }).collect()
}

fn step_job_to_config(step: &BuildStepJob) -> StepConfig {
    StepConfig {
        name: step.name.clone(),
        run: step.command.clone(),
        image: step.image.clone(),
        env: step.env.iter().cloned().collect(),
        timeout_minutes: step.timeout_secs.map(|s| s / 60),
        cache: None,
        artifacts: None,
    }
}

// ── Build Execution: DAG-parallel stages + fingerprint skipping ──

/// Execute build using the engine's DAG scheduler for parallel stage execution.
/// Stages at the same DAG level run concurrently via JoinSet.
/// Steps within a stage run sequentially. Fingerprints enable content-hash skipping.
async fn execute_build(
    store: &Arc<dyn Store>,
    job: &BuildJob,
    build_log_dir: Option<&std::path::Path>,
    workspace: &str,
    dep_env: &[(String, String)],
    cgroup: Option<&std::path::Path>,
) -> anyhow::Result<()> {
    // Build name→index map for O(1) stage lookup
    let stage_name_to_idx: HashMap<&str, usize> = job.stages.iter().enumerate()
        .map(|(i, s)| (s.name.as_str(), i))
        .collect();

    // Initialize mutable stage tracking
    let stages: Vec<BuildStage> = job.stages.iter().map(|s| BuildStage {
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

    // Shared mutable state — one Mutex per stage for zero contention between parallel stages
    let stages = Arc::new(tokio::sync::RwLock::new(stages));

    // Compute DAG execution levels — O(V+E) once, O(1) per query
    let stage_configs = stage_jobs_to_configs(&job.stages);
    let dag = DagScheduler::new(&stage_configs)
        .map_err(|e| anyhow::anyhow!("DAG error: {}", e))?;

    let levels = dag.execution_levels();
    tracing::info!(
        build_id = %job.build_id,
        levels = levels.len(),
        stages = dag.stage_count(),
        "DAG computed: {} levels, {} stages",
        levels.len(), dag.stage_count()
    );

    // Execute level by level — stages within a level run in parallel
    for (level_idx, level_stages) in levels.iter().enumerate() {
        tracing::info!(build_id = %job.build_id, level = level_idx, parallel = level_stages.len(), "executing level");

        if level_stages.len() == 1 {
            let stage_name = &level_stages[0];
            let stage_idx = stage_name_to_idx[stage_name.as_str()];
            execute_stage(
                store, job, stage_idx, &stages, build_log_dir, workspace, dep_env, cgroup,
            ).await?;
        } else {
            let mut join_set = JoinSet::new();

            for stage_name in level_stages {
                let stage_idx = stage_name_to_idx[stage_name.as_str()];
                let store = Arc::clone(store);
                let job = job.clone();
                let stages = Arc::clone(&stages);
                let log_dir = build_log_dir.map(|p| p.to_owned());
                let ws = workspace.to_string();
                let de = dep_env.to_vec();
                let cg = cgroup.map(|p| p.to_owned());

                join_set.spawn(async move {
                    execute_stage(
                        &store, &job, stage_idx, &stages, log_dir.as_deref(), &ws, &de, cg.as_deref(),
                    ).await
                });
            }

            // Collect results — fail fast if any stage fails
            let mut first_error: Option<anyhow::Error> = None;
            while let Some(result) = join_set.join_next().await {
                match result {
                    Ok(Ok(())) => {}
                    Ok(Err(e)) => {
                        if first_error.is_none() {
                            first_error = Some(e);
                        }
                        // Don't abort other stages — let them finish for complete status
                    }
                    Err(e) => {
                        if first_error.is_none() {
                            first_error = Some(anyhow::anyhow!("task panic: {}", e));
                        }
                    }
                }
            }

            if let Some(e) = first_error {
                return Err(e);
            }
        }
    }

    Ok(())
}

/// Execute a single stage: run steps sequentially, update status in DB.
async fn execute_stage(
    store: &Arc<dyn Store>,
    job: &BuildJob,
    stage_idx: usize,
    stages: &Arc<tokio::sync::RwLock<Vec<BuildStage>>>,
    build_log_dir: Option<&std::path::Path>,
    workspace: &str,
    dep_env: &[(String, String)],
    cgroup: Option<&std::path::Path>,
) -> anyhow::Result<()> {
    let stage_name = job.stages[stage_idx].name.clone();

    // Mark stage running
    {
        let mut s = stages.write().await;
        s[stage_idx].status = BuildStatus::Running;
        s[stage_idx].started_at = Some(chrono::Utc::now());
        let _ = store.update_build_stages(job.build_id, &s).await;
    }

    let mut stage_failed = false;

    for (step_idx, step_job) in job.stages[stage_idx].steps.iter().enumerate() {
        // Mark step running
        {
            let mut s = stages.write().await;
            s[stage_idx].steps[step_idx].status = BuildStatus::Running;
            s[stage_idx].steps[step_idx].started_at = Some(chrono::Utc::now());
            let _ = store.update_build_stages(job.build_id, &s).await;
        }

        let start = Instant::now();

        // ── Fingerprint check: skip if content-hash matches previous successful run ──
        let step_config = step_job_to_config(step_job);
        let repo_path = std::path::Path::new(workspace);
        let input_patterns: Vec<String> = step_config.cache
            .as_ref()
            .map(|c| c.paths.clone())
            .unwrap_or_default();

        let fingerprint = jetrun_engine::compute_fingerprint(&step_config, repo_path, &input_patterns)
            .await
            .ok();

        let cache_hit = match &fingerprint {
            Some(fp) => store.check_fingerprint(job.project_id, &fp.hash).await.unwrap_or(false),
            None => false,
        };

        if cache_hit {
            let duration = start.elapsed().as_millis() as u64;
            let fp = fingerprint.as_ref().unwrap();
            tracing::info!(
                step = %step_job.name,
                fingerprint = %fp.hash[..12],
                "cache hit — skipping step"
            );

            let mut s = stages.write().await;
            s[stage_idx].steps[step_idx].status = BuildStatus::Skipped;
            s[stage_idx].steps[step_idx].exit_code = Some(0);
            s[stage_idx].steps[step_idx].duration_ms = Some(duration);
            s[stage_idx].steps[step_idx].finished_at = Some(chrono::Utc::now());
            s[stage_idx].steps[step_idx].cache_hit = true;
            s[stage_idx].steps[step_idx].fingerprint = Some(fp.hash.clone());
            let _ = store.update_build_stages(job.build_id, &s).await;
            continue;
        }

        // ── Execute step ──
        let result = execute_step(step_job, workspace, build_log_dir, dep_env, cgroup).await;
        let duration = start.elapsed().as_millis() as u64;

        match result {
            Ok(code) => {
                let mut s = stages.write().await;
                s[stage_idx].steps[step_idx].exit_code = Some(code);
                s[stage_idx].steps[step_idx].duration_ms = Some(duration);
                s[stage_idx].steps[step_idx].finished_at = Some(chrono::Utc::now());
                s[stage_idx].steps[step_idx].status = if code == 0 { BuildStatus::Success } else { BuildStatus::Failed };
                if let Some(fp) = &fingerprint {
                    s[stage_idx].steps[step_idx].fingerprint = Some(fp.hash.clone());
                }
                let _ = store.update_build_stages(job.build_id, &s).await;

                // Store fingerprint on success for future cache hits
                if code == 0 {
                    if let Some(fp) = &fingerprint {
                        let _ = store.store_fingerprint(job.project_id, &fp.hash, &step_job.name).await;
                    }
                }

                if code != 0 {
                    stage_failed = true;
                    break;
                }
            }
            Err(e) => {
                let mut s = stages.write().await;
                s[stage_idx].steps[step_idx].status = BuildStatus::Failed;
                s[stage_idx].steps[step_idx].finished_at = Some(chrono::Utc::now());
                s[stage_idx].steps[step_idx].duration_ms = Some(duration);
                let _ = store.update_build_stages(job.build_id, &s).await;
                anyhow::bail!("step '{}': {}", step_job.name, e);
            }
        }
    }

    // Mark stage done
    {
        let mut s = stages.write().await;
        s[stage_idx].finished_at = Some(chrono::Utc::now());
        s[stage_idx].status = if stage_failed { BuildStatus::Failed } else { BuildStatus::Success };
        let _ = store.update_build_stages(job.build_id, &s).await;
    }

    if stage_failed {
        anyhow::bail!("stage '{}' failed", stage_name);
    }

    tracing::info!(stage = %stage_name, "stage done");
    Ok(())
}

// ── Step Execution ──

async fn execute_step(
    step: &BuildStepJob,
    working_dir: &str,
    build_log_dir: Option<&std::path::Path>,
    dep_env: &[(String, String)],
    cgroup: Option<&std::path::Path>,
) -> anyhow::Result<i32> {
    #[cfg(feature = "docker")]
    if let Some(image) = &step.image {
        return executor::docker::execute_docker(step, image, working_dir, build_log_dir).await;
    }

    #[cfg(all(target_os = "linux", feature = "namespace-isolation"))]
    return execute_with_logs(step, working_dir, build_log_dir, true, dep_env, cgroup).await;

    #[cfg(not(all(target_os = "linux", feature = "namespace-isolation")))]
    return execute_with_logs(step, working_dir, build_log_dir, false, dep_env, cgroup).await;
}

/// Execute a step with mpsc-buffered log writes.
/// stdout/stderr producers → bounded channel → single writer with batch flush.
/// Zero mutex contention, batched syscalls (flush at 4KB or channel drain).
#[allow(dead_code)]
pub async fn execute_with_logs(
    step: &BuildStepJob,
    working_dir: &str,
    build_log_dir: Option<&std::path::Path>,
    _use_namespace: bool,
    dep_env: &[(String, String)],
    _cgroup: Option<&std::path::Path>,
) -> anyhow::Result<i32> {
    use tokio::sync::mpsc;

    let start = Instant::now();

    let mut cmd = Command::new("sh");
    cmd.arg("-c").arg(&step.command)
        .current_dir(working_dir)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());

    // Step env vars
    for (k, v) in &step.env { cmd.env(k, v); }
    // Persistent dependency cache env (GOMODCACHE, GOCACHE, npm_config_cache, etc)
    for (k, v) in dep_env { cmd.env(k, v); }

    #[cfg(all(target_os = "linux", feature = "namespace-isolation"))]
    if _use_namespace {
        use std::os::unix::process::CommandExt;
        unsafe {
            cmd.pre_exec(|| {
                let flags_user = libc::CLONE_NEWUSER | libc::CLONE_NEWPID | libc::CLONE_NEWNS;
                if libc::unshare(flags_user) == 0 { return Ok(()); }
                let flags = libc::CLONE_NEWPID | libc::CLONE_NEWNS;
                if libc::unshare(flags) == 0 {
                    let _ = libc::mount(
                        b"proc\0".as_ptr() as *const libc::c_char,
                        b"/proc\0".as_ptr() as *const libc::c_char,
                        b"proc\0".as_ptr() as *const libc::c_char,
                        0, std::ptr::null(),
                    );
                    return Ok(());
                }
                Ok(())
            });
        }
    }

    let mut child = cmd.spawn()?;

    // Add process to cgroup for CPU pinning + memory limits
    if let Some(cg) = _cgroup {
        if let Some(pid) = child.id() {
            let _ = runtime::add_to_cgroup(cg, pid).await;
        }
    }

    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();

    // Bounded channel: stdout/stderr producers → single file writer
    // 4096 slots: high-throughput builds won't block on I/O
    let (tx, rx) = mpsc::channel::<(String, String)>(4096);

    // stdout producer
    let tx1 = tx.clone();
    let name1 = step.name.clone();
    let t1 = tokio::spawn(async move {
        let mut lines = BufReader::new(stdout).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            tracing::info!(step = %name1, "{}", line);
            let _ = tx1.try_send(("stdout".into(), line));
        }
    });

    // stderr producer
    let name2 = step.name.clone();
    let t2 = tokio::spawn(async move {
        let mut lines = BufReader::new(stderr).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            tracing::warn!(step = %name2, "{}", line);
            let _ = tx.try_send(("stderr".into(), line));
        }
    });

    // Single writer: batched flush, zero contention
    let log_path = build_log_dir.map(|dir| logs::step_log_path(dir, step.step_id));
    let writer = tokio::spawn(async move {
        let mut rx = rx;
        let file = match &log_path {
            Some(p) => tokio::fs::OpenOptions::new().create(true).append(true).open(p).await.ok(),
            None => { while rx.recv().await.is_some() {} return; }
        };
        let mut file = match file {
            Some(f) => f,
            None => { while rx.recv().await.is_some() {} return; }
        };

        use tokio::io::AsyncWriteExt;
        use std::io::Write as _;
        let mut buf = Vec::with_capacity(8192);

        while let Some((stream, content)) = rx.recv().await {
            let ts = chrono::Utc::now().format("%H:%M:%S%.3f");
            let _ = write!(buf, "[{}] [{}] {}\n", ts, stream, content);

            // Flush at 4KB or when channel is drained (no pending lines)
            if buf.len() >= 4096 || rx.is_empty() {
                let _ = file.write_all(&buf).await;
                buf.clear();
            }
        }

        if !buf.is_empty() {
            let _ = file.write_all(&buf).await;
        }
    });

    let status = match step.timeout_secs {
        Some(t) => tokio::time::timeout(std::time::Duration::from_secs(t as u64), child.wait())
            .await
            .map_err(|_| { let _ = child.start_kill(); anyhow::anyhow!("timeout {}s", t) })?,
        None => child.wait().await,
    }?;

    // Producers finish → senders drop → channel closes → writer drains remaining
    let _ = tokio::join!(t1, t2);
    let _ = writer.await;

    let code = status.code().unwrap_or(-1);
    tracing::info!(step = %step.name, code = code, ms = start.elapsed().as_millis() as u64, "step finished");
    Ok(code)
}
