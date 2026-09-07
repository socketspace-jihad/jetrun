use bytes::{Buf, BufMut, Bytes, BytesMut};


use crate::error::WireError;

/// Magic bytes at the start of every frame — detects desync/misconnection
pub const MAGIC: [u8; 2] = [b'J', b'R'];

/// Current protocol version
pub const PROTOCOL_VERSION: u16 = 1;

/// Maximum payload size: 8MB (bulk transfers use separate connection, not control frames)
pub const MAX_PAYLOAD_SIZE: u32 = 8 * 1024 * 1024;

/// Frame header size: 16 bytes
///
/// ```text
/// magic(2) | version(2 LE) | msg_type(2 LE) | flags(2 LE) | stream_id(4 LE) | payload_len(4 LE)
/// ```
pub const HEADER_SIZE: usize = 16;

/// Message type identifiers (u16, organized by namespace)
///
/// 0x0001-0x00FF: Session lifecycle
#[repr(u16)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageType {
    Request = 0x0001,
    Response = 0x0002,
    StreamStart = 0x0003,
    StreamData = 0x0004,
    StreamEnd = 0x0005,
    Error = 0x0006,
    Ping = 0x0007,
    Pong = 0x0008,
}

impl TryFrom<u16> for MessageType {
    type Error = WireError;

    fn try_from(v: u16) -> Result<Self, WireError> {
        match v {
            0x0001 => Ok(Self::Request),
            0x0002 => Ok(Self::Response),
            0x0003 => Ok(Self::StreamStart),
            0x0004 => Ok(Self::StreamData),
            0x0005 => Ok(Self::StreamEnd),
            0x0006 => Ok(Self::Error),
            0x0007 => Ok(Self::Ping),
            0x0008 => Ok(Self::Pong),
            _ => Err(WireError::InvalidFrame(format!(
                "unknown message type: 0x{:04x}",
                v
            ))),
        }
    }
}

bitflags::bitflags! {
    /// Frame flags (u16)
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct Flags: u16 {
        /// Payload is LZ4-compressed
        const COMPRESSED = 0b0000_0000_0000_0001;
        /// Last frame in a stream
        const LAST_FRAME = 0b0000_0000_0000_0010;
        /// Requires acknowledgment
        const NEEDS_ACK  = 0b0000_0000_0000_0100;
        /// Marks bulk transfer frames
        const BULK       = 0b0000_0000_0000_1000;
    }
}

/// Wire frame header + payload
///
/// ```text
/// ┌───────┬─────────┬──────────┬───────┬───────────┬──────────────┬─────────────┐
/// │ magic │ version │ msg_type │ flags │ stream_id │ payload_len  │   payload   │
/// │ 2 B   │ 2 B LE  │ 2 B LE   │ 2 B LE│ 4 B LE    │ 4 B LE       │  N bytes    │
/// └───────┴─────────┴──────────┴───────┴───────────┴──────────────┴─────────────┘
/// ```
#[derive(Debug, Clone)]
pub struct Frame {
    pub msg_type: MessageType,
    pub flags: Flags,
    pub stream_id: u32,
    pub payload: Bytes,
}

impl Frame {
    pub fn new(msg_type: MessageType, stream_id: u32, payload: Bytes) -> Self {
        Self {
            msg_type,
            flags: Flags::empty(),
            stream_id,
            payload,
        }
    }

    pub fn with_flags(mut self, flags: Flags) -> Self {
        self.flags = flags;
        self
    }

    /// Encode frame into bytes (header + payload)
    pub fn encode(&self, dst: &mut BytesMut) {
        dst.reserve(HEADER_SIZE + self.payload.len());
        // magic (2 bytes)
        dst.put_slice(&MAGIC);
        // version (2 bytes LE)
        dst.put_u16_le(PROTOCOL_VERSION);
        // msg_type (2 bytes LE)
        dst.put_u16_le(self.msg_type as u16);
        // flags (2 bytes LE)
        dst.put_u16_le(self.flags.bits());
        // stream_id (4 bytes LE)
        dst.put_u32_le(self.stream_id);
        // payload_len (4 bytes LE)
        dst.put_u32_le(self.payload.len() as u32);
        // payload
        dst.extend_from_slice(&self.payload);
    }

