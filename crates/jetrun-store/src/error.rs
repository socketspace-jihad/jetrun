use thiserror::Error;

#[derive(Debug, Error)]
pub enum StoreError {
    #[error("Database error: {0}")]
    Database(#[from] sqlx::Error),

    #[error("Not found: {entity} with {field}={value}")]
    NotFound {
        entity: &'static str,
        field: &'static str,
        value: String,
    },

    #[error("Already exists: {entity} with {field}={value}")]
    AlreadyExists {
        entity: &'static str,
        field: &'static str,
        value: String,
    },

    #[error("Migration error: {0}")]
    Migration(String),
}
