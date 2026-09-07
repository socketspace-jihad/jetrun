use std::path::PathBuf;

use bytes::Bytes;
use tokio::fs;

use super::zero_copy;

/// Durability level for CAS writes.
#[derive(Debug, Clone, Copy, Default)]
pub enum Durability {
    /// No fsync. Crash may lose recent objects — recreate by re-running step.
    /// Correct because the store is a cache: every object can be recreated.
    /// Default: avoids fsync per object, which dominates at 50K files/workspace.
    #[default]
    Relaxed,
    /// fsync before rename + fsync parent dir. Survives power loss.
    /// Use only where store is artifact archive of record.
    Synced,
}

/// Content-addressable storage using blake3 hashing.
///
/// Key properties:
/// - **Atomic insert**: write to tmp/ → seal read-only → rename to final path.
///   Reader never sees a partial object.
/// - **Deduplication**: identical content → identical hash → insert is no-op.
/// - **Zero-copy reads**: mmap via kernel page cache.
/// - **Concurrent safe**: multiple writers inserting same object need no locks.
pub struct ContentAddressableStore {
    root: PathBuf,
    tmp_dir: PathBuf,
    durability: Durability,
}

impl ContentAddressableStore {
    pub fn new(root: PathBuf) -> Self {
        let tmp_dir = root.join("tmp");
        Self {
            root,
            tmp_dir,
            durability: Durability::Relaxed,
        }
    }

    pub fn with_durability(mut self, durability: Durability) -> Self {
        self.durability = durability;
        self
    }

    /// Ensure tmp/ directory exists (call once at startup).
    pub async fn init(&self) -> anyhow::Result<()> {
        fs::create_dir_all(&self.tmp_dir).await?;
        fs::create_dir_all(self.root.join("blobs")).await?;
        Ok(())
    }

    /// Store data atomically and return its content hash.
    ///
    /// Flow: write to tmp/<hash>.staging → chmod 0444 → atomic rename to final path.
    /// A crash at any point leaves no corrupt object in the final location.
    pub async fn put(&self, data: &[u8]) -> anyhow::Result<String> {
        let hash = blake3::hash(data).to_hex().to_string();
        let final_path = self.blob_path(&hash);

        // Deduplication: skip if already exists
        if fs::try_exists(&final_path).await.unwrap_or(false) {
            return Ok(hash);
        }

        // Ensure parent directory exists
        if let Some(parent) = final_path.parent() {
            fs::create_dir_all(parent).await?;
        }

        // Atomic insert: tmp → seal → rename
        let staging_path = self.tmp_dir.join(format!("{}.staging", hash));
        let durability = self.durability;
        let staging = staging_path.clone();
        let final_p = final_path.clone();
        let owned_data = data.to_vec(); // Own the data for spawn_blocking

        tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
            use std::io::Write;

            // 1. Write to staging file
            let mut file = std::fs::File::create(&staging)?;
            file.write_all(&owned_data)?;

            // 2. Fsync if Synced durability
            if matches!(durability, Durability::Synced) {
                file.sync_all()?;
            }
            drop(file);

            // 3. Seal read-only (defense in depth)
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let perms = std::fs::Permissions::from_mode(0o444);
                std::fs::set_permissions(&staging, perms)?;
            }

            // 4. Atomic rename to final path
            std::fs::rename(&staging, &final_p)?;

            // 5. Fsync parent directory if Synced
            if matches!(durability, Durability::Synced) {
                if let Some(parent) = final_p.parent() {
                    let dir = std::fs::File::open(parent)?;
                    dir.sync_all()?;
                }
            }

            Ok(())
        })
        .await??;

        tracing::debug!(hash = %hash, size = data.len(), "stored blob (atomic)");
        Ok(hash)
    }

    /// Retrieve data by content hash using zero-copy mmap.
    /// Returns Bytes backed by a memory-mapped file — no data copy.
    pub async fn get(&self, hash: &str) -> anyhow::Result<Option<Bytes>> {
        let path = self.blob_path(hash);

        if !fs::try_exists(&path).await.unwrap_or(false) {
            return Ok(None);
        }

        let bytes = tokio::task::spawn_blocking(move || zero_copy::mmap_to_bytes(&path)).await??;
        Ok(Some(bytes))
    }

    /// Check if a blob exists.
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

    /// List all blob hashes in the store (for GC).
    pub async fn list_all_hashes(&self) -> anyhow::Result<Vec<String>> {
        let blobs_dir = self.root.join("blobs");
        let blobs_dir_clone = blobs_dir.clone();

        tokio::task::spawn_blocking(move || {
            let mut hashes = Vec::new();
            let prefix_dirs = match std::fs::read_dir(&blobs_dir_clone) {
                Ok(d) => d,
                Err(_) => return Ok(hashes),
            };

            for prefix_entry in prefix_dirs {
                let prefix_entry = prefix_entry?;
                if !prefix_entry.file_type()?.is_dir() {
                    continue;
                }
                let prefix = prefix_entry.file_name().to_string_lossy().to_string();

                for blob_entry in std::fs::read_dir(prefix_entry.path())? {
                    let blob_entry = blob_entry?;
                    let rest = blob_entry.file_name().to_string_lossy().to_string();
                    hashes.push(format!("{}{}", prefix, rest));
                }
            }
            Ok(hashes)
        })
        .await?
    }

    /// Garbage collect: delete blobs not in the live set, older than min_age.
    pub async fn gc(
        &self,
        live_hashes: &std::collections::HashSet<String>,
        min_age: std::time::Duration,
    ) -> anyhow::Result<GcStats> {
        let all_hashes = self.list_all_hashes().await?;
        let now = std::time::SystemTime::now();
        let mut stats = GcStats::default();

        for hash in all_hashes {
            if live_hashes.contains(&hash) {
                stats.live += 1;
                continue;
            }

            let path = self.blob_path(&hash);
            let metadata = match fs::metadata(&path).await {
                Ok(m) => m,
                Err(_) => continue,
            };

            // Check age — spare young objects (may be in-flight)
            if let Ok(modified) = metadata.modified() {
                if let Ok(age) = now.duration_since(modified) {
                    if age < min_age {
                        stats.spared_young += 1;
                        continue;
                    }
                }
            }

            stats.bytes_freed += metadata.len();
            stats.deleted += 1;
            let _ = fs::remove_file(&path).await;
        }

        tracing::info!(
            live = stats.live,
            deleted = stats.deleted,
            freed = stats.bytes_freed,
            spared = stats.spared_young,
            "CAS garbage collection complete"
        );

        Ok(stats)
    }

    /// 2-char prefix sharding for filesystem performance.
    fn blob_path(&self, hash: &str) -> PathBuf {
        let (prefix, rest) = hash.split_at(2.min(hash.len()));
        self.root.join("blobs").join(prefix).join(rest)
    }
}

#[derive(Debug, Default)]
pub struct GcStats {
    pub live: u64,
    pub deleted: u64,
    pub bytes_freed: u64,
    pub spared_young: u64,
}
