use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::RwLock;

use bytes::Bytes;
use chrono::Utc;
use dashmap::DashMap;
use uuid::Uuid;

use jetrun_common::models::{CacheEntry, CacheScope, CacheStats, Compression};

use super::cas::ContentAddressableStore;
use super::compression;

/// Local disk-based cache with content-addressable storage backend,
/// compression, and O(1) time-bucketed LRU eviction.
pub struct LocalStore {
    cas: ContentAddressableStore,
    entries: DashMap<String, CacheEntry>,
    /// Time-bucketed access index for O(1) LRU eviction.
    /// Maps unix timestamp (seconds) → list of cache keys accessed at that time.
    access_index: RwLock<BTreeMap<i64, Vec<String>>>,
}

impl LocalStore {
    pub fn new(cache_dir: PathBuf) -> Self {
        let cas = ContentAddressableStore::new(cache_dir);
        Self {
            cas,
            entries: DashMap::new(),
            access_index: RwLock::new(BTreeMap::new()),
        }
    }

    /// Store data with a user-defined key.
    /// Automatically selects compression based on data size.
    pub async fn put(&self, key: &str, data: &[u8]) -> anyhow::Result<CacheEntry> {
        let original_size = data.len() as u64;

        // Auto-select and apply compression
        let method = compression::auto_select(data.len());
        let compressed = compression::compress(data, method)?;
        let content_hash = self.cas.put(&compressed).await?;

        let now = Utc::now();
        let entry = CacheEntry {
            id: Uuid::new_v4(),
            key: key.to_string(),
            content_hash,
            size_bytes: original_size,
            compression: method,
            scope: CacheScope::Global,
            created_at: now,
            last_accessed: now,
            access_count: 0,
        };

        // Update access index
        if let Ok(mut index) = self.access_index.write() {
            index.entry(now.timestamp()).or_default().push(key.to_string());
        }

        self.entries.insert(key.to_string(), entry.clone());

        tracing::debug!(
            key = %key,
            original = original_size,
            compressed = compressed.len(),
            method = ?method,
            ratio = format!("{:.1}x", compression::ratio(data.len(), compressed.len())),
            "cache put"
        );

        Ok(entry)
    }

    /// Get data by user-defined key. Returns decompressed bytes.
    pub async fn get(&self, key: &str) -> anyhow::Result<Option<Bytes>> {
        let entry = match self.entries.get(key) {
            Some(e) => e.clone(),
            None => return Ok(None),
        };

        let compressed_data = match self.cas.get(&entry.content_hash).await? {
            Some(d) => d,
            None => return Ok(None),
        };

        // Decompress
        let decompressed = compression::decompress(&compressed_data, entry.compression)?;

        // Update access stats and index
        let now = Utc::now();
        if let Some(mut e) = self.entries.get_mut(key) {
            let old_ts = e.last_accessed.timestamp();
            e.last_accessed = now;
            e.access_count += 1;

            // Update access index: remove from old bucket, add to new
            if let Ok(mut index) = self.access_index.write() {
                if let Some(bucket) = index.get_mut(&old_ts) {
                    bucket.retain(|k| k != key);
                    if bucket.is_empty() {
                        index.remove(&old_ts);
                    }
                }
                index.entry(now.timestamp()).or_default().push(key.to_string());
            }
        }

        Ok(Some(Bytes::from(decompressed)))
    }

    /// Get by key or try restore_keys (prefix match, most recent first).
    pub async fn get_with_restore(
        &self,
        key: &str,
        restore_keys: &[String],
    ) -> anyhow::Result<Option<Bytes>> {
        // Try exact key first
        if let Some(data) = self.get(key).await? {
            return Ok(Some(data));
        }

        // Try restore keys (prefix match, most recent first)
        for restore_key in restore_keys {
            let mut matches: Vec<CacheEntry> = self
                .entries
                .iter()
                .filter(|e| e.key().starts_with(restore_key.as_str()))
                .map(|e| e.value().clone())
                .collect();

            matches.sort_by(|a, b| b.created_at.cmp(&a.created_at));

            if let Some(entry) = matches.first() {
                if let Some(data) = self.get(&entry.key).await? {
                    return Ok(Some(data));
                }
            }
        }

        Ok(None)
    }

    /// Check if a key exists.
    pub fn has(&self, key: &str) -> bool {
        self.entries.contains_key(key)
    }

    /// Remove a cache entry.
    pub async fn evict(&self, key: &str) -> anyhow::Result<bool> {
        if let Some((_, entry)) = self.entries.remove(key) {
            // Remove from access index
            if let Ok(mut index) = self.access_index.write() {
                let ts = entry.last_accessed.timestamp();
                if let Some(bucket) = index.get_mut(&ts) {
                    bucket.retain(|k| k != key);
                    if bucket.is_empty() {
                        index.remove(&ts);
                    }
                }
            }
            self.cas.delete(&entry.content_hash).await?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// Evict entries using time-bucketed LRU until total size is under max_bytes.
    /// O(1) amortized per eviction (pops oldest time bucket).
    pub async fn evict_lru(&self, max_bytes: u64) -> anyhow::Result<u64> {
        let total: u64 = self.entries.iter().map(|e| e.size_bytes).sum();
        if total <= max_bytes {
            return Ok(0);
        }

        let target = total - max_bytes;
        let mut freed = 0u64;
        let mut evicted = 0u64;

        loop {
            if freed >= target {
                break;
            }

            // Pop the oldest time bucket
            let oldest_keys = {
                let mut index = self.access_index.write().unwrap();
                match index.keys().next().copied() {
                    Some(ts) => index.remove(&ts).unwrap_or_default(),
                    None => break,
                }
            };

            for key in oldest_keys {
                if freed >= target {
                    break;
                }
                if let Some((_, entry)) = self.entries.remove(&key) {
                    freed += entry.size_bytes;
                    evicted += 1;
                    let _ = self.cas.delete(&entry.content_hash).await;
                }
            }
        }

        tracing::info!(evicted = evicted, freed_bytes = freed, "LRU eviction complete");
        Ok(evicted)
    }

    /// Get cache statistics.
    pub fn stats(&self) -> CacheStats {
        let total_entries = self.entries.len() as u64;
        let total_size_bytes: u64 = self.entries.iter().map(|e| e.size_bytes).sum();

        CacheStats {
            total_entries,
            total_size_bytes,
            hit_count: 0,
            miss_count: 0,
            hit_rate: 0.0,
            eviction_count: 0,
        }
    }
}
