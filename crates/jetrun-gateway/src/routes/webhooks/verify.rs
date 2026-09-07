/// HMAC-SHA256 signature verification for GitHub webhooks.
/// Only compiled when `webhook-github` feature is enabled.
#[cfg(feature = "webhook-github")]
pub fn verify_github_signature(secret: &str, payload: &[u8], signature: &str) -> bool {
    use hmac::{Hmac, Mac};
    use sha2::Sha256;

    // GitHub sends signature as "sha256=<hex>"
    let expected = match signature.strip_prefix("sha256=") {
        Some(hex_sig) => hex_sig,
        None => return false,
    };

    let Ok(expected_bytes) = hex::decode(expected) else {
        return false;
    };

    let mut mac = match Hmac::<Sha256>::new_from_slice(secret.as_bytes()) {
        Ok(m) => m,
        Err(_) => return false,
    };

    mac.update(payload);
    mac.verify_slice(&expected_bytes).is_ok()
}

/// Token-based verification for GitLab webhooks.
/// GitLab sends the secret token in the `X-Gitlab-Token` header.
#[cfg(feature = "webhook-gitlab")]
pub fn verify_gitlab_token(expected_token: &str, received_token: &str) -> bool {
    // Constant-time comparison to prevent timing attacks
    if expected_token.len() != received_token.len() {
        return false;
    }
    expected_token
        .bytes()
        .zip(received_token.bytes())
        .fold(0u8, |acc, (a, b)| acc | (a ^ b))
        == 0
}

#[cfg(test)]
mod tests {
    #[test]
    #[cfg(feature = "webhook-github")]
    fn test_github_signature_verification() {
        use super::verify_github_signature;
        use hmac::{Hmac, Mac};
        use sha2::Sha256;

        let secret = "test-secret";
        let payload = b"hello world";

        // Generate a valid signature
        let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes()).unwrap();
        mac.update(payload);
        let result = mac.finalize();
        let signature = format!("sha256={}", hex::encode(result.into_bytes()));

        assert!(verify_github_signature(secret, payload, &signature));
        assert!(!verify_github_signature("wrong-secret", payload, &signature));
        assert!(!verify_github_signature(secret, b"wrong payload", &signature));
    }

    #[test]
    #[cfg(feature = "webhook-gitlab")]
    fn test_gitlab_token_verification() {
        use super::verify_gitlab_token;

        assert!(verify_gitlab_token("my-secret-token", "my-secret-token"));
        assert!(!verify_gitlab_token("my-secret-token", "wrong-token"));
        assert!(!verify_gitlab_token("short", "longer-token"));
    }
}