    /// Decode a frame from bytes. Returns None if not enough data.
    pub fn decode(src: &mut BytesMut) -> Result<Option<Frame>, WireError> {
        if src.len() < HEADER_SIZE {
            return Ok(None); // Not enough data for header
        }

        // Validate magic bytes
        if src[0] != MAGIC[0] || src[1] != MAGIC[1] {
            return Err(WireError::BadMagic(src[0], src[1]));
        }

        // Validate protocol version
        let version = u16::from_le_bytes([src[2], src[3]]);
        if version != PROTOCOL_VERSION {
            return Err(WireError::VersionMismatch {
                got: version,
                expected: PROTOCOL_VERSION,
            });
        }

        // Peek at payload length without advancing (bytes 12..16)
        let payload_len =
            u32::from_le_bytes([src[12], src[13], src[14], src[15]]) as usize;

        if payload_len > MAX_PAYLOAD_SIZE as usize {
            return Err(WireError::PayloadTooLarge {
                size: payload_len as u32,
                max: MAX_PAYLOAD_SIZE,
            });
        }

        let total_len = HEADER_SIZE + payload_len;
        if src.len() < total_len {
            // Reserve space for the full frame
            src.reserve(total_len - src.len());
            return Ok(None); // Not enough data yet
        }

        // Now consume the frame
        let msg_type = MessageType::try_from(u16::from_le_bytes([src[4], src[5]]))?;
        let flags = Flags::from_bits_truncate(u16::from_le_bytes([src[6], src[7]]));
        let stream_id = u32::from_le_bytes([src[8], src[9], src[10], src[11]]);

        src.advance(HEADER_SIZE);
        let payload = src.split_to(payload_len).freeze();

        Ok(Some(Frame {
            msg_type,
            flags,
            stream_id,
            payload,
        }))
    }

    /// Create a ping frame
    pub fn ping() -> Self {
        Self::new(MessageType::Ping, 0, Bytes::new())
    }

    /// Create a pong frame
    pub fn pong() -> Self {
        Self::new(MessageType::Pong, 0, Bytes::new())
    }
}

