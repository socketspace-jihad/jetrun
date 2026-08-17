//! Content digests.
//!
//! Everything jetrun caches, transfers, or skips is named by one of these. A
//! digest is 32 bytes of BLAKE3 over the content, which buys three properties
//! the rest of the system leans on hard:
//!
//! * the name is a *proof* of the content, so corruption cannot pass silently;
//! * the name is immutable, so there is no cache invalidation -- only lookup;
//! * the bits are uniformly distributed, which makes the digest a free and
//!   perfectly balanced shard key (see [`Digest::shard`]).

use std::fmt;
use std::str::FromStr;

use serde::de::{self, Visitor};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// Length of a digest in bytes.
pub const DIGEST_LEN: usize = 32;

/// Human-facing prefix, so a digest is recognizable in logs and CLI output.
pub const DIGEST_PREFIX: &str = "b3:";

/// A BLAKE3 content digest.
///
/// `Ord` is derived and is byte-lexicographic. That matters: action keys are
/// built from a *sorted* set of observed inputs, and the sort must be stable
/// across machines and across releases or identical work would produce
/// different keys. Byte order over a fixed-width array is the only ordering
/// that cannot drift.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
#[cfg_attr(
    feature = "rkyv",
    derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)
)]
pub struct Digest([u8; DIGEST_LEN]);

impl Digest {
    /// The all-zero digest. Used as an explicit "absent" marker; it is not a
    /// valid hash of any content we would store.
    pub const ZERO: Digest = Digest([0u8; DIGEST_LEN]);

    /// Hash a byte slice.
    #[inline]
    pub fn of(bytes: &[u8]) -> Self {
        Digest(*blake3::hash(bytes).as_bytes())
    }

    /// Hash a byte slice with a domain-separation context.
    ///
    /// Use this for anything that is *not* raw content -- action keys, step
    /// identities, tree manifests. Without domain separation an action key
    /// could collide with the digest of a blob that happens to hold the same
    /// bytes, and a lookup in one namespace could be answered from the other.
    /// BLAKE3's keyed derivation makes the two namespaces provably disjoint.
    #[inline]
    pub fn keyed(context: &str, bytes: &[u8]) -> Self {
        let key = blake3::derive_key(context, &[]);
        Digest(*blake3::keyed_hash(&key, bytes).as_bytes())
    }

    #[inline]
    pub const fn from_bytes(bytes: [u8; DIGEST_LEN]) -> Self {
        Digest(bytes)
    }

    #[inline]
    pub const fn as_bytes(&self) -> &[u8; DIGEST_LEN] {
        &self.0
    }

    /// Not constant-time on purpose: digests are public data and this sits on a
    /// hot path. Constant-time comparison of secrets lives in `jet-authz`.
    #[inline]
    pub fn is_zero(&self) -> bool {
        self.0 == [0u8; DIGEST_LEN]
    }

    /// Lowercase hex, no prefix. This is the on-disk object filename.
    pub fn to_hex(&self) -> String {
        hex::encode(self.0)
    }

    /// Hex with the `b3:` prefix, for logs and user-facing output.
    pub fn to_prefixed(&self) -> String {
        format!("{DIGEST_PREFIX}{}", self.to_hex())
    }

    /// First byte as two hex chars -- the fan-out directory in the object
    /// store (`objects/ab/<full-hex>`).
    ///
    /// One byte gives 256 directories. On ext4 with dir_index that keeps
    /// per-directory entry counts reasonable well past a million objects,
    /// and it keeps the path short enough to stay under PATH_MAX when nested
    /// inside a long workspace root.
    pub fn fanout(&self) -> [u8; 2] {
        const HEX: &[u8; 16] = b"0123456789abcdef";
        [
            HEX[(self.0[0] >> 4) as usize],
            HEX[(self.0[0] & 0x0f) as usize],
        ]
    }

