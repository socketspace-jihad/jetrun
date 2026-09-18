use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Secret {
    pub id: Uuid,
    pub org_id: Uuid,
    pub name: String,
    pub description: Option<String>,
    pub secret_type: SecretType,
    /// AES-256-GCM encrypted value. NEVER expose via API.
    #[serde(skip_serializing)]
    pub encrypted_value: String,
    /// Public key for SSH secrets (safe to display to admins)
    pub ssh_public_key: Option<String>,
    pub created_by: Uuid,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SecretType {
    SshKey,
    Token,
    Password,
}

impl std::fmt::Display for SecretType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::SshKey => write!(f, "ssh_key"),
            Self::Token => write!(f, "token"),
            Self::Password => write!(f, "password"),
        }
    }
}
