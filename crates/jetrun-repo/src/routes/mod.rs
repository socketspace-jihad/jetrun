use std::sync::Arc;

use axum::{
    extract::{Path, State},
    routing::{get, post},
    Json, Router,
};
use chrono::Utc;
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

use jetrun_broker::traits::MessageBroker;
use jetrun_broker::types::RepoJob;
use jetrun_common::models::Project;
use jetrun_store::traits::Store;

#[derive(Clone)]
pub struct AppState {
    pub store: Arc<dyn Store>,
    pub broker: Arc<dyn MessageBroker>,
}

pub mod secrets;

pub fn api_routes() -> Router<AppState> {
    Router::new()
        .route("/projects", get(list_projects).post(create_project))
        .route("/projects/{id}", get(get_project).delete(delete_project))
        .route("/projects/{id}/trigger", post(trigger_build))
        // Secrets — admin management
        .nest("/secrets", secrets::admin_routes())
        // Secrets — developer dropdown (names only)
        .nest("/secrets", secrets::names_route())
}

#[derive(Debug, Deserialize)]
struct CreateProjectRequest {
    name: String,
    repo_url: String,
    #[serde(default = "default_branch")]
    branch: String,
    #[serde(default = "default_config_path")]
    config_path: String,
    org_id: Option<Uuid>,
    credential_id: Option<Uuid>,
}

fn default_branch() -> String { "main".into() }
fn default_config_path() -> String { ".jetrun/pipeline.yaml".into() }

async fn create_project(
    State(state): State<AppState>,
    Json(req): Json<CreateProjectRequest>,
) -> Json<Value> {
    let project = Project {
        id: Uuid::new_v4(),
        org_id: req.org_id,
        name: req.name,
        slug: "".into(), // will be set below
        repo_url: req.repo_url.clone(),
        default_branch: req.branch.clone(),
        webhook_secret: None,
        config_path: req.config_path,
        credential_id: req.credential_id,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };

    let project_id = project.id;

    if let Err(e) = state.store.create_project(&project).await {
        tracing::error!(error = %e, "failed to create project");
        return Json(json!({ "error": "Failed to create project" }));
    }

    // Enqueue initial sync job
    let job = RepoJob::Sync {
        project_id,
        repo_url: req.repo_url,
        branch: req.branch,
        commit_sha: None,
        trigger: "manual".into(),
        triggered_by: None,
    };

    if let Ok(payload) = job.to_bytes() {
        if let Err(e) = state.broker.publish(job.subject(), &payload).await {
            tracing::warn!(error = %e, "failed to enqueue initial sync — controller may not be running");
        }
    }

    let webhook_url = format!(
        "https://api.jetrun.devopsinstitute.id/api/v1/webhooks/github?project_id={}",
        project_id
    );

    Json(json!({
        "id": project_id,
        "name": project.name,
        "repo_url": project.repo_url,
        "branch": project.default_branch,
        "webhook_url": webhook_url,
        "created": true,
    }))
}

async fn list_projects(State(state): State<AppState>) -> Json<Value> {
    let projects = state.store.list_all_projects().await.unwrap_or_default();
    let list: Vec<Value> = projects.into_iter().map(|p| json!({
        "id": p.id, "name": p.name, "repo_url": p.repo_url,
        "branch": p.default_branch, "config_path": p.config_path,
        "org_id": p.org_id, "created_at": p.created_at,
    })).collect();
    Json(json!({ "projects": list }))
}

async fn get_project(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Json<Value> {
    match state.store.find_project_by_id(id).await.ok().flatten() {
        Some(p) => {
            let webhook_url = format!(
                "https://api.jetrun.devopsinstitute.id/api/v1/webhooks/github?project_id={}",
                id
            );
            Json(json!({
                "id": p.id, "name": p.name, "repo_url": p.repo_url,
                "branch": p.default_branch, "config_path": p.config_path,
                "webhook_url": webhook_url, "created_at": p.created_at,
            }))
        }
        None => Json(json!({ "error": "Project not found" })),
    }
}

async fn delete_project(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Json<Value> {
    // Enqueue delete job to clean up cached repo
    let job = RepoJob::Delete { project_id: id };
    if let Ok(payload) = job.to_bytes() {
        let _ = state.broker.publish(job.subject(), &payload).await;
    }

    match state.store.delete_project(id).await {
        Ok(true) => Json(json!({ "deleted": true })),
        _ => Json(json!({ "error": "Project not found" })),
    }
}

async fn trigger_build(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Json<Value> {
    let _project = match state.store.find_project_by_id(id).await.ok().flatten() {
        Some(p) => p,
        None => return Json(json!({ "error": "Project not found" })),
    };

    let job = RepoJob::Sync {
        project_id: id,
        repo_url: "".into(), // controller will look up from DB
        branch: "main".into(),
        commit_sha: None,
        trigger: "manual".into(),
        triggered_by: None,
    };

    match job.to_bytes() {
        Ok(payload) => match state.broker.publish(job.subject(), &payload).await {
            Ok(()) => Json(json!({ "status": "queued", "message": "Build triggered" })),
            Err(e) => Json(json!({ "error": format!("Failed to queue: {}", e) })),
        },
        Err(e) => Json(json!({ "error": format!("Serialization failed: {}", e) })),
    }
}
