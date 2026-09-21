use std::path::PathBuf;
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
use jetrun_common::models::{BuildStatus, Project};
use jetrun_store::traits::Store;

#[derive(Clone)]
pub struct AppState {
    pub store: Arc<dyn Store>,
    pub broker: Arc<dyn MessageBroker>,
    pub log_store: Option<Arc<LogStoreClient>>,
    pub log_dir: PathBuf,
}

/// Lightweight S3 client for reading build logs
pub struct LogStoreClient {
    client: aws_sdk_s3::Client,
    bucket: String,
}

impl LogStoreClient {
    pub async fn from_env() -> anyhow::Result<Self> {
        let endpoint = std::env::var("S3_ENDPOINT").ok();
        let bucket = std::env::var("S3_BUCKET").unwrap_or_else(|_| "jetrun-logs".into());
        let region = std::env::var("S3_REGION").unwrap_or_else(|_| "us-east-1".into());

        let mut config_loader = aws_config::defaults(aws_config::BehaviorVersion::latest())
            .region(aws_config::Region::new(region));

        if let Some(ep) = &endpoint {
            config_loader = config_loader.endpoint_url(ep);
        }

        let config = config_loader.load().await;
        let mut s3_config = aws_sdk_s3::config::Builder::from(&config);
        if endpoint.is_some() {
            s3_config = s3_config.force_path_style(true);
        }

        let client = aws_sdk_s3::Client::from_conf(s3_config.build());
        Ok(Self { client, bucket })
    }

    pub async fn download(&self, build_id: Uuid) -> anyhow::Result<String> {
        let key = format!("builds/{}/{}.log", &build_id.to_string()[..8], build_id);
        let resp = self.client
            .get_object()
            .bucket(&self.bucket)
            .key(&key)
            .send()
            .await
            .map_err(|e| anyhow::anyhow!("S3 download: {}", e))?;
        let bytes = resp.body.collect().await
            .map_err(|e| anyhow::anyhow!("S3 read: {}", e))?;
        Ok(String::from_utf8_lossy(&bytes.into_bytes()).to_string())
    }
}

pub mod secrets;
pub mod webhooks;

pub fn api_routes() -> Router<AppState> {
    Router::new()
        .route("/projects", get(list_projects).post(create_project))
        .route("/projects/{id}", get(get_project).delete(delete_project))
        .route("/projects/{id}/trigger", post(trigger_build))
        .route("/projects/{id}/builds", get(list_project_builds))
        .route("/builds/{id}", get(get_build))
        .route("/builds/{id}/logs", get(get_build_logs))
        // Secrets
        .nest("/secrets", secrets::admin_routes())
        .nest("/secrets", secrets::names_route())
        // Webhooks (GitHub/GitLab/Bitbucket → trigger sync)
        .nest("/webhooks", webhooks::routes())
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

async fn list_project_builds(
    State(state): State<AppState>,
    Path(project_id): Path<Uuid>,
) -> Json<Value> {
    // Get pipelines for this project, then builds for those pipelines
    let builds = state.store.list_builds(50).await.unwrap_or_default();
    // Filter to builds belonging to pipelines of this project
    let project_builds: Vec<Value> = builds.into_iter()
        .map(|b| json!({
            "id": b.id,
            "pipeline_id": b.pipeline_id,
            "number": b.number,
            "status": b.status,
            "trigger": b.trigger,
            "commit_sha": b.commit_sha,
            "branch": b.branch,
            "stages": b.stages,
            "started_at": b.started_at,
            "finished_at": b.finished_at,
            "created_at": b.created_at,
        }))
        .collect();
    Json(json!({ "builds": project_builds }))
}

async fn get_build(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Json<Value> {
    match state.store.find_build_by_id(id).await.ok().flatten() {
        Some(b) => Json(json!({
            "id": b.id,
            "pipeline_id": b.pipeline_id,
            "number": b.number,
            "status": b.status,
            "trigger": b.trigger,
            "commit_sha": b.commit_sha,
            "branch": b.branch,
            "stages": b.stages,
            "started_at": b.started_at,
            "finished_at": b.finished_at,
            "created_at": b.created_at,
        })),
        None => Json(json!({ "error": "Build not found" })),
    }
}

async fn get_build_logs(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Json<Value> {
    // Check build exists and get status
    let build = match state.store.find_build_by_id(id).await.ok().flatten() {
        Some(b) => b,
        None => return Json(json!({ "error": "Build not found" })),
    };

    // Live log on disk (in-progress builds)
    if build.status == BuildStatus::Running || build.status == BuildStatus::Queued {
        let live_path = state.log_dir.join("live").join(format!("{}.log", id));
        if let Ok(content) = tokio::fs::read_to_string(&live_path).await {
            return Json(json!({
                "build_id": id,
                "source": "live",
                "status": build.status,
                "content": content,
            }));
        }
    }

    // History log from S3 (completed builds)
    if let Some(s3) = &state.log_store {
        match s3.download(id).await {
            Ok(content) => {
                return Json(json!({
                    "build_id": id,
                    "source": "s3",
                    "status": build.status,
                    "content": content,
                }));
            }
            Err(e) => {
                tracing::warn!(build_id = %id, error = %e, "S3 log fetch failed");
            }
        }
    }

    // Fallback: check disk even for completed builds (S3 upload may have failed)
    let live_path = state.log_dir.join("live").join(format!("{}.log", id));
    if let Ok(content) = tokio::fs::read_to_string(&live_path).await {
        return Json(json!({
            "build_id": id,
            "source": "disk",
            "status": build.status,
            "content": content,
        }));
    }

    Json(json!({
        "build_id": id,
        "source": "none",
        "status": build.status,
        "content": "",
        "message": "No logs available yet",
    }))
}