    /// Which shard owns this digest, given `n_shards`.
    ///
    /// The digest is already uniformly random, so this needs no mixing and no
    /// rebalancing: every shard gets an equal share by construction, and a
    /// given digest always routes to the same shard on every node. That is why
    /// the CAS can be shard-per-core with no cross-shard coordination and no
    /// locks -- a shard exclusively owns its slice of the keyspace.
    ///
    /// Panics if `n_shards` is zero.
    #[inline]
    pub fn shard(&self, n_shards: usize) -> usize {
        assert!(n_shards > 0, "n_shards must be non-zero");
        let head = u64::from_le_bytes([
            self.0[0], self.0[1], self.0[2], self.0[3], self.0[4], self.0[5], self.0[6], self.0[7],
        ]);
        if n_shards.is_power_of_two() {
            // Cheaper than a division, and exact because the input is uniform.
            (head as usize) & (n_shards - 1)
        } else {
            (head % n_shards as u64) as usize
        }
    }

    /// Parse from hex, with or without the `b3:` prefix.
    pub fn parse(s: &str) -> Result<Self, DigestParseError> {
        let hexpart = s.strip_prefix(DIGEST_PREFIX).unwrap_or(s);
        if hexpart.len() != DIGEST_LEN * 2 {
            return Err(DigestParseError::BadLength {
                expected: DIGEST_LEN * 2,
                found: hexpart.len(),
            });
        }
        let mut out = [0u8; DIGEST_LEN];
        hex::decode_to_slice(hexpart, &mut out).map_err(|_| DigestParseError::NotHex)?;
        Ok(Digest(out))
    }
}

/// Streaming hasher, for content too large to hold in memory.
///
/// Wraps BLAKE3's own tree hasher, so `update` may be called with arbitrary
/// chunk boundaries and still yields the same digest as [`Digest::of`] over the
/// concatenation.
#[derive(Clone, Default)]
pub struct Hasher(blake3::Hasher);

impl Hasher {
    #[inline]
    pub fn new() -> Self {
        Hasher(blake3::Hasher::new())
    }

    /// Streaming equivalent of [`Digest::keyed`].
    #[inline]
    pub fn keyed(context: &str) -> Self {
        let key = blake3::derive_key(context, &[]);
        Hasher(blake3::Hasher::new_keyed(&key))
    }

    #[inline]
    pub fn update(&mut self, bytes: &[u8]) -> &mut Self {
        self.0.update(bytes);
        self
    }

    /// Hash `bytes` using all available cores.
    ///
    /// Worth it above roughly 128 KiB; below that the rayon dispatch costs
    /// more than the hashing saves. Callers should check the threshold rather
    /// than reaching for this unconditionally.
    #[inline]
    pub fn update_rayon(&mut self, bytes: &[u8]) -> &mut Self {
        #[cfg(feature = "rayon")]
        {
            self.0.update_rayon(bytes);
        }
        #[cfg(not(feature = "rayon"))]
        {
            self.0.update(bytes);
        }
        self
    }

    /// Feed a length-prefixed field.
    ///
    /// Composite keys (action keys, tree manifests) must never be built by
    /// concatenating raw fields: `("ab", "c")` and `("a", "bc")` would hash
    /// identically and two different actions would share a cache entry. The
    /// length prefix makes the encoding unambiguous.
    #[inline]
    pub fn update_framed(&mut self, bytes: &[u8]) -> &mut Self {
        self.0.update(&(bytes.len() as u64).to_le_bytes());
        self.0.update(bytes);
        self
    }

    #[inline]
    pub fn finalize(&self) -> Digest {
        Digest(*self.0.finalize().as_bytes())
    }
}

impl fmt::Display for Digest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_hex())
    }
}

impl fmt::Debug for Digest {
    /// Truncated, because full digests make logs unreadable.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let h = self.to_hex();
        write!(f, "{DIGEST_PREFIX}{}…", &h[..12])
    }
}

impl FromStr for Digest {
    type Err = DigestParseError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Digest::parse(s)
    }
}

impl Serialize for Digest {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        // Hex rather than bytes: digests end up in JSON APIs, SQLite text
        // columns, and YAML lock files, and a hex string is greppable in all
        // three.
        s.serialize_str(&self.to_hex())
    }
}

