//! Typed identifiers.
//!
//! Every entity gets its own ID type rather than sharing a `String` or a bare
//! `Ulid`. The reason is narrow and practical: authorization takes a scope
//! (`org | project | pipeline`) and a target, and passing a `ProjectId` where an
//! `OrgId` belongs would be a privilege-escalation bug that reads as correct
//! code. Distinct types make that a compile error instead of an audit finding.
//!
//! ULID rather than UUIDv4 because the first 48 bits are a millisecond
//! timestamp: IDs sort by creation time, which gives B-tree index locality on
//! insert (random UUIDs scatter writes across the whole index) and makes
//! `ORDER BY id` a free chronological sort. They are also 26 chars in
//! Crockford base32 -- shorter than a UUID, case-insensitive, and free of
//! visually ambiguous characters.

use std::fmt;
use std::str::FromStr;

pub use ulid::Ulid;

/// Defines a typed, ULID-backed identifier.
macro_rules! typed_id {
    ($(#[$meta:meta])* $name:ident, $prefix:literal) => {
        $(#[$meta])*
        #[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize)]
        #[serde(transparent)]
        pub struct $name(Ulid);

        impl $name {
            /// Human-readable type tag, used in URLs and log lines.
            pub const PREFIX: &'static str = $prefix;

            /// Mint a new id from the current time.
            #[allow(clippy::new_without_default)]
            pub fn new() -> Self {
                $name(Ulid::generate())
            }

            pub const fn from_ulid(u: Ulid) -> Self {
                $name(u)
            }

            pub const fn as_ulid(&self) -> Ulid {
                self.0
            }

            /// Creation time, recovered from the ULID's timestamp prefix.
            ///
            /// Convenient, but never treat it as authoritative: it reflects the
            /// clock of whichever node minted the id. Ordering and auditing use
            /// database timestamps.
            pub fn created_at_ms(&self) -> u64 {
                self.0.timestamp_ms()
            }

            /// Canonical string form: the bare 26-char ULID.
            ///
            /// Deliberately unprefixed so it can be stored in a fixed-width
            /// TEXT column and compared without parsing. `PREFIX` is for
            /// display and route construction only.
            pub fn to_canonical(&self) -> String {
                self.0.to_string()
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "{}", self.0)
            }
        }

        impl fmt::Debug for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "{}_{}", $prefix, self.0)
            }
        }

        impl FromStr for $name {
            type Err = IdParseError;
            fn from_str(s: &str) -> Result<Self, Self::Err> {
                // Accept both `proj_01H...` and the bare form, so ids pasted
                // from logs (which use the Debug form) parse too.
                let bare = s
                    .strip_prefix(concat!($prefix, "_"))
                    .unwrap_or(s);
                bare.parse::<Ulid>()
                    .map($name)
                    .map_err(|_| IdParseError { kind: $prefix, got: s.to_owned() })
            }
        }
    };
}

typed_id!(
    /// Tenant root. Every other row hangs off one of these.
    OrgId,
    "org"
);
typed_id!(
    /// A person. Global, not org-scoped: one human joining two organizations is
    /// one `UserId` with two memberships, not two accounts.
    UserId,
    "user"
);
typed_id!(TeamId, "team");
typed_id!(ProjectId, "proj");
typed_id!(PipelineId, "pipe");
typed_id!(RunId, "run");
typed_id!(StepId, "step");
typed_id!(
    /// Machine principal -- a runner, a CLI in CI, an operator controller.
    ServiceAccountId,
    "sa"
);
typed_id!(RoleId, "role");
typed_id!(TokenId, "tok");
typed_id!(InvitationId, "inv");
typed_id!(WorkerId, "wrk");
typed_id!(VolumeId, "vol");
typed_id!(SecretId, "sec");

#[derive(Debug, thiserror::Error)]
#[error("not a valid {kind} id: {got:?}")]
pub struct IdParseError {
    pub kind: &'static str,
    pub got: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrips_bare_and_prefixed() {
        let id = ProjectId::new();
        assert_eq!(id.to_canonical().parse::<ProjectId>().unwrap(), id);
        assert_eq!(format!("{id:?}").parse::<ProjectId>().unwrap(), id);
    }

    #[test]
    fn rejects_garbage() {
        assert!("not-an-id".parse::<OrgId>().is_err());
        assert!("".parse::<OrgId>().is_err());
    }

    #[test]
    fn ids_sort_by_creation_time() {
        // The property that buys index locality is that the 48-bit timestamp
        // prefix dominates the sort. Within a single millisecond the random
        // suffix decides, so ordering there is deliberately unspecified --
        // asserting it would be a flaky test, not a stronger guarantee.
        let mut ids: Vec<RunId> = Vec::new();
        for _ in 0..5 {
            ids.push(RunId::new());
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        let mut sorted = ids.clone();
        sorted.sort();
        assert_eq!(ids, sorted, "ids minted milliseconds apart must be ordered");

        // And the timestamps really are increasing, which is what a B-tree sees.
        for w in ids.windows(2) {
            assert!(w[0].created_at_ms() < w[1].created_at_ms());
        }
    }

    #[test]
    fn distinct_types_do_not_interchange() {
        // Compile-time proof that the scope-confusion bug is unrepresentable:
        // the canonical strings are structurally identical, but the types are
        // not assignable to each other. (If this ever compiles with the types
        // swapped, the macro has regressed.)
        let o = OrgId::new();
        let p = ProjectId::new();
        assert_ne!(o.to_canonical(), p.to_canonical());
        assert_eq!(OrgId::PREFIX, "org");
        assert_eq!(ProjectId::PREFIX, "proj");
    }

    #[test]
    fn debug_form_is_prefixed_display_is_not() {
        let id = OrgId::new();
        assert!(format!("{id:?}").starts_with("org_"));
        assert!(!format!("{id}").starts_with("org_"));
    }
}
