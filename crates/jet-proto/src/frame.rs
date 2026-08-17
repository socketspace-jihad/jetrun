//! JRP framing.
//!
//! ```text
//! header -- 16 bytes, fixed width, no varints on the hot path
//!   0..2   magic          'J' 'R'
//!   2..4   version        u16   negotiated at handshake
//!   4..6   msg_type       u16   stable numeric code, never reordered
//!   6..8   flags          u16
//!   8..12  stream_id      u32
//!  12..16  payload_len    u32
//! ```
//!
//! Fixed width matters: parsing a frame header is four aligned loads and no
//! branching, versus HTTP/2's HPACK state machine. The magic and version live in
//! *every* header rather than only in the handshake, so a desynchronized stream
//! is detected at the next frame instead of being interpreted as garbage
//! payload.

use std::fmt;

/// `JR`. Present in every frame; see module docs.
pub const MAGIC: [u8; 2] = *b"JR";

/// Wire format version this build speaks.
///
/// Bump on any layout change. The handshake negotiates the highest version both
/// sides support, so a worker fleet can be upgraded one node at a time -- which
/// is the difference between a rolling upgrade and an outage.
pub const VERSION: u16 = 1;

/// Oldest version this build can still talk.
pub const MIN_VERSION: u16 = 1;

pub const HEADER_LEN: usize = 16;

/// Largest control payload accepted.
///
/// This is a hard limit checked *before* allocating, because `payload_len`
/// arrives from the network and a hostile 4 GiB length must not become a 4 GiB
/// reservation. Bulk object transfer deliberately does not go through control
/// frames -- it uses the splice path -- so 8 MiB is generous for anything that
/// legitimately needs a control frame.
pub const MAX_PAYLOAD: u32 = 8 << 20;

/// Message kinds.
///
/// The numeric values are wire format. **Never renumber or reuse a code**, even
/// for a message that is removed: an old peer on the other end of a rolling
/// upgrade will still send it, and silently reinterpreting it as a different
/// message is far worse than an explicit "unknown type" error.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
#[repr(u16)]
pub enum MsgType {
    // 0x00xx -- session lifecycle
    Hello = 0x0001,
    HelloAck = 0x0002,
    Goodbye = 0x0003,
    Error = 0x0004,
    Ping = 0x0005,
    Pong = 0x0006,

    // 0x01xx -- authentication
    Auth = 0x0101,
    AuthOk = 0x0102,

    // 0x02xx -- runs
    SubmitRun = 0x0201,
    RunAccepted = 0x0202,
    GetRun = 0x0203,
    RunInfo = 0x0204,
    CancelRun = 0x0205,

    // 0x03xx -- logs
    TailLogs = 0x0301,
    LogChunk = 0x0302,

    // 0x04xx -- CAS transfer negotiation
    CasQuery = 0x0401,
    CasMissing = 0x0402,
    CasPut = 0x0403,
    CasGet = 0x0404,
    /// Announces that `payload_len` raw bytes follow on the bulk connection.
    /// The bytes themselves never pass through rkyv or userspace buffers.
    BulkHeader = 0x0405,
}

impl MsgType {
    pub fn from_u16(v: u16) -> Option<MsgType> {
        use MsgType::*;
        Some(match v {
            0x0001 => Hello,
            0x0002 => HelloAck,
            0x0003 => Goodbye,
            0x0004 => Error,
            0x0005 => Ping,
            0x0006 => Pong,
            0x0101 => Auth,
            0x0102 => AuthOk,
            0x0201 => SubmitRun,
            0x0202 => RunAccepted,
            0x0203 => GetRun,
            0x0204 => RunInfo,
            0x0205 => CancelRun,
            0x0301 => TailLogs,
            0x0302 => LogChunk,
            0x0401 => CasQuery,
            0x0402 => CasMissing,
            0x0403 => CasPut,
            0x0404 => CasGet,
            0x0405 => BulkHeader,
            _ => return None,
        })
    }

    /// Whether this message may be sent before authentication succeeds.
    ///
    /// Enforced by the server so that an unauthenticated peer cannot reach
    /// anything but the handshake. Keeping the list here, next to the message
    /// definitions, makes adding a message without deciding its auth status
    /// impossible to do accidentally.
    pub fn allowed_preauth(self) -> bool {
        matches!(
            self,
            MsgType::Hello
                | MsgType::HelloAck
                | MsgType::Auth
                | MsgType::AuthOk
                | MsgType::Error
                | MsgType::Ping
                | MsgType::Pong
                | MsgType::Goodbye
        )
    }
}

