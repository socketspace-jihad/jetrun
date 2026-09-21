//! Linux namespace executor — PID + mount isolation via unshare(2).
//! Zero daemon overhead. Falls back to bare sh -c on non-Linux.

use std::time::Instant;

use tokio::process::Command;
use tokio::io::{AsyncBufReadExt, BufReader};

use jetrun_broker::types::BuildStepJob;

/// Execute a step inside Linux namespaces (PID + mount isolation).
/// Network is NOT isolated by default (builds need internet for go get, npm install, etc).
#[cfg(target_os = "linux")]
pub async fn execute_isolated(step: &BuildStepJob, working_dir: &str) -> anyhow::Result<i32> {
    use std::os::unix::process::CommandExt;

    let start = Instant::now();

    let mut cmd = Command::new("sh");
    cmd.arg("-c").arg(&step.command)
        .current_dir(working_dir)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());

    for (k, v) in &step.env { cmd.env(k, v); }

    // Pre-exec: create new PID + mount namespaces before exec
    unsafe {
        cmd.pre_exec(|| {
            // PID namespace: isolated process tree (can't see/kill other builds)
            // Mount namespace: isolated filesystem view
            // Skip CLONE_NEWNET: builds need internet (go get, npm install, etc)
            let flags = libc::CLONE_NEWPID | libc::CLONE_NEWNS;

            if libc::unshare(flags) != 0 {
                // Non-fatal: if unshare fails (e.g. unprivileged user), proceed without isolation
                let err = std::io::Error::last_os_error();
                eprintln!("jetrun: namespace unshare failed ({}), running without isolation", err);
                return Ok(());
            }

            // Remount /proc for the new PID namespace
            // This makes `ps` inside the build show only build processes
            let _ = libc::mount(
                b"proc\0".as_ptr() as *const libc::c_char,
                b"/proc\0".as_ptr() as *const libc::c_char,
                b"proc\0".as_ptr() as *const libc::c_char,
                0,
                std::ptr::null(),
            );

            Ok(())
        });
    }

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
    tracing::info!(step = %step.name, code = code, ms = start.elapsed().as_millis() as u64, "step finished (namespaced)");
    Ok(code)
}

/// Non-Linux fallback: bare sh -c, no isolation
#[cfg(not(target_os = "linux"))]
pub async fn execute_isolated(step: &BuildStepJob, working_dir: &str) -> anyhow::Result<i32> {
    crate::execute_native(step, working_dir).await
}
