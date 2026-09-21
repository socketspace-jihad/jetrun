use async_trait::async_trait;
use uuid::Uuid;

use jetrun_common::models::{
    ApiKey, Build, BuildStatus, Organization, OrgMember, Permission, Pipeline, Role,
    RolePermission, Session, Team, TeamMember, User,
};

use crate::error::StoreError;

// ── User ──

#[async_trait]
pub trait UserRepo: Send + Sync {
    async fn create_user(&self, user: &User) -> Result<(), StoreError>;
    async fn find_user_by_id(&self, id: Uuid) -> Result<Option<User>, StoreError>;
    async fn find_user_by_email(&self, email: &str) -> Result<Option<User>, StoreError>;
    async fn find_user_by_username(&self, username: &str) -> Result<Option<User>, StoreError>;
    async fn update_user(&self, user: &User) -> Result<(), StoreError>;
    async fn has_any_user(&self) -> Result<bool, StoreError>;
}

// ── Organization + Membership ──

#[async_trait]
pub trait OrgRepo: Send + Sync {
    async fn create_org(&self, org: &Organization) -> Result<(), StoreError>;
    async fn find_org_by_id(&self, id: Uuid) -> Result<Option<Organization>, StoreError>;
    async fn update_org(&self, org: &Organization) -> Result<(), StoreError>;
    async fn has_any_org(&self) -> Result<bool, StoreError>;
    async fn list_user_orgs(&self, user_id: Uuid) -> Result<Vec<(Organization, Role)>, StoreError>;

    async fn add_member(&self, member: &OrgMember) -> Result<(), StoreError>;
    async fn remove_member(&self, org_id: Uuid, user_id: Uuid) -> Result<bool, StoreError>;
    async fn list_members(&self, org_id: Uuid) -> Result<Vec<(User, Role, OrgMember)>, StoreError>;
    async fn is_member(&self, user_id: Uuid, org_id: Uuid) -> Result<bool, StoreError>;
    async fn get_user_role_in_org(&self, user_id: Uuid, org_id: Uuid) -> Result<Option<Role>, StoreError>;
}

// ── Role + Permission ──

#[async_trait]
pub trait RoleRepo: Send + Sync {
    async fn create_role(&self, role: &Role) -> Result<(), StoreError>;
    async fn find_role_by_id(&self, id: Uuid) -> Result<Option<Role>, StoreError>;
    async fn find_builtin_role(&self, name: &str) -> Result<Option<Role>, StoreError>;
    async fn list_roles(&self) -> Result<Vec<Role>, StoreError>;
    async fn update_role(&self, role: &Role) -> Result<(), StoreError>;
    async fn delete_role(&self, id: Uuid) -> Result<bool, StoreError>;

    async fn create_permission(&self, perm: &Permission) -> Result<(), StoreError>;
    async fn find_permission_by_name(&self, name: &str) -> Result<Option<Permission>, StoreError>;
    async fn list_permissions(&self) -> Result<Vec<Permission>, StoreError>;

    async fn set_role_permissions(&self, role_id: Uuid, perm_ids: &[Uuid]) -> Result<(), StoreError>;
    async fn get_role_permission_names(&self, role_id: Uuid) -> Result<Vec<String>, StoreError>;
}

// ── Session ──

#[async_trait]
pub trait SessionRepo: Send + Sync {
    async fn create_session(&self, session: &Session) -> Result<(), StoreError>;
    async fn find_by_refresh_hash(&self, hash: &str) -> Result<Option<Session>, StoreError>;
    async fn update_session(&self, session: &Session) -> Result<(), StoreError>;
    async fn delete_session(&self, id: Uuid) -> Result<bool, StoreError>;
    async fn list_user_sessions(&self, user_id: Uuid) -> Result<Vec<Session>, StoreError>;
    async fn delete_all_user_sessions(&self, user_id: Uuid) -> Result<u64, StoreError>;
    async fn delete_expired(&self) -> Result<u64, StoreError>;
}

// ── API Key ──

#[async_trait]
pub trait ApiKeyRepo: Send + Sync {
    async fn create_api_key(&self, key: &ApiKey) -> Result<(), StoreError>;
    async fn find_by_prefix(&self, prefix: &str) -> Result<Option<ApiKey>, StoreError>;
    async fn revoke_key(&self, id: Uuid) -> Result<bool, StoreError>;
    async fn list_user_keys(&self, user_id: Uuid) -> Result<Vec<ApiKey>, StoreError>;
}

