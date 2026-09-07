use std::path::PathBuf;

use bytes::Bytes;
use tokio::fs;

use super::zero_copy;

/// Content-addressable storage using blake3 hashing.
/// Files are stored by their content hash, enabling deduplication.
/// Reads use mmap for zero-copy access via the kernel page cache.
pub struct ContentAddressableStore {
    root: PathBuf,
}

impl ContentAddressableStore {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    /// Store data and return its content hash.
    /// Deduplicates: skips write if blob already exists.
    pub async fn put(&self, data: &[u8]) -> anyhow::Result<String> {
        let hash = blake3::hash(data).to_hex().to_string();
        let path = self.blob_path(&hash);

        // Async existence check (no blocking Tokio thread)
        if fs::try_exists(&path).await.unwrap_or(false) {
            return Ok(hash); // Already stored (deduplication)
        }

        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).await?;
        }

        fs::write(&path, data).await?;
        tracing::debug!(hash = %hash, size = data.len(), "stored blob");

        Ok(hash)
    }

    /// Retrieve data by content hash using zero-copy mmap.
    /// Returns Bytes backed by a memory-mapped file — no data copy.
    pub async fn get(&self, hash: &str) -> anyhow::Result<Option<Bytes>> {
        let path = self.blob_path(hash);

        if !fs::try_exists(&path).await.unwrap_or(false) {
            return Ok(None);
        }

        // mmap is a blocking call (page fault on first access),
        // so run in spawn_blocking to avoid blocking Tokio
        let bytes = tokio::task::spawn_blocking(move || zero_copy::mmap_to_bytes(&path)).await??;

        Ok(Some(bytes))
    }

    /// Check if a blob exists (async, non-blocking).
    pub async fn has(&self, hash: &str) -> bool {
        let path = self.blob_path(hash);
        fs::try_exists(&path).await.unwrap_or(false)
    }

    /// Delete a blob by hash.
    pub async fn delete(&self, hash: &str) -> anyhow::Result<bool> {
        let path = self.blob_path(hash);
        if fs::try_exists(&path).await.unwrap_or(false) {
            fs::remove_file(&path).await?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// Compute blake3 hash of a file using mmap + SIMD parallel hashing.
    /// 10-50x faster than reading chunks through async I/O for large files.
    pub async fn hash_file(path: &std::path::Path) -> anyhow::Result<String> {
        let path = path.to_owned();
        tokio::task::spawn_blocking(move || zero_copy::hash_file_mmap(&path)).await?
    }

    /// Get the size of a blob in bytes.
    pub async fn blob_size(&self, hash: &str) -> anyhow::Result<Option<u64>> {
        let path = self.blob_path(hash);
        match fs::metadata(&path).await {
            Ok(meta) => Ok(Some(meta.len())),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    /// Use first 2 chars of hash as directory prefix for filesystem performance.
    /// This limits inode density per directory (important for ext4/xfs with >10K files).
    fn blob_path(&self, hash: &str) -> PathBuf {
        let (prefix, rest) = hash.split_at(2.min(hash.len()));
        self.root.join("blobs").join(prefix).join(rest)
    }
}