/// Helper to write a valid 16-byte header into a BytesMut for testing purposes.
#[cfg(test)]
fn write_test_header(
    buf: &mut BytesMut,
    magic: [u8; 2],
    version: u16,
    msg_type: u16,
    flags: u16,
    stream_id: u32,
    payload_len: u32,
) {
    buf.put_slice(&magic);
    buf.put_u16_le(version);
    buf.put_u16_le(msg_type);
    buf.put_u16_le(flags);
    buf.put_u32_le(stream_id);
    buf.put_u32_le(payload_len);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encode_decode_roundtrip() {
        let original = Frame::new(
            MessageType::Request,
            42,
            Bytes::from_static(b"hello world"),
        );

        let mut buf = BytesMut::new();
        original.encode(&mut buf);

        assert_eq!(buf.len(), HEADER_SIZE + 11); // 16 + "hello world"

        let decoded = Frame::decode(&mut buf).unwrap().unwrap();
        assert_eq!(decoded.msg_type, MessageType::Request);
        assert_eq!(decoded.stream_id, 42);
        assert_eq!(decoded.payload, Bytes::from_static(b"hello world"));
        assert_eq!(decoded.flags, Flags::empty());
    }

    #[test]
    fn test_encode_decode_with_flags() {
        let original = Frame::new(MessageType::StreamData, 7, Bytes::from_static(b"data"))
            .with_flags(Flags::COMPRESSED | Flags::LAST_FRAME);

        let mut buf = BytesMut::new();
        original.encode(&mut buf);

        let decoded = Frame::decode(&mut buf).unwrap().unwrap();
        assert!(decoded.flags.contains(Flags::COMPRESSED));
        assert!(decoded.flags.contains(Flags::LAST_FRAME));
        assert!(!decoded.flags.contains(Flags::NEEDS_ACK));
    }

    #[test]
    fn test_decode_incomplete_header() {
        let mut buf = BytesMut::from(&[b'J', b'R'][..]);
        assert!(Frame::decode(&mut buf).unwrap().is_none());
    }

    #[test]
    fn test_decode_incomplete_payload() {
        let mut buf = BytesMut::new();
        write_test_header(
            &mut buf,
            MAGIC,
            PROTOCOL_VERSION,
            0x0001, // Request
            0x0000, // no flags
            1,      // stream_id
            100,    // payload_len = 100
        );
        buf.extend_from_slice(&[0u8; 50]); // only 50 bytes of payload

        assert!(Frame::decode(&mut buf).unwrap().is_none());
    }

    #[test]
    fn test_decode_payload_too_large() {
        let mut buf = BytesMut::new();
        write_test_header(
            &mut buf,
            MAGIC,
            PROTOCOL_VERSION,
            0x0001,
            0x0000,
            1,
            MAX_PAYLOAD_SIZE + 1,
        );

        assert!(Frame::decode(&mut buf).is_err());
    }

    #[test]
    fn test_empty_payload() {
        let original = Frame::new(MessageType::Ping, 0, Bytes::new());

        let mut buf = BytesMut::new();
        original.encode(&mut buf);

        assert_eq!(buf.len(), HEADER_SIZE);

        let decoded = Frame::decode(&mut buf).unwrap().unwrap();
        assert_eq!(decoded.msg_type, MessageType::Ping);
        assert!(decoded.payload.is_empty());
    }

    #[test]
    fn test_invalid_message_type() {
        let mut buf = BytesMut::new();
        write_test_header(
            &mut buf,
            MAGIC,
            PROTOCOL_VERSION,
            0x00FF, // invalid message type
            0x0000,
            0,
            0,
        );

        assert!(Frame::decode(&mut buf).is_err());
    }

    // ── New tests for Phase 3 ──

    #[test]
    fn test_bad_magic_rejected() {
        let mut buf = BytesMut::new();
        write_test_header(
            &mut buf,
            [b'X', b'Y'], // wrong magic
            PROTOCOL_VERSION,
            0x0001,
            0x0000,
            0,
            0,
        );

        let err = Frame::decode(&mut buf).unwrap_err();
        match err {
            WireError::BadMagic(a, b) => {
                assert_eq!(a, b'X');
                assert_eq!(b, b'Y');
            }
            other => panic!("expected BadMagic, got {:?}", other),
        }
    }

    #[test]
    fn test_version_mismatch_rejected() {
        let mut buf = BytesMut::new();
        write_test_header(
            &mut buf,
            MAGIC,
            99, // wrong version
            0x0001,
            0x0000,
            0,
            0,
        );

        let err = Frame::decode(&mut buf).unwrap_err();
        match err {
            WireError::VersionMismatch { got, expected } => {
                assert_eq!(got, 99);
                assert_eq!(expected, PROTOCOL_VERSION);
            }
            other => panic!("expected VersionMismatch, got {:?}", other),
        }
    }

    #[test]
    fn test_bulk_flag() {
        let original = Frame::new(MessageType::StreamData, 100, Bytes::from_static(b"bulk"))
            .with_flags(Flags::BULK);

        let mut buf = BytesMut::new();
        original.encode(&mut buf);

        let decoded = Frame::decode(&mut buf).unwrap().unwrap();
        assert!(decoded.flags.contains(Flags::BULK));
        assert!(!decoded.flags.contains(Flags::COMPRESSED));
    }

    #[test]
    fn test_stream_id_encode_decode() {
        // Test large stream_id (> u16 range)
        let stream_id: u32 = 0x1234_5678;
        let original = Frame::new(MessageType::Request, stream_id, Bytes::from_static(b"x"));

        let mut buf = BytesMut::new();
        original.encode(&mut buf);

        let decoded = Frame::decode(&mut buf).unwrap().unwrap();
        assert_eq!(decoded.stream_id, stream_id);
    }

    #[test]
    fn test_stream_id_max_value() {
        let original = Frame::new(MessageType::Response, u32::MAX, Bytes::new());

        let mut buf = BytesMut::new();
        original.encode(&mut buf);

        let decoded = Frame::decode(&mut buf).unwrap().unwrap();
        assert_eq!(decoded.stream_id, u32::MAX);
    }

    #[test]
    fn test_all_flags_combined() {
        let all_flags = Flags::COMPRESSED | Flags::LAST_FRAME | Flags::NEEDS_ACK | Flags::BULK;
        let original = Frame::new(MessageType::StreamEnd, 1, Bytes::new())
            .with_flags(all_flags);

        let mut buf = BytesMut::new();
        original.encode(&mut buf);

        let decoded = Frame::decode(&mut buf).unwrap().unwrap();
        assert_eq!(decoded.flags, all_flags);
    }

    #[test]
    fn test_header_size_is_16() {
        assert_eq!(HEADER_SIZE, 16);
    }

    #[test]
    fn test_max_payload_size_is_8mb() {
        assert_eq!(MAX_PAYLOAD_SIZE, 8 * 1024 * 1024);
    }

    #[test]
    fn test_magic_bytes() {
        let frame = Frame::ping();
        let mut buf = BytesMut::new();
        frame.encode(&mut buf);

        assert_eq!(buf[0], b'J');
        assert_eq!(buf[1], b'R');
    }

    #[test]
    fn test_protocol_version_in_header() {
        let frame = Frame::ping();
        let mut buf = BytesMut::new();
        frame.encode(&mut buf);

        let version = u16::from_le_bytes([buf[2], buf[3]]);
        assert_eq!(version, PROTOCOL_VERSION);
    }
}
