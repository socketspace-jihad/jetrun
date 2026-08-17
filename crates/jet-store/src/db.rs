//! Connection management and migration.
//!
//! # Why two pools
//!
//! SQLite permits exactly one writer at a time. The usual result is
//! `SQLITE_BUSY` errors appearing under load, which get papered over with retry
//! loops that turn a deterministic constraint into a flaky one.
//!
//! Instead, writes go through a pool capped at **one** connection. Concurrent
//! writers then queue in the connection pool -- fair, bounded, and with normal
//! backpressure -- rather than racing at the database and losing. Reads use a
//! separate pool of many connections, which WAL mode allows to proceed
//! concurrently with the writer without blocking it.
//!
//! This costs nothing in the self-hosted case and is the difference between a
//! control plane that degrades gracefully and one that emits mysterious
//! contention errors. When the Postgres backend arrives for SaaS/HA, the
//! `writer`/`reader` split stays and simply stops being a limit.
//!
//! # Pragmas that are not optional
//!
//! * `foreign_keys = ON` -- **SQLite disables foreign keys by default**, per
//!   connection. Every `REFERENCES` clause in the migrations is decorative
//!   without this, which is a genuinely dangerous default to be unaware of.
//! * `journal_mode = WAL` -- readers do not block the writer.
//! * `synchronous = NORMAL` -- safe under WAL (a crash cannot corrupt the
//!   database, it can only lose the tail of the most recent transactions) and
//!   dramatically faster than FULL.
//! * `busy_timeout` -- backstop for the checkpointer, not for writer
//!   contention, which the single-writer pool already handles.

use std::path::Path;
use std::str::FromStr;
use std::time::Duration;

use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous};
use sqlx::{Sqlite, SqlitePool, Transaction};

/// Embedded migrations. Compiled into the binary so a single artifact can
/// self-migrate on startup with no external files and no migration tool.
pub static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./migrations");

/// Handle to the metadata store.
#[derive(Clone, Debug)]
pub struct Db {
    reader: SqlitePool,
    writer: SqlitePool,
}

impl Db {
    /// Open (creating if needed) the database at `path` and run migrations.
    pub async fn open(path: impl AsRef<Path>) -> Result<Self, StoreError> {
        let path = path.as_ref();
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
        {
            std::fs::create_dir_all(parent).map_err(StoreError::Fs)?;
        }
        let url = format!("sqlite://{}", path.display());
        Self::connect(&url, true).await
    }

    /// In-memory database, for tests.
    ///
    /// `max_connections(1)` on both pools is required, not a tuning choice: each
    /// connection to `:memory:` would otherwise get its *own* empty database, so
    /// a migration run on one connection would be invisible to the next. The
    /// shared cache keeps them looking at the same data.
    pub async fn open_in_memory() -> Result<Self, StoreError> {
        let opts = SqliteConnectOptions::from_str("sqlite::memory:")
            .map_err(StoreError::Sqlx)?
            .foreign_keys(true)
            .shared_cache(true);

        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .idle_timeout(None)
            .max_lifetime(None)
            .connect_with(opts)
            .await
            .map_err(StoreError::Sqlx)?;

        MIGRATOR.run(&pool).await.map_err(StoreError::Migrate)?;

        Ok(Db {
            reader: pool.clone(),
            writer: pool,
        })
    }

    async fn connect(url: &str, create: bool) -> Result<Self, StoreError> {
        let base = SqliteConnectOptions::from_str(url)
            .map_err(StoreError::Sqlx)?
            .create_if_missing(create)
            // See module docs: none of these four are optional.
            .foreign_keys(true)
            .journal_mode(SqliteJournalMode::Wal)
            .synchronous(SqliteSynchronous::Normal)
            .busy_timeout(Duration::from_secs(5));

        // One writer. Contention becomes queueing instead of SQLITE_BUSY.
        let writer = SqlitePoolOptions::new()
            .max_connections(1)
            .acquire_timeout(Duration::from_secs(30))
            .connect_with(base.clone())
            .await
            .map_err(StoreError::Sqlx)?;

        // Migrate on the writer, before readers exist, so no reader can observe
        // a half-migrated schema.
        MIGRATOR.run(&writer).await.map_err(StoreError::Migrate)?;

        let reader = SqlitePoolOptions::new()
            .max_connections(
                std::thread::available_parallelism()
                    .map(|n| n.get() as u32)
                    .unwrap_or(4)
                    .clamp(2, 16),
            )
            .acquire_timeout(Duration::from_secs(10))
            .connect_with(base.read_only(true))
            .await
            .map_err(StoreError::Sqlx)?;

        Ok(Db { reader, writer })
    }

    /// Pool for queries that do not write.
    ///
    /// Backed by read-only connections, so an accidental write here fails loudly
    /// rather than silently bypassing the single-writer discipline.
    pub fn read(&self) -> &SqlitePool {
        &self.reader
    }

    /// Pool for writes. Serialized by construction.
    pub fn write(&self) -> &SqlitePool {
        &self.writer
    }

