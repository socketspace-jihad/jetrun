//! Linux namespace executor — PID + mount isolation via unshare(2).
//! Zero daemon overhead. Falls back to bare sh -c on non-Linux.

use jetrun_broker::types::BuildStepJob;

#[cfg(target_os = "linux")]
pub async fn execute_isolated(
    step: &BuildStepJob,
    working_dir: &str,
    build_log_dir: Option<&std::path::Path>,
    dep_env: &[(String, String)],
    cgroup: Option<&std::path::Path>,
) -> anyhow::Result<i32> {
    crate::execute_with_logs(step, working_dir, build_log_dir, true, dep_env, cgroup).await
}

#[cfg(not(target_os = "linux"))]
pub async fn execute_isolated(
    step: &BuildStepJob,
    working_dir: &str,
    build_log_dir: Option<&std::path::Path>,
    dep_env: &[(String, String)],
    cgroup: Option<&std::path::Path>,
) -> anyhow::Result<i32> {
    crate::execute_with_logs(step, working_dir, build_log_dir, false, dep_env, cgroup).await
}
