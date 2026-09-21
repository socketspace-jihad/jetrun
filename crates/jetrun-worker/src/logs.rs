//! Build log management — live logs on disk, history on S3-compatible storage.
//!
//! Flow:
//!   1. During build: step stdout/stderr → appended to disk file
//!   2. After build: upload log file to S3 → delete local file
//!   3. Dashboard: live → read from disk, history → fetch from S3

use std::path::{Path, PathBuf};
use tokio::fs;
use tokio::io::AsyncWriteExt;
use uuid::Uuid;

/// Local log directory for in-progress builds
pub struct LogWriter {
    log_dir: PathBuf,
    build_id: Uuid,
    file: tokio::fs::File,
}

impl LogWriter {
    /// Create a new log writer for a build
    pub async fn new(log_dir: &Path, build_id: Uuid) -> anyhow::Result<Self> {
        let dir = log_dir.join("live");
        fs::create_dir_all(&dir).await?;
        let path = dir.join(format!("{}.log", build_id));
        let file = fs::OpenOptions::new().create(true).append(true).open(&path).await?;
        Ok(Self { log_dir: log_dir.to_owned(), build_id, file })
    }

    /// Append a log line
    pub async fn write_line(&mut self, step: &str, stream: &str, content: &str) -> anyhow::Result<()> {
        let ts = chrono::Utc::now().format("%H:%M:%S%.3f");
        let line = format!("[{}] [{}] [{}] {}\n", ts, step, stream, content);
        self.file.write_all(line.as_bytes()).await?;
        Ok(())
    }

    /// Flush to disk
    pub async fn flush(&mut self) -> anyhow::Result<()> {
        self.file.flush().await?;
        Ok(())
    }

    /// Get the path to the live log file (for reading/streaming)
    pub fn live_path(&self) -> PathBuf {
        self.log_dir.join("live").join(format!("{}.log", self.build_id))
    }
}

/// S3-compatible log storage for build history
pub struct LogStore {
    client: aws_sdk_s3::Client,
    bucket: String,
}

impl LogStore {
    /// Create from environment variables:
    ///   S3_ENDPOINT    — e.g. http://localhost:9000 (MinIO) or https://s3.amazonaws.com
    ///   S3_BUCKET      — bucket name
    ///   S3_REGION      — region (default: us-east-1)
    ///   AWS_ACCESS_KEY_ID / AWS_SECRET_ACCESS_KEY — credentials
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

        // Force path style for MinIO and other S3-compatible stores
        if endpoint.is_some() {
            s3_config = s3_config.force_path_style(true);
        }

        let client = aws_sdk_s3::Client::from_conf(s3_config.build());

        // Ensure bucket exists (ignore error if already exists)
        let _ = client.create_bucket().bucket(&bucket).send().await;

        tracing::info!(bucket = %bucket, endpoint = ?endpoint, "S3 log store initialized");
        Ok(Self { client, bucket })
    }

    /// Upload a build log file to S3
    pub async fn upload(&self, build_id: Uuid, log_path: &Path) -> anyhow::Result<String> {
        let key = format!("builds/{}/{}.log", &build_id.to_string()[..8], build_id);
        let body = fs::read(log_path).await?;

        self.client
            .put_object()
            .bucket(&self.bucket)
            .key(&key)
            .body(body.into())
            .content_type("text/plain")
            .send()
            .await
            .map_err(|e| anyhow::anyhow!("S3 upload: {}", e))?;

        tracing::info!(build_id = %build_id, key = %key, "log uploaded to S3");
        Ok(key)
    }

    /// Download a build log from S3
    pub async fn download(&self, build_id: Uuid) -> anyhow::Result<String> {
        let key = format!("builds/{}/{}.log", &build_id.to_string()[..8], build_id);

        let resp = self.client
            .get_object()
            .bucket(&self.bucket)
            .key(&key)
            .send()
            .await
            .map_err(|e| anyhow::anyhow!("S3 download: {}", e))?;

        let bytes = resp.body.collect().await
            .map_err(|e| anyhow::anyhow!("S3 read body: {}", e))?;

        Ok(String::from_utf8_lossy(&bytes.into_bytes()).to_string())
    }
}

/// Read a live log file from disk (for in-progress builds)
pub async fn read_live_log(log_dir: &Path, build_id: Uuid) -> anyhow::Result<String> {
    let path = log_dir.join("live").join(format!("{}.log", build_id));
    match fs::read_to_string(&path).await {
        Ok(content) => Ok(content),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
        Err(e) => Err(e.into()),
    }
}

/// Delete local live log after upload to S3
pub async fn cleanup_live_log(log_dir: &Path, build_id: Uuid) -> anyhow::Result<()> {
    let path = log_dir.join("live").join(format!("{}.log", build_id));
    let _ = fs::remove_file(&path).await;
    Ok(())
}
