mod users;
mod orgs;
mod roles;
mod sessions;
mod api_keys;
mod pipelines;
mod builds;

use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;

use crate::error::StoreError;

/// PostgreSQL-backed store. Implements all repository traits.
///
/// Migrations are tracked in `_sqlx_migrations` table.
/// Only new migrations run — already-applied ones are skipped.
/// To add a schema change: create `migrations/YYYYMMDDHHMMSS_description.sql`.
#[derive(Clone)]
pub struct PgStore {
    pub pool: PgPool,
}

impl PgStore {
    /// Connect to PostgreSQL and run pending migrations automatically.
    pub async fn connect(database_url: &str) -> Result<Self, StoreError> {
        let pool = PgPoolOptions::new()
            .max_connections(20)
            .connect(database_url)
            .await
            .map_err(StoreError::Database)?;

        tracing::info!("connected to PostgreSQL");

        Self::migrate(&pool).await?;

        Ok(Self { pool })
    }

    async fn migrate(pool: &PgPool) -> Result<(), StoreError> {
        sqlx::migrate!("./migrations")
            .run(pool)
            .await
            .map_err(|e| StoreError::Migration(e.to_string()))?;

        tracing::info!("database migrations applied");
        Ok(())
    }
}
