use std::fmt;

use serde::{Deserialize, Serialize};

/// Domain-separation contexts for keyed hashing.
pub mod context {
    pub const BLOB: &str = "jetrun.blob.v1";
    pub const TREE: &str = "jetrun.tree.v1";
}

/// A 32-byte blake3 digest with domain-separation support.
///
/// Used throughout the cache layer to identify blobs, trees, and other
/// content-addressed objects. The keyed constructor ensures that identical
/// bytes hashed under different contexts produce distinct digests.
#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct Digest([u8; 32]);

impl Digest {
    /// Compute a plain blake3 hash of `bytes`.
    pub fn of(bytes: &[u8]) -> Self {
        let hash = blake3::hash(bytes);
        Self(*hash.as_bytes())
    }

    /// Compute a domain-separated hash.
    ///
    /// Uses `blake3::derive_key` to produce a 32-byte key from `context`,
    /// then hashes `bytes` with that key. Two calls with the same `bytes`
    /// but different `context` strings will produce distinct digests.
    pub fn keyed(context: &str, bytes: &[u8]) -> Self {
        let key = blake3::derive_key(context, bytes);
        Self(key)
    }

    /// Return the first 2 hex characters — used for directory fanout / sharding.
    pub fn fanout(&self) -> String {
        hex::encode(&self.0[..1])
    }

    /// Return the full 64-character lowercase hex string.
    pub fn to_hex(&self) -> String {
        hex::encode(self.0)
    }

    /// Parse a 64-character hex string into a `Digest`.
    pub fn parse(hex_str: &str) -> Result<Self, DigestParseError> {
        let bytes = hex::decode(hex_str).map_err(|_| DigestParseError::InvalidHex)?;
        if bytes.len() != 32 {
            return Err(DigestParseError::WrongLength(bytes.len()));
        }
        let mut arr = [0u8; 32];
        arr.copy_from_slice(&bytes);
        Ok(Self(arr))
    }

    /// Access the raw 32 bytes.
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

/// Errors that can occur when parsing a hex string into a `Digest`.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DigestParseError {
    #[error("invalid hex encoding")]
    InvalidHex,
    #[error("expected 32 bytes, got {0}")]
    WrongLength(usize),
}

impl fmt::Debug for Digest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Digest({})", self.to_hex())
    }
}

impl fmt::Display for Digest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_hex())
    }
}

impl Serialize for Digest {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_hex())
    }
}

impl<'de> Deserialize<'de> for Digest {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        Digest::parse(&s).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_of_deterministic() {
        let d1 = Digest::of(b"hello world");
        let d2 = Digest::of(b"hello world");
        assert_eq!(d1, d2);
    }

    #[test]
    fn test_of_different_input() {
        let d1 = Digest::of(b"hello");
        let d2 = Digest::of(b"world");
        assert_ne!(d1, d2);
    }

    #[test]
    fn test_keyed_deterministic() {
        let d1 = Digest::keyed(context::BLOB, b"data");
        let d2 = Digest::keyed(context::BLOB, b"data");
        assert_eq!(d1, d2);
    }

    #[test]
    fn test_domain_separation() {
        let blob = Digest::keyed(context::BLOB, b"same bytes");
        let tree = Digest::keyed(context::TREE, b"same bytes");
        assert_ne!(blob, tree, "same bytes under different contexts must differ");
    }

    #[test]
    fn test_hex_roundtrip() {
        let original = Digest::of(b"roundtrip test");
        let hex = original.to_hex();
        assert_eq!(hex.len(), 64);
        let parsed = Digest::parse(&hex).unwrap();
        assert_eq!(original, parsed);
    }

    #[test]
    fn test_fanout() {
        let d = Digest::of(b"fanout");
        let f = d.fanout();
        assert_eq!(f.len(), 2);
        // fanout must match the first 2 hex chars of the full hex string
        assert_eq!(f, &d.to_hex()[..2]);
    }

    #[test]
    fn test_parse_invalid_hex() {
        assert!(Digest::parse("not-hex").is_err());
    }

    #[test]
    fn test_parse_wrong_length() {
        // valid hex but only 4 bytes
        assert!(Digest::parse("deadbeef").is_err());
    }

    #[test]
    fn test_serde_roundtrip() {
        let d = Digest::of(b"serde test");
        let json = serde_json::to_string(&d).unwrap();
        let d2: Digest = serde_json::from_str(&json).unwrap();
        assert_eq!(d, d2);
    }

    #[test]
    fn test_display() {
        let d = Digest::of(b"display");
        assert_eq!(format!("{}", d), d.to_hex());
    }
}
