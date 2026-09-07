use std::sync::Arc;

use dashmap::DashMap;
use uuid::Uuid;

use jetrun_common::models::{
    ApiKey, Organization, OrgMember, Permission, Role, RolePermission, Session, Team,
    TeamMember, User,
};

use crate::config::AuthServiceConfig;
use crate::services::rbac::RbacEngine;
use crate::services::session::SessionStore;

#[derive(Clone)]
pub struct AppState {
    pub inner: Arc<AppStateInner>,
    pub config: Arc<AuthServiceConfig>,
}

pub struct AppStateInner {
    // Core stores
    pub users: DashMap<Uuid, User>,
    pub organizations: DashMap<Uuid, Organization>,
    pub org_members: DashMap<Uuid, OrgMember>,
    pub teams: DashMap<Uuid, Team>,
    pub team_members: DashMap<Uuid, TeamMember>,
    pub roles: DashMap<Uuid, Role>,
    pub permissions: DashMap<String, Permission>, // keyed by name
    pub role_permissions: DashMap<Uuid, Vec<RolePermission>>, // role_id -> permissions
    pub api_keys: DashMap<Uuid, ApiKey>,

    // Services
    pub session_store: SessionStore,
    pub rbac: RbacEngine,
}

impl AppState {
    pub fn new(config: AuthServiceConfig) -> Self {
        Self {
            inner: Arc::new(AppStateInner {
                users: DashMap::new(),
                organizations: DashMap::new(),
                org_members: DashMap::new(),
                teams: DashMap::new(),
                team_members: DashMap::new(),
                roles: DashMap::new(),
                permissions: DashMap::new(),
                role_permissions: DashMap::new(),
                api_keys: DashMap::new(),
                session_store: SessionStore::new(),
                rbac: RbacEngine::new(),
            }),
            config: Arc::new(config),
        }
    }

    /// Find user by email
    pub fn find_user_by_email(&self, email: &str) -> Option<User> {
        self.inner
            .users
            .iter()
            .find(|entry| entry.value().email == email)
            .map(|entry| entry.value().clone())
    }

    /// Find user by username
    pub fn find_user_by_username(&self, username: &str) -> Option<User> {
        self.inner
            .users
            .iter()
            .find(|entry| entry.value().username == username)
            .map(|entry| entry.value().clone())
    }

    /// Get the role for a user in an org, or the default viewer role
    pub fn get_user_role(&self, user_id: Uuid) -> Option<Role> {
        // Find org membership
        let member = self
            .inner
            .org_members
            .iter()
            .find(|e| e.value().user_id == user_id);

        if let Some(member) = member {
            self.inner.roles.get(&member.role_id).map(|r| r.clone())
        } else {
            // Find first built-in super_admin role if user matches super admin
            self.inner
                .roles
                .iter()
                .find(|r| r.value().name == "super_admin" && r.value().is_builtin)
                .map(|r| r.value().clone())
        }
    }

    /// Get permission names for a role
    pub fn get_role_permission_names(&self, role_id: Uuid) -> Vec<String> {
        let rps = self
            .inner
            .role_permissions
            .get(&role_id)
            .map(|entry| entry.value().clone())
            .unwrap_or_default();

        rps.iter()
            .filter_map(|rp| {
                self.inner
                    .permissions
                    .iter()
                    .find(|p| p.value().id == rp.permission_id)
                    .map(|p| p.value().name.clone())
            })
            .collect()
    }

    /// Find API key by prefix
    pub fn find_api_key_by_prefix(&self, prefix: &str) -> Option<ApiKey> {
        self.inner
            .api_keys
            .iter()
            .find(|e| e.value().prefix == prefix)
            .map(|e| e.value().clone())
    }
}
