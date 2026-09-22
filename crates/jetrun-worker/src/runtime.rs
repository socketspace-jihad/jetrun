//! Worker runtime configuration — loaded from system_settings table.
//! Manages tmpfs workspace, dependency cache, cgroup CPU pinning, concurrency.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use jetrun_store::traits::Store;

/// Runtime configuration loaded from DB
#[derive(Debug, Clone)]
pub struct WorkerRuntime {
    pub max_parallel: usize,
    pub tmpfs_enabled: bool,
    pub tmpfs_size_mb: u64,
    pub dep_cache_enabled: bool,
    pub cpu_pinning_enabled: bool,
    pub memory_limit_mb: u64,
    pub data_dir: PathBuf,
}

impl WorkerRuntime {
    /// Load config from system_settings. Falls back to sensible defaults.
    pub async fn load(store: &Arc<dyn Store>, data_dir: &Path) -> Self {
        let settings = store.get_all_settings("worker.").await.unwrap_or_default();

        let max_parallel_setting: usize = settings.get("worker.max_parallel")
            .and_then(|v| v.parse().ok())
            .unwrap_or(0);

        // 0 = auto-detect from CPU count
        let max_parallel = if max_parallel_setting == 0 {
            std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4)
        } else {
            max_parallel_setting
        };

        Self {
            max_parallel,
            tmpfs_enabled: settings.get("worker.tmpfs_enabled").map(|v| v == "true").unwrap_or(true),
            tmpfs_size_mb: settings.get("worker.tmpfs_size_mb").and_then(|v| v.parse().ok()).unwrap_or(4096),
            dep_cache_enabled: settings.get("worker.dep_cache_enabled").map(|v| v == "true").unwrap_or(true),
            cpu_pinning_enabled: settings.get("worker.cpu_pinning_enabled").map(|v| v == "true").unwrap_or(true),
            memory_limit_mb: settings.get("worker.memory_limit_mb").and_then(|v| v.parse().ok()).unwrap_or(2048),
            data_dir: data_dir.to_owned(),
        }
    }

    /// Workspace base directory (tmpfs mount point or regular disk)
    pub fn workspace_dir(&self) -> PathBuf {
        if self.tmpfs_enabled {
            self.data_dir.join("workspace-tmpfs")
        } else {
            self.data_dir.join("workspace")
        }
    }

    /// Persistent dependency cache directory (survives between builds)
    pub fn dep_cache_dir(&self) -> PathBuf {
        self.data_dir.join("dep-cache")
    }

    /// Per-build workspace path
    pub fn build_workspace(&self, build_id: uuid::Uuid) -> PathBuf {
        self.workspace_dir().join(build_id.to_string())
    }
}

// ── tmpfs management ──

/// Mount tmpfs for build workspace (Linux only)
#[cfg(target_os = "linux")]
pub async fn setup_tmpfs(rt: &WorkerRuntime) -> anyhow::Result<()> {
    if !rt.tmpfs_enabled { return Ok(()); }

    let mount_point = rt.workspace_dir();
    tokio::fs::create_dir_all(&mount_point).await?;

    // Check if already mounted
    let output = tokio::process::Command::new("mountpoint")
        .arg("-q")
        .arg(&mount_point)
        .status()
        .await;

    if output.map(|s| s.success()).unwrap_or(false) {
        tracing::info!(path = ?mount_point, "tmpfs already mounted");
        return Ok(());
    }

    let size = format!("size={}m", rt.tmpfs_size_mb);
    let status = tokio::process::Command::new("mount")
        .args(["-t", "tmpfs", "-o", &size, "tmpfs"])
        .arg(&mount_point)
        .status()
        .await?;

    if status.success() {
        tracing::info!(path = ?mount_point, size_mb = rt.tmpfs_size_mb, "tmpfs mounted");
    } else {
        tracing::warn!("tmpfs mount failed (needs root or fstab entry), falling back to disk");
    }

    Ok(())
}

#[cfg(not(target_os = "linux"))]
pub async fn setup_tmpfs(rt: &WorkerRuntime) -> anyhow::Result<()> {
    if rt.tmpfs_enabled {
        tracing::info!("tmpfs not available on this platform, using disk workspace");
    }
    tokio::fs::create_dir_all(rt.workspace_dir()).await?;
    Ok(())
}

// ── Dependency cache setup ──

/// Create persistent dep cache directories
pub async fn setup_dep_cache(rt: &WorkerRuntime) -> anyhow::Result<()> {
    if !rt.dep_cache_enabled { return Ok(()); }

    let cache_dir = rt.dep_cache_dir();
    // Go module cache
    tokio::fs::create_dir_all(cache_dir.join("go/pkg/mod")).await?;
    tokio::fs::create_dir_all(cache_dir.join("go/build-cache")).await?;
    // Node modules cache
    tokio::fs::create_dir_all(cache_dir.join("npm")).await?;
    // Rust cargo cache
    tokio::fs::create_dir_all(cache_dir.join("cargo/registry")).await?;

    tracing::info!(path = ?cache_dir, "dependency cache directories ready");
    Ok(())
}

