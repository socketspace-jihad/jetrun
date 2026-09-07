use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Role {
    pub id: Uuid,
    pub name: String,
    pub display_name: String,
    pub description: Option<String>,
    pub is_builtin: bool,
    pub org_id: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Permission {
    pub id: Uuid,
    pub name: String,
    pub description: String,
    pub resource: String,
    pub action: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RolePermission {
    pub role_id: Uuid,
    pub permission_id: Uuid,
}

/// Built-in role names — these cannot be deleted
pub const ROLE_SUPER_ADMIN: &str = "super_admin";
pub const ROLE_ADMIN: &str = "admin";
pub const ROLE_DEVELOPER: &str = "developer";
pub const ROLE_VIEWER: &str = "viewer";

/// All 22 permissions in the system
pub const ALL_PERMISSIONS: &[(&str, &str, &str, &str)] = &[
    // (name, description, resource, action)
    ("project:read", "View projects", "project", "read"),
    ("project:create", "Create new projects", "project", "create"),
    ("project:update", "Edit project settings", "project", "update"),
    ("project:delete", "Delete projects", "project", "delete"),
    ("pipeline:read", "View pipelines and configs", "pipeline", "read"),
    ("pipeline:create", "Create/import pipelines", "pipeline", "create"),
    ("pipeline:update", "Edit pipeline configs", "pipeline", "update"),
    ("pipeline:delete", "Delete pipelines", "pipeline", "delete"),
    ("build:read", "View builds and logs", "build", "read"),
    ("build:trigger", "Manually trigger builds", "build", "trigger"),
    ("build:cancel", "Cancel running builds", "build", "cancel"),
    ("build:retry", "Retry failed builds", "build", "retry"),
    ("cache:read", "View cache stats", "cache", "read"),
    ("cache:purge", "Purge cache entries", "cache", "purge"),
    ("user:read", "View user list/profiles", "user", "read"),
    ("user:create", "Invite/create users", "user", "create"),
    ("user:update", "Edit other users' profiles", "user", "update"),
    ("user:delete", "Deactivate/remove users", "user", "delete"),
    ("user:manage", "Assign roles, manage access", "user", "manage"),
    ("org:update", "Edit org settings", "org", "update"),
    ("org:manage", "Manage teams, billing, SSO config", "org", "manage"),
    ("api_key:manage", "Create/revoke API keys for the org", "api_key", "manage"),
];

/// Default permission sets for built-in roles
pub fn builtin_role_permissions(role_name: &str) -> &'static [&'static str] {
    match role_name {
        ROLE_SUPER_ADMIN => &[
            "project:read", "project:create", "project:update", "project:delete",
            "pipeline:read", "pipeline:create", "pipeline:update", "pipeline:delete",
            "build:read", "build:trigger", "build:cancel", "build:retry",
            "cache:read", "cache:purge",
            "user:read", "user:create", "user:update", "user:delete", "user:manage",
            "org:update", "org:manage",
            "api_key:manage",
        ],
        ROLE_ADMIN => &[
            "project:read", "project:create", "project:update", "project:delete",
            "pipeline:read", "pipeline:create", "pipeline:update", "pipeline:delete",
            "build:read", "build:trigger", "build:cancel", "build:retry",
            "cache:read", "cache:purge",
            "user:read", "user:create", "user:update", "user:manage",
            "org:update",
            "api_key:manage",
        ],
        ROLE_DEVELOPER => &[
            "project:read", "project:create", "project:update",
            "pipeline:read", "pipeline:create", "pipeline:update",
            "build:read", "build:trigger", "build:cancel", "build:retry",
            "cache:read",
            "user:read",
        ],
        ROLE_VIEWER => &[
            "project:read",
            "pipeline:read",
            "build:read",
            "cache:read",
            "user:read",
        ],
        _ => &[],
    }
}