    /// Begin a write transaction.
    ///
    /// Several operations here span multiple tables and are only correct as a
    /// unit -- accepting an invitation creates a membership *and* a role
    /// assignment, and a run allocates its per-pipeline number in the same
    /// transaction as its insert.
    pub async fn begin(&self) -> Result<Transaction<'static, Sqlite>, StoreError> {
        self.writer.begin().await.map_err(StoreError::Sqlx)
    }

    pub async fn close(&self) {
        self.reader.close().await;
        self.writer.close().await;
    }
}

/// Current wall clock in Unix milliseconds -- the timestamp representation used
/// by every table.
pub fn now_ms() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("database error: {0}")]
    Sqlx(#[source] sqlx::Error),
    #[error("migration failed: {0}")]
    Migrate(#[source] sqlx::migrate::MigrateError),
    #[error("filesystem error: {0}")]
    Fs(#[source] std::io::Error),
    #[error("{entity} {id} not found")]
    NotFound { entity: &'static str, id: String },
    #[error("{0}")]
    Conflict(String),
    #[error("invalid input: {0}")]
    Invalid(String),
}

impl StoreError {
    pub fn not_found(entity: &'static str, id: impl std::fmt::Display) -> Self {
        StoreError::NotFound {
            entity,
            id: id.to_string(),
        }
    }

    /// Whether a sqlx error is a uniqueness violation, so callers can turn a
    /// race into a meaningful conflict instead of a generic 500.
    pub fn is_unique_violation(e: &sqlx::Error) -> bool {
        matches!(e, sqlx::Error::Database(db) if db.is_unique_violation())
    }
}

impl From<sqlx::Error> for StoreError {
    fn from(e: sqlx::Error) -> Self {
        StoreError::Sqlx(e)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn migrations_apply_cleanly() {
        let db = Db::open_in_memory().await.unwrap();
        // Every migration's tables should exist.
        for table in [
            "users",
            "credentials",
            "organizations",
            "memberships",
            "teams",
            "team_members",
            "permissions",
            "roles",
            "role_permissions",
            "service_accounts",
            "api_tokens",
            "role_assignments",
            "invitations",
            "audit_log",
            "projects",
            "pipelines",
            "runs",
            "steps",
            "secrets",
            "workers",
        ] {
            let found: Option<String> =
                sqlx::query_scalar("SELECT name FROM sqlite_master WHERE type='table' AND name=?")
                    .bind(table)
                    .fetch_optional(db.read())
                    .await
                    .unwrap();
            assert_eq!(found.as_deref(), Some(table), "missing table {table}");
        }
    }

    #[tokio::test]
    async fn permission_catalog_is_seeded() {
        let db = Db::open_in_memory().await.unwrap();
        let n: i64 = sqlx::query_scalar("SELECT count(*) FROM permissions")
            .fetch_one(db.read())
            .await
            .unwrap();
        assert!(n >= 24, "expected the full permission catalog, got {n}");
    }

    #[tokio::test]
    async fn foreign_keys_are_actually_enforced() {
        // SQLite disables FK enforcement by default, per connection. If this
        // regresses, every REFERENCES clause in the schema silently becomes a
        // comment -- so it is worth an explicit test rather than trust.
        let db = Db::open_in_memory().await.unwrap();
        let err = sqlx::query(
            "INSERT INTO memberships (org_id, user_id, status, joined_at)
             VALUES ('org_does_not_exist', 'user_does_not_exist', 'active', 0)",
        )
        .execute(db.write())
        .await
        .expect_err("insert with dangling foreign keys must be rejected");
        assert!(
            format!("{err}").to_lowercase().contains("foreign key"),
            "expected a foreign key error, got: {err}"
        );
    }

    #[tokio::test]
    async fn check_constraints_reject_bad_enums() {
        let db = Db::open_in_memory().await.unwrap();
        sqlx::query(
            "INSERT INTO organizations (id, slug, name, plan, created_at, updated_at)
             VALUES ('o1','acme','Acme','self-hosted',0,0)",
        )
        .execute(db.write())
        .await
        .unwrap();

        let err = sqlx::query(
            "INSERT INTO role_assignments
                (id, org_id, principal_kind, principal_id, role_id,
                 scope_kind, scope_id, created_at)
             VALUES ('ra1','o1','martian','p1','r1','org','o1',0)",
        )
        .execute(db.write())
        .await
        .expect_err("an unknown principal_kind must be rejected");
        let msg = format!("{err}").to_lowercase();
        assert!(
            msg.contains("check") || msg.contains("constraint"),
            "expected a CHECK violation, got: {err}"
        );
    }

    #[tokio::test]
    async fn audit_log_is_append_only() {
        // An audit trail the application can rewrite is not an audit trail. The
        // guarantee lives in the database, so verify it there.
        let db = Db::open_in_memory().await.unwrap();
        sqlx::query(
            "INSERT INTO audit_log (id, org_id, actor_kind, actor_id, action, created_at)
             VALUES ('a1','o1','user','u1','org.create',1)",
        )
        .execute(db.write())
        .await
        .unwrap();

        let upd = sqlx::query("UPDATE audit_log SET action='tampered' WHERE id='a1'")
            .execute(db.write())
            .await;
        assert!(upd.is_err(), "audit_log must reject UPDATE");

        let del = sqlx::query("DELETE FROM audit_log WHERE id='a1'")
            .execute(db.write())
            .await;
        assert!(del.is_err(), "audit_log must reject DELETE");

        // The record is still there and unchanged.
        let action: String = sqlx::query_scalar("SELECT action FROM audit_log WHERE id='a1'")
            .fetch_one(db.read())
            .await
            .unwrap();
        assert_eq!(action, "org.create");
    }

    #[tokio::test]
    async fn system_and_custom_roles_have_separate_uniqueness() {
        let db = Db::open_in_memory().await.unwrap();
        sqlx::query(
            "INSERT INTO organizations (id, slug, name, plan, created_at, updated_at)
             VALUES ('o1','acme','Acme','self-hosted',0,0),
                    ('o2','other','Other','self-hosted',0,0)",
        )
        .execute(db.write())
        .await
        .unwrap();

        // One global 'admin' system role...
        sqlx::query(
            "INSERT INTO roles (id, org_id, key, name, is_system, created_at)
             VALUES ('r1', NULL, 'admin', 'Admin', 1, 0)",
        )
        .execute(db.write())
        .await
        .unwrap();
        // ...and a second is a conflict.
        assert!(
            sqlx::query(
                "INSERT INTO roles (id, org_id, key, name, is_system, created_at)
                 VALUES ('r2', NULL, 'admin', 'Admin', 1, 0)"
            )
            .execute(db.write())
            .await
            .is_err(),
            "duplicate system role key must be rejected"
        );

        // But two different orgs may each define their own custom 'deployer'.
        for (id, org) in [("r3", "o1"), ("r4", "o2")] {
            sqlx::query(
                "INSERT INTO roles (id, org_id, key, name, is_system, created_at)
                 VALUES (?, ?, 'deployer', 'Deployer', 0, 0)",
            )
            .bind(id)
            .bind(org)
            .execute(db.write())
            .await
            .unwrap();
        }
        // And one org cannot define it twice.
        assert!(
            sqlx::query(
                "INSERT INTO roles (id, org_id, key, name, is_system, created_at)
                 VALUES ('r5','o1','deployer','Deployer',0,0)"
            )
            .execute(db.write())
            .await
            .is_err()
        );
    }

    #[tokio::test]
    async fn duplicate_grants_collapse() {
        // Revocation deletes one row; if duplicate grants were possible, revoking
        // would appear to succeed while access remained.
        let db = Db::open_in_memory().await.unwrap();
        sqlx::query(
            "INSERT INTO organizations (id, slug, name, plan, created_at, updated_at)
             VALUES ('o1','acme','Acme','self-hosted',0,0)",
        )
        .execute(db.write())
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO roles (id, org_id, key, name, is_system, created_at)
             VALUES ('r1', NULL, 'viewer', 'Viewer', 1, 0)",
        )
        .execute(db.write())
        .await
        .unwrap();

        let insert = "INSERT INTO role_assignments
              (id, org_id, principal_kind, principal_id, role_id, scope_kind, scope_id, created_at)
              VALUES (?, 'o1','user','u1','r1','org','o1',0)";
        sqlx::query(insert).bind("ra1").execute(db.write()).await.unwrap();
        let dup = sqlx::query(insert).bind("ra2").execute(db.write()).await;
        assert!(dup.is_err(), "the same grant twice must collide");
    }

    #[tokio::test]
    async fn read_pool_rejects_writes() {
        // Guards the single-writer discipline: a stray write on the read pool
        // should fail rather than quietly bypass it.
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open(dir.path().join("jetrun.db")).await.unwrap();
        let res = sqlx::query(
            "INSERT INTO organizations (id, slug, name, plan, created_at, updated_at)
             VALUES ('o1','acme','Acme','self-hosted',0,0)",
        )
        .execute(db.read())
        .await;
        assert!(res.is_err(), "read pool must not accept writes");
        db.close().await;
    }

    #[tokio::test]
    async fn reopening_a_file_database_preserves_data() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("jetrun.db");

        let db = Db::open(&path).await.unwrap();
        sqlx::query(
            "INSERT INTO organizations (id, slug, name, plan, created_at, updated_at)
             VALUES ('o1','acme','Acme','self-hosted',0,0)",
        )
        .execute(db.write())
        .await
        .unwrap();
        db.close().await;

        // Re-running migrations on an existing database must be a no-op, not an
        // error -- the binary self-migrates on every start.
        let db2 = Db::open(&path).await.unwrap();
        let slug: String = sqlx::query_scalar("SELECT slug FROM organizations WHERE id='o1'")
            .fetch_one(db2.read())
            .await
            .unwrap();
        assert_eq!(slug, "acme");
        db2.close().await;
    }
}
