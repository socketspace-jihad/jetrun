use std::collections::HashMap;
use std::process::Stdio;
use std::time::Instant;

use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tokio::sync::mpsc;

use super::{ExecOutput, ExecResult, ExecStream};

/// Executes build steps as native processes on the host
pub struct NativeExecutor {
    child_id: tokio::sync::Mutex<Option<u32>>,
}

impl NativeExecutor {
    pub fn new() -> Self {
        Self {
            child_id: tokio::sync::Mutex::new(None),
        }
    }

    pub async fn execute(
        &self,
        command: &str,
        env: &HashMap<String, String>,
        working_dir: &str,
        output_tx: mpsc::Sender<ExecOutput>,
    ) -> anyhow::Result<ExecResult> {
        let start = Instant::now();

        let mut child = Command::new("sh")
            .arg("-c")
            .arg(command)
            .envs(env)
            .current_dir(working_dir)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;

        if let Some(pid) = child.id() {
            *self.child_id.lock().await = Some(pid);
        }

        let stdout = child.stdout.take().unwrap();
        let stderr = child.stderr.take().unwrap();

        let stdout_tx = output_tx.clone();
        let stdout_task = tokio::spawn(async move {
            let mut reader = BufReader::new(stdout).lines();
            while let Ok(Some(line)) = reader.next_line().await {
                let _ = stdout_tx
                    .send(ExecOutput {
                        stream: ExecStream::Stdout,
                        content: line,
                    })
                    .await;
            }
        });

        let stderr_tx = output_tx;
        let stderr_task = tokio::spawn(async move {
            let mut reader = BufReader::new(stderr).lines();
            while let Ok(Some(line)) = reader.next_line().await {
                let _ = stderr_tx
                    .send(ExecOutput {
                        stream: ExecStream::Stderr,
                        content: line,
                    })
                    .await;
            }
        });

        let _ = tokio::join!(stdout_task, stderr_task);
        let status = child.wait().await?;

        *self.child_id.lock().await = None;

        Ok(ExecResult {
            exit_code: status.code().unwrap_or(-1),
            duration_ms: start.elapsed().as_millis() as u64,
        })
    }

    pub async fn cancel(&self) -> anyhow::Result<()> {
        if let Some(pid) = *self.child_id.lock().await {
            // Send SIGTERM to the process
            #[cfg(unix)]
            {
                use std::process::Command;
                let _ = Command::new("kill")
                    .args(["-TERM", &pid.to_string()])
                    .output();
            }
            let _ = pid; // suppress unused on non-unix
        }
        Ok(())
    }
}

impl Default for NativeExecutor {
    fn default() -> Self {
        Self::new()
    }
}