/// Frame flags.
#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub struct Flags(pub u16);

impl Flags {
    pub const NONE: Flags = Flags(0);
    /// Payload is zstd-compressed.
    pub const COMPRESSED: Flags = Flags(1 << 0);
    /// Last frame on this stream; the receiver may release stream state.
    pub const END_STREAM: Flags = Flags(1 << 1);
    /// A bulk transfer follows out-of-band rather than in this frame.
    pub const BULK: Flags = Flags(1 << 2);

    pub fn contains(self, other: Flags) -> bool {
        self.0 & other.0 == other.0
    }

    pub fn with(self, other: Flags) -> Flags {
        Flags(self.0 | other.0)
    }
}

impl fmt::Debug for Flags {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut parts = Vec::new();
        if self.contains(Flags::COMPRESSED) {
            parts.push("COMPRESSED");
        }
        if self.contains(Flags::END_STREAM) {
            parts.push("END_STREAM");
        }
        if self.contains(Flags::BULK) {
            parts.push("BULK");
        }
        if parts.is_empty() {
            return f.write_str("NONE");
        }
        f.write_str(&parts.join("|"))
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FrameHeader {
    pub version: u16,
    pub msg_type: MsgType,
    pub flags: Flags,
    /// Multiplexing identifier. Odd ids are client-initiated, even
    /// server-initiated, so the two sides can allocate concurrently without a
    /// shared counter or a round trip to agree.
    pub stream_id: u32,
    pub payload_len: u32,
}

impl FrameHeader {
    pub fn new(msg_type: MsgType, stream_id: u32, payload_len: u32) -> Self {
        FrameHeader {
            version: VERSION,
            msg_type,
            flags: Flags::NONE,
            stream_id,
            payload_len,
        }
    }

    pub fn with_flags(mut self, flags: Flags) -> Self {
        self.flags = flags;
        self
    }

    pub fn encode(&self) -> [u8; HEADER_LEN] {
        let mut b = [0u8; HEADER_LEN];
        b[0..2].copy_from_slice(&MAGIC);
        b[2..4].copy_from_slice(&self.version.to_le_bytes());
        b[4..6].copy_from_slice(&(self.msg_type as u16).to_le_bytes());
        b[6..8].copy_from_slice(&self.flags.0.to_le_bytes());
        b[8..12].copy_from_slice(&self.stream_id.to_le_bytes());
        b[12..16].copy_from_slice(&self.payload_len.to_le_bytes());
        b
    }

