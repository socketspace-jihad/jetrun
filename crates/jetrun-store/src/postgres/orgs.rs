use async_trait::async_trait;
use uuid::Uuid;

use jetrun_common::models::{AuthProvider, Organization, OrgMember, Role, User};

use super::PgStore;
use crate::error::StoreError;
use crate::traits::OrgRepo;

#[async_trait]
impl OrgRepo for PgStore {
    async fn create_org(&self, org: &Organization) -> Result<(), StoreError> {
        sqlx::query(
            "INSERT INTO organizations (id, name, slug, owner_id, created_at, updated_at) VALUES ($1, $2, $3, $4, $5, $6)"
        )
        .bind(org.id).bind(&org.name).bind(&org.slug).bind(org.owner_id).bind(org.created_at).bind(org.updated_at)
        .execute(&self.pool).await?;
        Ok(())
    }

    async fn find_org_by_id(&self, id: Uuid) -> Result<Option<Organization>, StoreError> {
        let row = sqlx::query_as::<_, OrgRow>(
            "SELECT id, name, slug, owner_id, created_at, updated_at FROM organizations WHERE id = $1"
        ).bind(id).fetch_optional(&self.pool).await?;
        Ok(row.map(|r| r.into()))
    }

    async fn update_org(&self, org: &Organization) -> Result<(), StoreError> {
        sqlx::query("UPDATE organizations SET name = $2, slug = $3, updated_at = $4 WHERE id = $1")
            .bind(org.id).bind(&org.name).bind(&org.slug).bind(org.updated_at)
            .execute(&self.pool).await?;
        Ok(())
    }

    async fn has_any_org(&self) -> Result<bool, StoreError> {
        let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM organizations")
            .fetch_one(&self.pool).await?;
        Ok(count.0 > 0)
    }

    async fn list_user_orgs(&self, user_id: Uuid) -> Result<Vec<(Organization, Role)>, StoreError> {
        let rows = sqlx::query_as::<_, OrgWithRoleRow>(
            "SELECT o.id, o.name, o.slug, o.owner_id, o.created_at, o.updated_at,
                    r.id as role_id, r.name as role_name, r.display_name as role_display_name, r.description as role_description, r.is_builtin, r.org_id as role_org_id, r.created_at as role_created_at, r.updated_at as role_updated_at
             FROM org_members m
             JOIN organizations o ON o.id = m.org_id
             JOIN roles r ON r.id = m.role_id
             WHERE m.user_id = $1"
        ).bind(user_id).fetch_all(&self.pool).await?;

        Ok(rows.into_iter().map(|r| (r.to_org(), r.to_role())).collect())
    }

    async fn add_member(&self, member: &OrgMember) -> Result<(), StoreError> {
        sqlx::query(
            "INSERT INTO org_members (id, org_id, user_id, role_id, joined_at) VALUES ($1, $2, $3, $4, $5) ON CONFLICT (org_id, user_id) DO NOTHING"
        ).bind(member.id).bind(member.org_id).bind(member.user_id).bind(member.role_id).bind(member.joined_at)
        .execute(&self.pool).await?;
        Ok(())
    }

    async fn remove_member(&self, org_id: Uuid, user_id: Uuid) -> Result<bool, StoreError> {
        let result = sqlx::query("DELETE FROM org_members WHERE org_id = $1 AND user_id = $2")
            .bind(org_id).bind(user_id).execute(&self.pool).await?;
        Ok(result.rows_affected() > 0)
    }

    async fn list_members(&self, org_id: Uuid) -> Result<Vec<(User, Role, OrgMember)>, StoreError> {
        let rows = sqlx::query_as::<_, MemberRow>(
            "SELECT u.id, u.email, u.username, u.display_name, u.avatar_url, u.password_hash, u.auth_provider, u.provider_id, u.is_active, u.email_verified, u.last_login_at, u.created_at, u.updated_at,
                    r.id as role_id, r.name as role_name, r.display_name as role_display_name, r.description as role_description, r.is_builtin, r.org_id as role_org_id, r.created_at as role_created_at, r.updated_at as role_updated_at,
                    m.id as member_id, m.org_id, m.joined_at
             FROM org_members m
             JOIN users u ON u.id = m.user_id
             JOIN roles r ON r.id = m.role_id
             WHERE m.org_id = $1"
        ).bind(org_id).fetch_all(&self.pool).await?;

        Ok(rows.into_iter().map(|r| r.into_tuple()).collect())
    }

