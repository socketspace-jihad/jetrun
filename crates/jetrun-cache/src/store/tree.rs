use std::collections::BTreeMap;
use std::fmt;

use jetrun_common::models::digest::{context, Digest};

pub const TREE_FORMAT_VERSION: u16 = 1;

const TAG_FILE: u8 = 1;
const TAG_DIR: u8 = 2;
const TAG_SYMLINK: u8 = 3;

/// Errors that can occur when working with trees and entry names.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TreeError {
    #[error("invalid entry name: {0}")]
    InvalidName(String),
    #[error("unknown tag byte: {0}")]
    UnknownTag(u8),
    #[error("truncated data at offset {0}")]
    TruncatedData(usize),
    #[error("version mismatch: expected {expected}, got {got}")]
    VersionMismatch { expected: u16, got: u16 },
    #[error("too many entries: {0}")]
    TooManyEntries(u32),
}

/// A validated path component (single directory or file name).
///
/// Rejects: empty, ".", "..", contains '/' or '\0', longer than 255 bytes.
#[derive(Clone, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct EntryName(String);

impl EntryName {
    pub fn new(name: &str) -> Result<Self, TreeError> {
        if name.is_empty()
            || name == "."
            || name == ".."
            || name.contains('/')
            || name.contains('\0')
            || name.len() > 255
        {
            return Err(TreeError::InvalidName(name.to_string()));
        }
        Ok(Self(name.to_string()))
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
        write!(f, "EntryName({:?})", self.0)
    }
}

/// A single entry in a tree manifest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Node {
    File {
        digest: Digest,
        size: u64,
        executable: bool,
    },
    Dir {
        digest: Digest,
    },
    Symlink {
        target: String,
    },
}

/// An ordered directory manifest — maps entry names to nodes.
///
/// Encoding is canonical: entries are always sorted by name (byte order),
/// so the same logical tree always encodes to identical bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tree {
    entries: BTreeMap<EntryName, Node>,
}

impl Tree {
    pub fn new() -> Self {
        Self {
            entries: BTreeMap::new(),
        }
    }

    pub fn insert(&mut self, name: EntryName, node: Node) {
        self.entries.insert(name, node);
    }

