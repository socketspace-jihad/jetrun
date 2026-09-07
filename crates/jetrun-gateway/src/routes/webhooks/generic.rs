use axum::{
    extract::State,
    http::StatusCode,
    routing::post,
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::state::AppState;
use jetrun_common::models::{WebhookEvent, WebhookEventType, WebhookProvider};

pub fn routes() -> Router<AppState> {
    Router::new().route("/", post(handle_generic_webhook))
}

/// Generic webhook — a simple JSON payload for custom integrations.
///
/// ```json
/// POST /api/v1/webhooks/generic
/// {
///   "repo_url": "https://github.com/user/repo.git",
///   "repo_name": "user/repo",
///   "branch": "main",
///   "commit_sha": "abc123",
///   "commit_message": "fix: something",
///   "author": "dev",
///   "event_type": "push"
/// }
/// ```
async fn handle_generic_webhook(
    State(state): State<AppState>,
    Json(payload): Json<GenericWebhookPayload>,
) -> Result<Json<Value>, StatusCode> {
    let event_type = match payload.event_type.as_deref() {
        Some("push") => WebhookEventType::Push,
        Some("pull_request" | "merge_request") => WebhookEventType::PullRequest,
        Some("tag") => WebhookEventType::Tag,
        _ => WebhookEventType::Push, // default to push
    };

    let event = WebhookEvent {
        provider: WebhookProvider::Generic,
        event_type,
        repo_url: payload.repo_url,
        repo_name: payload.repo_name,
        branch: payload.branch,
        commit_sha: payload.commit_sha,
        commit_message: payload.commit_message,
        author: payload.author,
        source_branch: payload.source_branch,
        target_branch: payload.target_branch,
        pr_number: payload.pr_number,
        pr_title: payload.pr_title,
    };

    tracing::info!(
        provider = %event.provider,
        event_type = ?event.event_type,
        repo = %event.repo_name,
        branch = %event.branch,
        commit = %event.commit_sha,
        "generic webhook event received"
    );

    state.push_webhook_event(event.clone());

    Ok(Json(json!({
        "status": "accepted",
        "provider": "generic",
        "event_type": event.event_type,
        "repo": event.repo_name,
        "branch": event.branch,
        "commit": event.commit_sha,
    })))
}

#[derive(Debug, Deserialize)]
struct GenericWebhookPayload {
    repo_url: String,
    repo_name: String,
    branch: String,
    commit_sha: String,
    #[serde(default)]
    commit_message: Option<String>,
    #[serde(default)]
    author: Option<String>,
    #[serde(default)]
    event_type: Option<String>,
    #[serde(default)]
    source_branch: Option<String>,
    #[serde(default)]
    target_branch: Option<String>,
    #[serde(default)]
    pr_number: Option<u64>,
    #[serde(default)]
    pr_title: Option<String>,
}
