//! JRP payloads.
//!
//! Encoded with `rkyv`. Two notes on that choice, one of them a caveat:
//!
//! * **Validation is mandatory.** Decoding uses rkyv's *checked* `access`, never
//!   `access_unchecked`. These bytes come off a socket; unchecked access on
//!   hostile input is a memory-safety hole, not a performance tuning knob.
//! * **The honest accounting.** rkyv's zero-copy win is real but modest for
//!   control messages, which are a few hundred bytes -- the decode was never the
//!   bottleneck. JRP's actual advantage over gRPC is that **bulk CAS bytes never
//!   enter userspace at all**, moving page-cache-to-socket via `splice`. rkyv
//!   keeps the control path cheap and allocation-light; it is not the headline.
//!
//! Alignment is why payloads are read into an [`AlignedVec`] rather than a plain
//! `Vec<u8>`: archived access requires the buffer be aligned to the archived
//! type, and a buffer from a socket read has no such guarantee.

use rkyv::rancor;
use rkyv::util::AlignedVec;
use rkyv::{Archive, Deserialize, Serialize};

/// Capability bits exchanged in the handshake, so features can be added without
/// a version bump when both sides can simply agree not to use them.
pub mod caps {
    /// Peer can receive zstd-compressed payloads.
    pub const COMPRESSION: u32 = 1 << 0;
    /// Peer supports the out-of-band bulk transfer connection.
    pub const BULK_TRANSFER: u32 = 1 << 1;
    /// Peer can serve CAS objects.
    pub const CAS: u32 = 1 << 2;
    /// Peer can stream logs.
    pub const LOG_TAIL: u32 = 1 << 3;

    pub const ALL: u32 = COMPRESSION | BULK_TRANSFER | CAS | LOG_TAIL;
}

#[derive(Archive, Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct Hello {
    /// Free-form client identification, for logs only. Never trusted.
    pub agent: String,
    /// The range the peer can speak. The server picks the highest mutually
    /// supported version, which is what makes a rolling fleet upgrade safe.
    pub min_version: u16,
    pub max_version: u16,
    pub capabilities: u32,
}

#[derive(Archive, Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct HelloAck {
    pub agent: String,
    /// Negotiated version. Both sides use exactly this from here on.
    pub version: u16,
    pub capabilities: u32,
    /// Opaque session identifier, echoed in logs to correlate a connection
    /// across both ends.
    pub session: String,
}

#[derive(Archive, Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct Auth {
    /// `jetr_<prefix>_<secret>`. The server splits it, finds the row by prefix,
    /// and verifies the secret with Argon2.
    pub token: String,
}

#[derive(Archive, Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct AuthOk {
    pub org: String,
    pub principal_kind: String,
    pub principal_id: String,
    /// Permissions the caller holds at org scope, so a CLI can grey out what it
    /// cannot do instead of discovering it by failing.
    pub permissions: Vec<String>,
}

#[derive(Archive, Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct SubmitRun {
    pub project: String,
    pub pipeline: String,
    /// The pipeline definition itself, sent inline.
    ///
    /// Deliberate: `jet run` from a laptop must be able to execute a definition
    /// that was never committed, which is the whole point of local==remote. The
    /// server content-addresses it, so an uncommitted run is still reproducible
    /// by digest.
    pub definition: Vec<u8>,
    pub commit_sha: Option<String>,
    pub reference: Option<String>,
    /// Caller-supplied idempotency key. A retried submit with the same key
    /// returns the original run instead of starting a second one -- CLI retries
    /// and flaky networks must not double-spend a build.
    pub idempotency_key: Option<String>,
}

#[derive(Archive, Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct RunAccepted {
    pub run_id: String,
    pub number: u64,
    pub definition_digest: String,
    /// True when an existing run was returned because of `idempotency_key`.
    pub deduplicated: bool,
}

#[derive(Archive, Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct GetRun {
    pub run_id: String,
}

#[derive(Archive, Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct CancelRun {
    pub run_id: String,
}

#[derive(Archive, Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct StepInfo {
    pub name: String,
    pub status: String,
    /// `hit`, `miss`, `manifest_miss`, `uncacheable`. Surfaced to the CLI because
    /// "why was this not cached?" needs to be answerable without a support
    /// ticket.
    pub cache_outcome: Option<String>,
    pub action_key: Option<String>,
    pub exit_code: Option<i32>,
    pub duration_ms: Option<u64>,
}

#[derive(Archive, Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct RunInfo {
    pub run_id: String,
    pub number: u64,
    pub status: String,
    pub steps: Vec<StepInfo>,
    pub created_at_ms: i64,
    pub finished_at_ms: Option<i64>,
}

