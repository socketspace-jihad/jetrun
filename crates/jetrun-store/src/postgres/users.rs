use async_trait::async_trait;
use uuid::Uuid;

use jetrun_common::models::{AuthProvider, User};

use super::PgStore;
use crate::error::StoreError;
use crate::traits::UserRepo;

#[async_trait]
impl UserRepo for PgStore {
    async fn create_user(&self, user: &User) -> Result<(), StoreError> {
        sqlx::query(
            "INSERT INTO users (id, email, username, display_name, avatar_url, password_hash, auth_provider, provider_id, is_active, email_verified, last_login_at, created_at, updated_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13)"
        )
        .bind(user.id)
        .bind(&user.email)
        .bind(&user.username)
        .bind(&user.display_name)
        .bind(&user.avatar_url)
        .bind(&user.password_hash)
        .bind(user.auth_provider.to_string())
        .bind(&user.provider_id)
        .bind(user.is_active)
        .bind(user.email_verified)
        .bind(user.last_login_at)
        .bind(user.created_at)
        .bind(user.updated_at)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn find_user_by_id(&self, id: Uuid) -> Result<Option<User>, StoreError> {
        let row = sqlx::query_as::<_, UserRow>(
            "SELECT id, email, username, display_name, avatar_url, password_hash, auth_provider, provider_id, is_active, email_verified, last_login_at, created_at, updated_at FROM users WHERE id = $1"
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(|r| r.into()))
    }

    async fn find_user_by_email(&self, email: &str) -> Result<Option<User>, StoreError> {
        let row = sqlx::query_as::<_, UserRow>(
            "SELECT id, email, username, display_name, avatar_url, password_hash, auth_provider, provider_id, is_active, email_verified, last_login_at, created_at, updated_at FROM users WHERE lower(email) = lower($1) AND is_active = true"
        )
        .bind(email)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(|r| r.into()))
    }

    async fn find_user_by_username(&self, username: &str) -> Result<Option<User>, StoreError> {
        let row = sqlx::query_as::<_, UserRow>(
            "SELECT id, email, username, display_name, avatar_url, password_hash, auth_provider, provider_id, is_active, email_verified, last_login_at, created_at, updated_at FROM users WHERE lower(username) = lower($1) AND is_active = true"
        )
        .bind(username)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(|r| r.into()))
    }

    async fn update_user(&self, user: &User) -> Result<(), StoreError> {
        sqlx::query(
            "UPDATE users SET display_name = $2, avatar_url = $3, password_hash = $4, is_active = $5, email_verified = $6, last_login_at = $7, updated_at = $8 WHERE id = $1"
        )
        .bind(user.id)
        .bind(&user.display_name)
        .bind(&user.avatar_url)
        .bind(&user.password_hash)
        .bind(user.is_active)
        .bind(user.email_verified)
        .bind(user.last_login_at)
        .bind(user.updated_at)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn has_any_user(&self) -> Result<bool, StoreError> {
        let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM users")
            .fetch_one(&self.pool)
            .await?;
        Ok(count.0 > 0)
    }
}

#[derive(sqlx::FromRow)]
struct UserRow {
    id: Uuid,
    email: String,
    username: String,
    display_name: Option<String>,
    avatar_url: Option<String>,
    password_hash: Option<String>,
    auth_provider: String,
    provider_id: Option<String>,
    is_active: bool,
    email_verified: bool,
    last_login_at: Option<chrono::DateTime<chrono::Utc>>,
    created_at: chrono::DateTime<chrono::Utc>,
    updated_at: chrono::DateTime<chrono::Utc>,
}

impl From<UserRow> for User {
    fn from(r: UserRow) -> Self {
        User {
            id: r.id,
            email: r.email,
            username: r.username,
            display_name: r.display_name,
            avatar_url: r.avatar_url,
            password_hash: r.password_hash,
            auth_provider: match r.auth_provider.as_str() {
                "google" => AuthProvider::Google,
                "github" => AuthProvider::Github,
                "gitlab" => AuthProvider::Gitlab,
                "bitbucket" => AuthProvider::Bitbucket,
                "apple" => AuthProvider::Apple,
                "saml" => AuthProvider::Saml,
                _ => AuthProvider::Local,
            },
            provider_id: r.provider_id,
            is_active: r.is_active,
            email_verified: r.email_verified,
            last_login_at: r.last_login_at,
            created_at: r.created_at,
            updated_at: r.updated_at,
        }
    }
}
