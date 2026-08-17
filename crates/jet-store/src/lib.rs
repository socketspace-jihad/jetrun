//! Metadata store: schema, migrations, and repositories.
//!
//! Everything that cannot be recomputed lives here -- organizations, users,
//! grants, runs, audit records. This is the opposite durability posture from
//! `jet-cas`, which is a cache and may lose its tail on a crash.
//!
//! # Queries are runtime-checked, not macro-checked
//!
//! `sqlx::query` is used throughout rather than the compile-time-verified
//! `sqlx::query!` macros. The macros need a live database (or a checked-in
//! `.sqlx` cache) at *build* time, which would mean the project could not be
//! compiled from a clean checkout without extra setup, and would make the
//! Postgres backend require a second set of cached metadata. For a tool people
//! self-host and build themselves, that friction is not worth the extra
//! verification -- the schema is covered by tests instead.

pub mod bootstrap;
pub mod db;

pub use bootstrap::{bootstrap, system_role_id};
pub use db::{Db, MIGRATOR, StoreError, now_ms};
