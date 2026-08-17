//! Directory manifests.
//!
//! A [`Tree`] is a Merkle node: a sorted list of named entries, each pointing at
//! a blob digest, a nested tree digest, or a symlink target. Hashing the
//! canonical encoding gives one digest that names an entire directory
//! structure, which is what lets a workspace be referred to, transferred, and
//! compared as a single value.
//!
//! # Determinism is the whole contract
//!
//! Two machines that ingest identical content must produce byte-identical
//! manifests, or their action keys diverge and the cache never hits across
//! hosts. Everything below serves that:
//!
//! * entries are sorted by **name bytes** -- never by locale collation, which
//!   varies with `LC_COLLATE`;
//! * the encoding is a hand-rolled canonical binary format rather than JSON or
//!   YAML, so no serializer's whitespace, key ordering, or escaping choices can
//!   drift between releases;
//! * only the executable bit of the mode is retained. Full permissions embed the
//!   builder's umask, which makes identical source produce different digests on
//!   different developer machines;
//! * **no timestamps.** mtime is precisely the unreliable signal this system
//!   exists to replace.
//!
//! # Names are untrusted input
//!
//! A tree may arrive over the network from another node or another tenant, and
//! it is then used to create files on disk. An entry named `../../etc/cron.d/x`
//! or a symlink to `/` turns materialization into arbitrary file write -- the
//! Zip Slip class of bug, which has burned essentially every archive extractor
//! ever written. [`EntryName::new`] rejects such names at construction, so an
//! invalid name cannot be represented, and validation cannot be forgotten at a
//! call site.

use std::collections::BTreeMap;
use std::fmt;

use jet_core::{Digest, Hasher, context};

/// Current manifest encoding version.
///
/// Written into every manifest and checked on decode. A future layout change
/// bumps this *and* `context::TREE`, so old and new nodes miss each other's
/// cache entries rather than misinterpreting each other's bytes.
pub const TREE_FORMAT_VERSION: u16 = 1;

const TAG_FILE: u8 = 1;
const TAG_DIR: u8 = 2;
const TAG_SYMLINK: u8 = 3;

const FLAG_EXECUTABLE: u8 = 1 << 0;

/// Maximum length of a single path component, matching Linux `NAME_MAX`.
pub const NAME_MAX: usize = 255;

/// A validated single path component.
///
/// Guaranteed non-empty, free of `/` and NUL, not `.` or `..`, and at most
/// [`NAME_MAX`] bytes. Holding one of these is proof the name is safe to join
/// onto a directory path.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct EntryName(String);

impl EntryName {
    pub fn new(name: impl Into<String>) -> Result<Self, TreeError> {
        let name = name.into();
        if name.is_empty() {
            return Err(TreeError::EmptyName);
        }
        if name.len() > NAME_MAX {
            return Err(TreeError::NameTooLong(name.len()));
        }
        if name == "." || name == ".." {
            return Err(TreeError::DotName(name));
        }
        if name.contains('/') {
            return Err(TreeError::NameHasSeparator(name));
        }
        if name.contains('\0') {
            return Err(TreeError::NameHasNul(name));
        }
        Ok(EntryName(name))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for EntryName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl fmt::Debug for EntryName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self.0)
    }
}

/// What an entry points at.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Node {
    File {
        digest: Digest,
        /// Kept for prefetch sizing and for detecting a truncated object; the
        /// digest remains the authority on content.
        size: u64,
        executable: bool,
    },
    /// A nested directory, named by its own tree digest.
    Dir { digest: Digest },
    /// A symlink. The target is stored verbatim and **never resolved by this
    /// crate**; see [`Tree`] docs on materialization safety.
    Symlink { target: String },
}

impl Node {
    pub fn tag(&self) -> u8 {
        match self {
            Node::File { .. } => TAG_FILE,
            Node::Dir { .. } => TAG_DIR,
            Node::Symlink { .. } => TAG_SYMLINK,
        }
    }
}

/// A directory manifest.
///
/// Entries live in a `BTreeMap`, so iteration is always in sorted name order
/// and the canonical encoding falls out of the data structure rather than
/// depending on the caller remembering to sort.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Tree {
    entries: BTreeMap<EntryName, Node>,
}