    pub fn get(&self, name: &EntryName) -> Option<&Node> {
        self.entries.get(name)
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = (&EntryName, &Node)> {
        self.entries.iter()
    }

    /// Canonical binary encoding.
    ///
    /// Layout: version(u16 LE) | entry_count(u32 LE) | entries...
    /// Each entry: tag(u8) | name_len(u16 LE) | name | payload
    pub fn encode(&self) -> Vec<u8> {
        let mut buf = Vec::new();

        // Header
        buf.extend_from_slice(&TREE_FORMAT_VERSION.to_le_bytes());
        buf.extend_from_slice(&(self.entries.len() as u32).to_le_bytes());

        // Entries (BTreeMap iterates in sorted order)
        for (name, node) in &self.entries {
            let name_bytes = name.as_str().as_bytes();

            match node {
                Node::File {
                    digest,
                    size,
                    executable,
                } => {
                    buf.push(TAG_FILE);
                    buf.extend_from_slice(&(name_bytes.len() as u16).to_le_bytes());
                    buf.extend_from_slice(name_bytes);
                    buf.extend_from_slice(digest.as_bytes());
                    buf.extend_from_slice(&size.to_le_bytes());
                    buf.push(if *executable { 1 } else { 0 });
                }
                Node::Dir { digest } => {
                    buf.push(TAG_DIR);
                    buf.extend_from_slice(&(name_bytes.len() as u16).to_le_bytes());
                    buf.extend_from_slice(name_bytes);
                    buf.extend_from_slice(digest.as_bytes());
                }
                Node::Symlink { target } => {
                    let target_bytes = target.as_bytes();
                    buf.push(TAG_SYMLINK);
                    buf.extend_from_slice(&(name_bytes.len() as u16).to_le_bytes());
                    buf.extend_from_slice(name_bytes);
                    buf.extend_from_slice(&(target_bytes.len() as u16).to_le_bytes());
                    buf.extend_from_slice(target_bytes);
                }
            }
        }

        buf
    }

    /// Decode a tree from its canonical binary representation.
    pub fn decode(bytes: &[u8]) -> Result<Self, TreeError> {
        let mut pos = 0;

        // Version
        if bytes.len() < 2 {
            return Err(TreeError::TruncatedData(pos));
        }
        let version = u16::from_le_bytes([bytes[0], bytes[1]]);
        pos += 2;
        if version != TREE_FORMAT_VERSION {
            return Err(TreeError::VersionMismatch {
                expected: TREE_FORMAT_VERSION,
                got: version,
            });
        }

        // Entry count
        if bytes.len() < pos + 4 {
            return Err(TreeError::TruncatedData(pos));
        }
        let entry_count =
            u32::from_le_bytes([bytes[pos], bytes[pos + 1], bytes[pos + 2], bytes[pos + 3]]);
        pos += 4;

        // Sanity limit: 1M entries
        if entry_count > 1_000_000 {
            return Err(TreeError::TooManyEntries(entry_count));
        }

        let mut tree = Tree::new();

        for _ in 0..entry_count {
            // Tag
            if pos >= bytes.len() {
                return Err(TreeError::TruncatedData(pos));
            }
            let tag = bytes[pos];
            pos += 1;

            // Name
            if pos + 2 > bytes.len() {
                return Err(TreeError::TruncatedData(pos));
            }
            let name_len = u16::from_le_bytes([bytes[pos], bytes[pos + 1]]) as usize;
            pos += 2;
            if pos + name_len > bytes.len() {
                return Err(TreeError::TruncatedData(pos));
            }
            let name_str = std::str::from_utf8(&bytes[pos..pos + name_len])
                .map_err(|_| TreeError::InvalidName("<non-utf8>".to_string()))?;
            let name = EntryName::new(name_str)?;
            pos += name_len;

            let node = match tag {
                TAG_FILE => {
                    // digest (32) + size (8) + flags (1) = 41
                    if pos + 41 > bytes.len() {
                        return Err(TreeError::TruncatedData(pos));
                    }
                    let mut digest_bytes = [0u8; 32];
                    digest_bytes.copy_from_slice(&bytes[pos..pos + 32]);
                    pos += 32;
                    let size = u64::from_le_bytes([
                        bytes[pos],
                        bytes[pos + 1],
                        bytes[pos + 2],
                        bytes[pos + 3],
                        bytes[pos + 4],
                        bytes[pos + 5],
                        bytes[pos + 6],
                        bytes[pos + 7],
                    ]);
                    pos += 8;
                    let executable = bytes[pos] & 1 != 0;
                    pos += 1;

                    // Reconstruct digest from raw bytes via hex roundtrip
                    let hex_str = hex::encode(digest_bytes);
                    let digest = Digest::parse(&hex_str)
                        .map_err(|_| TreeError::TruncatedData(pos))?;

                    Node::File {
                        digest,
                        size,
                        executable,
                    }
                }
                TAG_DIR => {
                    if pos + 32 > bytes.len() {
                        return Err(TreeError::TruncatedData(pos));
                    }
                    let mut digest_bytes = [0u8; 32];
                    digest_bytes.copy_from_slice(&bytes[pos..pos + 32]);
                    pos += 32;

                    let hex_str = hex::encode(digest_bytes);
                    let digest = Digest::parse(&hex_str)
                        .map_err(|_| TreeError::TruncatedData(pos))?;

                    Node::Dir { digest }
                }
                TAG_SYMLINK => {
                    if pos + 2 > bytes.len() {
                        return Err(TreeError::TruncatedData(pos));
                    }
                    let target_len =
                        u16::from_le_bytes([bytes[pos], bytes[pos + 1]]) as usize;
                    pos += 2;
                    if pos + target_len > bytes.len() {
                        return Err(TreeError::TruncatedData(pos));
                    }
                    let target = std::str::from_utf8(&bytes[pos..pos + target_len])
                        .map_err(|_| TreeError::InvalidName("<non-utf8 target>".to_string()))?
                        .to_string();
                    pos += target_len;

                    Node::Symlink { target }
                }
                other => return Err(TreeError::UnknownTag(other)),
            };

            tree.insert(name, node);
        }

        Ok(tree)
    }

    /// Compute the tree's digest: `Digest::keyed(context::TREE, &self.encode())`.
    pub fn digest(&self) -> Digest {
        Digest::keyed(context::TREE, &self.encode())
    }

    /// Collect all file blob digests (useful for GC mark phase).
    pub fn blob_digests(&self) -> Vec<Digest> {
        self.entries
            .values()
            .filter_map(|node| match node {
                Node::File { digest, .. } => Some(*digest),
                _ => None,
            })
            .collect()
    }

    /// Collect all subtree (directory) digests (useful for GC mark phase).
    pub fn subtree_digests(&self) -> Vec<Digest> {
        self.entries
            .values()
            .filter_map(|node| match node {
                Node::Dir { digest } => Some(*digest),
                _ => None,
            })
            .collect()
    }
}

impl Default for Tree {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_tree() -> Tree {
        let mut tree = Tree::new();
        tree.insert(
            EntryName::new("main.rs").unwrap(),
            Node::File {
                digest: Digest::of(b"fn main() {}"),
                size: 12,
                executable: false,
            },
        );
        tree.insert(
            EntryName::new("build.sh").unwrap(),
            Node::File {
                digest: Digest::of(b"#!/bin/bash"),
                size: 11,
                executable: true,
            },
        );
        tree.insert(
            EntryName::new("src").unwrap(),
            Node::Dir {
                digest: Digest::of(b"subdir placeholder"),
            },
        );
        tree.insert(
            EntryName::new("link").unwrap(),
            Node::Symlink {
                target: "../other".to_string(),
            },
        );
        tree
    }

