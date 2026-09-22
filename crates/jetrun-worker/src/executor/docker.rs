use std::time::Instant;

use bollard::container::{Config, CreateContainerOptions, LogsOptions, RemoveContainerOptions, WaitContainerOptions};
use bollard::models::HostConfig;
use bollard::Docker;
use futures::StreamExt;
use tokio::sync::mpsc;
use uuid::Uuid;

use jetrun_broker::types::BuildStepJob;

/// Execute a build step inside a Docker container.
/// Mounts working_dir as /workspace. Bounded mpsc channel decouples
/// Docker log streaming from file I/O — log reading never blocks on disk.
pub async fn execute_docker(
    step: &BuildStepJob,
    image: &str,
    working_dir: &str,
    build_log_dir: Option<&std::path::Path>,
) -> anyhow::Result<i32> {
    let start = Instant::now();
    let client = Docker::connect_with_local_defaults()
        .map_err(|e| anyhow::anyhow!("Docker connect: {}", e))?;

    let _ = pull_image(&client, image).await;

    let env_vars: Vec<String> = step.env.iter().map(|(k, v)| format!("{k}={v}")).collect();
    let binds = vec![format!("{}:/workspace", working_dir)];

    let config = Config {
        image: Some(image.to_string()),
        cmd: Some(vec!["sh".into(), "-c".into(), step.command.clone()]),
        env: Some(env_vars),
        working_dir: Some("/workspace".into()),
        host_config: Some(HostConfig {
            binds: Some(binds),
            memory: Some(2 * 1024 * 1024 * 1024),   // 2GB
            nano_cpus: Some(2_000_000_000),           // 2 CPU cores
            pids_limit: Some(512),
            ..Default::default()
        }),
        ..Default::default()
    };

    let container_name = format!("jetrun-{}-{}", &step.step_id.to_string()[..8], &Uuid::new_v4().to_string()[..8]);
    let container = client
        .create_container(
            Some(CreateContainerOptions { name: &container_name, platform: None }),
            config,
        )
        .await
        .map_err(|e| anyhow::anyhow!("Docker create: {}", e))?;

    let container_id = container.id.clone();

    if let Err(e) = client.start_container::<String>(&container_id, None).await {
        let _ = client.remove_container(&container_id, Some(RemoveContainerOptions { force: true, ..Default::default() })).await;
        return Err(anyhow::anyhow!("Docker start: {}", e));
    }

    tracing::info!(step = %step.name, image = %image, container = %container_name, "Docker container started");

    // Bounded channel: Docker log producer → file writer consumer
    // 4096 capacity = ~4K lines buffered before backpressure. Docker log streaming
    // never blocks on disk I/O; the writer task drains the channel independently.
    let (tx, rx) = mpsc::channel::<(String, String)>(4096);

    // Producer: read Docker logs → send to channel
    let log_client = client.clone();
    let log_cid = container_id.clone();
    let step_name = step.name.clone();
    let producer = tokio::spawn(async move {
        let mut logs = log_client.logs::<String>(
            &log_cid,
            Some(LogsOptions { follow: true, stdout: true, stderr: true, ..Default::default() }),
        );

        while let Some(Ok(output)) = logs.next().await {
            let (stream_tag, content) = match output {
                bollard::container::LogOutput::StdOut { message } => ("stdout", String::from_utf8_lossy(&message).to_string()),
                bollard::container::LogOutput::StdErr { message } => ("stderr", String::from_utf8_lossy(&message).to_string()),
                _ => continue,
            };

            let content = content.trim_end_matches('\n').to_string();
            if content.is_empty() { continue; }

            match stream_tag {
                "stdout" => tracing::info!(step = %step_name, "{}", content),
                _ => tracing::warn!(step = %step_name, "{}", content),
            }

            // Non-blocking send to writer. If channel full (disk too slow),
            // drop line rather than stalling Docker log stream.
            let _ = tx.try_send((stream_tag.to_string(), content));
        }
    });

    // Consumer: drain channel → write to disk
    let log_path = build_log_dir.map(|dir| crate::logs::step_log_path(dir, step.step_id));
    let consumer = tokio::spawn(async move {
        let mut rx = rx;
        let file = match &log_path {
            Some(p) => tokio::fs::OpenOptions::new().create(true).append(true).open(p).await.ok(),
            None => None,
        };
        let mut file = match file {
            Some(f) => f,
            None => { while rx.recv().await.is_some() {} return; }
        };

        use tokio::io::AsyncWriteExt;
        use std::io::Write as _;
        // Batch buffer: accumulate lines, flush periodically for fewer syscalls
        let mut buf = Vec::with_capacity(8192);

        while let Some((stream_tag, content)) = rx.recv().await {
            let ts = chrono::Utc::now().format("%H:%M:%S%.3f");
            let _ = write!(buf, "[{}] [{}] {}\n", ts, stream_tag, content);

            // Flush when buffer exceeds 4KB or channel is empty (no more pending)
            if buf.len() >= 4096 || rx.is_empty() {
                let _ = file.write_all(&buf).await;
                buf.clear();
            }
        }

        // Flush remaining
        if !buf.is_empty() {
            let _ = file.write_all(&buf).await;
        }
    });

    // Wait for container with optional timeout
    let exit_code = match step.timeout_secs {
        Some(t) => {
            match tokio::time::timeout(
                std::time::Duration::from_secs(t as u64),
                wait_container(&client, &container_id),
            ).await {
                Ok(code) => code?,
                Err(_) => {
                    let _ = client.kill_container::<String>(&container_id, None).await;
                    let _ = client.remove_container(&container_id, Some(RemoveContainerOptions { force: true, ..Default::default() })).await;
                    anyhow::bail!("timeout {}s", t);
                }
            }
        }
        None => wait_container(&client, &container_id).await?,
    };

    // Wait for log tasks to drain
    let _ = producer.await;
    let _ = consumer.await;

    // Cleanup container
    let _ = client.remove_container(
        &container_id,
        Some(RemoveContainerOptions { force: true, ..Default::default() }),
    ).await;

    tracing::info!(
        step = %step.name, image = %image, code = exit_code,
        ms = start.elapsed().as_millis() as u64, "Docker step finished"
    );

    Ok(exit_code)
}

async fn wait_container(client: &Docker, container_id: &str) -> anyhow::Result<i32> {
    let mut wait_stream = client.wait_container(
        container_id,
        Some(WaitContainerOptions { condition: "not-running" }),
    );

    match wait_stream.next().await {
        Some(Ok(r)) => Ok(r.status_code as i32),
        Some(Err(e)) => Err(anyhow::anyhow!("Docker wait: {}", e)),
        None => Ok(-1),
    }
}

async fn pull_image(client: &Docker, image: &str) -> anyhow::Result<()> {
    use bollard::image::CreateImageOptions;

    let (repo, tag) = match image.split_once(':') {
        Some((r, t)) => (r, t),
        None => (image, "latest"),
    };

    let mut stream = client.create_image(
        Some(CreateImageOptions { from_image: repo, tag, ..Default::default() }),
        None,
        None,
    );

    while let Some(result) = stream.next().await {
        match result {
            Ok(info) => {
                if let Some(status) = info.status {
                    tracing::debug!(image = %image, "{}", status);
                }
            }
            Err(e) => {
                tracing::warn!(image = %image, error = %e, "pull failed, using cached image");
                return Ok(());
            }
        }
    }

    tracing::info!(image = %image, "image pulled");
    Ok(())
}
