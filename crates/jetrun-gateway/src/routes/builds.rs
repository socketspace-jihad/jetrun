use axum::{
    extract::{Path, State},
    routing::get,
    Json, Router,
};
use serde_json::{json, Value};
use uuid::Uuid;

use crate::state::AppState;
use jetrun_common::models::{Build, BuildStatus, BuildTrigger};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/", get(list_builds))
        .route("/{id}", get(get_build))
        .route("/{id}/cancel", axum::routing::post(cancel_build))
        .route("/{id}/retry", axum::routing::post(retry_build))
        .route("/{id}/logs", get(get_build_logs))
}

async fn list_builds(State(state): State<AppState>) -> Json<Value> {
    let builds: Vec<_> = state
        .inner
        .builds
        .iter()
        .map(|entry| serde_json::to_value(entry.value()).unwrap())
        .collect();
    Json(json!({ "builds": builds }))
}

async fn get_build(State(state): State<AppState>, Path(id): Path<Uuid>) -> Json<Value> {
    match state.inner.builds.get(&id) {
        Some(build) => Json(json!({ "build": build.value() })),
        None => Json(json!({ "error": "Build not found" })),
    }
}

async fn cancel_build(State(state): State<AppState>, Path(id): Path<Uuid>) -> Json<Value> {
    match state.inner.builds.get_mut(&id) {
        Some(mut build) => {
            build.status = BuildStatus::Cancelled;
            build.finished_at = Some(chrono::Utc::now());
            Json(json!({ "cancelled": true }))
        }
        None => Json(json!({ "error": "Build not found" })),
    }
}

async fn retry_build(State(state): State<AppState>, Path(id): Path<Uuid>) -> Json<Value> {
    let original = match state.inner.builds.get(&id) {
        Some(build) => build.value().clone(),
        None => return Json(json!({ "error": "Build not found" })),
    };

    let new_build = Build {
        id: Uuid::new_v4(),
        pipeline_id: original.pipeline_id,
        number: original.number + 1,
        status: BuildStatus::Queued,
        trigger: BuildTrigger::Manual,
        commit_sha: original.commit_sha,
        branch: original.branch,
        matrix_values: original.matrix_values,
        stages: vec![],
        started_at: None,
        finished_at: None,
        created_at: chrono::Utc::now(),
    };

    let new_id = new_build.id;
    state.inner.builds.insert(new_id, new_build);
    Json(json!({ "build_id": new_id, "retried": true }))
}

async fn get_build_logs(Path(id): Path<Uuid>) -> Json<Value> {
    // Stub: will return paginated logs from storage
    Json(json!({ "build_id": id, "logs": [] }))
}
