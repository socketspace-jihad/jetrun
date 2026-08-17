//! Webhook authentication.
//!
//! This is the only part of jetrun that an unauthenticated stranger on the
//! internet can reach, so it is written to fail closed at every branch.
//!
//! # The three providers do not agree, and one header name is a trap
//!
//! | provider  | header                  | scheme                    |
//! |-----------|-------------------------|---------------------------|
//! | GitHub    | `X-Hub-Signature-256`   | HMAC-SHA256 over raw body |
//! | Bitbucket | `X-Hub-Signature`       | HMAC-SHA256 over raw body |
//! | GitLab    | `X-Gitlab-Token`        | shared secret, verbatim   |
//!
//! Note the collision: **GitHub's `X-Hub-Signature` is HMAC-SHA1**, while
//! Bitbucket's `X-Hub-Signature` is HMAC-SHA256. Same header, different
//! algorithm. A "generic" verifier that keys off the header name would either
//! reject valid Bitbucket deliveries or accept SHA-1 from GitHub. So the scheme
//! is chosen by *provider*, never inferred from the request, and GitHub's SHA-1
//! header is refused outright rather than accepted as a fallback -- a downgrade
//! an attacker gets to choose is not a fallback.
//!
//! # GitLab's model is weaker, and callers should know
//!
//! GitLab does not sign the body. `X-Gitlab-Token` is a bearer secret: it proves
//! the sender knew the secret, and provides **no integrity guarantee over the
//! payload**. TLS is therefore mandatory for GitLab webhooks, not merely
//! advisable, and [`Verified::body_authenticated`] reports which guarantee was
//! actually obtained so downstream code cannot silently assume the stronger one.

use hmac::{Hmac, Mac};
use sha2::Sha256;
use subtle::ConstantTimeEq;

use super::Provider;

type HmacSha256 = Hmac<Sha256>;

/// GitHub's modern signature header.
pub const GITHUB_SIG_256: &str = "x-hub-signature-256";
/// GitHub's legacy SHA-1 header. Recognized only so it can be refused.
pub const GITHUB_SIG_LEGACY: &str = "x-hub-signature";
/// Bitbucket's signature header -- same name as GitHub's legacy one, different
/// algorithm. See module docs.
pub const BITBUCKET_SIG: &str = "x-hub-signature";
/// GitLab's shared-secret header.
pub const GITLAB_TOKEN: &str = "x-gitlab-token";

/// Outcome of a successful check.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Verified {
    /// Whether the payload bytes themselves are covered by the proof.
    ///
    /// True for HMAC providers. **False for GitLab**, where a valid token says
    /// nothing about whether the body was modified in transit.
    pub body_authenticated: bool,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum VerifyError {
    /// No secret is configured for this provider.
    ///
    /// Deliberately an error rather than "skip verification". An endpoint that
    /// accepts unsigned payloads because nobody configured a secret is an open
    /// door on the public internet, and the failure would be invisible.
    #[error("no webhook secret configured for {0}; refusing to accept unverified payloads")]
    NoSecretConfigured(Provider),

    #[error("missing {0} header")]
    MissingHeader(&'static str),

    #[error("malformed signature header")]
    MalformedSignature,

    /// GitHub sent only the SHA-1 header.
    #[error(
        "refusing HMAC-SHA1 signature: configure the webhook to send \
         X-Hub-Signature-256 instead"
    )]
    LegacySha1Refused,

    #[error("signature does not match")]
    SignatureMismatch,
}