impl Tree {
    pub fn new() -> Self {
        Tree::default()
    }

    /// Insert an entry, replacing any existing one with the same name.
    pub fn insert(&mut self, name: EntryName, node: Node) -> Option<Node> {
        self.entries.insert(name, node)
    }

    pub fn get(&self, name: &str) -> Option<&Node> {
        // BTreeMap<EntryName, _> cannot be probed by &str without a Borrow impl
        // that would have to agree with Ord; a linear scan is fine here because
        // lookup-by-name is a debug/CLI path, not the hot materialize path.
        self.entries
            .iter()
            .find(|(k, _)| k.as_str() == name)
            .map(|(_, v)| v)
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Entries in canonical (sorted-by-name-bytes) order.
    pub fn iter(&self) -> impl Iterator<Item = (&EntryName, &Node)> {
        self.entries.iter()
    }

    /// Digests of every blob directly referenced by this tree.
    pub fn blob_digests(&self) -> impl Iterator<Item = Digest> + '_ {
        self.entries.values().filter_map(|n| match n {
            Node::File { digest, .. } => Some(*digest),
            _ => None,
        })
    }

    /// Digests of every subtree directly referenced by this tree.
    pub fn subtree_digests(&self) -> impl Iterator<Item = Digest> + '_ {
        self.entries.values().filter_map(|n| match n {
            Node::Dir { digest } => Some(*digest),
            _ => None,
        })
    }

    /// Encode to the canonical binary form.
    ///
    /// ```text
    /// header  := u16 version, u32 entry_count
    /// entry   := u8 tag, u16 name_len, name bytes, payload
    /// file    := 32-byte digest, u64 size, u8 flags
    /// dir     := 32-byte digest
    /// symlink := u16 target_len, target bytes
    /// ```
    ///
    /// All integers little-endian. Every variable-length field is
    /// length-prefixed, so no pair of distinct trees can encode to the same
    /// bytes.
    pub fn encode(&self) -> Vec<u8> {
        // Rough preallocation: header plus a typical entry.
        let mut out = Vec::with_capacity(6 + self.entries.len() * 64);
        out.extend_from_slice(&TREE_FORMAT_VERSION.to_le_bytes());
        out.extend_from_slice(&(self.entries.len() as u32).to_le_bytes());

        for (name, node) in &self.entries {
            out.push(node.tag());
            let nb = name.as_str().as_bytes();
            out.extend_from_slice(&(nb.len() as u16).to_le_bytes());
            out.extend_from_slice(nb);

            match node {
                Node::File {
                    digest,
                    size,
                    executable,
                } => {
                    out.extend_from_slice(digest.as_bytes());
                    out.extend_from_slice(&size.to_le_bytes());
                    out.push(if *executable { FLAG_EXECUTABLE } else { 0 });
                }
                Node::Dir { digest } => out.extend_from_slice(digest.as_bytes()),
                Node::Symlink { target } => {
                    let tb = target.as_bytes();
                    out.extend_from_slice(&(tb.len() as u16).to_le_bytes());
                    out.extend_from_slice(tb);
                }
            }
        }
        out
    }

    /// Decode from canonical binary form, validating every name.
    pub fn decode(bytes: &[u8]) -> Result<Self, TreeError> {
        let mut r = Reader { b: bytes, at: 0 };

        let version = r.u16()?;
        if version != TREE_FORMAT_VERSION {
            return Err(TreeError::UnsupportedVersion(version));
        }
        let count = r.u32()? as usize;

        // Guard against a hostile count claiming billions of entries: each
        // entry needs at least 4 bytes on the wire, so anything larger than the
        // remaining buffer is a lie and must not drive an allocation.
        if count > r.remaining() {
            return Err(TreeError::Truncated);
        }

        let mut entries = BTreeMap::new();
        for _ in 0..count {
            let tag = r.u8()?;
            let name_len = r.u16()? as usize;
            let name = EntryName::new(
                std::str::from_utf8(r.take(name_len)?).map_err(|_| TreeError::NameNotUtf8)?,
            )?;

            let node = match tag {
                TAG_FILE => {
                    let digest = Digest::from_bytes(
                        r.take(jet_core::DIGEST_LEN)?
                            .try_into()
                            .map_err(|_| TreeError::Truncated)?,
                    );
                    let size = r.u64()?;
                    let flags = r.u8()?;
                    Node::File {
                        digest,
                        size,
                        executable: flags & FLAG_EXECUTABLE != 0,
                    }
                }
                TAG_DIR => Node::Dir {
                    digest: Digest::from_bytes(
                        r.take(jet_core::DIGEST_LEN)?
                            .try_into()
                            .map_err(|_| TreeError::Truncated)?,
                    ),
                },
                TAG_SYMLINK => {
                    let n = r.u16()? as usize;
                    let target = std::str::from_utf8(r.take(n)?)
                        .map_err(|_| TreeError::NameNotUtf8)?
                        .to_owned();
                    if target.is_empty() {
                        return Err(TreeError::EmptySymlinkTarget);
                    }
                    Node::Symlink { target }
                }
                other => return Err(TreeError::UnknownTag(other)),
            };

            if entries.insert(name.clone(), node).is_some() {
                // Duplicates would make the manifest ambiguous and let two
                // different trees share a digest.
                return Err(TreeError::DuplicateName(name.to_string()));
            }
        }

        if !r.is_exhausted() {
            return Err(TreeError::TrailingBytes(r.remaining()));
        }
        Ok(Tree { entries })
    }

    /// This tree's digest: a keyed hash over the canonical encoding.
    ///
    /// The `context::TREE` domain separation means a tree digest can never
    /// collide with the digest of a blob that happens to hold the same bytes, so
    /// a blob lookup can never be satisfied by a manifest or vice versa.
    pub fn digest(&self) -> Digest {
        let mut h = Hasher::keyed(context::TREE);
        h.update(&self.encode());
        h.finalize()
    }
}

