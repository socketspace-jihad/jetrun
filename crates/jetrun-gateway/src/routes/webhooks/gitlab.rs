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

use super::verify::verify_gitlab_token;

pub fn routes() -> Router<AppState> {
    Router::new().route("/", post(handle_gitlab_webhook))
}

async fn handle_gitlab_webhook(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Json<Value>, StatusCode> {
    let event_type = headers
        .get("X-Gitlab-Event")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("unknown");

    // Verify token if configured
    if let Some(expected_token) = &state.webhook_secret() {
        let received_token = headers
            .get("X-Gitlab-Token")
            .and_then(|v| v.to_str().ok())
            .ok_or_else(|| {
                tracing::warn!("gitlab webhook missing X-Gitlab-Token header");
                StatusCode::UNAUTHORIZED
            })?;

        if !verify_gitlab_token(expected_token, received_token) {
            tracing::warn!("gitlab webhook token verification failed");
            return Err(StatusCode::UNAUTHORIZED);
        }
    }

    let event = match event_type {
        "Push Hook" => parse_push_event(&body)?,
        "Merge Request Hook" => parse_merge_request_event(&body)?,
        "Tag Push Hook" => parse_tag_push_event(&body)?,
        other => {
            tracing::debug!(event = other, "ignoring unhandled gitlab event type");
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

    state.push_webhook_event(event.clone());

    Ok(Json(json!({
        "status": "accepted",
        "provider": "gitlab",
        "event_type": event.event_type,
        "repo": event.repo_name,
        "branch": event.branch,
        "commit": event.commit_sha,
    })))
}

// --- GitLab payload types ---

#[derive(Debug, Deserialize)]
struct GitlabPushPayload {
    #[serde(rename = "ref")]
    git_ref: String,
    after: String,
    project: GitlabProject,
    commits: Vec<GitlabCommit>,
    user_name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GitlabMergeRequestPayload {
    object_attributes: GitlabMergeRequest,
    project: GitlabProject,
    user: GitlabUser,
}

#[derive(Debug, Deserialize)]
struct GitlabMergeRequest {
    iid: u64,
    title: String,
    action: Option<String>,
    source_branch: String,
    target_branch: String,
    last_commit: GitlabCommit,
}

#[derive(Debug, Deserialize)]
struct GitlabProject {
    path_with_namespace: String,
    git_http_url: String,
}

#[derive(Debug, Deserialize)]
struct GitlabCommit {
    id: String,
    message: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GitlabUser {
    name: String,
}

// --- Parsers ---

fn parse_push_event(body: &[u8]) -> Result<WebhookEvent, StatusCode> {
    let payload: GitlabPushPayload = serde_json::from_slice(body).map_err(|e| {
        tracing::error!(error = %e, "failed to parse gitlab push payload");
        StatusCode::BAD_REQUEST
    })?;

    let branch = payload
        .git_ref
        .strip_prefix("refs/heads/")
        .unwrap_or(&payload.git_ref)
        .to_string();

    let commit_message = payload.commits.first().and_then(|c| c.message.clone());

    Ok(WebhookEvent {
        provider: WebhookProvider::Gitlab,
        event_type: WebhookEventType::Push,
        repo_url: payload.project.git_http_url,
        repo_name: payload.project.path_with_namespace,
        branch,
        commit_sha: payload.after,
        commit_message,
        author: payload.user_name,
        source_branch: None,
        target_branch: None,
        pr_number: None,
        pr_title: None,
    })
}

fn parse_tag_push_event(body: &[u8]) -> Result<WebhookEvent, StatusCode> {
    let payload: GitlabPushPayload = serde_json::from_slice(body).map_err(|e| {
        tracing::error!(error = %e, "failed to parse gitlab tag push payload");
        StatusCode::BAD_REQUEST
    })?;

    let tag = payload
        .git_ref
        .strip_prefix("refs/tags/")
        .unwrap_or(&payload.git_ref)
        .to_string();

    Ok(WebhookEvent {
        provider: WebhookProvider::Gitlab,
        event_type: WebhookEventType::Tag,
        repo_url: payload.project.git_http_url,
        repo_name: payload.project.path_with_namespace,
        branch: tag,
        commit_sha: payload.after,
        commit_message: None,
        author: payload.user_name,
        source_branch: None,
        target_branch: None,
        pr_number: None,
        pr_title: None,
    })
}

fn parse_merge_request_event(body: &[u8]) -> Result<WebhookEvent, StatusCode> {
    let payload: GitlabMergeRequestPayload = serde_json::from_slice(body).map_err(|e| {
        tracing::error!(error = %e, "failed to parse gitlab merge request payload");
        StatusCode::BAD_REQUEST
    })?;

    // Only trigger on actionable MR events
    let event_type = match payload
        .object_attributes
        .action
        .as_deref()
    {
        Some("open" | "reopen" | "update") => WebhookEventType::PullRequest,
        _ => WebhookEventType::Unknown,
    };

    Ok(WebhookEvent {
        provider: WebhookProvider::Gitlab,
        event_type,
        repo_url: payload.project.git_http_url,
        repo_name: payload.project.path_with_namespace,
        branch: payload.object_attributes.source_branch.clone(),
        commit_sha: payload.object_attributes.last_commit.id,
        commit_message: payload.object_attributes.last_commit.message,
        author: Some(payload.user.name),
        source_branch: Some(payload.object_attributes.source_branch),
        target_branch: Some(payload.object_attributes.target_branch),
        pr_number: Some(payload.object_attributes.iid),
        pr_title: Some(payload.object_attributes.title),
    })
}