    async fn is_member(&self, user_id: Uuid, org_id: Uuid) -> Result<bool, StoreError> {
        let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM org_members WHERE user_id = $1 AND org_id = $2")
            .bind(user_id).bind(org_id).fetch_one(&self.pool).await?;
        Ok(count.0 > 0)
    }

    async fn get_user_role_in_org(&self, user_id: Uuid, org_id: Uuid) -> Result<Option<Role>, StoreError> {
        let row = sqlx::query_as::<_, RoleRow>(
            "SELECT r.id, r.name, r.display_name, r.description, r.is_builtin, r.org_id, r.created_at, r.updated_at
             FROM org_members m JOIN roles r ON r.id = m.role_id
             WHERE m.user_id = $1 AND m.org_id = $2"
        ).bind(user_id).bind(org_id).fetch_optional(&self.pool).await?;
        Ok(row.map(|r| r.into()))
    }
}

// ── Row types ──

#[derive(sqlx::FromRow)]
struct OrgRow {
    id: Uuid, name: String, slug: String, owner_id: Uuid,
    created_at: chrono::DateTime<chrono::Utc>, updated_at: chrono::DateTime<chrono::Utc>,
}

impl From<OrgRow> for Organization {
    fn from(r: OrgRow) -> Self {
        Organization { id: r.id, name: r.name, slug: r.slug, owner_id: r.owner_id, created_at: r.created_at, updated_at: r.updated_at }
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
struct OrgWithRoleRow {
    id: Uuid, name: String, slug: String, owner_id: Uuid,
    created_at: chrono::DateTime<chrono::Utc>, updated_at: chrono::DateTime<chrono::Utc>,
    role_id: Uuid, role_name: String, role_display_name: String, role_description: Option<String>,
    is_builtin: bool, role_org_id: Option<Uuid>,
    role_created_at: chrono::DateTime<chrono::Utc>, role_updated_at: chrono::DateTime<chrono::Utc>,
}

impl OrgWithRoleRow {
    fn to_org(&self) -> Organization {
        Organization { id: self.id, name: self.name.clone(), slug: self.slug.clone(), owner_id: self.owner_id, created_at: self.created_at, updated_at: self.updated_at }
    }
    fn to_role(&self) -> Role {
        Role { id: self.role_id, name: self.role_name.clone(), display_name: self.role_display_name.clone(), description: self.role_description.clone(), is_builtin: self.is_builtin, org_id: self.role_org_id, created_at: self.role_created_at, updated_at: self.role_updated_at }
    }
}

#[derive(sqlx::FromRow)]
struct MemberRow {
    // user fields
    id: Uuid, email: String, username: String, display_name: Option<String>, avatar_url: Option<String>,
    password_hash: Option<String>, auth_provider: String, provider_id: Option<String>,
    is_active: bool, email_verified: bool, last_login_at: Option<chrono::DateTime<chrono::Utc>>,
    created_at: chrono::DateTime<chrono::Utc>, updated_at: chrono::DateTime<chrono::Utc>,
    // role fields
    role_id: Uuid, role_name: String, role_display_name: String, role_description: Option<String>,
    is_builtin: bool, role_org_id: Option<Uuid>,
    role_created_at: chrono::DateTime<chrono::Utc>, role_updated_at: chrono::DateTime<chrono::Utc>,
    // member fields
    member_id: Uuid, org_id: Uuid, joined_at: chrono::DateTime<chrono::Utc>,
}

impl MemberRow {
    fn into_tuple(self) -> (User, Role, OrgMember) {
        let user = User {
            id: self.id, email: self.email, username: self.username,
            display_name: self.display_name, avatar_url: self.avatar_url,
            password_hash: self.password_hash,
            auth_provider: match self.auth_provider.as_str() {
                "google" => AuthProvider::Google, "github" => AuthProvider::Github,
                "gitlab" => AuthProvider::Gitlab, "bitbucket" => AuthProvider::Bitbucket,
                "apple" => AuthProvider::Apple, "saml" => AuthProvider::Saml,
                _ => AuthProvider::Local,
            },
            provider_id: self.provider_id, is_active: self.is_active,
            email_verified: self.email_verified, last_login_at: self.last_login_at,
            created_at: self.created_at, updated_at: self.updated_at,
        };
        let role = Role {
            id: self.role_id, name: self.role_name, display_name: self.role_display_name,
            description: self.role_description, is_builtin: self.is_builtin,
            org_id: self.role_org_id, created_at: self.role_created_at, updated_at: self.role_updated_at,
        };
        let member = OrgMember { id: self.member_id, org_id: self.org_id, user_id: self.id, role_id: self.role_id, joined_at: self.joined_at };
        (user, role, member)
    }
}