// ── Secret ──

#[async_trait]
pub trait SecretRepo: Send + Sync {
    async fn create_secret(&self, secret: &jetrun_common::models::Secret) -> Result<(), StoreError>;
    async fn find_secret_by_id(&self, id: Uuid) -> Result<Option<jetrun_common::models::Secret>, StoreError>;
    async fn list_secrets_by_org(&self, org_id: Uuid) -> Result<Vec<jetrun_common::models::Secret>, StoreError>;
    async fn delete_secret(&self, id: Uuid) -> Result<bool, StoreError>;
    /// Returns only id, name, type — for developer dropdown (no encrypted values)
    async fn list_secret_names_by_org(&self, org_id: Uuid) -> Result<Vec<(Uuid, String, String)>, StoreError>;
}

// ── Project ──

#[async_trait]
pub trait ProjectRepo: Send + Sync {
    async fn create_project(&self, project: &jetrun_common::models::Project) -> Result<(), StoreError>;
    async fn find_project_by_id(&self, id: Uuid) -> Result<Option<jetrun_common::models::Project>, StoreError>;
    async fn list_projects_by_org(&self, org_id: Uuid) -> Result<Vec<jetrun_common::models::Project>, StoreError>;
    async fn list_all_projects(&self) -> Result<Vec<jetrun_common::models::Project>, StoreError>;
    async fn update_project(&self, project: &jetrun_common::models::Project) -> Result<(), StoreError>;
    async fn delete_project(&self, id: Uuid) -> Result<bool, StoreError>;
}

// ── Pipeline ──

#[async_trait]
pub trait PipelineRepo: Send + Sync {
    async fn create_pipeline(&self, pipeline: &Pipeline) -> Result<(), StoreError>;
    async fn find_pipeline_by_id(&self, id: Uuid) -> Result<Option<Pipeline>, StoreError>;
    async fn list_pipelines(&self) -> Result<Vec<Pipeline>, StoreError>;
    async fn delete_pipeline(&self, id: Uuid) -> Result<bool, StoreError>;
}

// ── Build ──

#[async_trait]
pub trait BuildRepo: Send + Sync {
    async fn create_build(&self, build: &Build) -> Result<(), StoreError>;
    async fn find_build_by_id(&self, id: Uuid) -> Result<Option<Build>, StoreError>;
    async fn list_builds(&self, limit: i64) -> Result<Vec<Build>, StoreError>;
    async fn update_build_status(&self, id: Uuid, status: BuildStatus, finished_at: Option<chrono::DateTime<chrono::Utc>>) -> Result<(), StoreError>;
    async fn update_build_stages(&self, id: Uuid, stages: &[jetrun_common::models::BuildStage]) -> Result<(), StoreError>;
}

// ── Group (Team) ──

#[async_trait]
pub trait GroupRepo: Send + Sync {
    async fn create_group(&self, group: &Team) -> Result<(), StoreError>;
    async fn find_group_by_id(&self, id: Uuid) -> Result<Option<Team>, StoreError>;
    async fn list_groups(&self, org_id: Uuid) -> Result<Vec<Team>, StoreError>;
    async fn update_group(&self, group: &Team) -> Result<(), StoreError>;
    async fn delete_group(&self, id: Uuid) -> Result<bool, StoreError>;

    async fn add_group_member(&self, member: &TeamMember) -> Result<(), StoreError>;
    async fn remove_group_member(&self, team_id: Uuid, user_id: Uuid) -> Result<bool, StoreError>;
    async fn list_group_members(&self, team_id: Uuid) -> Result<Vec<(User, TeamMember)>, StoreError>;

    async fn set_group_role(&self, team_id: Uuid, role_id: Uuid) -> Result<(), StoreError>;
    async fn get_group_role(&self, team_id: Uuid) -> Result<Option<Role>, StoreError>;
}

// ── Combined Store ──

pub trait Store:
    UserRepo + OrgRepo + RoleRepo + SessionRepo + ApiKeyRepo + SecretRepo + ProjectRepo + PipelineRepo + BuildRepo + GroupRepo
{
}

impl<T> Store for T where
    T: UserRepo + OrgRepo + RoleRepo + SessionRepo + ApiKeyRepo + SecretRepo + ProjectRepo + PipelineRepo + BuildRepo + GroupRepo
{
}
