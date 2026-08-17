//! Seeding the built-in roles.
//!
//! Permissions are seeded by SQL migration (their primary key is the string
//! itself), but roles need generated ULIDs, so they are seeded here instead --
//! idempotently, on every startup.
//!
//! # Code is the source of truth for system roles
//!
//! `bootstrap` does not just create missing roles; it **reconciles** each one's
//! permission set to whatever [`SystemRole::permissions`] currently says. That
//! matters because system role definitions live in Rust and get reviewed as a
//! diff: without reconciliation, tightening a role in code would leave every
//! existing deployment running the old, looser grants, and the change would look
//! applied while doing nothing. Custom org-defined roles are never touched.

use jet_core::{Permission, SystemRole};

use crate::db::{Db, StoreError, now_ms};

/// Create or reconcile the built-in roles. Safe to call on every startup.
pub async fn bootstrap(db: &Db) -> Result<(), StoreError> {
    verify_permission_catalog(db).await?;

    let mut tx = db.begin().await?;
    let now = now_ms();

    for role in SystemRole::ALL {
        // Insert if absent. `INSERT OR IGNORE` rather than an upsert because
        // there is nothing on the role row itself we want to overwrite -- an
        // operator may have renamed it for display purposes.
        let id = jet_core::id::Ulid::generate().to_string();
        sqlx::query(
            "INSERT OR IGNORE INTO roles (id, org_id, key, name, description, is_system, created_at)
             VALUES (?, NULL, ?, ?, ?, 1, ?)",
        )
        .bind(&id)
        .bind(role.key())
        .bind(role.display_name())
        .bind(role.description())
        .bind(now)
        .execute(&mut *tx)
        .await?;

        // Read back the id, which may be the one we just made or a pre-existing
        // one.
        let role_id: String =
            sqlx::query_scalar("SELECT id FROM roles WHERE org_id IS NULL AND key = ?")
                .bind(role.key())
                .fetch_one(&mut *tx)
                .await?;

        // Reconcile: clear and re-add, so removals in code take effect too.
        sqlx::query("DELETE FROM role_permissions WHERE role_id = ?")
            .bind(&role_id)
            .execute(&mut *tx)
            .await?;

        for perm in role.permissions() {
            sqlx::query(
                "INSERT INTO role_permissions (role_id, permission_key) VALUES (?, ?)",
            )
            .bind(&role_id)
            .bind(perm.as_str())
            .execute(&mut *tx)
            .await?;
        }
    }

    tx.commit().await?;
    Ok(())
}

/// Fail fast if the SQL catalog and the Rust enum have drifted.
///
/// A missing permission would otherwise surface as a foreign-key error deep
/// inside role reconciliation, or -- worse, if the FK were ever relaxed -- as a
/// role that silently fails to grant something.
async fn verify_permission_catalog(db: &Db) -> Result<(), StoreError> {
    let rows: Vec<String> = sqlx::query_scalar("SELECT key FROM permissions")
        .fetch_all(db.read())
        .await?;
    let in_db: std::collections::HashSet<&str> = rows.iter().map(|s| s.as_str()).collect();

    let missing: Vec<&str> = Permission::ALL
        .iter()
        .map(|p| p.as_str())
        .filter(|p| !in_db.contains(p))
        .collect();
    if !missing.is_empty() {
        return Err(StoreError::Invalid(format!(
            "permission catalog is missing {missing:?}; a migration is needed to add them"
        )));
    }

    let extra: Vec<&str> = in_db
        .iter()
        .copied()
        .filter(|k| k.parse::<Permission>().is_err())
        .collect();
    if !extra.is_empty() {
        // Not fatal: an older binary against a newer database is a normal state
        // during a rolling upgrade, and refusing to start would turn a deploy
        // into an outage.
        tracing::warn!(
            unknown = ?extra,
            "database defines permissions this binary does not know about; \
             continuing (likely a newer schema during a rolling upgrade)"
        );
    }
    Ok(())
}

