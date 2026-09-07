use axum::{
    body::Bytes,
    extract::State,
    http::{HeaderMap, StatusCode},
    routing::post,
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::state::AppState;
use jetrun_common::models::{WebhookEvent, WebhookEventType, WebhookProvider};

use super::verify::verify_github_signature;

pub fn routes() -> Router<AppState> {
    Router::new().route("/", post(handle_github_webhook))
}

async fn handle_github_webhook(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Json<Value>, StatusCode> {
    // Extract headers
    let event_type = headers
        .get("X-GitHub-Event")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("unknown");

    let signature = headers
        .get("X-Hub-Signature-256")
        .and_then(|v| v.to_str().ok());

    // Verify signature if a webhook secret is configured
    if let Some(secret) = &state.webhook_secret() {
        let sig = signature.ok_or_else(|| {
            tracing::warn!("github webhook missing X-Hub-Signature-256 header");
            StatusCode::UNAUTHORIZED
        })?;

        if !verify_github_signature(secret, &body, sig) {
            tracing::warn!("github webhook signature verification failed");
            return Err(StatusCode::UNAUTHORIZED);
        }
    }

    // Parse the event
    let event = match event_type {
        "push" => parse_push_event(&body)?,
        "pull_request" => parse_pull_request_event(&body)?,
        "ping" => {
            tracing::info!("github webhook ping received");
            return Ok(Json(json!({ "status": "pong" })));
        }
        other => {
            tracing::debug!(event = other, "ignoring unhandled github event type");
            return Ok(Json(json!({ "status": "ignored", "event": other })));
        }
    };

    tracing::info!(
        provider = %event.provider,
        event_type = ?event.event_type,
        repo = %event.repo_name,
        branch = %event.branch,
        commit = %event.commit_sha,
        "webhook event received"
    );

    // Store the event for the engine to pick up
    state.push_webhook_event(event.clone());

    Ok(Json(json!({
        "status": "accepted",
        "provider": "github",
        "event_type": event.event_type,
        "repo": event.repo_name,
        "branch": event.branch,
        "commit": event.commit_sha,
    })))
}

// --- GitHub payload types ---

#[derive(Debug, Deserialize)]
struct GithubPushPayload {
    #[serde(rename = "ref")]
    git_ref: String,
    after: String,
    repository: GithubRepository,
    head_commit: Option<GithubCommit>,
    pusher: Option<GithubUser>,
}

#[derive(Debug, Deserialize)]
struct GithubPullRequestPayload {
    action: String,
    number: u64,
    pull_request: GithubPullRequest,
    repository: GithubRepository,
}

#[derive(Debug, Deserialize)]
struct GithubPullRequest {
    title: String,
    head: GithubBranchRef,
    base: GithubBranchRef,
    user: GithubUser,
}

#[derive(Debug, Deserialize)]
struct GithubBranchRef {
    #[serde(rename = "ref")]
    branch: String,
    sha: String,
}

#[derive(Debug, Deserialize)]
struct GithubRepository {
    full_name: String,
    clone_url: String,
}

#[derive(Debug, Deserialize)]
struct GithubCommit {
    message: String,
}

#[derive(Debug, Deserialize)]
struct GithubUser {
    #[serde(alias = "login")]
    name: String,
}

// --- Parsers ---

fn parse_push_event(body: &[u8]) -> Result<WebhookEvent, StatusCode> {
    let payload: GithubPushPayload = serde_json::from_slice(body).map_err(|e| {
        tracing::error!(error = %e, "failed to parse github push payload");
        StatusCode::BAD_REQUEST
    })?;

    // Extract branch name from "refs/heads/main" → "main"
    let branch = payload
        .git_ref
        .strip_prefix("refs/heads/")
        .or_else(|| payload.git_ref.strip_prefix("refs/tags/"))
        .unwrap_or(&payload.git_ref)
        .to_string();

    let event_type = if payload.git_ref.starts_with("refs/tags/") {
        WebhookEventType::Tag
    } else {
        WebhookEventType::Push
    };

    Ok(WebhookEvent {
        provider: WebhookProvider::Github,
        event_type,
        repo_url: payload.repository.clone_url,
        repo_name: payload.repository.full_name,
        branch,
        commit_sha: payload.after,
        commit_message: payload.head_commit.map(|c| c.message),
        author: payload.pusher.map(|u| u.name),
        source_branch: None,
        target_branch: None,
        pr_number: None,
        pr_title: None,
    })
}

fn parse_pull_request_event(body: &[u8]) -> Result<WebhookEvent, StatusCode> {
    let payload: GithubPullRequestPayload = serde_json::from_slice(body).map_err(|e| {
        tracing::error!(error = %e, "failed to parse github pull_request payload");
        StatusCode::BAD_REQUEST
    })?;

    // Only trigger builds on actionable PR events
    match payload.action.as_str() {
        "opened" | "synchronize" | "reopened" => {}
        other => {
            tracing::debug!(action = other, "ignoring non-actionable PR event");
            // Return a "no-op" event that won't trigger a build
            return Ok(WebhookEvent {
                provider: WebhookProvider::Github,
                event_type: WebhookEventType::Unknown,
                repo_url: payload.repository.clone_url,
                repo_name: payload.repository.full_name,
                branch: payload.pull_request.head.branch.clone(),
                commit_sha: payload.pull_request.head.sha.clone(),
                commit_message: None,
                author: Some(payload.pull_request.user.name),
                source_branch: Some(payload.pull_request.head.branch),
                target_branch: Some(payload.pull_request.base.branch),
                pr_number: Some(payload.number),
                pr_title: Some(payload.pull_request.title),
            });
        }
    }

    Ok(WebhookEvent {
        provider: WebhookProvider::Github,
        event_type: WebhookEventType::PullRequest,
        repo_url: payload.repository.clone_url,
        repo_name: payload.repository.full_name,
        branch: payload.pull_request.head.branch.clone(),
        commit_sha: payload.pull_request.head.sha.clone(),
        commit_message: None,
        author: Some(payload.pull_request.user.name),
        source_branch: Some(payload.pull_request.head.branch),
        target_branch: Some(payload.pull_request.base.branch),
        pr_number: Some(payload.number),
        pr_title: Some(payload.pull_request.title),
    })
}
