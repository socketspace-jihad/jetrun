use std::sync::Arc;

use chrono::Utc;
use dashmap::DashMap;
use uuid::Uuid;

use jetrun_common::models::{
    ApiKey, AuthUser, Organization, OrgMember, Permission, Role, RolePermission, Team,
    TeamMember, User,
};

use crate::config::AuthServiceConfig;
use crate::services::session::SessionStore;

#[derive(Clone)]
pub struct AppState {
    pub inner: Arc<AppStateInner>,
    pub config: Arc<AuthServiceConfig>,
}

pub struct AppStateInner {
    pub users: DashMap<Uuid, User>,
    pub organizations: DashMap<Uuid, Organization>,
    pub org_members: DashMap<Uuid, OrgMember>,
    pub teams: DashMap<Uuid, Team>,
    pub team_members: DashMap<Uuid, TeamMember>,
    pub roles: DashMap<Uuid, Role>,
    pub permissions: DashMap<String, Permission>,
    pub role_permissions: DashMap<Uuid, Vec<RolePermission>>,
    pub api_keys: DashMap<Uuid, ApiKey>,
    pub session_store: SessionStore,
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
            }),
            config: Arc::new(config),
        }
    }

    // ── User lookups ──

    pub fn find_user_by_email(&self, email: &str) -> Option<User> {
        self.inner
            .users
            .iter()
            .find(|e| e.value().email == email)
            .map(|e| e.value().clone())
    }

    pub fn find_user_by_username(&self, username: &str) -> Option<User> {
        self.inner
            .users
            .iter()
            .find(|e| e.value().username == username)
            .map(|e| e.value().clone())
    }

    // ── Multi-org: list orgs a user belongs to ──

    pub fn list_user_orgs(&self, user_id: Uuid) -> Vec<(Organization, Role)> {
        self.inner
            .org_members
            .iter()
            .filter(|e| e.value().user_id == user_id)
            .filter_map(|e| {
                let member = e.value();
                let org = self.inner.organizations.get(&member.org_id)?.clone();
                let role = self.inner.roles.get(&member.role_id)?.clone();
                Some((org, role))
            })
            .collect()
    }

    // ── Multi-org: get role for a user IN A SPECIFIC org ──

    pub fn get_user_role_in_org(&self, user_id: Uuid, org_id: Uuid) -> Option<Role> {
        self.inner
            .org_members
            .iter()
            .find(|e| e.value().user_id == user_id && e.value().org_id == org_id)
            .and_then(|e| self.inner.roles.get(&e.value().role_id).map(|r| r.clone()))
    }

    /// Check if user is a platform super_admin (not scoped to any org)
    pub fn is_super_admin(&self, user_id: Uuid) -> bool {
        let user = match self.inner.users.get(&user_id) {
            Some(u) => u.clone(),
            None => return false,
        };
        // Super admin is the seeded admin user
        user.email == self.config.superadmin_email
    }

    /// Build an AuthUser for a specific org context.
    /// If org_id is None, returns super_admin context if applicable, else empty permissions.
    pub fn build_auth_user(&self, user_id: Uuid, org_id: Option<Uuid>) -> Option<AuthUser> {
        let user = self.inner.users.get(&user_id)?.clone();

        let (role_name, permissions) = if self.is_super_admin(user_id) {
            // Platform super admin gets all permissions regardless of org
            let role = self.find_builtin_role("super_admin");
            let perms = role
                .as_ref()
                .map(|r| self.get_role_permission_names(r.id))
                .unwrap_or_default();
            ("super_admin".to_string(), perms)
        } else if let Some(oid) = org_id {
            // Resolve role in the specific org
            match self.get_user_role_in_org(user_id, oid) {
                Some(role) => {
                    let perms = self.get_role_permission_names(role.id);
                    (role.name.clone(), perms)
                }
                None => return None, // User not a member of this org
            }
        } else {
            // No org context, no permissions (except super admin handled above)
            ("none".to_string(), vec![])
        };

        Some(AuthUser {
            user_id,
            email: user.email,
            username: user.username,
            org_id,
            role: role_name,
            permissions,
        })
    }

    // ── Org membership management ──

    pub fn add_org_member(&self, org_id: Uuid, user_id: Uuid, role_id: Uuid) -> OrgMember {
        // Check if already a member
        let existing = self
            .inner
            .org_members
            .iter()
            .find(|e| e.value().user_id == user_id && e.value().org_id == org_id);

        if let Some(e) = existing {
            return e.value().clone();
        }

        let member = OrgMember {
            id: Uuid::new_v4(),
            org_id,
            user_id,
            role_id,
            joined_at: Utc::now(),
        };
        self.inner.org_members.insert(member.id, member.clone());
        member
    }

    pub fn remove_org_member(&self, org_id: Uuid, user_id: Uuid) -> bool {
        let to_remove: Option<Uuid> = self
            .inner
            .org_members
            .iter()
            .find(|e| e.value().user_id == user_id && e.value().org_id == org_id)
            .map(|e| *e.key());

        if let Some(id) = to_remove {
            self.inner.org_members.remove(&id);
            true
        } else {
            false
        }
    }

    pub fn list_org_members(&self, org_id: Uuid) -> Vec<(User, Role, OrgMember)> {
        self.inner
            .org_members
            .iter()
            .filter(|e| e.value().org_id == org_id)
            .filter_map(|e| {
                let member = e.value().clone();
                let user = self.inner.users.get(&member.user_id)?.clone();
                let role = self.inner.roles.get(&member.role_id)?.clone();
                Some((user, role, member))
            })
            .collect()
    }

    pub fn is_org_member(&self, user_id: Uuid, org_id: Uuid) -> bool {
        self.inner
            .org_members
            .iter()
            .any(|e| e.value().user_id == user_id && e.value().org_id == org_id)
    }

    // ── Role helpers ──

    pub fn find_builtin_role(&self, name: &str) -> Option<Role> {
        self.inner
            .roles
            .iter()
            .find(|r| r.value().name == name && r.value().is_builtin)
            .map(|r| r.value().clone())
    }

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

    pub fn find_api_key_by_prefix(&self, prefix: &str) -> Option<ApiKey> {
        self.inner
            .api_keys
            .iter()
            .find(|e| e.value().prefix == prefix)
            .map(|e| e.value().clone())
    }
}