    #[test]
    fn test_encode_decode_roundtrip() {
        let tree = sample_tree();
        let encoded = tree.encode();
        let decoded = Tree::decode(&encoded).unwrap();
        assert_eq!(tree, decoded);
    }

    #[test]
    fn test_deterministic_encoding() {
        // Insert in different order, should still produce the same encoding.
        let mut tree1 = Tree::new();
        tree1.insert(
            EntryName::new("b").unwrap(),
            Node::File {
                digest: Digest::of(b"b"),
                size: 1,
                executable: false,
            },
        );
        tree1.insert(
            EntryName::new("a").unwrap(),
            Node::File {
                digest: Digest::of(b"a"),
                size: 1,
                executable: false,
            },
        );

        let mut tree2 = Tree::new();
        tree2.insert(
            EntryName::new("a").unwrap(),
            Node::File {
                digest: Digest::of(b"a"),
                size: 1,
                executable: false,
            },
        );
        tree2.insert(
            EntryName::new("b").unwrap(),
            Node::File {
                digest: Digest::of(b"b"),
                size: 1,
                executable: false,
            },
        );

        assert_eq!(tree1.encode(), tree2.encode());
        assert_eq!(tree1.digest(), tree2.digest());
    }

    #[test]
    fn test_entry_name_validation_dotdot() {
        assert!(EntryName::new("..").is_err());
    }

    #[test]
    fn test_entry_name_validation_dot() {
        assert!(EntryName::new(".").is_err());
    }

    #[test]
    fn test_entry_name_validation_slash() {
        assert!(EntryName::new("foo/bar").is_err());
    }

    #[test]
    fn test_entry_name_validation_nul() {
        assert!(EntryName::new("foo\0bar").is_err());
    }

    #[test]
    fn test_entry_name_validation_empty() {
        assert!(EntryName::new("").is_err());
    }

    #[test]
    fn test_entry_name_validation_too_long() {
        let long_name = "a".repeat(256);
        assert!(EntryName::new(&long_name).is_err());
    }

    #[test]
    fn test_entry_name_valid() {
        assert!(EntryName::new("hello.txt").is_ok());
        assert!(EntryName::new(".hidden").is_ok());
        assert!(EntryName::new("...").is_ok());
    }

    #[test]
    fn test_decode_truncated() {
        // Just a version, no entry count
        let bytes = vec![1, 0];
        assert!(matches!(
            Tree::decode(&bytes),
            Err(TreeError::TruncatedData(_))
        ));
    }

    #[test]
    fn test_decode_version_mismatch() {
        let mut bytes = vec![0; 6];
        bytes[0] = 99; // bad version
        bytes[1] = 0;
        assert!(matches!(
            Tree::decode(&bytes),
            Err(TreeError::VersionMismatch { .. })
        ));
    }

    #[test]
    fn test_decode_unknown_tag() {
        let mut buf = Vec::new();
        buf.extend_from_slice(&1u16.to_le_bytes()); // version
        buf.extend_from_slice(&1u32.to_le_bytes()); // 1 entry
        buf.push(255); // unknown tag
        buf.extend_from_slice(&3u16.to_le_bytes()); // name len
        buf.extend_from_slice(b"foo");
        assert!(matches!(
            Tree::decode(&buf),
            Err(TreeError::UnknownTag(255))
        ));
    }

    #[test]
    fn test_decode_hostile_too_many_entries() {
        let mut buf = Vec::new();
        buf.extend_from_slice(&1u16.to_le_bytes());
        buf.extend_from_slice(&2_000_000u32.to_le_bytes()); // over 1M limit
        assert!(matches!(
            Tree::decode(&buf),
            Err(TreeError::TooManyEntries(_))
        ));
    }

    #[test]
    fn test_domain_separation_tree_vs_blob() {
        let data = sample_tree().encode();
        let tree_digest = Digest::keyed(context::TREE, &data);
        let blob_digest = Digest::keyed(context::BLOB, &data);
        assert_ne!(
            tree_digest, blob_digest,
            "tree digest must differ from blob digest of the same bytes"
        );
    }

    #[test]
    fn test_blob_digests() {
        let tree = sample_tree();
        let blobs = tree.blob_digests();
        assert_eq!(blobs.len(), 2); // main.rs and build.sh
    }

    #[test]
    fn test_subtree_digests() {
        let tree = sample_tree();
        let subs = tree.subtree_digests();
        assert_eq!(subs.len(), 1); // src
    }

    #[test]
    fn test_empty_tree() {
        let tree = Tree::new();
        assert!(tree.is_empty());
        assert_eq!(tree.len(), 0);
        let encoded = tree.encode();
        let decoded = Tree::decode(&encoded).unwrap();
        assert_eq!(tree, decoded);
    }
}
