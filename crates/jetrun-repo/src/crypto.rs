use aes_gcm::{
    aead::{Aead, KeyInit, OsRng},
    Aes256Gcm, Nonce,
};
use base64::Engine;

const NONCE_SIZE: usize = 12;

/// Derive a 32-byte encryption key from the env var or a passphrase
pub fn get_encryption_key() -> [u8; 32] {
    let key_str = std::env::var("CREDENTIAL_ENCRYPTION_KEY")
        .unwrap_or_else(|_| {
            // Fallback: derive from JWT_SECRET (not ideal for production)
            std::env::var("JWT_SECRET").unwrap_or_else(|_| "jetrun-default-key-change-me-in-prod".into())
        });

    let hash = blake3::hash(key_str.as_bytes());
    *hash.as_bytes()
}

/// Encrypt a plaintext string with AES-256-GCM.
/// Returns base64-encoded `nonce:ciphertext`.
pub fn encrypt(plaintext: &str, key: &[u8; 32]) -> anyhow::Result<String> {
    let cipher = Aes256Gcm::new_from_slice(key)
        .map_err(|e| anyhow::anyhow!("cipher init: {}", e))?;

    let mut nonce_bytes = [0u8; NONCE_SIZE];
    rand::fill(&mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);

    let ciphertext = cipher
        .encrypt(nonce, plaintext.as_bytes())
        .map_err(|e| anyhow::anyhow!("encrypt: {}", e))?;

    // Prepend nonce to ciphertext, then base64 encode
    let mut combined = Vec::with_capacity(NONCE_SIZE + ciphertext.len());
    combined.extend_from_slice(&nonce_bytes);
    combined.extend_from_slice(&ciphertext);

    Ok(base64::engine::general_purpose::STANDARD.encode(&combined))
}

/// Decrypt a base64-encoded `nonce:ciphertext` string.
pub fn decrypt(encrypted: &str, key: &[u8; 32]) -> anyhow::Result<String> {
    let combined = base64::engine::general_purpose::STANDARD
        .decode(encrypted)
        .map_err(|e| anyhow::anyhow!("base64 decode: {}", e))?;

    if combined.len() < NONCE_SIZE {
        anyhow::bail!("encrypted data too short");
    }

    let (nonce_bytes, ciphertext) = combined.split_at(NONCE_SIZE);
    let nonce = Nonce::from_slice(nonce_bytes);

    let cipher = Aes256Gcm::new_from_slice(key)
        .map_err(|e| anyhow::anyhow!("cipher init: {}", e))?;

    let plaintext = cipher
        .decrypt(nonce, ciphertext)
        .map_err(|e| anyhow::anyhow!("decrypt: {}", e))?;

    String::from_utf8(plaintext).map_err(|e| anyhow::anyhow!("utf8: {}", e))
}

/// Generate an Ed25519 SSH keypair.
/// Returns (private_key_pem, public_key_openssh).
pub fn generate_ssh_keypair() -> anyhow::Result<(String, String)> {
    use std::process::Command;

    let dir = tempfile::tempdir()?;
    let key_path = dir.path().join("id_ed25519");
    let key_path_str = key_path.to_str().unwrap();

    let output = Command::new("ssh-keygen")
        .args(["-t", "ed25519", "-f", key_path_str, "-N", "", "-C", "jetrun-deploy-key"])
        .output()?;

    if !output.status.success() {
        anyhow::bail!("ssh-keygen failed: {}", String::from_utf8_lossy(&output.stderr));
    }

    let private_key = std::fs::read_to_string(&key_path)?;
    let public_key = std::fs::read_to_string(format!("{}.pub", key_path_str))?;

    Ok((private_key.trim().to_string(), public_key.trim().to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encrypt_decrypt() {
        let key = [42u8; 32];
        let plaintext = "super-secret-ssh-key-content";
        let encrypted = encrypt(plaintext, &key).unwrap();
        assert_ne!(encrypted, plaintext);
        let decrypted = decrypt(&encrypted, &key).unwrap();
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn test_different_encryptions() {
        let key = [42u8; 32];
        let e1 = encrypt("same", &key).unwrap();
        let e2 = encrypt("same", &key).unwrap();
        // Different nonces → different ciphertexts
        assert_ne!(e1, e2);
        // But both decrypt to same value
        assert_eq!(decrypt(&e1, &key).unwrap(), "same");
        assert_eq!(decrypt(&e2, &key).unwrap(), "same");
    }

    #[test]
    fn test_wrong_key_fails() {
        let key1 = [1u8; 32];
        let key2 = [2u8; 32];
        let encrypted = encrypt("secret", &key1).unwrap();
        assert!(decrypt(&encrypted, &key2).is_err());
    }
}
