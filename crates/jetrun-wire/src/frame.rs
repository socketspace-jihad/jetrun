use bytes::{Buf, BufMut, Bytes, BytesMut};

use crate::error::WireError;

/// Maximum payload size: 64MB (sufficient for cache blobs)
pub const MAX_PAYLOAD_SIZE: u32 = 64 * 1024 * 1024;

/// Frame header size: 8 bytes
pub const HEADER_SIZE: usize = 8;

/// Message type identifiers
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageType {
    Request = 0x01,
    Response = 0x02,
    StreamStart = 0x03,
    StreamData = 0x04,
    StreamEnd = 0x05,
    Error = 0x06,
    Ping = 0x07,
    Pong = 0x08,
}

impl TryFrom<u8> for MessageType {
    type Error = WireError;

    fn try_from(v: u8) -> Result<Self, WireError> {
        match v {
            0x01 => Ok(Self::Request),
            0x02 => Ok(Self::Response),
            0x03 => Ok(Self::StreamStart),
            0x04 => Ok(Self::StreamData),
            0x05 => Ok(Self::StreamEnd),
            0x06 => Ok(Self::Error),
            0x07 => Ok(Self::Ping),
            0x08 => Ok(Self::Pong),
            _ => Err(WireError::InvalidFrame(format!(
                "unknown message type: 0x{:02x}",
                v
            ))),
        }
    }
}

bitflags::bitflags! {
    /// Frame flags
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct Flags: u8 {
        /// Payload is LZ4-compressed
        const COMPRESSED = 0b0000_0001;
        /// Last frame in a stream
        const LAST_FRAME = 0b0000_0010;
        /// Requires acknowledgment
        const NEEDS_ACK  = 0b0000_0100;
    }
}

/// Wire frame header + payload
///
/// ```text
/// ┌──────────┬───────┬──────────────┬────────────┬─────────────┐
/// │ msg_type │ flags │ payload_len  │ request_id │   payload   │
/// │  1 byte  │ 1 byte│   4 bytes LE │  2 bytes LE│  N bytes    │
/// └──────────┴───────┴──────────────┴────────────┴─────────────┘
/// ```
#[derive(Debug, Clone)]
pub struct Frame {
    pub msg_type: MessageType,
    pub flags: Flags,
    pub request_id: u16,
    pub payload: Bytes,
}

impl Frame {
    pub fn new(msg_type: MessageType, request_id: u16, payload: Bytes) -> Self {
        Self {
            msg_type,
            flags: Flags::empty(),
            request_id,
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
        dst.put_u8(self.msg_type as u8);
        dst.put_u8(self.flags.bits());
        dst.put_u32_le(self.payload.len() as u32);
        dst.put_u16_le(self.request_id);
        dst.extend_from_slice(&self.payload);
    }

    /// Decode a frame from bytes. Returns None if not enough data.
    pub fn decode(src: &mut BytesMut) -> Result<Option<Frame>, WireError> {
        if src.len() < HEADER_SIZE {
            return Ok(None); // Not enough data for header
        }

        // Peek at payload length without advancing
        let payload_len =
            u32::from_le_bytes([src[2], src[3], src[4], src[5]]) as usize;

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
        let msg_type = MessageType::try_from(src[0])?;
        let flags = Flags::from_bits_truncate(src[1]);
        // skip [2..6] payload_len already read
        let request_id = u16::from_le_bytes([src[6], src[7]]);

        src.advance(HEADER_SIZE);
        let payload = src.split_to(payload_len).freeze();

        Ok(Some(Frame {
            msg_type,
            flags,
            request_id,
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

        assert_eq!(buf.len(), HEADER_SIZE + 11); // 8 + "hello world"

        let decoded = Frame::decode(&mut buf).unwrap().unwrap();
        assert_eq!(decoded.msg_type, MessageType::Request);
        assert_eq!(decoded.request_id, 42);
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
        let mut buf = BytesMut::from(&[0x01, 0x00][..]);
        assert!(Frame::decode(&mut buf).unwrap().is_none());
    }

    #[test]
    fn test_decode_incomplete_payload() {
        let mut buf = BytesMut::new();
        buf.put_u8(0x01); // msg_type
        buf.put_u8(0x00); // flags
        buf.put_u32_le(100); // payload_len = 100
        buf.put_u16_le(1); // request_id
        buf.extend_from_slice(&[0u8; 50]); // only 50 bytes of payload

        assert!(Frame::decode(&mut buf).unwrap().is_none());
    }

    #[test]
    fn test_decode_payload_too_large() {
        let mut buf = BytesMut::new();
        buf.put_u8(0x01);
        buf.put_u8(0x00);
        buf.put_u32_le(MAX_PAYLOAD_SIZE + 1);
        buf.put_u16_le(1);

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
        buf.put_u8(0xFF); // invalid
        buf.put_u8(0x00);
        buf.put_u32_le(0);
        buf.put_u16_le(0);

        assert!(Frame::decode(&mut buf).is_err());
    }
}
