use std::sync::Arc;
use std::sync::atomic::AtomicU32;

#[derive(Clone)]
pub struct AppState {
    pub inner: Arc<AppStateInner>,
}

pub struct AppStateInner {
    pub active_steps: AtomicU32,
    pub max_concurrent: u32,
}

impl AppState {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(AppStateInner {
                active_steps: AtomicU32::new(0),
                max_concurrent: 4,
            }),
        }
    }
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}