/// Environment variables that point build tools to the persistent cache
pub fn dep_cache_env(rt: &WorkerRuntime) -> Vec<(String, String)> {
    if !rt.dep_cache_enabled { return vec![]; }

    let cache = rt.dep_cache_dir();
    vec![
        ("GOMODCACHE".into(), cache.join("go/pkg/mod").to_string_lossy().into()),
        ("GOCACHE".into(), cache.join("go/build-cache").to_string_lossy().into()),
        ("GOPATH".into(), cache.join("go").to_string_lossy().into()),
        ("npm_config_cache".into(), cache.join("npm").to_string_lossy().into()),
        ("CARGO_HOME".into(), cache.join("cargo").to_string_lossy().into()),
    ]
}

// ── Cgroup CPU pinning ──

/// Create a cgroup for a build and pin it to specific CPUs.
/// Returns the cgroup path for adding child PIDs.
#[cfg(target_os = "linux")]
pub async fn create_build_cgroup(
    rt: &WorkerRuntime,
    build_id: uuid::Uuid,
    cpu_start: usize,
    cpu_count: usize,
) -> anyhow::Result<Option<PathBuf>> {
    if !rt.cpu_pinning_enabled { return Ok(None); }

    let cgroup_path = PathBuf::from(format!("/sys/fs/cgroup/jetrun/build-{}", build_id));

    // Create cgroup directory
    if let Err(e) = tokio::fs::create_dir_all(&cgroup_path).await {
        tracing::warn!(error = %e, "cgroup creation failed, running without CPU pinning");
        return Ok(None);
    }

    // Pin to CPU range
    let cpu_end = cpu_start + cpu_count - 1;
    let cpuset = format!("{}-{}", cpu_start, cpu_end);
    if tokio::fs::write(cgroup_path.join("cpuset.cpus"), &cpuset).await.is_err() {
        // cgroup v2 might use different path
        let _ = tokio::fs::write(cgroup_path.join("cpu.max"), format!("{} 100000", cpu_count * 100_000)).await;
    }

    // Memory limit
    let mem_bytes = rt.memory_limit_mb * 1024 * 1024;
    let _ = tokio::fs::write(cgroup_path.join("memory.max"), mem_bytes.to_string()).await;

    // PID limit
    let _ = tokio::fs::write(cgroup_path.join("pids.max"), "512").await;

    tracing::debug!(build_id = %build_id, cpuset = %cpuset, mem_mb = rt.memory_limit_mb, "cgroup created");
    Ok(Some(cgroup_path))
}

#[cfg(not(target_os = "linux"))]
pub async fn create_build_cgroup(
    _rt: &WorkerRuntime,
    _build_id: uuid::Uuid,
    _cpu_start: usize,
    _cpu_count: usize,
) -> anyhow::Result<Option<PathBuf>> {
    Ok(None)
}

/// Add a process to a cgroup
#[cfg(target_os = "linux")]
pub async fn add_to_cgroup(cgroup_path: &Path, pid: u32) -> anyhow::Result<()> {
    tokio::fs::write(cgroup_path.join("cgroup.procs"), pid.to_string()).await?;
    Ok(())
}

#[cfg(not(target_os = "linux"))]
pub async fn add_to_cgroup(_cgroup_path: &Path, _pid: u32) -> anyhow::Result<()> {
    Ok(())
}

/// Clean up cgroup after build
#[cfg(target_os = "linux")]
pub async fn cleanup_cgroup(build_id: uuid::Uuid) {
    let path = format!("/sys/fs/cgroup/jetrun/build-{}", build_id);
    let _ = tokio::fs::remove_dir(&path).await;
}

#[cfg(not(target_os = "linux"))]
pub async fn cleanup_cgroup(_build_id: uuid::Uuid) {}

/// Prepare build workspace: copy repo into tmpfs workspace, return workspace path
pub async fn prepare_workspace(
    rt: &WorkerRuntime,
    build_id: uuid::Uuid,
    repo_path: &str,
) -> anyhow::Result<PathBuf> {
    let workspace = rt.build_workspace(build_id);
    tokio::fs::create_dir_all(&workspace).await?;

    // Copy repo into workspace (cp -a for speed, preserves structure)
    let status = tokio::process::Command::new("cp")
        .args(["-a", repo_path, workspace.to_str().unwrap_or(".")])
        .status()
        .await?;

    if !status.success() {
        // Fallback: use the repo path directly
        tracing::warn!("workspace copy failed, using repo path directly");
        return Ok(PathBuf::from(repo_path));
    }

    // The workspace is now {workspace}/{repo_dir_name}
    // Repo path might be /opt/jetrun/data/repos/{project_id}
    let repo_name = Path::new(repo_path).file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();

    let build_dir = workspace.join(&repo_name);
    if build_dir.exists() {
        Ok(build_dir)
    } else {
        Ok(workspace)
    }
}

/// Clean up build workspace
pub async fn cleanup_workspace(rt: &WorkerRuntime, build_id: uuid::Uuid) {
    let workspace = rt.build_workspace(build_id);
    let _ = tokio::fs::remove_dir_all(&workspace).await;
}
