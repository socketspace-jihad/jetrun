use std::sync::Arc;

use dashmap::DashMap;
use uuid::Uuid;

use jetrun_common::models::Build;

#[derive(Clone)]
pub struct AppState {
    pub inner: Arc<AppStateInner>,
}

pub struct AppStateInner {
    pub active_builds: DashMap<Uuid, Build>,
}

impl AppState {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(AppStateInner {
                active_builds: DashMap::new(),
            }),
        }
    }
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}