impl<'de> Deserialize<'de> for Digest {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct V;
        impl Visitor<'_> for V {
            type Value = Digest;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a 64-character hex blake3 digest, optionally `b3:`-prefixed")
            }
            fn visit_str<E: de::Error>(self, v: &str) -> Result<Digest, E> {
                Digest::parse(v).map_err(de::Error::custom)
            }
        }
        d.deserialize_str(V)
    }
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum DigestParseError {
    #[error("digest must be {expected} hex chars, got {found}")]
    BadLength { expected: usize, found: usize },
    #[error("digest is not valid hex")]
    NotHex,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_roundtrip() {
        let d = Digest::of(b"hello");
        assert_eq!(Digest::parse(&d.to_hex()).unwrap(), d);
        assert_eq!(Digest::parse(&d.to_prefixed()).unwrap(), d);
    }

    #[test]
    fn rejects_malformed() {
        assert!(matches!(
            Digest::parse("abc"),
            Err(DigestParseError::BadLength { .. })
        ));
        assert_eq!(
            Digest::parse(&"z".repeat(64)),
            Err(DigestParseError::NotHex)
        );
    }

    #[test]
    fn streaming_matches_oneshot() {
        // Chunk boundaries must not affect the result, or a large file hashed
        // in 64 KiB reads would not match the same file hashed in one shot.
        let data: Vec<u8> = (0..300_000u32).map(|i| (i % 251) as u8).collect();
        let oneshot = Digest::of(&data);
        for chunk in [1usize, 7, 1024, 65536, 299_999] {
            let mut h = Hasher::new();
            for part in data.chunks(chunk) {
                h.update(part);
            }
            assert_eq!(h.finalize(), oneshot, "chunk size {chunk}");
        }
    }

    #[test]
    fn domain_separation_is_real() {
        // The whole point: same bytes, different namespace, different digest.
        let raw = Digest::of(b"payload");
        let a = Digest::keyed("jetrun.action.v1", b"payload");
        let b = Digest::keyed("jetrun.tree.v1", b"payload");
        assert_ne!(raw, a);
        assert_ne!(a, b);
    }

    #[test]
    fn framing_prevents_field_ambiguity() {
        // Without length prefixes these two would collide and two distinct
        // actions would share one cache entry.
        let mut x = Hasher::new();
        x.update_framed(b"ab").update_framed(b"c");
        let mut y = Hasher::new();
        y.update_framed(b"a").update_framed(b"bc");
        assert_ne!(x.finalize(), y.finalize());
    }

    #[test]
    fn shard_distribution_is_even() {
        // A skewed shard key would silently serialize the CAS onto one core.
        const SHARDS: usize = 8;
        let mut counts = [0usize; SHARDS];
        let n = 100_000;
        for i in 0..n {
            counts[Digest::of(&(i as u64).to_le_bytes()).shard(SHARDS)] += 1;
        }
        let expected = n / SHARDS;
        for (i, c) in counts.iter().enumerate() {
            let dev = (*c as f64 - expected as f64).abs() / expected as f64;
            assert!(dev < 0.05, "shard {i} deviates {:.1}%", dev * 100.0);
        }
    }

    #[test]
    fn shard_is_stable_and_in_range() {
        let d = Digest::of(b"stable");
        for n in [1usize, 3, 4, 7, 8, 16, 64] {
            let s = d.shard(n);
            assert!(s < n);
            assert_eq!(s, d.shard(n), "must be deterministic");
        }
    }

    #[test]
    fn fanout_matches_hex_head() {
        let d = Digest::of(b"fanout");
        let f = d.fanout();
        assert_eq!(
            std::str::from_utf8(&f).unwrap(),
            &d.to_hex()[..2],
            "fanout dir must be the first two hex chars of the object name"
        );
    }

    #[test]
    fn zero_is_zero() {
        assert!(Digest::ZERO.is_zero());
        assert!(!Digest::of(b"").is_zero());
    }

    #[test]
    fn serde_is_hex_string() {
        let d = Digest::of(b"serde");
        let json = serde_json::to_string(&d).unwrap();
        assert_eq!(json, format!("\"{}\"", d.to_hex()));
        assert_eq!(serde_json::from_str::<Digest>(&json).unwrap(), d);
    }
}
