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

pub fn routes() -> Router<AppState> {
    Router::new().route("/", post(handle_bitbucket_webhook))
}

async fn handle_bitbucket_webhook(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Json<Value>, StatusCode> {
    let event_key = headers
        .get("X-Event-Key")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("unknown");

    let event = match event_key {
        "repo:push" => parse_push_event(&body)?,
        "pullrequest:created" | "pullrequest:updated" => parse_pull_request_event(&body)?,
        "repo:refs_changed" => parse_push_event(&body)?, // Bitbucket Server
        other => {
            tracing::debug!(event = other, "ignoring unhandled bitbucket event");
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
        "provider": "bitbucket",
        "event_type": event.event_type,
        "repo": event.repo_name,
        "branch": event.branch,
        "commit": event.commit_sha,
    })))
}

// --- Bitbucket Cloud payload types ---

#[derive(Debug, Deserialize)]
struct BitbucketPushPayload {
    push: BitbucketPushData,
    repository: BitbucketRepository,
    actor: BitbucketActor,
}

#[derive(Debug, Deserialize)]
struct BitbucketPushData {
    changes: Vec<BitbucketChange>,
}

#[derive(Debug, Deserialize)]
struct BitbucketChange {
    new: Option<BitbucketTarget>,
}

#[derive(Debug, Deserialize)]
struct BitbucketTarget {
    name: String,
    #[serde(rename = "type")]
    target_type: String, // "branch" or "tag"
    target: BitbucketCommitTarget,
}

#[derive(Debug, Deserialize)]
struct BitbucketCommitTarget {
    hash: String,
    message: Option<String>,
}

#[derive(Debug, Deserialize)]
struct BitbucketPullRequestPayload {
    pullrequest: BitbucketPullRequest,
    repository: BitbucketRepository,
    actor: BitbucketActor,
}

#[derive(Debug, Deserialize)]
struct BitbucketPullRequest {
    id: u64,
    title: String,
    source: BitbucketPREndpoint,
    destination: BitbucketPREndpoint,
}

#[derive(Debug, Deserialize)]
struct BitbucketPREndpoint {
    branch: BitbucketBranch,
    commit: BitbucketPRCommit,
}

#[derive(Debug, Deserialize)]
struct BitbucketBranch {
    name: String,
}

#[derive(Debug, Deserialize)]
struct BitbucketPRCommit {
    hash: String,
}

#[derive(Debug, Deserialize)]
struct BitbucketRepository {
    full_name: String,
    links: BitbucketLinks,
}

#[derive(Debug, Deserialize)]
struct BitbucketLinks {
    html: BitbucketLink,
}

#[derive(Debug, Deserialize)]
struct BitbucketLink {
    href: String,
}

#[derive(Debug, Deserialize)]
struct BitbucketActor {
    display_name: String,
}

// --- Parsers ---

fn parse_push_event(body: &[u8]) -> Result<WebhookEvent, StatusCode> {
    let payload: BitbucketPushPayload = serde_json::from_slice(body).map_err(|e| {
        tracing::error!(error = %e, "failed to parse bitbucket push payload");
        StatusCode::BAD_REQUEST
    })?;

    let change = payload
        .push
        .changes
        .into_iter()
        .find_map(|c| c.new)
        .ok_or_else(|| {
            tracing::warn!("bitbucket push event has no new changes");
            StatusCode::BAD_REQUEST
        })?;

    let event_type = if change.target_type == "tag" {
        WebhookEventType::Tag
    } else {
        WebhookEventType::Push
    };

    // Bitbucket Cloud doesn't provide a clone URL in push payloads directly,
    // construct it from the full_name
    let repo_url = format!(
        "https://bitbucket.org/{}.git",
        payload.repository.full_name
    );

    Ok(WebhookEvent {
        provider: WebhookProvider::Bitbucket,
        event_type,
        repo_url,
        repo_name: payload.repository.full_name,
        branch: change.name,
        commit_sha: change.target.hash,
        commit_message: change.target.message,
        author: Some(payload.actor.display_name),
        source_branch: None,
        target_branch: None,
        pr_number: None,
        pr_title: None,
    })
}

fn parse_pull_request_event(body: &[u8]) -> Result<WebhookEvent, StatusCode> {
    let payload: BitbucketPullRequestPayload = serde_json::from_slice(body).map_err(|e| {
        tracing::error!(error = %e, "failed to parse bitbucket pull request payload");
        StatusCode::BAD_REQUEST
    })?;

    let repo_url = format!(
        "https://bitbucket.org/{}.git",
        payload.repository.full_name
    );

    Ok(WebhookEvent {
        provider: WebhookProvider::Bitbucket,
        event_type: WebhookEventType::PullRequest,
        repo_url,
        repo_name: payload.repository.full_name,
        branch: payload.pullrequest.source.branch.name.clone(),
        commit_sha: payload.pullrequest.source.commit.hash,
        commit_message: None,
        author: Some(payload.actor.display_name),
        source_branch: Some(payload.pullrequest.source.branch.name),
        target_branch: Some(payload.pullrequest.destination.branch.name),
        pr_number: Some(payload.pullrequest.id),
        pr_title: Some(payload.pullrequest.title),
    })
}
