//! Shared primitives for jetrun.
//!
//! This crate exists to hold the handful of types that genuinely everything
//! needs -- content digests and typed ids -- so that the CAS, the store, the
//! authorizer and the wire protocol can all speak about the same values without
//! depending on each other. It deliberately has no I/O, no async runtime, and no
//! database: anything with a side effect belongs a layer up.

pub mod authz;
pub mod digest;
pub mod id;

pub use authz::{
    Permission, Principal, PrincipalKind, Scope, ScopeKind, SystemRole, UnknownPermission,
};
pub use digest::{DIGEST_LEN, DIGEST_PREFIX, Digest, DigestParseError, Hasher};
pub use id::{
    InvitationId, OrgId, PipelineId, ProjectId, RoleId, RunId, SecretId, ServiceAccountId, StepId,
    TeamId, TokenId, Ulid, UserId, VolumeId, WorkerId,
};

/// Domain-separation contexts for keyed hashing.
///
/// Every composite key in the system draws its context from here. Two rules,
/// both load-bearing:
///
/// 1. **Never reuse a context across key kinds.** Distinct contexts are what
///    make an action key provably unable to collide with a blob digest or a
///    tree digest, so a lookup in one namespace can never be answered from
///    another.
/// 2. **Bump the version suffix whenever a key's *layout* changes.** The
///    contexts are versioned rather than the cache being wiped: an old jetrun
///    and a new jetrun sharing one CAS will simply miss each other's entries
///    instead of trusting a key computed under different rules. Silent
///    cross-version hits would serve wrong build outputs, which is the worst
///    failure this system can have.
pub mod context {
    /// A file's content. The only namespace that hashes raw bytes.
    pub const BLOB: &str = "jetrun.blob.v1";
    /// A directory manifest (the Merkle node over its entries).
    pub const TREE: &str = "jetrun.tree.v1";
    /// `argv` + env allowlist + image digest + platform. Identifies a step
    /// independently of what it reads.
    pub const STEP_IDENTITY: &str = "jetrun.step-identity.v1";
    /// Step identity + the sorted set of observed inputs. The key that decides
    /// whether work is skipped.
    pub const ACTION_KEY: &str = "jetrun.action-key.v1";
    /// Step identity -> predicted input set (the manifest-cache key).
    pub const INPUT_MANIFEST: &str = "jetrun.input-manifest.v1";
    /// A cache volume version.
    pub const VOLUME_VERSION: &str = "jetrun.volume-version.v1";
    /// API token hashing is *not* here on purpose -- tokens use Argon2 in
    /// `jet-authz`, because BLAKE3 is far too fast to protect a secret against
    /// offline guessing.
    pub const _RESERVED: &str = "";
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contexts_are_unique() {
        // A copy-paste duplicate here would silently merge two key namespaces,
        // which is exactly the class of bug domain separation exists to prevent.
        let all = [
            context::BLOB,
            context::TREE,
            context::STEP_IDENTITY,
            context::ACTION_KEY,
            context::INPUT_MANIFEST,
            context::VOLUME_VERSION,
        ];
        let mut seen = std::collections::HashSet::new();
        for c in all {
            assert!(seen.insert(c), "duplicate domain-separation context: {c}");
            assert!(c.starts_with("jetrun."), "context {c} lacks the jetrun prefix");
            assert!(
                c.rsplit('.').next().is_some_and(|v| v.starts_with('v')),
                "context {c} is missing a version suffix"
            );
        }
    }

    #[test]
    fn every_context_yields_a_distinct_namespace() {
        let payload = b"identical bytes in every namespace";
        let keys: Vec<Digest> = [
            context::BLOB,
            context::TREE,
            context::STEP_IDENTITY,
            context::ACTION_KEY,
            context::INPUT_MANIFEST,
            context::VOLUME_VERSION,
        ]
        .iter()
        .map(|c| Digest::keyed(c, payload))
        .collect();

        for (i, a) in keys.iter().enumerate() {
            for b in &keys[i + 1..] {
                assert_ne!(a, b, "two contexts collided on identical input");
            }
        }
    }
}