/// Verify a delivery.
///
/// `headers` is looked up case-insensitively by the caller (HTTP header names are
/// case-insensitive, and providers are not consistent about casing). `body` must
/// be the **exact bytes received** -- see [`super`] on why parsing before
/// verifying breaks the signature.
pub fn verify(
    provider: Provider,
    headers: &dyn HeaderLookup,
    body: &[u8],
    secret: Option<&[u8]>,
) -> Result<Verified, VerifyError> {
    let secret = secret.ok_or(VerifyError::NoSecretConfigured(provider))?;
    if secret.is_empty() {
        // An empty secret is a misconfiguration that would otherwise produce a
        // stable, guessable HMAC key.
        return Err(VerifyError::NoSecretConfigured(provider));
    }

    match provider {
        Provider::GitHub => {
            match headers.get(GITHUB_SIG_256) {
                Some(sig) => verify_hmac_sha256(sig, body, secret),
                None => {
                    // Present but legacy-only: refuse explicitly so the operator
                    // learns to reconfigure, instead of silently failing.
                    if headers.get(GITHUB_SIG_LEGACY).is_some() {
                        Err(VerifyError::LegacySha1Refused)
                    } else {
                        Err(VerifyError::MissingHeader(GITHUB_SIG_256))
                    }
                }
            }
        }
        Provider::Bitbucket => {
            let sig = headers
                .get(BITBUCKET_SIG)
                .ok_or(VerifyError::MissingHeader(BITBUCKET_SIG))?;
            verify_hmac_sha256(sig, body, secret)
        }
        Provider::GitLab => {
            let token = headers
                .get(GITLAB_TOKEN)
                .ok_or(VerifyError::MissingHeader(GITLAB_TOKEN))?;
            // Constant-time even though this is a plain comparison: a
            // byte-at-a-time early exit leaks the secret one character per
            // request.
            if token.as_bytes().ct_eq(secret).into() {
                Ok(Verified {
                    body_authenticated: false,
                })
            } else {
                Err(VerifyError::SignatureMismatch)
            }
        }
    }
}

/// Verify a `sha256=<hex>` HMAC header against the raw body.
fn verify_hmac_sha256(header: &str, body: &[u8], secret: &[u8]) -> Result<Verified, VerifyError> {
    // Both GitHub and Bitbucket prefix the algorithm. Require it rather than
    // accepting a bare hex digest, so a header from some other scheme cannot be
    // reinterpreted here.
    let hex_part = header
        .strip_prefix("sha256=")
        .ok_or(VerifyError::MalformedSignature)?;

    let mut expected = [0u8; 32];
    if hex_part.len() != 64 {
        return Err(VerifyError::MalformedSignature);
    }
    hex::decode_to_slice(hex_part, &mut expected).map_err(|_| VerifyError::MalformedSignature)?;

    let mut mac = HmacSha256::new_from_slice(secret).map_err(|_| VerifyError::MalformedSignature)?;
    mac.update(body);
    let actual = mac.finalize().into_bytes();

    // ct_eq, not ==. Comparing MACs with a short-circuiting equality is the
    // textbook timing oracle: an attacker recovers the correct digest one byte at
    // a time by measuring response latency.
    if actual.as_slice().ct_eq(&expected).into() {
        Ok(Verified {
            body_authenticated: true,
        })
    } else {
        Err(VerifyError::SignatureMismatch)
    }
}

/// Compute the signature a provider would send. Used by tests and by
/// `jet debug webhook` to reproduce a delivery locally.
pub fn sign_hmac_sha256(body: &[u8], secret: &[u8]) -> String {
    let mut mac = HmacSha256::new_from_slice(secret).expect("hmac accepts any key length");
    mac.update(body);
    format!("sha256={}", hex::encode(mac.finalize().into_bytes()))
}

/// Case-insensitive header access, so this module does not depend on `http`.
pub trait HeaderLookup {
    /// `name` is always lowercase.
    fn get(&self, name: &str) -> Option<&str>;
}