    pub fn decode(b: &[u8; HEADER_LEN]) -> Result<Self, FrameError> {
        if b[0..2] != MAGIC {
            return Err(FrameError::BadMagic([b[0], b[1]]));
        }
        let version = u16::from_le_bytes([b[2], b[3]]);
        if version < MIN_VERSION || version > VERSION {
            return Err(FrameError::UnsupportedVersion(version));
        }
        let raw_type = u16::from_le_bytes([b[4], b[5]]);
        let msg_type = MsgType::from_u16(raw_type).ok_or(FrameError::UnknownType(raw_type))?;
        let payload_len = u32::from_le_bytes([b[12], b[13], b[14], b[15]]);
        if payload_len > MAX_PAYLOAD {
            // Rejected before any allocation happens.
            return Err(FrameError::PayloadTooLarge(payload_len));
        }
        Ok(FrameHeader {
            version,
            msg_type,
            flags: Flags(u16::from_le_bytes([b[6], b[7]])),
            stream_id: u32::from_le_bytes([b[8], b[9], b[10], b[11]]),
            payload_len,
        })
    }
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum FrameError {
    #[error("bad frame magic {0:?}: stream is not JRP or has desynchronized")]
    BadMagic([u8; 2]),
    #[error("peer speaks JRP version {0}, this build supports {MIN_VERSION}..={VERSION}")]
    UnsupportedVersion(u16),
    #[error("unknown message type 0x{0:04x}")]
    UnknownType(u16),
    #[error("payload of {0} bytes exceeds the {MAX_PAYLOAD} byte limit")]
    PayloadTooLarge(u32),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header_roundtrip() {
        let h = FrameHeader::new(MsgType::SubmitRun, 7, 1234)
            .with_flags(Flags::COMPRESSED.with(Flags::END_STREAM));
        assert_eq!(FrameHeader::decode(&h.encode()).unwrap(), h);
    }

    #[test]
    fn header_is_exactly_sixteen_bytes() {
        assert_eq!(FrameHeader::new(MsgType::Ping, 1, 0).encode().len(), 16);
    }

    #[test]
    fn rejects_foreign_stream() {
        // Someone pointing an HTTP client at the JRP port should get a clear
        // error, not a confusing parse failure deeper in.
        let mut b = [0u8; HEADER_LEN];
        b[0..4].copy_from_slice(b"GET ");
        assert!(matches!(
            FrameHeader::decode(&b),
            Err(FrameError::BadMagic(_))
        ));
    }

    #[test]
    fn rejects_unsupported_version() {
        let mut b = FrameHeader::new(MsgType::Ping, 1, 0).encode();
        b[2..4].copy_from_slice(&999u16.to_le_bytes());
        assert!(matches!(
            FrameHeader::decode(&b),
            Err(FrameError::UnsupportedVersion(999))
        ));
    }

    #[test]
    fn rejects_unknown_message_type() {
        let mut b = FrameHeader::new(MsgType::Ping, 1, 0).encode();
        b[4..6].copy_from_slice(&0xBEEFu16.to_le_bytes());
        assert!(matches!(
            FrameHeader::decode(&b),
            Err(FrameError::UnknownType(0xBEEF))
        ));
    }

    #[test]
    fn rejects_oversized_payload_before_allocating() {
        // The allocation-DoS guard: a hostile length must be refused by the
        // header decoder, not by whatever tries to read it.
        let mut b = FrameHeader::new(MsgType::SubmitRun, 1, 0).encode();
        b[12..16].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(matches!(
            FrameHeader::decode(&b),
            Err(FrameError::PayloadTooLarge(u32::MAX))
        ));
    }

    #[test]
    fn message_codes_are_unique_and_stable() {
        // Renumbering breaks every peer mid-upgrade, so pin the codes with a
        // test rather than trusting review.
        let all = [
            (MsgType::Hello, 0x0001),
            (MsgType::HelloAck, 0x0002),
            (MsgType::Goodbye, 0x0003),
            (MsgType::Error, 0x0004),
            (MsgType::Ping, 0x0005),
            (MsgType::Pong, 0x0006),
            (MsgType::Auth, 0x0101),
            (MsgType::AuthOk, 0x0102),
            (MsgType::SubmitRun, 0x0201),
            (MsgType::RunAccepted, 0x0202),
            (MsgType::GetRun, 0x0203),
            (MsgType::RunInfo, 0x0204),
            (MsgType::CancelRun, 0x0205),
            (MsgType::TailLogs, 0x0301),
            (MsgType::LogChunk, 0x0302),
            (MsgType::CasQuery, 0x0401),
            (MsgType::CasMissing, 0x0402),
            (MsgType::CasPut, 0x0403),
            (MsgType::CasGet, 0x0404),
            (MsgType::BulkHeader, 0x0405),
        ];
        let mut seen = std::collections::HashSet::new();
        for (t, code) in all {
            assert_eq!(t as u16, code, "{t:?} changed its wire code");
            assert_eq!(MsgType::from_u16(code), Some(t));
            assert!(seen.insert(code), "duplicate wire code 0x{code:04x}");
        }
    }

    #[test]
    fn only_handshake_messages_are_allowed_before_auth() {
        assert!(MsgType::Hello.allowed_preauth());
        assert!(MsgType::Auth.allowed_preauth());
        // Anything that touches real data must not be.
        for t in [
            MsgType::SubmitRun,
            MsgType::GetRun,
            MsgType::CancelRun,
            MsgType::TailLogs,
            MsgType::CasGet,
            MsgType::CasPut,
            MsgType::BulkHeader,
        ] {
            assert!(!t.allowed_preauth(), "{t:?} must require authentication");
        }
    }

    #[test]
    fn flags_compose_and_report() {
        let f = Flags::COMPRESSED.with(Flags::BULK);
        assert!(f.contains(Flags::COMPRESSED));
        assert!(f.contains(Flags::BULK));
        assert!(!f.contains(Flags::END_STREAM));
        assert_eq!(format!("{f:?}"), "COMPRESSED|BULK");
        assert_eq!(format!("{:?}", Flags::NONE), "NONE");
    }

    #[test]
    fn stream_id_parity_convention_holds() {
        // Client odd, server even -- so both sides allocate ids without
        // coordinating and can never collide.
        let client = FrameHeader::new(MsgType::GetRun, 1, 0);
        let server = FrameHeader::new(MsgType::LogChunk, 2, 0);
        assert_eq!(client.stream_id % 2, 1);
        assert_eq!(server.stream_id % 2, 0);
    }
}
