use std::path::{Path, PathBuf};

use tokio::fs;
use uuid::Uuid;

/// Manages build artifacts — files produced by build steps that can be downloaded later.
pub struct ArtifactStore {
    root: PathBuf,
}

impl ArtifactStore {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    /// Store an artifact for a build
    pub async fn store(
        &self,
        build_id: Uuid,
        name: &str,
        data: &[u8],
    ) -> anyhow::Result<PathBuf> {
        let dir = self.root.join(build_id.to_string());
        fs::create_dir_all(&dir).await?;

        let path = dir.join(name);
        fs::write(&path, data).await?;

        tracing::info!(
            build_id = %build_id,
            name = %name,
            size = data.len(),
            "stored artifact"
        );

        Ok(path)
    }

    /// Retrieve an artifact
    pub async fn get(&self, build_id: Uuid, name: &str) -> anyhow::Result<Option<Vec<u8>>> {
        let path = self.root.join(build_id.to_string()).join(name);
        if !path.exists() {
            return Ok(None);
        }
        let data = fs::read(&path).await?;
        Ok(Some(data))
    }

    /// List artifacts for a build
    pub async fn list(&self, build_id: Uuid) -> anyhow::Result<Vec<String>> {
        let dir = self.root.join(build_id.to_string());
        if !dir.exists() {
            return Ok(vec![]);
        }

        let mut names = Vec::new();
        let mut entries = fs::read_dir(&dir).await?;
        while let Some(entry) = entries.next_entry().await? {
            if let Some(name) = entry.file_name().to_str() {
                names.push(name.to_string());
            }
        }
        Ok(names)
    }

    /// Delete all artifacts for a build
    pub async fn cleanup(&self, build_id: Uuid) -> anyhow::Result<()> {
        let dir = self.root.join(build_id.to_string());
        if dir.exists() {
            fs::remove_dir_all(&dir).await?;
        }
        Ok(())
    }

    /// Delete artifacts older than retention_days
    pub async fn cleanup_expired(&self, _retention_days: u32) -> anyhow::Result<u64> {
        // Stub: will scan directories and check timestamps
        Ok(0)
    }
}
