use std::sync::{Arc, Mutex};

use dashmap::DashMap;
use tokio::sync::broadcast;
use uuid::Uuid;

use jetrun_common::models::{Build, Pipeline, Project, WebhookEvent};

/// Shared application state for the gateway
#[derive(Clone)]
pub struct AppState {
    pub inner: Arc<AppStateInner>,
}

pub struct AppStateInner {
    // In-memory stores (will be replaced with DB in production)
    pub projects: DashMap<Uuid, Project>,
    pub pipelines: DashMap<Uuid, Pipeline>,
    pub builds: DashMap<Uuid, Build>,

    // Build log broadcast channels keyed by build ID
    pub log_channels: DashMap<Uuid, broadcast::Sender<String>>,

    // Webhook secret for signature verification (shared across providers)
    webhook_secret: Option<String>,

    // Incoming webhook events queue (consumed by the engine)
    webhook_events: Mutex<Vec<WebhookEvent>>,
}

impl AppState {
    pub fn new() -> Self {
        let webhook_secret = std::env::var("WEBHOOK_SECRET").ok();

        Self {
            inner: Arc::new(AppStateInner {
                projects: DashMap::new(),
                pipelines: DashMap::new(),
                builds: DashMap::new(),
                log_channels: DashMap::new(),
                webhook_secret,
                webhook_events: Mutex::new(Vec::new()),
            }),
        }
    }

    pub fn get_or_create_log_channel(&self, build_id: Uuid) -> broadcast::Sender<String> {
        self.inner
            .log_channels
            .entry(build_id)
            .or_insert_with(|| {
                let (tx, _) = broadcast::channel(1024);
                tx
            })
            .clone()
    }

    /// Get the configured webhook secret (from WEBHOOK_SECRET env var)
    pub fn webhook_secret(&self) -> &Option<String> {
        &self.inner.webhook_secret
    }

    /// Queue a webhook event for the engine to process
    pub fn push_webhook_event(&self, event: WebhookEvent) {
        if let Ok(mut events) = self.inner.webhook_events.lock() {
            events.push(event);
        }
    }

    /// Drain all pending webhook events (called by engine polling or gRPC push)
    pub fn drain_webhook_events(&self) -> Vec<WebhookEvent> {
        if let Ok(mut events) = self.inner.webhook_events.lock() {
            std::mem::take(&mut *events)
        } else {
            Vec::new()
        }
    }
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}
