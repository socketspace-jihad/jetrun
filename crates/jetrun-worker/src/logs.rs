//! Build log management — per-step logs on disk, history on S3-compatible storage.
//!
//! Layout:
//!   live:  {log_dir}/live/{build_id}/{step_id}.log
//!   S3:    builds/{prefix}/{build_id}/{step_id}.log

use std::path::{Path, PathBuf};
use tokio::fs;
use uuid::Uuid;

/// Create the per-build log directory and return its path
pub async fn create_build_log_dir(log_dir: &Path, build_id: Uuid) -> anyhow::Result<PathBuf> {
    let dir = log_dir.join("live").join(build_id.to_string());
    fs::create_dir_all(&dir).await?;
    Ok(dir)
}

/// Get the log file path for a specific step
pub fn step_log_path(build_log_dir: &Path, step_id: Uuid) -> PathBuf {
    build_log_dir.join(format!("{}.log", step_id))
}

/// List step log files in a build's log directory
pub async fn list_step_logs(log_dir: &Path, build_id: Uuid) -> Vec<Uuid> {
    let dir = log_dir.join("live").join(build_id.to_string());
    let mut entries = match fs::read_dir(&dir).await {
        Ok(e) => e,
        Err(_) => return vec![],
    };
    let mut step_ids = Vec::new();
    while let Ok(Some(entry)) = entries.next_entry().await {
        if let Some(name) = entry.file_name().to_str() {
            if let Some(id_str) = name.strip_suffix(".log") {
                if let Ok(id) = id_str.parse::<Uuid>() {
                    step_ids.push(id);
                }
            }
        }
    }
    step_ids
}

/// Read a step's live log from disk
pub async fn read_step_log(log_dir: &Path, build_id: Uuid, step_id: Uuid) -> anyhow::Result<String> {
    let path = log_dir.join("live").join(build_id.to_string()).join(format!("{}.log", step_id));
    match fs::read_to_string(&path).await {
        Ok(content) => Ok(content),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
        Err(e) => Err(e.into()),
    }
}

/// S3-compatible log storage for build history
pub struct LogStore {
    client: aws_sdk_s3::Client,
    bucket: String,
}

impl LogStore {
    pub async fn from_env() -> anyhow::Result<Self> {
        let endpoint = std::env::var("S3_ENDPOINT").ok();
        let bucket = std::env::var("S3_BUCKET").unwrap_or_else(|_| "jetrun-logs".into());
        let region = std::env::var("S3_REGION").unwrap_or_else(|_| "us-east-1".into());

        let mut config_loader = aws_config::defaults(aws_config::BehaviorVersion::latest())
            .region(aws_config::Region::new(region));

        if let Some(ep) = &endpoint {
            config_loader = config_loader.endpoint_url(ep);
        }

        let config = config_loader.load().await;
        let mut s3_config = aws_sdk_s3::config::Builder::from(&config);
        if endpoint.is_some() {
            s3_config = s3_config.force_path_style(true);
        }

        let client = aws_sdk_s3::Client::from_conf(s3_config.build());
        let _ = client.create_bucket().bucket(&bucket).send().await;

        tracing::info!(bucket = %bucket, endpoint = ?endpoint, "S3 log store initialized");
        Ok(Self { client, bucket })
    }

    /// Upload all step logs for a build to S3
    pub async fn upload_build_logs(&self, log_dir: &Path, build_id: Uuid) -> anyhow::Result<()> {
        let dir = log_dir.join("live").join(build_id.to_string());
        let mut entries = fs::read_dir(&dir).await?;
        let prefix = &build_id.to_string()[..8];

        while let Ok(Some(entry)) = entries.next_entry().await {
            let name = entry.file_name();
            let name_str = name.to_string_lossy();
            if !name_str.ends_with(".log") { continue; }

            let key = format!("builds/{}/{}/{}", prefix, build_id, name_str);
            let body = fs::read(entry.path()).await?;

            self.client
                .put_object()
                .bucket(&self.bucket)
                .key(&key)
                .body(body.into())
                .content_type("text/plain")
                .send()
                .await
                .map_err(|e| anyhow::anyhow!("S3 upload {}: {}", key, e))?;
        }

        tracing::info!(build_id = %build_id, "step logs uploaded to S3");
        Ok(())
    }

    /// Download a step log from S3
    pub async fn download_step(&self, build_id: Uuid, step_id: Uuid) -> anyhow::Result<String> {
        let prefix = &build_id.to_string()[..8];
        let key = format!("builds/{}/{}/{}.log", prefix, build_id, step_id);

        let resp = self.client
            .get_object()
            .bucket(&self.bucket)
            .key(&key)
            .send()
            .await
            .map_err(|e| anyhow::anyhow!("S3 download: {}", e))?;

        let bytes = resp.body.collect().await
            .map_err(|e| anyhow::anyhow!("S3 read: {}", e))?;

        Ok(String::from_utf8_lossy(&bytes.into_bytes()).to_string())
    }
}

/// Delete local build log directory after S3 upload
pub async fn cleanup_build_logs(log_dir: &Path, build_id: Uuid) -> anyhow::Result<()> {
    let dir = log_dir.join("live").join(build_id.to_string());
    let _ = fs::remove_dir_all(&dir).await;
    Ok(())
}
