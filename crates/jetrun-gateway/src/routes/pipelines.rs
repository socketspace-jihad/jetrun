use axum::{
    extract::{Path, State},
    routing::{get, post},
    Json, Router,
};
use serde_json::{json, Value};
use uuid::Uuid;

use crate::state::AppState;
use jetrun_common::models::PipelineConfig;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/", get(list_pipelines).post(create_pipeline))
        .route("/{id}", get(get_pipeline).delete(delete_pipeline))
        .route("/{id}/validate", post(validate_pipeline))
}

async fn list_pipelines(State(state): State<AppState>) -> Json<Value> {
    let pipelines: Vec<_> = state
        .inner
        .pipelines
        .iter()
        .map(|entry| serde_json::to_value(entry.value()).unwrap())
        .collect();
    Json(json!({ "pipelines": pipelines }))
}

async fn create_pipeline(
    State(state): State<AppState>,
    Json(config): Json<PipelineConfig>,
) -> Json<Value> {
    let id = Uuid::new_v4();
    let pipeline = jetrun_common::models::Pipeline {
        id,
        project_id: Uuid::nil(),
        name: config.name.clone(),
        description: config.description.clone(),
        config_path: ".jetrun/pipeline.yml".into(),
        config,
        active: true,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
    };
    state.inner.pipelines.insert(id, pipeline.clone());
    Json(json!({ "pipeline": pipeline }))
}

async fn get_pipeline(State(state): State<AppState>, Path(id): Path<Uuid>) -> Json<Value> {
    match state.inner.pipelines.get(&id) {
        Some(pipeline) => Json(json!({ "pipeline": pipeline.value() })),
        None => Json(json!({ "error": "Pipeline not found" })),
    }
}

async fn delete_pipeline(State(state): State<AppState>, Path(id): Path<Uuid>) -> Json<Value> {
    match state.inner.pipelines.remove(&id) {
        Some(_) => Json(json!({ "deleted": true })),
        None => Json(json!({ "error": "Pipeline not found" })),
    }
}

async fn validate_pipeline(Json(yaml_str): Json<String>) -> Json<Value> {
    match serde_yaml::from_str::<PipelineConfig>(&yaml_str) {
        Ok(config) => Json(json!({ "valid": true, "config": config })),
        Err(e) => {
            let msg = e.to_string();
            Json(json!({ "valid": false, "error": msg }))
        }
    }
}
