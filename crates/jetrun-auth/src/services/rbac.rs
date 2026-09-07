use dashmap::DashMap;
use uuid::Uuid;

use jetrun_common::models::{AuthUser, Permission, Role, RolePermission};

/// In-memory RBAC engine for fast permission checks.
/// Loaded from DB at startup, updated when roles/permissions change.
pub struct RbacEngine {
    roles: DashMap<Uuid, Role>,
    permissions: DashMap<Uuid, Permission>,
    role_permissions: DashMap<Uuid, Vec<String>>, // role_id -> permission names
}

impl RbacEngine {
    pub fn new() -> Self {
        Self {
            roles: DashMap::new(),
            permissions: DashMap::new(),
            role_permissions: DashMap::new(),
        }
    }

    /// Load roles into the engine
    pub fn load_roles(&self, roles: Vec<Role>) {
        for role in roles {
            self.roles.insert(role.id, role);
        }
    }

    /// Load permissions into the engine
    pub fn load_permissions(&self, permissions: Vec<Permission>) {
        for perm in permissions {
            self.permissions.insert(perm.id, perm);
        }
    }

    /// Load role-permission mappings
    pub fn load_role_permissions(&self, mappings: Vec<RolePermission>) {
        // Group by role_id
        let mut by_role: std::collections::HashMap<Uuid, Vec<String>> =
            std::collections::HashMap::new();

        for mapping in mappings {
            if let Some(perm) = self.permissions.get(&mapping.permission_id) {
                by_role
                    .entry(mapping.role_id)
                    .or_default()
                    .push(perm.name.clone());
            }
        }

        for (role_id, perms) in by_role {
            self.role_permissions.insert(role_id, perms);
        }
    }

    /// Get all permission names for a role
    pub fn get_role_permissions(&self, role_id: &Uuid) -> Vec<String> {
        self.role_permissions
            .get(role_id)
            .map(|perms| perms.clone())
            .unwrap_or_default()
    }

    /// Find a role by name (optionally scoped to an org)
    pub fn find_role_by_name(&self, name: &str, org_id: Option<Uuid>) -> Option<Role> {
        self.roles.iter().find_map(|entry| {
            let role = entry.value();
            if role.name == name && role.org_id == org_id {
                Some(role.clone())
            } else {
                None
            }
        })
    }

    /// Check if a user has a specific permission
    pub fn check_permission(user: &AuthUser, permission: &str) -> bool {
        user.has_permission(permission)
    }

    /// Get all available permissions
    pub fn all_permissions(&self) -> Vec<Permission> {
        self.permissions.iter().map(|e| e.value().clone()).collect()
    }

    /// Get all roles
    pub fn all_roles(&self) -> Vec<Role> {
        self.roles.iter().map(|e| e.value().clone()).collect()
    }
}

impl Default for RbacEngine {
    fn default() -> Self {
        Self::new()
    }
}