impl HeaderLookup for std::collections::HashMap<String, String> {
    fn get(&self, name: &str) -> Option<&str> {
        // Callers building a map for tests may use any casing.
        self.iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    const SECRET: &[u8] = b"correct horse battery staple";
    const BODY: &[u8] = br#"{"ref":"refs/heads/main"}"#;

    fn headers(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    // ---------------------------------------------------------- happy paths

    #[test]
    fn github_accepts_a_valid_sha256_signature() {
        let h = headers(&[("X-Hub-Signature-256", &sign_hmac_sha256(BODY, SECRET))]);
        let v = verify(Provider::GitHub, &h, BODY, Some(SECRET)).unwrap();
        assert!(v.body_authenticated);
    }

    #[test]
    fn bitbucket_accepts_a_valid_signature_on_the_shared_header_name() {
        let h = headers(&[("X-Hub-Signature", &sign_hmac_sha256(BODY, SECRET))]);
        let v = verify(Provider::Bitbucket, &h, BODY, Some(SECRET)).unwrap();
        assert!(v.body_authenticated);
    }

    #[test]
    fn gitlab_accepts_the_shared_secret_but_reports_weaker_guarantee() {
        let h = headers(&[("X-Gitlab-Token", "correct horse battery staple")]);
        let v = verify(Provider::GitLab, &h, BODY, Some(SECRET)).unwrap();
        assert!(
            !v.body_authenticated,
            "GitLab does not sign the body; downstream code must not assume it does"
        );
    }

    #[test]
    fn header_casing_does_not_matter() {
        // Providers are inconsistent, and HTTP header names are case-insensitive.
        for name in [
            "x-hub-signature-256",
            "X-Hub-Signature-256",
            "X-HUB-SIGNATURE-256",
        ] {
            let h = headers(&[(name, &sign_hmac_sha256(BODY, SECRET))]);
            assert!(verify(Provider::GitHub, &h, BODY, Some(SECRET)).is_ok(), "{name}");
        }
    }

    // ------------------------------------------------------- fail-closed

    #[test]
    fn absent_secret_is_refused_not_skipped() {
        // The critical one: no configured secret must never mean "accept
        // anything". This endpoint faces the internet.
        let h = headers(&[("X-Hub-Signature-256", &sign_hmac_sha256(BODY, SECRET))]);
        assert_eq!(
            verify(Provider::GitHub, &h, BODY, None),
            Err(VerifyError::NoSecretConfigured(Provider::GitHub))
        );
    }

    #[test]
    fn empty_secret_is_refused() {
        let h = headers(&[("X-Hub-Signature-256", &sign_hmac_sha256(BODY, b""))]);
        assert_eq!(
            verify(Provider::GitHub, &h, BODY, Some(b"")),
            Err(VerifyError::NoSecretConfigured(Provider::GitHub))
        );
    }

    #[test]
    fn missing_signature_header_is_rejected() {
        let h = headers(&[("X-GitHub-Event", "push")]);
        assert_eq!(
            verify(Provider::GitHub, &h, BODY, Some(SECRET)),
            Err(VerifyError::MissingHeader(GITHUB_SIG_256))
        );
    }

    #[test]
    fn github_sha1_downgrade_is_refused() {
        // An attacker must not be able to choose the weaker algorithm. Since
        // SHA-1 is what GitHub sends on the *same header name Bitbucket uses for
        // SHA-256*, accepting it generically would be a real vulnerability.
        let h = headers(&[("X-Hub-Signature", "sha1=0000000000000000000000000000000000000000")]);
        assert_eq!(
            verify(Provider::GitHub, &h, BODY, Some(SECRET)),
            Err(VerifyError::LegacySha1Refused)
        );
    }

    #[test]
    fn tampered_body_is_rejected() {
        let sig = sign_hmac_sha256(BODY, SECRET);
        let h = headers(&[("X-Hub-Signature-256", &sig)]);
        let tampered = br#"{"ref":"refs/heads/evil"}"#;
        assert_eq!(
            verify(Provider::GitHub, &h, tampered, Some(SECRET)),
            Err(VerifyError::SignatureMismatch)
        );
    }

    #[test]
    fn wrong_secret_is_rejected() {
        let h = headers(&[("X-Hub-Signature-256", &sign_hmac_sha256(BODY, b"other"))]);
        assert_eq!(
            verify(Provider::GitHub, &h, BODY, Some(SECRET)),
            Err(VerifyError::SignatureMismatch)
        );
    }

    #[test]
    fn a_single_flipped_bit_is_rejected() {
        let mut sig = sign_hmac_sha256(BODY, SECRET);
        // Flip the last hex nibble.
        let last = sig.pop().unwrap();
        sig.push(if last == '0' { '1' } else { '0' });
        let h = headers(&[("X-Hub-Signature-256", &sig)]);
        assert_eq!(
            verify(Provider::GitHub, &h, BODY, Some(SECRET)),
            Err(VerifyError::SignatureMismatch)
        );
    }

    #[test]
    fn malformed_signature_headers_are_rejected() {
        for bad in [
            "",                              // empty
            "deadbeef",                      // no algorithm prefix
            "sha256=",                       // prefix only
            "sha256=xyz",                    // too short
            "sha256=zz00000000000000000000000000000000000000000000000000000000000000", // not hex
            "sha512=0000000000000000000000000000000000000000000000000000000000000000", // wrong algo
        ] {
            let h = headers(&[("X-Hub-Signature-256", bad)]);
            let got = verify(Provider::GitHub, &h, BODY, Some(SECRET));
            assert!(
                matches!(got, Err(VerifyError::MalformedSignature)),
                "{bad:?} should be malformed, got {got:?}"
            );
        }
    }

    #[test]
    fn signature_of_wrong_length_is_rejected_before_hex_decode() {
        // 63 and 65 hex chars: guards against an off-by-one in the length check
        // letting a truncated digest through.
        for n in [63usize, 65, 128] {
            let h = headers(&[("X-Hub-Signature-256", &format!("sha256={}", "a".repeat(n)))]);
            assert_eq!(
                verify(Provider::GitHub, &h, BODY, Some(SECRET)),
                Err(VerifyError::MalformedSignature),
                "length {n}"
            );
        }
    }

    #[test]
    fn gitlab_rejects_a_wrong_token() {
        let h = headers(&[("X-Gitlab-Token", "wrong")]);
        assert_eq!(
            verify(Provider::GitLab, &h, BODY, Some(SECRET)),
            Err(VerifyError::SignatureMismatch)
        );
    }

    #[test]
    fn gitlab_rejects_a_token_prefix() {
        // A prefix must not pass; length equality is part of the comparison.
        let h = headers(&[("X-Gitlab-Token", "correct horse")]);
        assert_eq!(
            verify(Provider::GitLab, &h, BODY, Some(SECRET)),
            Err(VerifyError::SignatureMismatch)
        );
    }

    #[test]
    fn empty_body_still_verifies_correctly() {
        // GitHub's `ping` can carry a minimal body; the HMAC must still be over
        // exactly those bytes.
        let h = headers(&[("X-Hub-Signature-256", &sign_hmac_sha256(b"", SECRET))]);
        assert!(verify(Provider::GitHub, &h, b"", Some(SECRET)).is_ok());
    }

    #[test]
    fn providers_do_not_accept_each_others_schemes() {
        // GitLab's token header must not satisfy GitHub, and vice versa.
        let gitlab_h = headers(&[("X-Gitlab-Token", "correct horse battery staple")]);
        assert!(verify(Provider::GitHub, &gitlab_h, BODY, Some(SECRET)).is_err());

        let github_h = headers(&[("X-Hub-Signature-256", &sign_hmac_sha256(BODY, SECRET))]);
        assert!(verify(Provider::GitLab, &github_h, BODY, Some(SECRET)).is_err());
    }

    #[test]
    fn signature_is_computed_over_exact_bytes_including_whitespace() {
        // Why the raw body must be verified before parsing: re-serialized JSON is
        // semantically identical and cryptographically different.
        let pretty = b"{\n  \"ref\": \"refs/heads/main\"\n}";
        let compact = b"{\"ref\":\"refs/heads/main\"}";
        let h = headers(&[("X-Hub-Signature-256", &sign_hmac_sha256(pretty, SECRET))]);

        assert!(verify(Provider::GitHub, &h, pretty, Some(SECRET)).is_ok());
        assert_eq!(
            verify(Provider::GitHub, &h, compact, Some(SECRET)),
            Err(VerifyError::SignatureMismatch),
            "reparsing and reserializing the body must invalidate the signature"
        );
    }
}
