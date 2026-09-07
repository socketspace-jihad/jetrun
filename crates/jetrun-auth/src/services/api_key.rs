use chrono::Utc;
use uuid::Uuid;

use jetrun_common::models::{ApiKey, API_KEY_PREFIX};

/// Generate a new API key. Returns (full_key, api_key_record).
/// The full key is only returned once — store/display it immediately.
pub fn generate_api_key(
    user_id: Uuid,
    org_id: Option<Uuid>,
    name: &str,
    scopes: Vec<String>,
    expires_at: Option<chrono::DateTime<Utc>>,
) -> (String, ApiKey) {
    use base64::Engine;

    // Generate random key bytes
    let mut key_bytes = [0u8; 24];
    rand::fill(&mut key_bytes);
    let key_suffix = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(key_bytes);
    let full_key = format!("{}{}", API_KEY_PREFIX, key_suffix);

    // Hash for storage
    let key_hash = blake3::hash(full_key.as_bytes()).to_hex().to_string();

    // First 16 chars as the displayable prefix
    let prefix = full_key[..API_KEY_PREFIX.len() + 8].to_string();

    let api_key = ApiKey {
        id: Uuid::new_v4(),
        user_id,
        org_id,
        name: name.to_string(),
        prefix,
        key_hash,
        scopes,
        last_used_at: None,
        expires_at,
        created_at: Utc::now(),
        revoked_at: None,
    };

    (full_key, api_key)
}

/// Validate a raw API key against a stored hash
pub fn validate_api_key(raw_key: &str, stored_hash: &str) -> bool {
    let computed_hash = blake3::hash(raw_key.as_bytes()).to_hex().to_string();
    computed_hash == stored_hash
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_api_key() {
        let (full_key, api_key) = generate_api_key(
            Uuid::new_v4(),
            None,
            "Test Key",
            vec!["build:read".into()],
            None,
        );

        assert!(full_key.starts_with(API_KEY_PREFIX));
        assert!(api_key.prefix.starts_with(API_KEY_PREFIX));
        assert!(!api_key.key_hash.is_empty());
        assert!(api_key.is_valid());
    }

    #[test]
    fn test_validate_api_key() {
        let (full_key, api_key) = generate_api_key(
            Uuid::new_v4(),
            None,
            "Test",
            vec![],
            None,
        );

        assert!(validate_api_key(&full_key, &api_key.key_hash));
        assert!(!validate_api_key("jr_live_wrong_key", &api_key.key_hash));
    }
}
