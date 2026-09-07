use axum::{
    extract::{Path, State},
    routing::{delete, get, post},
    Json, Router,
};
use chrono::Utc;
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

use jetrun_common::models::{Team, TeamMember};

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
    State(state): State<AppState>,
    Path(org_id): Path<Uuid>,
) -> Json<Value> {
    let teams: Vec<Value> = state
        .inner
        .teams
        .iter()
        .filter(|e| e.value().org_id == org_id)
        .map(|e| {
            let t = e.value();
            json!({
                "id": t.id,
                "name": t.name,
                "slug": t.slug,
                "description": t.description,
                "created_at": t.created_at,
            })
        })
        .collect();

    Json(json!({ "teams": teams }))
}

#[derive(Debug, Deserialize)]
struct CreateTeamRequest {
    name: String,
    slug: String,
    description: Option<String>,
}

async fn create_team(
    State(state): State<AppState>,
    Path(org_id): Path<Uuid>,
    Json(req): Json<CreateTeamRequest>,
) -> Json<Value> {
    let team = Team {
        id: Uuid::new_v4(),
        org_id,
        name: req.name,
        slug: req.slug,
        description: req.description,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };

    let id = team.id;
    state.inner.teams.insert(id, team);
    Json(json!({ "id": id, "created": true }))
}

async fn get_team(
    State(state): State<AppState>,
    Path((_org_id, team_id)): Path<(Uuid, Uuid)>,
) -> Json<Value> {
    match state.inner.teams.get(&team_id) {
        Some(t) => Json(json!({
            "id": t.id,
            "name": t.name,
            "slug": t.slug,
            "description": t.description,
            "created_at": t.created_at,
        })),
        None => Json(json!({ "error": "Team not found" })),
    }
}

async fn delete_team(
    State(state): State<AppState>,
    Path((_org_id, team_id)): Path<(Uuid, Uuid)>,
) -> Json<Value> {
    match state.inner.teams.remove(&team_id) {
        Some(_) => {
            // Remove all team members
            let to_remove: Vec<Uuid> = state
                .inner
                .team_members
                .iter()
                .filter(|e| e.value().team_id == team_id)
                .map(|e| *e.key())
                .collect();
            for id in to_remove {
                state.inner.team_members.remove(&id);
            }
            Json(json!({ "deleted": true }))
        }
        None => Json(json!({ "error": "Team not found" })),
    }
}

async fn list_team_members(
    State(state): State<AppState>,
    Path((_org_id, team_id)): Path<(Uuid, Uuid)>,
) -> Json<Value> {
    let members: Vec<Value> = state
        .inner
        .team_members
        .iter()
        .filter(|e| e.value().team_id == team_id)
        .filter_map(|e| {
            let m = e.value();
            let user = state.inner.users.get(&m.user_id)?;
            Some(json!({
                "id": m.id,
                "user_id": user.id,
                "email": user.email,
                "username": user.username,
                "added_at": m.added_at,
            }))
        })
        .collect();

    Json(json!({ "members": members }))
}

#[derive(Debug, Deserialize)]
struct AddMemberRequest {
    user_id: Uuid,
}

async fn add_team_member(
    State(state): State<AppState>,
    Path((_org_id, team_id)): Path<(Uuid, Uuid)>,
    Json(req): Json<AddMemberRequest>,
) -> Json<Value> {
    let member = TeamMember {
        id: Uuid::new_v4(),
        team_id,
        user_id: req.user_id,
        added_at: Utc::now(),
    };

    let id = member.id;
    state.inner.team_members.insert(id, member);
    Json(json!({ "id": id, "added": true }))
}

async fn remove_team_member(
    State(state): State<AppState>,
    Path((_org_id, team_id, user_id)): Path<(Uuid, Uuid, Uuid)>,
) -> Json<Value> {
    let to_remove: Option<Uuid> = state
        .inner
        .team_members
        .iter()
        .find(|e| e.value().team_id == team_id && e.value().user_id == user_id)
        .map(|e| *e.key());

    match to_remove {
        Some(id) => {
            state.inner.team_members.remove(&id);
            Json(json!({ "removed": true }))
        }
        None => Json(json!({ "error": "Member not found" })),
    }
}
