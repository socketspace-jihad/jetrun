use chrono::Utc;
use uuid::Uuid;

use jetrun_common::models::{
    builtin_role_permissions, Permission, Role, RolePermission, User, AuthProvider, AuthUser,
    ALL_PERMISSIONS, ROLE_ADMIN, ROLE_DEVELOPER, ROLE_SUPER_ADMIN, ROLE_VIEWER,
};

use crate::state::AppState;
use crate::services::password;

/// Seed the system with built-in roles, permissions, and a super admin user.
/// Called on first startup. Idempotent — skips if data already exists.
pub fn seed(state: &AppState) {
    // Seed permissions
    let mut perm_map: std::collections::HashMap<String, Uuid> = std::collections::HashMap::new();

    for (name, description, resource, action) in ALL_PERMISSIONS {
        if state.inner.permissions.contains_key(*name) {
            if let Some(entry) = state.inner.permissions.get(*name) {
                perm_map.insert(name.to_string(), entry.value().id);
            }
            continue;
        }

        let perm = Permission {
            id: Uuid::new_v4(),
            name: name.to_string(),
            description: description.to_string(),
            resource: resource.to_string(),
            action: action.to_string(),
        };
        perm_map.insert(name.to_string(), perm.id);
        state.inner.permissions.insert(name.to_string(), perm);
    }

    tracing::info!(count = ALL_PERMISSIONS.len(), "permissions seeded");

    // Seed built-in roles
    let builtin_roles = [
        (ROLE_SUPER_ADMIN, "Super Admin", "Full system control"),
        (ROLE_ADMIN, "Admin", "Full org control, cannot delete users or manage org-level SSO"),
        (ROLE_DEVELOPER, "Developer", "Create/edit projects and pipelines, trigger builds"),
        (ROLE_VIEWER, "Viewer", "Read-only access to all resources"),
    ];

    for (name, display_name, description) in &builtin_roles {
        if state.inner.roles.iter().any(|r| r.value().name == *name && r.value().is_builtin) {
            continue;
        }

        let role = Role {
            id: Uuid::new_v4(),
            name: name.to_string(),
            display_name: display_name.to_string(),
            description: Some(description.to_string()),
            is_builtin: true,
            org_id: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };

        // Map permissions to this role
        let role_perms = builtin_role_permissions(name);
        for perm_name in role_perms {
            if let Some(perm_id) = perm_map.get(*perm_name) {
                let rp = RolePermission {
                    role_id: role.id,
                    permission_id: *perm_id,
                };
                state.inner.role_permissions.entry(role.id)
                    .or_insert_with(Vec::new)
                    .push(rp);
            }
        }

        state.inner.roles.insert(role.id, role);
    }

    tracing::info!(count = builtin_roles.len(), "built-in roles seeded");

    // Seed super admin user if none exists
    let has_super_admin = state.inner.users.iter().any(|u| {
        let user = u.value();
        user.auth_provider == AuthProvider::Local && user.email == state.config.superadmin_email
    });

    if !has_super_admin {
        let password_hash = password::hash_password(&state.config.superadmin_password)
            .expect("failed to hash super admin password");

        let user = User {
            id: Uuid::new_v4(),
            email: state.config.superadmin_email.clone(),
            username: "admin".into(),
            display_name: Some("Super Admin".into()),
            avatar_url: None,
            password_hash: Some(password_hash),
            auth_provider: AuthProvider::Local,
            provider_id: None,
            is_active: true,
            email_verified: true,
            last_login_at: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };

        tracing::info!(email = %user.email, "super admin user seeded");
        state.inner.users.insert(user.id, user);
    }
}
