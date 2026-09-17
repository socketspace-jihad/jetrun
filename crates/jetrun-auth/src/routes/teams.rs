use axum::{
    extract::{Path, State},
    routing::{delete, get},
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

use crate::state::AppState;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/orgs/{org_id}/teams", get(list_teams).post(create_team))
        .route(
            "/orgs/{org_id}/teams/{team_id}",
            get(get_team).delete(delete_team),
        )
        .route(
            "/orgs/{org_id}/teams/{team_id}/members",
            get(list_team_members).post(add_team_member),
        )
        .route(
            "/orgs/{org_id}/teams/{team_id}/members/{user_id}",
            delete(remove_team_member),
        )
}

async fn list_teams(
    State(_state): State<AppState>,
    Path(_org_id): Path<Uuid>,
) -> Json<Value> {
    // TODO: Implement once TeamRepo trait is added to jetrun-store
    Json(json!({ "teams": [], "note": "Team store not yet implemented" }))
}

#[derive(Debug, Deserialize)]
struct CreateTeamRequest {
    #[allow(dead_code)]
    name: String,
    #[allow(dead_code)]
    slug: String,
    #[allow(dead_code)]
    description: Option<String>,
}

async fn create_team(
    State(_state): State<AppState>,
    Path(_org_id): Path<Uuid>,
    Json(_req): Json<CreateTeamRequest>,
) -> Json<Value> {
    // TODO: Implement once TeamRepo trait is added to jetrun-store
    Json(json!({ "error": "Team store not yet implemented" }))
}

async fn get_team(
    State(_state): State<AppState>,
    Path((_org_id, _team_id)): Path<(Uuid, Uuid)>,
) -> Json<Value> {
    // TODO: Implement once TeamRepo trait is added to jetrun-store
    Json(json!({ "error": "Team store not yet implemented" }))
}

async fn delete_team(
    State(_state): State<AppState>,
    Path((_org_id, _team_id)): Path<(Uuid, Uuid)>,
) -> Json<Value> {
    // TODO: Implement once TeamRepo trait is added to jetrun-store
    Json(json!({ "error": "Team store not yet implemented" }))
}

async fn list_team_members(
    State(_state): State<AppState>,
    Path((_org_id, _team_id)): Path<(Uuid, Uuid)>,
) -> Json<Value> {
    // TODO: Implement once TeamRepo trait is added to jetrun-store
    Json(json!({ "members": [], "note": "Team store not yet implemented" }))
}

#[derive(Debug, Deserialize)]
struct AddMemberRequest {
    #[allow(dead_code)]
    user_id: Uuid,
}

async fn add_team_member(
    State(_state): State<AppState>,
    Path((_org_id, _team_id)): Path<(Uuid, Uuid)>,
    Json(_req): Json<AddMemberRequest>,
) -> Json<Value> {
    // TODO: Implement once TeamRepo trait is added to jetrun-store
    Json(json!({ "error": "Team store not yet implemented" }))
}

async fn remove_team_member(
    State(_state): State<AppState>,
    Path((_org_id, _team_id, _user_id)): Path<(Uuid, Uuid, Uuid)>,
) -> Json<Value> {
    // TODO: Implement once TeamRepo trait is added to jetrun-store
    Json(json!({ "error": "Team store not yet implemented" }))
}
