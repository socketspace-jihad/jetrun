pub mod error;
pub mod traits;

#[cfg(feature = "postgres")]
pub mod postgres;

#[cfg(feature = "mysql")]
pub mod mysql;

#[cfg(feature = "sqlite")]
pub mod sqlite;

pub use error::StoreError;
pub use traits::*;

// Re-export the default store based on feature flag
#[cfg(feature = "postgres")]
pub use postgres::PgStore;