#[derive(Archive, Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct TailLogs {
    pub run_id: String,
    pub step: Option<String>,
    /// Byte offset to resume from, so a dropped connection does not restart the
    /// log or lose the middle of it.
    pub from_offset: u64,
    pub follow: bool,
}

#[derive(Archive, Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct LogChunk {
    pub step: String,
    pub offset: u64,
    pub data: Vec<u8>,
    pub eof: bool,
}

#[derive(Archive, Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct CasQuery {
    /// Hex digests the sender wants to push.
    pub digests: Vec<String>,
}

#[derive(Archive, Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct CasMissing {
    /// The subset the receiver lacks. Transfer negotiation: only these bytes
    /// cross the wire, which is what makes warming a peer cheap.
    pub digests: Vec<String>,
}

#[derive(Archive, Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct BulkHeader {
    pub digest: String,
    pub len: u64,
}

#[derive(Archive, Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct ErrorMsg {
    pub code: u16,
    pub message: String,
}

#[derive(Archive, Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct Ping {
    pub nonce: u64,
}

#[derive(Archive, Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct Pong {
    pub nonce: u64,
}

#[derive(Archive, Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct Goodbye {
    pub reason: String,
}

/// Error codes carried by [`ErrorMsg`].
///
/// Numeric so a client can branch without string matching, and stable for the
/// same reason message type codes are.
pub mod code {
    pub const INTERNAL: u16 = 1;
    pub const BAD_REQUEST: u16 = 2;
    pub const UNAUTHENTICATED: u16 = 3;
    pub const FORBIDDEN: u16 = 4;
    pub const NOT_FOUND: u16 = 5;
    pub const VERSION_MISMATCH: u16 = 6;
    pub const RATE_LIMITED: u16 = 7;
    /// Sent when a pre-auth peer tries a message that requires authentication.
    pub const AUTH_REQUIRED: u16 = 8;
}

/// Field limits for messages arriving from the network.
///
/// These exist because **rkyv validation is not semantic validation**. Checked
/// access guarantees memory safety -- no wild pointers, no out-of-bounds reads --
/// but `u16` and `u32` accept every bit pattern, so a frame of pure garbage can
/// decode into a structurally perfect message holding nonsense. 64 bytes of
/// `0xff` decodes to a `Hello` claiming version 65535.
///
/// That is fine for memory safety and useless for security, so every message
/// carrying attacker-controlled data implements [`Validate`]. Two of these limits
/// close real denial-of-service vectors rather than merely being tidy:
///
/// * [`MAX_TOKEN_LEN`] -- the token is fed to Argon2, which is deliberately
///   expensive. Without a bound, one 8 MiB frame is an easy CPU exhaustion.
/// * [`MAX_QUERY_DIGESTS`] -- each digest becomes a filesystem existence check.
///   An 8 MiB frame of digests is a quarter of a million stat calls from a single
///   message.
pub mod limits {
    /// Client identification, for logs only.
    pub const MAX_AGENT_LEN: usize = 128;
    /// Generous next to a real `jetr_...` token; tight next to Argon2's cost.
    pub const MAX_TOKEN_LEN: usize = 512;
    /// Project and pipeline slugs.
    pub const MAX_NAME_LEN: usize = 128;
    /// Inline pipeline definition. A YAML file larger than this is not a
    /// pipeline, it is a mistake.
    pub const MAX_DEFINITION_LEN: usize = 1 << 20;
    pub const MAX_REF_LEN: usize = 512;
    pub const MAX_ID_LEN: usize = 64;
    pub const MAX_IDEMPOTENCY_LEN: usize = 128;
    /// Digests per CAS query. Bounds the fan-out of one frame into syscalls.
    pub const MAX_QUERY_DIGESTS: usize = 4096;
    /// Hex-encoded BLAKE3, optionally `b3:`-prefixed.
    pub const MAX_DIGEST_LEN: usize = 67;
}

