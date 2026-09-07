use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use dashmap::DashMap;

use jetrun_common::models::CacheEntry;

#[derive(Clone)]
pub struct AppState {
    pub inner: Arc<AppStateInner>,
}

pub struct AppStateInner {
    pub entries: DashMap<String, CacheEntry>,
    pub cache_dir: PathBuf,
    pub hit_count: AtomicU64,
    pub miss_count: AtomicU64,
}

impl AppState {
    pub fn new(cache_dir: PathBuf) -> Self {
        Self {
            inner: Arc::new(AppStateInner {
                entries: DashMap::new(),
                cache_dir,
                hit_count: AtomicU64::new(0),
                miss_count: AtomicU64::new(0),
            }),
        }
    }

    pub fn record_hit(&self) {
        self.inner.hit_count.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_miss(&self) {
        self.inner.miss_count.fetch_add(1, Ordering::Relaxed);
    }

    pub fn hit_rate(&self) -> f64 {
        let hits = self.inner.hit_count.load(Ordering::Relaxed) as f64;
        let misses = self.inner.miss_count.load(Ordering::Relaxed) as f64;
        let total = hits + misses;
        if total == 0.0 { 0.0 } else { hits / total }
    }
}
