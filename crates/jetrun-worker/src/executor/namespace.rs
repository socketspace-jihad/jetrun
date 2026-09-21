//! Linux namespace executor — PID + mount isolation via unshare(2).
//! Zero daemon overhead. Falls back to bare sh -c on non-Linux.

use jetrun_broker::types::BuildStepJob;

/// Execute a step inside Linux namespaces (PID + mount isolation).
/// Tries user namespace first (unprivileged), falls back to CAP_SYS_ADMIN path.
#[cfg(target_os = "linux")]
pub async fn execute_isolated(step: &BuildStepJob, working_dir: &str, build_log_dir: Option<&std::path::Path>) -> anyhow::Result<i32> {
    crate::execute_with_logs(step, working_dir, build_log_dir, true).await
}

/// Non-Linux fallback: bare sh -c, no isolation
#[cfg(not(target_os = "linux"))]
pub async fn execute_isolated(step: &BuildStepJob, working_dir: &str, build_log_dir: Option<&std::path::Path>) -> anyhow::Result<i32> {
    crate::execute_with_logs(step, working_dir, build_log_dir, false).await
}
