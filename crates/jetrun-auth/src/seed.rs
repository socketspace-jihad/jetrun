use chrono::Utc;
use uuid::Uuid;

use jetrun_common::models::{
    builtin_role_permissions, Permission, Role, User, AuthProvider,
    ALL_PERMISSIONS, ROLE_ADMIN, ROLE_DEVELOPER, ROLE_SUPER_ADMIN, ROLE_VIEWER,
};

use crate::state::AppState;
use crate::services::password;

/// Seed the system with built-in roles, permissions, and a super admin user.
/// Called on first startup. Idempotent — skips if data already exists.
pub async fn seed(state: &AppState) {
    // Seed permissions
    let mut perm_map: std::collections::HashMap<String, Uuid> = std::collections::HashMap::new();

    for (name, description, resource, action) in ALL_PERMISSIONS {
        // Check if permission already exists
        if let Ok(Some(existing)) = state.store.find_permission_by_name(name).await {
            perm_map.insert(name.to_string(), existing.id);
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
        let _ = state.store.create_permission(&perm).await;
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
        // Check if role already exists
        if let Ok(Some(_)) = state.store.find_builtin_role(name).await {
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
        let perm_ids: Vec<Uuid> = role_perms
            .iter()
            .filter_map(|perm_name| perm_map.get(*perm_name).copied())
            .collect();

        let _ = state.store.create_role(&role).await;
        let _ = state.store.set_role_permissions(role.id, &perm_ids).await;
    }

    tracing::info!(count = builtin_roles.len(), "built-in roles seeded");

    // Seed super admin user if none exists
    let has_super_admin = state.store
        .find_user_by_email(&state.config.superadmin_email).await
        .ok()
        .flatten()
        .is_some();

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
        let _ = state.store.create_user(&user).await;
    }
}
