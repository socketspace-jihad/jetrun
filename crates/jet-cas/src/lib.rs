//! The content-addressed object store.
//!
//! Everything jetrun can skip, transfer, or share flows through here. Two kinds
//! of object live in the store, distinguished only by the domain-separation
//! context used to hash them:
//!
//! * **blobs** -- raw file content ([`jet_core::context::BLOB`]);
//! * **trees** -- directory manifests ([`tree::Tree`], hashed under
//!   [`jet_core::context::TREE`]).
//!
//! Both are immutable and named by their own hash, which is what removes cache
//! invalidation from the design: an address either resolves to exactly the bytes
//! it names, or it is absent.

pub mod link;
pub mod store;
pub mod tree;

pub use link::{Capabilities, Intent, LinkMode};
pub use store::{CasError, Durability, GcStats, MaterializeStats, ObjectStore, blob_digest};
pub use tree::{EntryName, Node, Tree, TreeError};