/// Resolve a system role to its row id.
pub async fn system_role_id(db: &Db, role: SystemRole) -> Result<String, StoreError> {
    sqlx::query_scalar("SELECT id FROM roles WHERE org_id IS NULL AND key = ?")
        .bind(role.key())
        .fetch_optional(db.read())
        .await?
        .ok_or_else(|| StoreError::not_found("system role", role.key()))
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn booted() -> Db {
        let db = Db::open_in_memory().await.unwrap();
        bootstrap(&db).await.unwrap();
        db
    }

    #[tokio::test]
    async fn seeds_every_system_role() {
        let db = booted().await;
        let n: i64 = sqlx::query_scalar("SELECT count(*) FROM roles WHERE is_system = 1")
            .fetch_one(db.read())
            .await
            .unwrap();
        assert_eq!(n as usize, SystemRole::ALL.len());

        for r in SystemRole::ALL {
            system_role_id(&db, *r).await.expect("role should exist");
        }
    }

    #[tokio::test]
    async fn is_idempotent() {
        // Runs on every startup, so a second call must not duplicate anything.
        let db = booted().await;
        bootstrap(&db).await.unwrap();
        bootstrap(&db).await.unwrap();

        let roles: i64 = sqlx::query_scalar("SELECT count(*) FROM roles")
            .fetch_one(db.read())
            .await
            .unwrap();
        assert_eq!(roles as usize, SystemRole::ALL.len());

        let owner = system_role_id(&db, SystemRole::Owner).await.unwrap();
        let perms: i64 =
            sqlx::query_scalar("SELECT count(*) FROM role_permissions WHERE role_id = ?")
                .bind(&owner)
                .fetch_one(db.read())
                .await
                .unwrap();
        assert_eq!(perms as usize, Permission::ALL.len());
    }

    #[tokio::test]
    async fn role_ids_are_stable_across_runs() {
        // Role ids are referenced by role_assignments; regenerating them would
        // orphan every existing grant.
        let db = booted().await;
        let before = system_role_id(&db, SystemRole::Admin).await.unwrap();
        bootstrap(&db).await.unwrap();
        let after = system_role_id(&db, SystemRole::Admin).await.unwrap();
        assert_eq!(before, after);
    }

    #[tokio::test]
    async fn reconciles_permissions_back_to_the_code_definition() {
        // The scenario: someone tightened a role in code, or tampered with the
        // table directly. Startup must restore what the code says.
        let db = booted().await;
        let viewer = system_role_id(&db, SystemRole::Viewer).await.unwrap();

        // Grant the viewer something alarming, out of band.
        sqlx::query("INSERT INTO role_permissions (role_id, permission_key) VALUES (?, ?)")
            .bind(&viewer)
            .bind(Permission::SecretRead.as_str())
            .execute(db.write())
            .await
            .unwrap();

        bootstrap(&db).await.unwrap();

        let has_secret_read: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM role_permissions WHERE role_id = ? AND permission_key = ?",
        )
        .bind(&viewer)
        .bind(Permission::SecretRead.as_str())
        .fetch_one(db.read())
        .await
        .unwrap();
        assert_eq!(
            has_secret_read, 0,
            "bootstrap must remove grants the code does not define"
        );
    }

    #[tokio::test]
    async fn does_not_touch_custom_org_roles() {
        let db = booted().await;
        sqlx::query(
            "INSERT INTO organizations (id, slug, name, plan, created_at, updated_at)
             VALUES ('o1','acme','Acme','self-hosted',0,0)",
        )
        .execute(db.write())
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO roles (id, org_id, key, name, is_system, created_at)
             VALUES ('rc','o1','deployer','Deployer',0,0)",
        )
        .execute(db.write())
        .await
        .unwrap();
        sqlx::query("INSERT INTO role_permissions (role_id, permission_key) VALUES ('rc', ?)")
            .bind(Permission::PipelineRun.as_str())
            .execute(db.write())
            .await
            .unwrap();

        bootstrap(&db).await.unwrap();

        let kept: i64 =
            sqlx::query_scalar("SELECT count(*) FROM role_permissions WHERE role_id = 'rc'")
                .fetch_one(db.read())
                .await
                .unwrap();
        assert_eq!(kept, 1, "custom role permissions must survive bootstrap");
    }

    #[tokio::test]
    async fn rust_catalog_and_sql_catalog_agree() {
        // The drift guard. Adding a Permission variant without a migration, or a
        // migration row without a variant, fails here rather than in production.
        let db = Db::open_in_memory().await.unwrap();
        let in_db: std::collections::HashSet<String> =
            sqlx::query_scalar("SELECT key FROM permissions")
                .fetch_all(db.read())
                .await
                .unwrap()
                .into_iter()
                .collect();
        let in_code: std::collections::HashSet<String> = Permission::ALL
            .iter()
            .map(|p| p.as_str().to_owned())
            .collect();

        let missing: Vec<_> = in_code.difference(&in_db).collect();
        let extra: Vec<_> = in_db.difference(&in_code).collect();
        assert!(
            missing.is_empty() && extra.is_empty(),
            "permission catalog drift -- only in code: {missing:?}, only in SQL: {extra:?}"
        );
    }

    #[tokio::test]
    async fn bootstrap_fails_loudly_if_a_permission_is_missing_from_sql() {
        let db = Db::open_in_memory().await.unwrap();
        sqlx::query("DELETE FROM role_permissions")
            .execute(db.write())
            .await
            .unwrap();
        sqlx::query("DELETE FROM permissions WHERE key = ?")
            .bind(Permission::CachePurge.as_str())
            .execute(db.write())
            .await
            .unwrap();

        let err = bootstrap(&db).await.expect_err("should refuse to proceed");
        assert!(
            format!("{err}").contains("cache.purge"),
            "error should name the missing permission, got: {err}"
        );
    }
}
