use async_trait::async_trait;
use uuid::Uuid;

use jetrun_common::models::{AuthProvider, Role, Team, TeamMember, User};

use super::PgStore;
use crate::error::StoreError;
use crate::traits::GroupRepo;

#[async_trait]
impl GroupRepo for PgStore {
    async fn create_group(&self, g: &Team) -> Result<(), StoreError> {
        sqlx::query(
            "INSERT INTO teams (id, org_id, name, slug, description, created_at, updated_at) VALUES ($1,$2,$3,$4,$5,$6,$7)"
        ).bind(g.id).bind(g.org_id).bind(&g.name).bind(&g.slug).bind(&g.description)
        .bind(g.created_at).bind(g.updated_at)
        .execute(&self.pool).await?;
        Ok(())
    }

    async fn find_group_by_id(&self, id: Uuid) -> Result<Option<Team>, StoreError> {
        let row = sqlx::query_as::<_, GroupRow>(
            "SELECT id, org_id, name, slug, description, created_at, updated_at FROM teams WHERE id = $1"
        ).bind(id).fetch_optional(&self.pool).await?;
        Ok(row.map(|r| r.into()))
    }

    async fn list_groups(&self, org_id: Uuid) -> Result<Vec<Team>, StoreError> {
        let rows = sqlx::query_as::<_, GroupRow>(
            "SELECT id, org_id, name, slug, description, created_at, updated_at FROM teams WHERE org_id = $1 ORDER BY name"
        ).bind(org_id).fetch_all(&self.pool).await?;
        Ok(rows.into_iter().map(|r| r.into()).collect())
    }

    async fn update_group(&self, g: &Team) -> Result<(), StoreError> {
        sqlx::query("UPDATE teams SET name = $2, description = $3, updated_at = $4 WHERE id = $1")
            .bind(g.id).bind(&g.name).bind(&g.description).bind(g.updated_at)
            .execute(&self.pool).await?;
        Ok(())
    }

    async fn delete_group(&self, id: Uuid) -> Result<bool, StoreError> {
        let r = sqlx::query("DELETE FROM teams WHERE id = $1").bind(id).execute(&self.pool).await?;
        Ok(r.rows_affected() > 0)
    }

    async fn add_group_member(&self, m: &TeamMember) -> Result<(), StoreError> {
        sqlx::query(
            "INSERT INTO team_members (id, team_id, user_id, added_at) VALUES ($1,$2,$3,$4) ON CONFLICT (team_id, user_id) DO NOTHING"
        ).bind(m.id).bind(m.team_id).bind(m.user_id).bind(m.added_at)
        .execute(&self.pool).await?;
        Ok(())
    }

    async fn remove_group_member(&self, team_id: Uuid, user_id: Uuid) -> Result<bool, StoreError> {
        let r = sqlx::query("DELETE FROM team_members WHERE team_id = $1 AND user_id = $2")
            .bind(team_id).bind(user_id).execute(&self.pool).await?;
        Ok(r.rows_affected() > 0)
    }

    async fn list_group_members(&self, team_id: Uuid) -> Result<Vec<(User, TeamMember)>, StoreError> {
        let rows = sqlx::query_as::<_, MemberRow>(
            "SELECT u.id, u.email, u.username, u.display_name, u.avatar_url, u.auth_provider, u.is_active,
                    u.created_at, u.updated_at,
                    tm.id as member_id, tm.team_id, tm.user_id as tm_user_id, tm.added_at
             FROM team_members tm JOIN users u ON u.id = tm.user_id
             WHERE tm.team_id = $1 ORDER BY tm.added_at"
        ).bind(team_id).fetch_all(&self.pool).await?;
        Ok(rows.into_iter().map(|r| r.into_tuple()).collect())
    }

    async fn set_group_role(&self, team_id: Uuid, role_id: Uuid) -> Result<(), StoreError> {
        sqlx::query(
            "INSERT INTO team_roles (team_id, role_id) VALUES ($1, $2) ON CONFLICT (team_id) DO UPDATE SET role_id = $2"
        ).bind(team_id).bind(role_id).execute(&self.pool).await?;
        Ok(())
    }

    async fn get_group_role(&self, team_id: Uuid) -> Result<Option<Role>, StoreError> {
        let row = sqlx::query_as::<_, RoleRow>(
            "SELECT r.id, r.name, r.display_name, r.description, r.is_builtin, r.org_id, r.created_at, r.updated_at
             FROM team_roles tr JOIN roles r ON r.id = tr.role_id WHERE tr.team_id = $1"
        ).bind(team_id).fetch_optional(&self.pool).await?;
        Ok(row.map(|r| r.into()))
    }
}

#[derive(sqlx::FromRow)]
struct GroupRow {
    id: Uuid, org_id: Uuid, name: String, slug: String, description: Option<String>,
    created_at: chrono::DateTime<chrono::Utc>, updated_at: chrono::DateTime<chrono::Utc>,
}

impl From<GroupRow> for Team {
    fn from(r: GroupRow) -> Self {
        Team { id: r.id, org_id: r.org_id, name: r.name, slug: r.slug, description: r.description, created_at: r.created_at, updated_at: r.updated_at }
    }
}

#[derive(sqlx::FromRow)]
struct RoleRow {
    id: Uuid, name: String, display_name: String, description: Option<String>,
    is_builtin: bool, org_id: Option<Uuid>,
    created_at: chrono::DateTime<chrono::Utc>, updated_at: chrono::DateTime<chrono::Utc>,
}

impl From<RoleRow> for Role {
    fn from(r: RoleRow) -> Self {
        Role { id: r.id, name: r.name, display_name: r.display_name, description: r.description, is_builtin: r.is_builtin, org_id: r.org_id, created_at: r.created_at, updated_at: r.updated_at }
    }
}

#[derive(sqlx::FromRow)]
struct MemberRow {
    id: Uuid, email: String, username: String, display_name: Option<String>, avatar_url: Option<String>,
    auth_provider: String, is_active: bool,
    created_at: chrono::DateTime<chrono::Utc>, updated_at: chrono::DateTime<chrono::Utc>,
    member_id: Uuid, team_id: Uuid, tm_user_id: Uuid, added_at: chrono::DateTime<chrono::Utc>,
}

impl MemberRow {
    fn into_tuple(self) -> (User, TeamMember) {
        let user = User {
            id: self.id, email: self.email, username: self.username,
            display_name: self.display_name, avatar_url: self.avatar_url,
            password_hash: None,
            auth_provider: match self.auth_provider.as_str() {
                "google" => AuthProvider::Google, "github" => AuthProvider::Github,
                "gitlab" => AuthProvider::Gitlab, "bitbucket" => AuthProvider::Bitbucket,
                "apple" => AuthProvider::Apple, _ => AuthProvider::Local,
            },
            provider_id: None, is_active: self.is_active, email_verified: true,
            last_login_at: None, created_at: self.created_at, updated_at: self.updated_at,
        };
        let member = TeamMember { id: self.member_id, team_id: self.team_id, user_id: self.tm_user_id, added_at: self.added_at };
        (user, member)
    }
}
