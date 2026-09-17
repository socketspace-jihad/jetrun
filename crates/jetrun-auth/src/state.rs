use std::sync::Arc;

use uuid::Uuid;

use jetrun_common::models::{AuthUser, Organization, OrgMember, Role, User};
use jetrun_store::traits::Store;

use crate::config::AuthServiceConfig;

#[derive(Clone)]
pub struct AppState {
    pub store: Arc<dyn Store>,
    pub config: Arc<AuthServiceConfig>,
}

impl AppState {
    pub fn new(store: Arc<dyn Store>, config: AuthServiceConfig) -> Self {
        Self {
            store,
            config: Arc::new(config),
        }
    }

    // ── User lookups (delegate to store) ──

    pub async fn find_user_by_email(&self, email: &str) -> Option<User> {
        self.store.find_user_by_email(email).await.ok().flatten()
    }

    pub async fn find_user_by_username(&self, username: &str) -> Option<User> {
        self.store.find_user_by_username(username).await.ok().flatten()
    }

    // ── Multi-org ──

    pub async fn list_user_orgs(&self, user_id: Uuid) -> Vec<(Organization, Role)> {
        self.store.list_user_orgs(user_id).await.unwrap_or_default()
    }

    pub async fn get_user_role_in_org(&self, user_id: Uuid, org_id: Uuid) -> Option<Role> {
        self.store.get_user_role_in_org(user_id, org_id).await.ok().flatten()
    }

    pub async fn is_super_admin(&self, user_id: Uuid) -> bool {
        let user = match self.store.find_user_by_id(user_id).await.ok().flatten() {
            Some(u) => u,
            None => return false,
        };
        user.email == self.config.superadmin_email
    }

    pub async fn build_auth_user(&self, user_id: Uuid, org_id: Option<Uuid>) -> Option<AuthUser> {
        let user = self.store.find_user_by_id(user_id).await.ok()??;

        let (role_name, permissions) = if self.is_super_admin(user_id).await {
            let role = self.find_builtin_role("super_admin").await;
            let perms = match role.as_ref() {
                Some(r) => self.get_role_permission_names(r.id).await,
                None => vec![],
            };
            ("super_admin".to_string(), perms)
        } else if let Some(oid) = org_id {
            match self.get_user_role_in_org(user_id, oid).await {
                Some(role) => {
                    let perms = self.get_role_permission_names(role.id).await;
                    (role.name.clone(), perms)
                }
                None => return None,
            }
        } else {
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

    // ── Membership ──

    pub async fn add_org_member(&self, org_id: Uuid, user_id: Uuid, role_id: Uuid) -> OrgMember {
        let member = OrgMember {
            id: Uuid::new_v4(),
            org_id,
            user_id,
            role_id,
            joined_at: chrono::Utc::now(),
        };
        let _ = self.store.add_member(&member).await;
        member
    }

    pub async fn remove_org_member(&self, org_id: Uuid, user_id: Uuid) -> bool {
        self.store.remove_member(org_id, user_id).await.unwrap_or(false)
    }

    pub async fn list_org_members(&self, org_id: Uuid) -> Vec<(User, Role, OrgMember)> {
        self.store.list_members(org_id).await.unwrap_or_default()
    }

    pub async fn is_org_member(&self, user_id: Uuid, org_id: Uuid) -> bool {
        self.store.is_member(user_id, org_id).await.unwrap_or(false)
    }

    // ── Role helpers ──

    pub async fn find_builtin_role(&self, name: &str) -> Option<Role> {
        self.store.find_builtin_role(name).await.ok().flatten()
    }

    pub async fn get_role_permission_names(&self, role_id: Uuid) -> Vec<String> {
        self.store.get_role_permission_names(role_id).await.unwrap_or_default()
    }

    // ── API Key ──

    pub async fn find_api_key_by_prefix(&self, prefix: &str) -> Option<jetrun_common::models::ApiKey> {
        self.store.find_by_prefix(prefix).await.ok().flatten()
    }
}