/// Minimal bounds-checked cursor. Every read is length-validated, because the
/// input may be hostile.
struct Reader<'a> {
    b: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], TreeError> {
        let end = self.at.checked_add(n).ok_or(TreeError::Truncated)?;
        let s = self.b.get(self.at..end).ok_or(TreeError::Truncated)?;
        self.at = end;
        Ok(s)
    }
    fn u8(&mut self) -> Result<u8, TreeError> {
        Ok(self.take(1)?[0])
    }
    fn u16(&mut self) -> Result<u16, TreeError> {
        Ok(u16::from_le_bytes(self.take(2)?.try_into().unwrap()))
    }
    fn u32(&mut self) -> Result<u32, TreeError> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    fn u64(&mut self) -> Result<u64, TreeError> {
        Ok(u64::from_le_bytes(self.take(8)?.try_into().unwrap()))
    }
    fn remaining(&self) -> usize {
        self.b.len().saturating_sub(self.at)
    }
    fn is_exhausted(&self) -> bool {
        self.at == self.b.len()
    }
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum TreeError {
    #[error("entry name is empty")]
    EmptyName,
    #[error("entry name is {0} bytes, limit is {NAME_MAX}")]
    NameTooLong(usize),
    #[error("entry name {0:?} is a path-traversal component")]
    DotName(String),
    #[error("entry name {0:?} contains a path separator")]
    NameHasSeparator(String),
    #[error("entry name {0:?} contains a NUL byte")]
    NameHasNul(String),
    #[error("entry name is not valid UTF-8")]
    NameNotUtf8,
    #[error("duplicate entry name {0:?}")]
    DuplicateName(String),
    #[error("symlink target is empty")]
    EmptySymlinkTarget,
    #[error("unsupported tree format version {0}")]
    UnsupportedVersion(u16),
    #[error("unknown node tag {0}")]
    UnknownTag(u8),
    #[error("manifest is truncated")]
    Truncated,
    #[error("{0} trailing bytes after manifest")]
    TrailingBytes(usize),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn name(s: &str) -> EntryName {
        EntryName::new(s).unwrap()
    }

    fn file(seed: &[u8], size: u64, exec: bool) -> Node {
        Node::File {
            digest: Digest::of(seed),
            size,
            executable: exec,
        }
    }

    fn sample() -> Tree {
        let mut t = Tree::new();
        t.insert(name("main.rs"), file(b"main", 120, false));
        t.insert(name("build.sh"), file(b"build", 44, true));
        t.insert(
            name("src"),
            Node::Dir {
                digest: Digest::of(b"subtree"),
            },
        );
        t.insert(
            name("link"),
            Node::Symlink {
                target: "main.rs".into(),
            },
        );
        t
    }

    // --- name validation: the path-traversal guard ---

    #[test]
    fn rejects_traversal_names() {
        for bad in ["..", ".", "../etc/passwd", "a/b", "/abs", "nul\0byte", ""] {
            assert!(
                EntryName::new(bad).is_err(),
                "must reject entry name {bad:?}"
            );
        }
    }

    #[test]
    fn rejects_overlong_name() {
        assert!(EntryName::new("x".repeat(NAME_MAX)).is_ok());
        assert!(matches!(
            EntryName::new("x".repeat(NAME_MAX + 1)),
            Err(TreeError::NameTooLong(_))
        ));
    }

    #[test]
    fn accepts_awkward_but_legal_names() {
        // Leading dots, spaces and unicode are legal filenames and must survive.
        for ok in [".hidden", "..leading", "a b", "ünïcode", "-", "name.tar.gz"] {
            assert!(EntryName::new(ok).is_ok(), "should accept {ok:?}");
        }
    }

    #[test]
    fn decode_rejects_traversal_name_from_the_wire() {
        // The important case: a hostile manifest, not a local mistake. We build
        // the bytes by hand because the type system forbids constructing this
        // tree in the first place.
        let mut b = Vec::new();
        b.extend_from_slice(&TREE_FORMAT_VERSION.to_le_bytes());
        b.extend_from_slice(&1u32.to_le_bytes());
        b.push(TAG_FILE);
        let n = b"..";
        b.extend_from_slice(&(n.len() as u16).to_le_bytes());
        b.extend_from_slice(n);
        b.extend_from_slice(Digest::of(b"x").as_bytes());
        b.extend_from_slice(&0u64.to_le_bytes());
        b.push(0);

        assert!(matches!(Tree::decode(&b), Err(TreeError::DotName(_))));
    }

    // --- canonical encoding ---

    #[test]
    fn encode_decode_roundtrip() {
        let t = sample();
        assert_eq!(Tree::decode(&t.encode()).unwrap(), t);
    }

    #[test]
    fn encoding_is_insertion_order_independent() {
        // Two ingests that walk a directory in different orders (readdir order
        // is not stable) must still produce identical bytes, or the same content
        // gets two digests and the cache never hits.
        let mut a = Tree::new();
        a.insert(name("z"), file(b"z", 1, false));
        a.insert(name("a"), file(b"a", 2, false));
        a.insert(name("m"), file(b"m", 3, false));

        let mut b = Tree::new();
        b.insert(name("m"), file(b"m", 3, false));
        b.insert(name("z"), file(b"z", 1, false));
        b.insert(name("a"), file(b"a", 2, false));

        assert_eq!(a.encode(), b.encode());
        assert_eq!(a.digest(), b.digest());
    }

    #[test]
    fn entries_iterate_in_byte_order() {
        let mut t = Tree::new();
        for n in ["b", "A", "a", "B", "_", "1"] {
            t.insert(name(n), file(n.as_bytes(), 0, false));
        }
        let got: Vec<&str> = t.iter().map(|(k, _)| k.as_str()).collect();
        let mut want = got.clone();
        want.sort_by_key(|s| s.as_bytes());
        assert_eq!(got, want, "iteration must follow raw byte order");
    }

    #[test]
    fn digest_is_stable_across_calls() {
        let t = sample();
        assert_eq!(t.digest(), t.digest());
        assert_eq!(t.digest(), Tree::decode(&t.encode()).unwrap().digest());
    }

    #[test]
    fn executable_bit_changes_the_digest() {
        // A script that loses +x is a broken build, so the bit must be part of
        // the identity.
        let mut a = Tree::new();
        a.insert(name("s.sh"), file(b"same", 10, false));
        let mut b = Tree::new();
        b.insert(name("s.sh"), file(b"same", 10, true));
        assert_ne!(a.digest(), b.digest());
    }

    #[test]
    fn node_kind_changes_the_digest() {
        let d = Digest::of(b"same-bytes");
        let mut as_file = Tree::new();
        as_file.insert(
            name("x"),
            Node::File {
                digest: d,
                size: 0,
                executable: false,
            },
        );
        let mut as_dir = Tree::new();
        as_dir.insert(name("x"), Node::Dir { digest: d });
        assert_ne!(as_file.digest(), as_dir.digest());
    }

    #[test]
    fn name_boundaries_are_unambiguous() {
        // Without length-prefixed names, {"ab": _} and {"a": _, "b": _} could
        // encode alike. This is the framing property, checked on real trees.
        let mut one = Tree::new();
        one.insert(name("ab"), file(b"x", 0, false));
        let mut two = Tree::new();
        two.insert(name("a"), file(b"x", 0, false));
        two.insert(name("b"), file(b"x", 0, false));
        assert_ne!(one.encode(), two.encode());
        assert_ne!(one.digest(), two.digest());
    }

    #[test]
    fn empty_tree_has_a_digest() {
        let t = Tree::new();
        assert!(t.is_empty());
        assert_eq!(Tree::decode(&t.encode()).unwrap(), t);
        assert!(!t.digest().is_zero());
    }

    // --- hostile input ---

    #[test]
    fn rejects_truncated_manifest() {
        let enc = sample().encode();
        for cut in 1..enc.len() {
            assert!(
                Tree::decode(&enc[..cut]).is_err(),
                "truncation at {cut} must not decode"
            );
        }
    }

    #[test]
    fn rejects_trailing_bytes() {
        let mut enc = sample().encode();
        enc.push(0xff);
        assert!(matches!(
            Tree::decode(&enc),
            Err(TreeError::TrailingBytes(1))
        ));
    }

    #[test]
    fn rejects_absurd_entry_count_without_allocating() {
        // A hostile header claiming 4 billion entries must not drive a
        // multi-gigabyte reservation.
        let mut b = Vec::new();
        b.extend_from_slice(&TREE_FORMAT_VERSION.to_le_bytes());
        b.extend_from_slice(&u32::MAX.to_le_bytes());
        assert!(matches!(Tree::decode(&b), Err(TreeError::Truncated)));
    }

    #[test]
    fn rejects_unknown_version_and_tag() {
        let mut b = Vec::new();
        b.extend_from_slice(&999u16.to_le_bytes());
        b.extend_from_slice(&0u32.to_le_bytes());
        assert!(matches!(
            Tree::decode(&b),
            Err(TreeError::UnsupportedVersion(999))
        ));

        let mut c = Vec::new();
        c.extend_from_slice(&TREE_FORMAT_VERSION.to_le_bytes());
        c.extend_from_slice(&1u32.to_le_bytes());
        c.push(77); // unknown tag
        c.extend_from_slice(&1u16.to_le_bytes());
        c.push(b'x');
        assert!(matches!(Tree::decode(&c), Err(TreeError::UnknownTag(77))));
    }

    #[test]
    fn rejects_empty_symlink_target() {
        let mut b = Vec::new();
        b.extend_from_slice(&TREE_FORMAT_VERSION.to_le_bytes());
        b.extend_from_slice(&1u32.to_le_bytes());
        b.push(TAG_SYMLINK);
        b.extend_from_slice(&1u16.to_le_bytes());
        b.push(b'l');
        b.extend_from_slice(&0u16.to_le_bytes());
        assert!(matches!(
            Tree::decode(&b),
            Err(TreeError::EmptySymlinkTarget)
        ));
    }

    #[test]
    fn reports_child_digests() {
        let t = sample();
        assert_eq!(t.blob_digests().count(), 2);
        assert_eq!(t.subtree_digests().count(), 1);
    }
}
