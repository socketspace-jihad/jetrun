use std::collections::HashMap;
use std::time::Instant;

use bollard::container::{Config, CreateContainerOptions, LogsOptions, RemoveContainerOptions};
use bollard::Docker;
use futures::StreamExt;
use tokio::sync::mpsc;

use super::{ExecOutput, ExecResult, ExecStream};

/// Executes build steps inside Docker containers
pub struct DockerExecutor {
    client: Docker,
    container_id: tokio::sync::Mutex<Option<String>>,
}

impl DockerExecutor {
    pub fn new() -> anyhow::Result<Self> {
        let client = Docker::connect_with_local_defaults()?;
        Ok(Self {
            client,
            container_id: tokio::sync::Mutex::new(None),
        })
    }

    pub async fn execute(
        &self,
        image: &str,
        command: &str,
        env: &HashMap<String, String>,
        working_dir: &str,
        output_tx: mpsc::Sender<ExecOutput>,
    ) -> anyhow::Result<ExecResult> {
        let start = Instant::now();

        let env_vars: Vec<String> = env.iter().map(|(k, v)| format!("{k}={v}")).collect();

        let config = Config {
            image: Some(image.to_string()),
            cmd: Some(vec![
                "sh".to_string(),
                "-c".to_string(),
                command.to_string(),
            ]),
            env: Some(env_vars),
            working_dir: Some(working_dir.to_string()),
            ..Default::default()
        };

        let container = self
            .client
            .create_container(None::<CreateContainerOptions<String>>, config)
            .await?;

        *self.container_id.lock().await = Some(container.id.clone());

        self.client
            .start_container::<String>(&container.id, None)
            .await?;

        // Stream logs
        let mut logs = self.client.logs::<String>(
            &container.id,
            Some(LogsOptions {
                follow: true,
                stdout: true,
                stderr: true,
                ..Default::default()
            }),
        );

        while let Some(Ok(output)) = logs.next().await {
            let (stream, content) = match output {
                bollard::container::LogOutput::StdOut { message } => {
                    (ExecStream::Stdout, String::from_utf8_lossy(&message).to_string())
                }
                bollard::container::LogOutput::StdErr { message } => {
                    (ExecStream::Stderr, String::from_utf8_lossy(&message).to_string())
                }
                _ => continue,
            };

            let _ = output_tx.send(ExecOutput { stream, content }).await;
        }

        // Wait for container to finish
        let wait = self
            .client
            .wait_container::<String>(&container.id, None)
            .next()
            .await;

        let exit_code = wait
            .and_then(|r| r.ok())
            .map(|r| r.status_code as i32)
            .unwrap_or(-1);

        // Cleanup
        let _ = self
            .client
            .remove_container(
                &container.id,
                Some(RemoveContainerOptions {
                    force: true,
                    ..Default::default()
                }),
            )
            .await;

        *self.container_id.lock().await = None;

        Ok(ExecResult {
            exit_code,
            duration_ms: start.elapsed().as_millis() as u64,
        })
    }

    pub async fn cancel(&self) -> anyhow::Result<()> {
        if let Some(id) = self.container_id.lock().await.as_ref() {
            self.client.kill_container::<String>(id, None).await?;
        }
        Ok(())
    }
}