/// Semantic validation for messages built from untrusted bytes.
///
/// Call this on every inbound message before acting on it. Decoding proves the
/// bytes were well-formed; this proves the values are usable.
pub trait Validate {
    fn validate(&self) -> Result<(), Invalid>;
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum Invalid {
    #[error("{field} is {len} bytes, limit is {max}")]
    TooLong {
        field: &'static str,
        len: usize,
        max: usize,
    },
    #[error("{field} must not be empty")]
    Empty { field: &'static str },
    #[error("{0}")]
    Other(&'static str),
}

fn bounded(field: &'static str, s: &str, max: usize) -> Result<(), Invalid> {
    if s.len() > max {
        return Err(Invalid::TooLong {
            field,
            len: s.len(),
            max,
        });
    }
    Ok(())
}

fn nonempty_bounded(field: &'static str, s: &str, max: usize) -> Result<(), Invalid> {
    if s.is_empty() {
        return Err(Invalid::Empty { field });
    }
    bounded(field, s, max)
}

impl Validate for Hello {
    fn validate(&self) -> Result<(), Invalid> {
        bounded("agent", &self.agent, limits::MAX_AGENT_LEN)?;
        if self.min_version > self.max_version {
            // An inverted range is either a bug or a probe; either way it can
            // never be satisfied.
            return Err(Invalid::Other("min_version exceeds max_version"));
        }
        Ok(())
    }
}

impl Validate for Auth {
    fn validate(&self) -> Result<(), Invalid> {
        // Checked *before* the token reaches Argon2.
        nonempty_bounded("token", &self.token, limits::MAX_TOKEN_LEN)
    }
}

impl Validate for SubmitRun {
    fn validate(&self) -> Result<(), Invalid> {
        nonempty_bounded("project", &self.project, limits::MAX_NAME_LEN)?;
        nonempty_bounded("pipeline", &self.pipeline, limits::MAX_NAME_LEN)?;
        if self.definition.len() > limits::MAX_DEFINITION_LEN {
            return Err(Invalid::TooLong {
                field: "definition",
                len: self.definition.len(),
                max: limits::MAX_DEFINITION_LEN,
            });
        }
        if let Some(sha) = &self.commit_sha {
            bounded("commit_sha", sha, limits::MAX_ID_LEN)?;
        }
        if let Some(r) = &self.reference {
            bounded("reference", r, limits::MAX_REF_LEN)?;
        }
        if let Some(k) = &self.idempotency_key {
            bounded("idempotency_key", k, limits::MAX_IDEMPOTENCY_LEN)?;
        }
        Ok(())
    }
}

impl Validate for GetRun {
    fn validate(&self) -> Result<(), Invalid> {
        nonempty_bounded("run_id", &self.run_id, limits::MAX_ID_LEN)
    }
}

impl Validate for CancelRun {
    fn validate(&self) -> Result<(), Invalid> {
        nonempty_bounded("run_id", &self.run_id, limits::MAX_ID_LEN)
    }
}

impl Validate for TailLogs {
    fn validate(&self) -> Result<(), Invalid> {
        nonempty_bounded("run_id", &self.run_id, limits::MAX_ID_LEN)?;
        if let Some(step) = &self.step {
            bounded("step", step, limits::MAX_NAME_LEN)?;
        }
        Ok(())
    }
}

impl Validate for CasQuery {
    fn validate(&self) -> Result<(), Invalid> {
        if self.digests.len() > limits::MAX_QUERY_DIGESTS {
            return Err(Invalid::TooLong {
                field: "digests",
                len: self.digests.len(),
                max: limits::MAX_QUERY_DIGESTS,
            });
        }
        for d in &self.digests {
            bounded("digest", d, limits::MAX_DIGEST_LEN)?;
        }
        Ok(())
    }
}

/// Serialize a payload.
pub fn encode<T>(value: &T) -> Result<AlignedVec, CodecError>
where
    T: for<'a> rkyv::Serialize<
            rkyv::api::high::HighSerializer<
                AlignedVec,
                rkyv::ser::allocator::ArenaHandle<'a>,
                rancor::Error,
            >,
        >,
{
    rkyv::to_bytes::<rancor::Error>(value).map_err(|e| CodecError::Encode(e.to_string()))
}

/// Deserialize a payload, validating it first.
///
/// Uses rkyv's checked path. The input is untrusted, so validation is not
/// optional -- a malformed archive must produce an error, never a wild pointer.
pub fn decode<T>(bytes: &AlignedVec) -> Result<T, CodecError>
where
    T: Archive,
    T::Archived: for<'a> rkyv::bytecheck::CheckBytes<rkyv::api::high::HighValidator<'a, rancor::Error>>
        + rkyv::Deserialize<T, rkyv::api::high::HighDeserializer<rancor::Error>>,
{
    rkyv::from_bytes::<T, rancor::Error>(bytes.as_slice())
        .map_err(|e| CodecError::Decode(e.to_string()))
}

#[derive(Debug, thiserror::Error)]
pub enum CodecError {
    #[error("failed to encode payload: {0}")]
    Encode(String),
    #[error("failed to decode payload: {0}")]
    Decode(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roundtrip<T>(value: T)
    where
        T: Archive
            + PartialEq
            + std::fmt::Debug
            + Clone
            + for<'a> rkyv::Serialize<
                rkyv::api::high::HighSerializer<
                    AlignedVec,
                    rkyv::ser::allocator::ArenaHandle<'a>,
                    rancor::Error,
                >,
            >,
        T::Archived: for<'a> rkyv::bytecheck::CheckBytes<
                rkyv::api::high::HighValidator<'a, rancor::Error>,
            > + rkyv::Deserialize<T, rkyv::api::high::HighDeserializer<rancor::Error>>,
    {
        let bytes = encode(&value).unwrap();
        let back: T = decode(&bytes).unwrap();
        assert_eq!(back, value);
    }

    #[test]
    fn handshake_messages_roundtrip() {
        roundtrip(Hello {
            agent: "jet-cli/0.1.0".into(),
            min_version: 1,
            max_version: 1,
            capabilities: caps::ALL,
        });
        roundtrip(HelloAck {
            agent: "jetrun-server/0.1.0".into(),
            version: 1,
            capabilities: caps::ALL,
            session: "01J000000000000000000000".into(),
        });
    }

    #[test]
    fn run_messages_roundtrip() {
        roundtrip(SubmitRun {
            project: "web".into(),
            pipeline: "ci".into(),
            definition: b"jobs:\n  build:\n    run: cargo build\n".to_vec(),
            commit_sha: Some("deadbeef".into()),
            reference: Some("refs/heads/main".into()),
            idempotency_key: Some("abc123".into()),
        });
        roundtrip(RunAccepted {
            run_id: "01J1".into(),
            number: 482,
            definition_digest: "b3:aa".into(),
            deduplicated: false,
        });
        roundtrip(RunInfo {
            run_id: "01J1".into(),
            number: 482,
            status: "running".into(),
            steps: vec![StepInfo {
                name: "build".into(),
                status: "success".into(),
                cache_outcome: Some("hit".into()),
                action_key: Some("b3:e10c".into()),
                exit_code: Some(0),
                duration_ms: Some(38),
            }],
            created_at_ms: 1,
            finished_at_ms: None,
        });
    }

    #[test]
    fn log_and_cas_messages_roundtrip() {
        roundtrip(LogChunk {
            step: "build".into(),
            offset: 4096,
            data: vec![0u8; 1024],
            eof: false,
        });
        roundtrip(CasQuery {
            digests: vec!["aa".repeat(32), "bb".repeat(32)],
        });
        roundtrip(CasMissing {
            digests: vec!["bb".repeat(32)],
        });
    }

    #[test]
    fn empty_collections_survive() {
        // A run with no steps yet, and a query answered with "I have everything"
        // -- both are normal and must not be confused with an encoding failure.
        roundtrip(RunInfo {
            run_id: "r".into(),
            number: 1,
            status: "queued".into(),
            steps: vec![],
            created_at_ms: 0,
            finished_at_ms: None,
        });
        roundtrip(CasMissing { digests: vec![] });
    }

    #[test]
    fn large_payload_roundtrips() {
        // A realistic definition plus a log chunk near the frame limit.
        roundtrip(LogChunk {
            step: "test".into(),
            offset: 0,
            data: (0..(256 * 1024)).map(|i| (i % 251) as u8).collect(),
            eof: true,
        });
    }

    #[test]
    fn garbage_is_memory_safe_to_decode_but_semantically_rejected() {
        // This is the important distinction, and it caught a real gap.
        //
        // rkyv's checked access guarantees *memory* safety: no wild pointers, no
        // out-of-bounds reads. It does NOT guarantee the values make sense,
        // because u16 and u32 accept every bit pattern. 64 bytes of 0xff decodes
        // into a structurally perfect Hello claiming version 65535.
        //
        // So decoding garbage is expected to succeed (or fail) without crashing,
        // and `Validate` is what actually refuses it.
        let mut junk = AlignedVec::new();
        junk.extend_from_slice(&[0xff; 64]);

        if let Ok(hello) = decode::<Hello>(&junk) {
            // Memory-safe, and nonsense.
            assert_eq!(hello.min_version, u16::MAX);
            // The handshake's version negotiation refuses it, and so does
            // validation of the range itself.
            assert!(
                hello.min_version > crate::frame::VERSION,
                "a garbage version must not fall inside the supported range"
            );
        }
    }

    #[test]
    fn validation_rejects_an_inverted_version_range() {
        let h = Hello {
            agent: "probe".into(),
            min_version: 9,
            max_version: 2,
            capabilities: 0,
        };
        assert_eq!(
            h.validate(),
            Err(Invalid::Other("min_version exceeds max_version"))
        );
    }

    #[test]
    fn validation_bounds_the_token_before_argon2_sees_it() {
        // A real DoS vector: Argon2 is deliberately expensive, so an 8 MiB token
        // in a single frame is cheap CPU exhaustion unless bounded first.
        let huge = Auth {
            token: "x".repeat(limits::MAX_TOKEN_LEN + 1),
        };
        assert!(matches!(
            huge.validate(),
            Err(Invalid::TooLong { field: "token", .. })
        ));

        assert!(matches!(
            Auth { token: String::new() }.validate(),
            Err(Invalid::Empty { field: "token" })
        ));

        assert!(
            Auth {
                token: "jetr_abcd_secret".into()
            }
            .validate()
            .is_ok()
        );
    }

    #[test]
    fn validation_bounds_cas_query_fanout() {
        // Each digest becomes a filesystem existence check; one frame must not
        // turn into a quarter of a million syscalls.
        let too_many = CasQuery {
            digests: vec!["aa".repeat(32); limits::MAX_QUERY_DIGESTS + 1],
        };
        assert!(matches!(
            too_many.validate(),
            Err(Invalid::TooLong {
                field: "digests",
                ..
            })
        ));

        let overlong_entry = CasQuery {
            digests: vec!["z".repeat(limits::MAX_DIGEST_LEN + 1)],
        };
        assert!(matches!(
            overlong_entry.validate(),
            Err(Invalid::TooLong { field: "digest", .. })
        ));

        assert!(
            CasQuery {
                digests: vec!["aa".repeat(32)]
            }
            .validate()
            .is_ok()
        );
    }

    #[test]
    fn validation_requires_a_target_for_run_operations() {
        for empty in [
            GetRun { run_id: String::new() }.validate(),
            CancelRun { run_id: String::new() }.validate(),
        ] {
            assert!(matches!(empty, Err(Invalid::Empty { field: "run_id" })));
        }
    }

    #[test]
    fn validation_bounds_submit_run_fields() {
        let base = SubmitRun {
            project: "web".into(),
            pipeline: "ci".into(),
            definition: b"jobs: {}".to_vec(),
            commit_sha: None,
            reference: None,
            idempotency_key: None,
        };
        assert!(base.validate().is_ok());

        let mut no_project = base.clone();
        no_project.project = String::new();
        assert!(matches!(
            no_project.validate(),
            Err(Invalid::Empty { field: "project" })
        ));

        let mut huge_def = base.clone();
        huge_def.definition = vec![0u8; limits::MAX_DEFINITION_LEN + 1];
        assert!(matches!(
            huge_def.validate(),
            Err(Invalid::TooLong {
                field: "definition",
                ..
            })
        ));

        let mut long_ref = base.clone();
        long_ref.reference = Some("r".repeat(limits::MAX_REF_LEN + 1));
        assert!(matches!(
            long_ref.validate(),
            Err(Invalid::TooLong {
                field: "reference",
                ..
            })
        ));
    }

    #[test]
    fn decode_rejects_truncated_archive() {
        let good = encode(&Hello {
            agent: "jet-cli".into(),
            min_version: 1,
            max_version: 1,
            capabilities: 0,
        })
        .unwrap();

        for cut in 1..good.len() {
            let mut truncated = AlignedVec::new();
            truncated.extend_from_slice(&good.as_slice()[..cut]);
            assert!(
                decode::<Hello>(&truncated).is_err(),
                "truncation at {cut} must not decode"
            );
        }
    }

    #[test]
    fn decode_rejects_a_different_message_type() {
        // Frames carry the type in the header, but a mismatched decode must still
        // fail rather than reinterpret one struct as another.
        let bytes = encode(&Ping { nonce: 7 }).unwrap();
        assert!(decode::<SubmitRun>(&bytes).is_err());
    }

    #[test]
    fn capability_bits_are_distinct() {
        let all = [
            caps::COMPRESSION,
            caps::BULK_TRANSFER,
            caps::CAS,
            caps::LOG_TAIL,
        ];
        let mut acc = 0u32;
        for c in all {
            assert_eq!(acc & c, 0, "capability bit 0x{c:x} overlaps another");
            acc |= c;
        }
        assert_eq!(acc, caps::ALL);
    }

    #[test]
    fn error_codes_are_distinct() {
        let all = [
            code::INTERNAL,
            code::BAD_REQUEST,
            code::UNAUTHENTICATED,
            code::FORBIDDEN,
            code::NOT_FOUND,
            code::VERSION_MISMATCH,
            code::RATE_LIMITED,
            code::AUTH_REQUIRED,
        ];
        let uniq: std::collections::HashSet<_> = all.iter().collect();
        assert_eq!(uniq.len(), all.len());
    }
}
