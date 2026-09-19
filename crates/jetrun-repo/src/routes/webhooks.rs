use axum::{
    body::Bytes,
    extract::{Query, State},
    http::{HeaderMap, StatusCode},
    routing::post,
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

use jetrun_broker::types::RepoJob;

use super::AppState;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/github", post(handle_github))
        .route("/gitlab", post(handle_gitlab))
        .route("/bitbucket", post(handle_bitbucket))
}

#[derive(Debug, Deserialize)]
struct WebhookQuery {
    project_id: Option<Uuid>,
}

// ── GitHub ──

async fn handle_github(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<WebhookQuery>,
    body: Bytes,
) -> Result<Json<Value>, StatusCode> {
    let event_type = headers
        .get("X-GitHub-Event")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("unknown");

    // Ping — always respond
    if event_type == "ping" {
        tracing::info!("github webhook ping received");
        return Ok(Json(json!({ "status": "pong" })));
    }

    // Only handle push and pull_request
    if event_type != "push" && event_type != "pull_request" {
        return Ok(Json(json!({ "status": "ignored", "event": event_type })));
    }

    // Parse push payload to extract branch and commit
    let payload: serde_json::Value = serde_json::from_slice(&body)
        .map_err(|e| {
            tracing::error!(error = %e, "failed to parse webhook body as JSON");
            StatusCode::BAD_REQUEST
        })?;

    let git_ref = payload.get("ref").and_then(|v| v.as_str()).unwrap_or("");
    let branch = git_ref.strip_prefix("refs/heads/").unwrap_or(git_ref);
    let commit_sha = payload.get("after").and_then(|v| v.as_str()).map(String::from);
    let repo_url = payload
        .get("repository")
        .and_then(|r| r.get("clone_url"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    // Find the project
    let project_id = match q.project_id {
        Some(id) => id,
        None => {
            tracing::warn!("webhook missing project_id query parameter");
            return Ok(Json(json!({ "error": "Missing project_id query parameter" })));
        }
    };

    // Verify project exists
    let project = state.store.find_project_by_id(project_id).await.ok().flatten();
    if project.is_none() {
        tracing::warn!(project_id = %project_id, "webhook for unknown project");
        return Ok(Json(json!({ "error": "Project not found" })));
    }

    // Enqueue sync job
    let job = RepoJob::Sync {
        project_id,
        repo_url: if repo_url.is_empty() { project.unwrap().repo_url } else { repo_url },
        branch: branch.to_string(),
        commit_sha,
        trigger: event_type.to_string(),
        triggered_by: None,
    };

    if let Ok(payload) = job.to_bytes() {
        match state.broker.publish(job.subject(), &payload).await {
            Ok(()) => {
                tracing::info!(project_id = %project_id, branch = %branch, event = %event_type, "webhook → sync job enqueued");
            }
            Err(e) => {
                tracing::error!(error = %e, "failed to enqueue sync job from webhook");
                return Ok(Json(json!({ "error": "Failed to enqueue build job" })));
            }
        }
    }

    Ok(Json(json!({
        "status": "accepted",
        "project_id": project_id,
        "branch": branch,
        "event": event_type,
    })))
}

// ── GitLab ──

async fn handle_gitlab(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<WebhookQuery>,
    body: Bytes,
) -> Result<Json<Value>, StatusCode> {
    let event_type = headers
        .get("X-Gitlab-Event")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("unknown");

    if event_type != "Push Hook" && event_type != "Merge Request Hook" {
        return Ok(Json(json!({ "status": "ignored", "event": event_type })));
    }

    let payload: serde_json::Value = serde_json::from_slice(&body).map_err(|_| StatusCode::BAD_REQUEST)?;
    let git_ref = payload.get("ref").and_then(|v| v.as_str()).unwrap_or("");
    let branch = git_ref.strip_prefix("refs/heads/").unwrap_or(git_ref);
    let commit_sha = payload.get("after").and_then(|v| v.as_str()).map(String::from);
    let repo_url = payload.get("project").and_then(|p| p.get("git_http_url")).and_then(|v| v.as_str()).unwrap_or("").to_string();

    let project_id = q.project_id.ok_or(StatusCode::BAD_REQUEST)?;

    let project = state.store.find_project_by_id(project_id).await.ok().flatten();
    let job = RepoJob::Sync {
        project_id,
        repo_url: if repo_url.is_empty() { project.map(|p| p.repo_url).unwrap_or_default() } else { repo_url },
        branch: branch.to_string(),
        commit_sha,
        trigger: "push".to_string(),
        triggered_by: None,
    };

    if let Ok(payload) = job.to_bytes() {
        let _ = state.broker.publish(job.subject(), &payload).await;
    }

    Ok(Json(json!({ "status": "accepted", "project_id": project_id })))
}

// ── Bitbucket ──

async fn handle_bitbucket(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<WebhookQuery>,
    body: Bytes,
) -> Result<Json<Value>, StatusCode> {
    let event_key = headers.get("X-Event-Key").and_then(|v| v.to_str().ok()).unwrap_or("unknown");

    if event_key != "repo:push" && !event_key.starts_with("pullrequest:") {
        return Ok(Json(json!({ "status": "ignored", "event": event_key })));
    }

    let payload: serde_json::Value = serde_json::from_slice(&body).map_err(|_| StatusCode::BAD_REQUEST)?;
    let project_id = q.project_id.ok_or(StatusCode::BAD_REQUEST)?;

    let project = state.store.find_project_by_id(project_id).await.ok().flatten();
    let job = RepoJob::Sync {
        project_id,
        repo_url: project.map(|p| p.repo_url).unwrap_or_default(),
        branch: "main".to_string(),
        commit_sha: None,
        trigger: "push".to_string(),
        triggered_by: None,
    };

    if let Ok(payload) = job.to_bytes() {
        let _ = state.broker.publish(job.subject(), &payload).await;
    }

    Ok(Json(json!({ "status": "accepted", "project_id": project_id })))
}
