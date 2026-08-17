//! The on-disk object store.
//!
//! ```text
//! <root>/objects/<ab>/<64-hex>   immutable blobs and tree manifests, mode 0444
//! <root>/tmp/                    staging area for atomic insert
//! ```
//!
//! # Insert is atomic, and dedupe is free
//!
//! An object is written to `tmp/`, sealed read-only, then `rename`d into its
//! final path. `rename` within a filesystem is atomic, so a reader never
//! observes a partial object. If the destination already exists the insert is
//! simply dropped -- identical content has identical bytes, so there is nothing
//! to reconcile and nothing to lock. That is the property that makes concurrent
//! ingest from many shards safe without any coordination at all.
//!
//! # Durability is deliberately weak by default
//!
//! [`Durability::Relaxed`] skips `fsync`. This looks reckless and is not: the
//! store is a **cache**, and every object in it can be recreated by re-running
//! the step that produced it. Trading a possible post-crash rebuild for the
//! removal of one `fsync` per object is clearly correct at 50,000 files per
//! workspace, where fsync would dominate the entire ingest. Metadata that
//! genuinely cannot be recomputed lives in `jet-store`, which is fully durable.

use std::fs;
use std::io;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use jet_core::{Digest, Hasher, context};

use crate::link::{self, Capabilities, Intent, LinkMode, OBJECT_MODE};
use crate::tree::{EntryName, Node, Tree, TreeError};

/// Files at or below this size are hashed via a single read; larger ones are
/// streamed. 1 MiB comfortably covers source files and most object files while
/// keeping peak memory per shard bounded.
const SMALL_FILE_LIMIT: u64 = 1 << 20;

/// Streaming read chunk. 256 KiB is large enough to amortize syscall overhead
/// and small enough to stay inside L2 on a per-shard basis.
const STREAM_CHUNK: usize = 256 << 10;

/// How hard to try to survive a crash.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Durability {
    /// No `fsync`. A crash may lose recently written objects; they get
    /// recreated on the next run. The correct default for a cache.
    #[default]
    Relaxed,
    /// `fsync` each object before rename and `fsync` the parent directory
    /// after, so a completed insert survives power loss. Roughly an order of
    /// magnitude slower on rotational media and noticeably slower on NVMe.
    /// Worth it only where the store is also the artifact archive of record.
    Synced,
}

/// Counters from a materialization, so a deployment can tell whether it is
/// actually getting cheap placement.
///
/// A run where `copied` dominates has silently lost most of the CAS's
/// advantage -- the usual cause is a store and a workspace on different
/// filesystems, or a filesystem without reflink support.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MaterializeStats {
    pub reflinked: u64,
    pub hardlinked: u64,
    pub copied: u64,
    pub dirs: u64,
    pub symlinks: u64,
    pub bytes: u64,
}

impl MaterializeStats {
    pub fn files(&self) -> u64 {
        self.reflinked + self.hardlinked + self.copied
    }

    fn record(&mut self, mode: LinkMode, size: u64) {
        match mode {
            LinkMode::Reflink => self.reflinked += 1,
            LinkMode::Hardlink => self.hardlinked += 1,
            LinkMode::Copy => self.copied += 1,
        }
        self.bytes += size;
    }

    /// Fraction of files placed without duplicating bytes.
    pub fn cheap_ratio(&self) -> f64 {
        let f = self.files();
        if f == 0 {
            return 1.0;
        }
        (self.reflinked + self.hardlinked) as f64 / f as f64
    }
}

/// Counters from a garbage collection pass.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct GcStats {
    pub live: u64,
    pub deleted: u64,
    pub bytes_freed: u64,
    /// Unreferenced but too young to delete; see [`ObjectStore::gc`].
    pub spared_young: u64,
}

pub struct ObjectStore {
    root: PathBuf,
    objects: PathBuf,
    tmp: PathBuf,
    caps: Capabilities,
    durability: Durability,
}

impl ObjectStore {
    /// Open (creating if absent) a store rooted at `root`.
    ///
    /// Probes reflink support once here rather than per file, so an ext4 store
    /// does not pay a guaranteed-to-fail ioctl 50,000 times per workspace.
    pub fn open(root: impl Into<PathBuf>) -> Result<Self, CasError> {
        Self::open_with(root, Durability::default())
    }

    pub fn open_with(root: impl Into<PathBuf>, durability: Durability) -> Result<Self, CasError> {
        let root = root.into();
        let objects = root.join("objects");
        let tmp = root.join("tmp");
        fs::create_dir_all(&objects).map_err(|e| CasError::io("create objects dir", &objects, e))?;
        fs::create_dir_all(&tmp).map_err(|e| CasError::io("create tmp dir", &tmp, e))?;

        let caps = Capabilities::new();
        // A probe failure is not fatal -- fall back to assuming no reflink.
        if caps.probe(&tmp).is_err() {
            let _ = caps.reflink_supported();
        }

        Ok(ObjectStore {
            root,
            objects,
            tmp,
            caps,
            durability,
        })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn capabilities(&self) -> &Capabilities {
        &self.caps
    }

    /// Absolute path an object occupies, whether or not it exists.
    pub fn object_path(&self, d: &Digest) -> PathBuf {
        let fan = d.fanout();
        // Build the path without a format! allocation on the hot path.
        let mut p = self.objects.clone();
        p.push(std::str::from_utf8(&fan).expect("fanout is ascii hex"));
        p.push(d.to_hex());
        p
    }

    pub fn has(&self, d: &Digest) -> bool {
        self.object_path(d).exists()
    }

    /// Which of `digests` are missing.
    ///
    /// This is the server side of transfer negotiation: a peer sends the digests
    /// it wants to push, we answer with the subset we lack, and only those bytes
    /// cross the wire.
    pub fn missing(&self, digests: &[Digest]) -> Vec<Digest> {
        digests
            .iter()
            .filter(|d| !self.has(d))
            .copied()
            .collect()
    }

    // ---------------------------------------------------------------- blobs

    /// Insert bytes as a blob, returning its digest.
    pub fn put_bytes(&self, bytes: &[u8]) -> Result<Digest, CasError> {
        let digest = blob_digest(bytes);
        if self.has(&digest) {
            return Ok(digest); // dedupe: nothing to do
        }
        self.insert_staged(&digest, |f| {
            use io::Write;
            f.write_all(bytes)
        })?;
        Ok(digest)
    }

    /// Insert an existing file's content as a blob.
    ///
    /// Hashes first, then places the bytes -- via reflink where the filesystem
    /// supports it, so ingesting a large artifact costs one read pass and no
    /// write.
    ///
    /// Note the [`Intent::PrivateInode`]: the store must **not** hardlink the
    /// caller's file. The source here is typically a developer's working tree or
    /// a build output directory, either of which may be modified moments later.
    /// A hardlink would mean that edit rewrites the store object in place,
    /// leaving an object whose content no longer matches the digest naming it --
    /// and sealing the object to 0444 would additionally chmod the user's own
    /// source file read-only.
    pub fn put_file(&self, path: &Path) -> Result<(Digest, u64), CasError> {
        let meta =
            fs::symlink_metadata(path).map_err(|e| CasError::io("stat for ingest", path, e))?;
        if !meta.is_file() {
            return Err(CasError::NotAFile(path.to_path_buf()));
        }
        let size = meta.len();
        let digest = hash_file(path, size)?;

        if self.has(&digest) {
            return Ok((digest, size));
        }

        // Try to place it without moving bytes.
        let staged = self.staging_path(&digest);
        let placed = link::materialize(path, &staged, Intent::PrivateInode, &self.caps);
        match placed {
            Ok(_) => {
                self.seal_and_commit(&digest, &staged)?;
                Ok((digest, size))
            }
            Err(_) => {
                // Fall back to a plain streaming copy.
                let _ = fs::remove_file(&staged);
                let mut src =
                    fs::File::open(path).map_err(|e| CasError::io("open for ingest", path, e))?;
                self.insert_staged(&digest, |dst| io::copy(&mut src, dst).map(|_| ()))?;
                Ok((digest, size))
            }
        }
    }

    /// Read a blob.
    pub fn get_bytes(&self, d: &Digest) -> Result<Vec<u8>, CasError> {
        let p = self.object_path(d);
        match fs::read(&p) {
            Ok(b) => Ok(b),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Err(CasError::Missing(*d)),
            Err(e) => Err(CasError::io("read object", &p, e)),
        }
    }

    /// Read a blob and verify its content actually hashes to `d`.
    ///
    /// The normal read path trusts the filename, because re-hashing every object
    /// on every read would negate the point of a cache. Use this from `fsck` and
    /// when accepting objects from an untrusted peer -- there, the digest is a
    /// claim and must be checked before it is believed.
    pub fn get_bytes_verified(&self, d: &Digest) -> Result<Vec<u8>, CasError> {
        let bytes = self.get_bytes(d)?;
        let actual = blob_digest(&bytes);
        if actual != *d {
            return Err(CasError::Corrupt {
                expected: *d,
                actual,
            });
        }
        Ok(bytes)
    }

    // ---------------------------------------------------------------- trees

    pub fn put_tree(&self, tree: &Tree) -> Result<Digest, CasError> {
        let encoded = tree.encode();
        let digest = tree.digest();
        if self.has(&digest) {
            return Ok(digest);
        }
        self.insert_staged(&digest, |f| {
            use io::Write;
            f.write_all(&encoded)
        })?;
        Ok(digest)
    }

    pub fn get_tree(&self, d: &Digest) -> Result<Tree, CasError> {
        let bytes = self.get_bytes(d)?;
        let tree = Tree::decode(&bytes)?;
        // Cheap integrity check: a manifest whose digest does not match is
        // either corrupt or from a different format version, and materializing
        // it would produce a workspace nobody can reproduce.
        let actual = tree.digest();
        if actual != *d {
            return Err(CasError::Corrupt {
                expected: *d,
                actual,
            });
        }
        Ok(tree)
    }

    // ------------------------------------------------------------- ingest

    /// Recursively ingest a directory, returning its tree digest.
    ///
    /// Symlinks are recorded as symlinks and never followed: following them
    /// would let a link into `/` pull the host filesystem into the store, and
    /// would turn a symlink cycle into an infinite walk.
    pub fn ingest_dir(&self, dir: &Path) -> Result<Digest, CasError> {
        let tree = self.ingest_dir_inner(dir)?;
        self.put_tree(&tree)
    }

    fn ingest_dir_inner(&self, dir: &Path) -> Result<Tree, CasError> {
        let mut tree = Tree::new();
        let rd = fs::read_dir(dir).map_err(|e| CasError::io("read_dir", dir, e))?;

        for entry in rd {
            let entry = entry.map_err(|e| CasError::io("read_dir entry", dir, e))?;
            let path = entry.path();
            let name = entry.file_name();
            let name = name
                .to_str()
                .ok_or_else(|| CasError::NonUtf8Name(path.clone()))?;
            let name = EntryName::new(name)?;

            // symlink_metadata, not metadata: we must observe the link itself.
            let meta = fs::symlink_metadata(&path)
                .map_err(|e| CasError::io("stat entry", &path, e))?;
            let ft = meta.file_type();

            let node = if ft.is_symlink() {
                let target = fs::read_link(&path)
                    .map_err(|e| CasError::io("read_link", &path, e))?
                    .to_str()
                    .ok_or_else(|| CasError::NonUtf8Name(path.clone()))?
                    .to_owned();
                Node::Symlink { target }
            } else if ft.is_dir() {
                let sub = self.ingest_dir_inner(&path)?;
                Node::Dir {
                    digest: self.put_tree(&sub)?,
                }
            } else if ft.is_file() {
                let (digest, size) = self.put_file(&path)?;
                Node::File {
                    digest,
                    size,
                    // Only the owner-execute bit is retained; see tree docs on
                    // why full modes are not.
                    executable: meta.permissions().mode() & 0o100 != 0,
                }
            } else {
                // Sockets, fifos, device nodes. A build that depends on one is
                // not reproducible, so refusing is more useful than silently
                // dropping it and producing a workspace that differs from what
                // was ingested.
                return Err(CasError::UnsupportedFileType(path));
            };

            tree.insert(name, node);
        }
        Ok(tree)
    }

    // --------------------------------------------------------- materialize

    /// Write the tree named by `d` into `dest`.
    ///
    /// `dest` must not already exist -- it is created here. Requiring a fresh
    /// destination is a safety property, not an inconvenience: materializing
    /// over existing content could write *through* a symlink left behind by a
    /// previous run and escape the workspace.
    pub fn materialize_tree(
        &self,
        d: &Digest,
        dest: &Path,
        intent: Intent,
    ) -> Result<MaterializeStats, CasError> {
        if fs::symlink_metadata(dest).is_ok() {
            return Err(CasError::DestinationExists(dest.to_path_buf()));
        }
        let mut stats = MaterializeStats::default();
        self.materialize_into(d, dest, intent, &mut stats, 0)?;
        Ok(stats)
    }

    fn materialize_into(
        &self,
        d: &Digest,
        dest: &Path,
        intent: Intent,
        stats: &mut MaterializeStats,
        depth: usize,
    ) -> Result<(), CasError> {
        // A malformed or hostile tree graph could nest arbitrarily deep; bound it
        // rather than overflowing the stack.
        const MAX_DEPTH: usize = 128;
        if depth > MAX_DEPTH {
            return Err(CasError::TreeTooDeep(MAX_DEPTH));
        }

        fs::create_dir(dest).map_err(|e| CasError::io("create dir", dest, e))?;
        stats.dirs += 1;

        let tree = self.get_tree(d)?;
        for (name, node) in tree.iter() {
            // Safe by construction: EntryName cannot contain `/`, `.` or `..`,
            // so this join cannot leave `dest`.
            let child = dest.join(name.as_str());
            match node {
                Node::File {
                    digest,
                    size,
                    executable,
                } => {
                    let src = self.object_path(digest);
                    if !src.exists() {
                        return Err(CasError::Missing(*digest));
                    }

                    // Mode is a property of the *inode*, but the executable bit
                    // is a property of the *tree entry* -- the same blob can be
                    // a script in one tree and plain data in another, which is
                    // exactly why the bit lives in the manifest and not in the
                    // content. A hardlink shares the store object's single 0444
                    // inode and therefore cannot carry a per-entry mode, so an
                    // executable entry must get an inode of its own even when
                    // the caller would otherwise allow sharing. Cheap in
                    // practice: almost nothing in a source tree is +x.
                    let place = if *executable {
                        Intent::PrivateInode
                    } else {
                        intent
                    };

                    let mode = link::materialize(&src, &child, place, &self.caps)
                        .map_err(|e| CasError::io("materialize file", &child, e))?;
                    stats.record(mode, *size);

                    // link::materialize deliberately sets no mode, so apply the
                    // policy here -- but only when we own the inode. Touching a
                    // hardlinked file's mode would rewrite the store object.
                    if mode != LinkMode::Hardlink {
                        let want = match (intent, *executable) {
                            // Writable workspace or export.
                            (Intent::PrivateInode, false) => 0o644,
                            (Intent::PrivateInode, true) => 0o755,
                            // Overlay lowerdir: read-only is correct, since any
                            // write copies up into the upperdir anyway.
                            (Intent::OverlayLower, false) => 0o444,
                            (Intent::OverlayLower, true) => 0o555,
                        };
                        let mut p = fs::metadata(&child)
                            .map_err(|e| CasError::io("stat for chmod", &child, e))?
                            .permissions();
                        p.set_mode(want);
                        fs::set_permissions(&child, p)
                            .map_err(|e| CasError::io("chmod", &child, e))?;
                    }
                }
                Node::Dir { digest } => {
                    self.materialize_into(digest, &child, intent, stats, depth + 1)?;
                }
                Node::Symlink { target } => {
                    // The target is written verbatim and never resolved. A
                    // dangling or escaping link is reproduced exactly as
                    // ingested; what makes that safe is that we never *write
                    // through* a symlink, because every directory in the path
                    // was created by the loop above.
                    std::os::unix::fs::symlink(target, &child)
                        .map_err(|e| CasError::io("symlink", &child, e))?;
                    stats.symlinks += 1;
                }
            }
        }
        Ok(())
    }

    // ------------------------------------------------------------------ gc

    /// Mark-and-sweep from `roots` (tree digests).
    ///
    /// `min_age` guards the fundamental race: an object written by an in-flight
    /// step is unreachable from any root until that step's tree is committed, so
    /// a naive sweep would delete objects out from under a running build. Sparing
    /// anything younger than `min_age` closes that window without needing a lock
    /// between GC and every writer. Set it comfortably above the longest step
    /// runtime; an hour is a reasonable default.
    pub fn gc(&self, roots: &[Digest], min_age: Duration) -> Result<GcStats, CasError> {
        let mut live = std::collections::HashSet::new();
        let mut queue: Vec<Digest> = roots.to_vec();

        while let Some(d) = queue.pop() {
            if !live.insert(d) {
                continue;
            }
            // A root may name a blob rather than a tree, and a tree may
            // reference an object that was already collected. Neither is fatal
            // during a sweep -- treat undecodable objects as leaves.
            if let Ok(tree) = self.get_tree(&d) {
                for b in tree.blob_digests() {
                    live.insert(b);
                }
                for s in tree.subtree_digests() {
                    if !live.contains(&s) {
                        queue.push(s);
                    }
                }
            }
        }

        let now = SystemTime::now();
        let mut stats = GcStats {
            live: live.len() as u64,
            ..Default::default()
        };

        for fan in fs::read_dir(&self.objects)
            .map_err(|e| CasError::io("read objects dir", &self.objects, e))?
        {
            let fan = fan.map_err(|e| CasError::io("read objects dir entry", &self.objects, e))?;
            if !fan.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                continue;
            }
            let fan_path = fan.path();
            for obj in
                fs::read_dir(&fan_path).map_err(|e| CasError::io("read fanout", &fan_path, e))?
            {
                let obj = obj.map_err(|e| CasError::io("read fanout entry", &fan_path, e))?;
                let path = obj.path();
                let Some(digest) = path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .and_then(|n| Digest::parse(n).ok())
                else {
                    continue; // not an object; leave it alone
                };
                if live.contains(&digest) {
                    continue;
                }

                let meta = match obj.metadata() {
                    Ok(m) => m,
                    Err(_) => continue,
                };
                let age = meta
                    .modified()
                    .ok()
                    .and_then(|m| now.duration_since(m).ok())
                    .unwrap_or(Duration::ZERO);
                if age < min_age {
                    stats.spared_young += 1;
                    continue;
                }

                let size = meta.len();
                match fs::remove_file(&path) {
                    Ok(()) => {
                        stats.deleted += 1;
                        stats.bytes_freed += size;
                    }
                    Err(e) if e.kind() == io::ErrorKind::NotFound => {}
                    Err(e) => return Err(CasError::io("unlink object", &path, e)),
                }
            }
        }
        Ok(stats)
    }

    // -------------------------------------------------------------- internals

    fn staging_path(&self, d: &Digest) -> PathBuf {
        // Digest-named so two shards inserting the same object cannot collide on
        // the staging path either -- and if they do, they are writing identical
        // bytes.
        self.tmp.join(format!("{}.staging", d.to_hex()))
    }

    /// Run `write` against a fresh staging file, then commit it atomically.
    fn insert_staged<F>(&self, digest: &Digest, write: F) -> Result<(), CasError>
    where
        F: FnOnce(&mut fs::File) -> io::Result<()>,
    {
        let staged = self.staging_path(digest);
        {
            let mut f = fs::OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .open(&staged)
                .map_err(|e| CasError::io("create staging file", &staged, e))?;
            write(&mut f).map_err(|e| CasError::io("write staging file", &staged, e))?;
            if self.durability == Durability::Synced {
                f.sync_all()
                    .map_err(|e| CasError::io("fsync staging file", &staged, e))?;
            }
        }
        self.seal_and_commit(digest, &staged)
    }

    /// Make `staged` read-only and rename it into its final object path.
    fn seal_and_commit(&self, digest: &Digest, staged: &Path) -> Result<(), CasError> {
        let mut perms = fs::metadata(staged)
            .map_err(|e| CasError::io("stat staging file", staged, e))?
            .permissions();
        perms.set_mode(OBJECT_MODE);
        fs::set_permissions(staged, perms)
            .map_err(|e| CasError::io("seal staging file", staged, e))?;

        let final_path = self.object_path(digest);
        let parent = final_path.parent().expect("object path has a parent");
        fs::create_dir_all(parent).map_err(|e| CasError::io("create fanout dir", parent, e))?;

        // rename is atomic within a filesystem, so a concurrent reader sees
        // either no object or the complete one -- never a partial write.
        match fs::rename(staged, &final_path) {
            Ok(()) => {}
            Err(e) => {
                let _ = fs::remove_file(staged);
                return Err(CasError::io("commit object", &final_path, e));
            }
        }

        if self.durability == Durability::Synced {
            // The rename itself needs a directory fsync to be durable; without
            // this the file contents survive a crash but the name may not.
            if let Ok(dir) = fs::File::open(parent) {
                let _ = dir.sync_all();
            }
        }
        Ok(())
    }
}

/// Digest of blob content, in the blob namespace.
pub fn blob_digest(bytes: &[u8]) -> Digest {
    let mut h = Hasher::keyed(context::BLOB);
    h.update(bytes);
    h.finalize()
}

/// Hash a file, reading it whole when small and streaming when not.
fn hash_file(path: &Path, size: u64) -> Result<Digest, CasError> {
    if size <= SMALL_FILE_LIMIT {
        let bytes = fs::read(path).map_err(|e| CasError::io("read for hashing", path, e))?;
        return Ok(blob_digest(&bytes));
    }

    use io::Read;
    let mut f = fs::File::open(path).map_err(|e| CasError::io("open for hashing", path, e))?;
    let mut h = Hasher::keyed(context::BLOB);
    let mut buf = vec![0u8; STREAM_CHUNK];
    loop {
        let n = f
            .read(&mut buf)
            .map_err(|e| CasError::io("read for hashing", path, e))?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(h.finalize())
}

#[derive(Debug, thiserror::Error)]
pub enum CasError {
    #[error("{op} {path}: {source}")]
    Io {
        op: &'static str,
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("object {0:?} is not in the store")]
    Missing(Digest),
    #[error("object corrupt: named {expected:?} but content hashes to {actual:?}")]
    Corrupt { expected: Digest, actual: Digest },
    #[error("{0} is not a regular file")]
    NotAFile(PathBuf),
    #[error("{0} has an unsupported file type (socket, fifo or device)")]
    UnsupportedFileType(PathBuf),
    #[error("{0} has a non-UTF-8 name")]
    NonUtf8Name(PathBuf),
    #[error("destination {0} already exists")]
    DestinationExists(PathBuf),
    #[error("tree nesting exceeds {0} levels")]
    TreeTooDeep(usize),
    #[error(transparent)]
    Tree(#[from] TreeError),
}

impl CasError {
    fn io(op: &'static str, path: &Path, source: io::Error) -> Self {
        CasError::Io {
            op,
            path: path.to_path_buf(),
            source,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> (tempfile::TempDir, ObjectStore) {
        let d = tempfile::tempdir().unwrap();
        let s = ObjectStore::open(d.path().join("cas")).unwrap();
        (d, s)
    }

    #[test]
    fn put_get_roundtrip() {
        let (_d, s) = store();
        let digest = s.put_bytes(b"hello world").unwrap();
        assert_eq!(s.get_bytes(&digest).unwrap(), b"hello world");
        assert!(s.has(&digest));
    }

    #[test]
    fn identical_content_dedupes() {
        let (_d, s) = store();
        let a = s.put_bytes(b"same").unwrap();
        let b = s.put_bytes(b"same").unwrap();
        assert_eq!(a, b);

        // One object on disk, not two -- this is the property that makes the
        // store shrink rather than grow as more of the org uses it.
        let count = fs::read_dir(&s.objects)
            .unwrap()
            .flat_map(|f| fs::read_dir(f.unwrap().path()).unwrap())
            .count();
        assert_eq!(count, 1);
    }

    #[test]
    fn objects_are_sealed_read_only() {
        let (_d, s) = store();
        let digest = s.put_bytes(b"immutable").unwrap();
        let mode = fs::metadata(s.object_path(&digest))
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, OBJECT_MODE, "objects must not be writable");
    }

    #[test]
    fn missing_object_is_reported_not_panicked() {
        let (_d, s) = store();
        let ghost = Digest::of(b"never stored");
        assert!(!s.has(&ghost));
        assert!(matches!(s.get_bytes(&ghost), Err(CasError::Missing(_))));
    }

    #[test]
    fn blob_and_tree_namespaces_do_not_collide() {
        // A blob whose bytes happen to be a valid manifest must not be readable
        // as a tree, and vice versa. Domain separation is what guarantees it.
        let (_d, s) = store();
        let mut t = Tree::new();
        t.insert(
            EntryName::new("f").unwrap(),
            Node::File {
                digest: Digest::of(b"x"),
                size: 1,
                executable: false,
            },
        );
        let tree_digest = s.put_tree(&t).unwrap();
        let blob_digest_of_same_bytes = s.put_bytes(&t.encode()).unwrap();
        assert_ne!(tree_digest, blob_digest_of_same_bytes);
    }

    #[test]
    fn detects_corruption_on_verified_read() {
        let (_d, s) = store();
        let digest = s.put_bytes(b"trustworthy").unwrap();
        let path = s.object_path(&digest);

        // Simulate bit rot / a tampering peer.
        let mut perms = fs::metadata(&path).unwrap().permissions();
        perms.set_mode(0o644);
        fs::set_permissions(&path, perms).unwrap();
        fs::write(&path, b"tampered!!!").unwrap();

        // The fast path trusts the name...
        assert_eq!(s.get_bytes(&digest).unwrap(), b"tampered!!!");
        // ...and the verified path catches it.
        assert!(matches!(
            s.get_bytes_verified(&digest),
            Err(CasError::Corrupt { .. })
        ));
    }

    #[test]
    fn missing_reports_only_absent_digests() {
        let (_d, s) = store();
        let have = s.put_bytes(b"present").unwrap();
        let lack = Digest::of(b"absent");
        assert_eq!(s.missing(&[have, lack]), vec![lack]);
        assert!(s.missing(&[have]).is_empty());
    }

    #[test]
    fn large_file_streams_to_same_digest_as_small_path() {
        let (d, s) = store();
        // Cross SMALL_FILE_LIMIT so the streaming branch is exercised.
        let data: Vec<u8> = (0..(SMALL_FILE_LIMIT as usize + 4096))
            .map(|i| (i % 251) as u8)
            .collect();
        let p = d.path().join("big.bin");
        fs::write(&p, &data).unwrap();

        let (digest, size) = s.put_file(&p).unwrap();
        assert_eq!(size, data.len() as u64);
        assert_eq!(digest, blob_digest(&data), "streaming must match one-shot");
        assert_eq!(s.get_bytes(&digest).unwrap(), data);
    }

    // ------------------------------------------------- ingest / materialize

    fn fixture(root: &Path) {
        fs::create_dir_all(root.join("src/nested")).unwrap();
        fs::write(root.join("Cargo.toml"), b"[package]").unwrap();
        fs::write(root.join("src/main.rs"), b"fn main(){}").unwrap();
        fs::write(root.join("src/nested/deep.rs"), b"// deep").unwrap();
        fs::write(root.join("build.sh"), b"#!/bin/sh\n").unwrap();
        fs::set_permissions(root.join("build.sh"), fs::Permissions::from_mode(0o755)).unwrap();
        std::os::unix::fs::symlink("src/main.rs", root.join("link-to-main")).unwrap();
    }

    #[test]
    fn ingest_then_materialize_reproduces_the_tree() {
        let (d, s) = store();
        let src = d.path().join("proj");
        fixture(&src);

        let digest = s.ingest_dir(&src).unwrap();
        let out = d.path().join("out");
        let stats = s
            .materialize_tree(&digest, &out, Intent::PrivateInode)
            .unwrap();

        assert_eq!(fs::read(out.join("Cargo.toml")).unwrap(), b"[package]");
        assert_eq!(fs::read(out.join("src/main.rs")).unwrap(), b"fn main(){}");
        assert_eq!(fs::read(out.join("src/nested/deep.rs")).unwrap(), b"// deep");
        assert_eq!(stats.files(), 4);
        assert_eq!(stats.symlinks, 1);
        assert_eq!(stats.dirs, 3); // out, src, src/nested
    }

    #[test]
    fn ingest_is_idempotent_and_content_addressed() {
        let (d, s) = store();
        let a = d.path().join("a");
        let b = d.path().join("b");
        fixture(&a);
        fixture(&b);
        // Two separate directories with identical content must yield one digest.
        assert_eq!(s.ingest_dir(&a).unwrap(), s.ingest_dir(&b).unwrap());
    }

    #[test]
    fn a_single_byte_change_changes_the_tree_digest() {
        let (d, s) = store();
        let a = d.path().join("a");
        fixture(&a);
        let before = s.ingest_dir(&a).unwrap();
        fs::write(a.join("src/main.rs"), b"fn main(){/*!*/}").unwrap();
        let after = s.ingest_dir(&a).unwrap();
        assert_ne!(before, after, "content change must change the digest");
    }

    #[test]
    fn executable_bit_survives_a_roundtrip() {
        let (d, s) = store();
        let src = d.path().join("proj");
        fixture(&src);
        let digest = s.ingest_dir(&src).unwrap();
        let out = d.path().join("out");
        s.materialize_tree(&digest, &out, Intent::PrivateInode).unwrap();

        let mode = fs::metadata(out.join("build.sh")).unwrap().permissions().mode();
        assert!(mode & 0o100 != 0, "build.sh lost its executable bit");
        let plain = fs::metadata(out.join("Cargo.toml")).unwrap().permissions().mode();
        assert!(plain & 0o111 == 0, "Cargo.toml should not be executable");
    }

    // --- regressions: ingest must not alias the caller's files ---

    #[test]
    fn ingest_leaves_the_source_tree_writable() {
        // Regression: put_file used to hardlink the source into the store, and
        // sealing objects to 0444 then chmod'd the developer's own working tree
        // read-only. Their next edit failed with EACCES.
        let (d, s) = store();
        let src = d.path().join("proj");
        fixture(&src);
        s.ingest_dir(&src).unwrap();

        for f in ["Cargo.toml", "src/main.rs", "build.sh"] {
            let mode = fs::metadata(src.join(f)).unwrap().permissions().mode();
            assert!(
                mode & 0o200 != 0,
                "{f} was left read-only by ingest (mode {mode:o})"
            );
            fs::write(src.join(f), b"edited after ingest")
                .unwrap_or_else(|e| panic!("cannot rewrite {f} after ingest: {e}"));
        }
    }

    #[test]
    fn editing_a_source_file_after_ingest_cannot_corrupt_the_store() {
        // The silent half of the same bug, and the more dangerous one: with a
        // hardlink, an in-place edit rewrites the store object, leaving content
        // that no longer matches the digest naming it. Every later step that
        // materialized that digest would get the wrong bytes.
        use std::os::unix::fs::MetadataExt;
        let (d, s) = store();
        let src = d.path().join("proj");
        fs::create_dir_all(&src).unwrap();
        let f = src.join("main.rs");
        fs::write(&f, b"original").unwrap();

        let (digest, _) = s.put_file(&f).unwrap();
        let obj = s.object_path(&digest);
        assert_ne!(
            fs::metadata(&f).unwrap().ino(),
            fs::metadata(&obj).unwrap().ino(),
            "store object must not alias the ingested file"
        );

        fs::write(&f, b"mutated!").unwrap();

        assert_eq!(s.get_bytes(&digest).unwrap(), b"original");
        // And the object still verifies against its own name.
        s.get_bytes_verified(&digest)
            .expect("store object was corrupted by an edit to the source file");
    }

    #[test]
    fn overlay_lower_files_are_read_only_but_present() {
        let (d, s) = store();
        let src = d.path().join("proj");
        fixture(&src);
        let digest = s.ingest_dir(&src).unwrap();
        let out = d.path().join("lower");
        s.materialize_tree(&digest, &out, Intent::OverlayLower).unwrap();

        let mode = fs::metadata(out.join("Cargo.toml")).unwrap().permissions().mode();
        assert_eq!(mode & 0o222, 0, "lowerdir entries should not be writable");
        assert_eq!(fs::read(out.join("Cargo.toml")).unwrap(), b"[package]");
    }

    #[test]
    fn executable_bit_is_correct_even_under_overlay_lower() {
        // The store keeps one 0444 inode per blob, so an executable entry cannot
        // be a hardlink -- mode is per-inode while the +x bit is per tree entry.
        let (d, s) = store();
        let src = d.path().join("proj");
        fixture(&src);
        let digest = s.ingest_dir(&src).unwrap();
        let out = d.path().join("lower");
        s.materialize_tree(&digest, &out, Intent::OverlayLower).unwrap();

        let mode = fs::metadata(out.join("build.sh")).unwrap().permissions().mode();
        assert!(
            mode & 0o111 != 0,
            "build.sh must stay executable in a lowerdir (mode {mode:o})"
        );

        // ...and the shared store object must not have been chmod'd to get there.
        let tree = s.get_tree(&digest).unwrap();
        for blob in tree.blob_digests() {
            let m = fs::metadata(s.object_path(&blob)).unwrap().permissions().mode() & 0o777;
            assert_eq!(m, OBJECT_MODE, "a store object had its mode rewritten");
        }
    }

    #[test]
    fn workspace_files_are_writable_so_builds_can_run() {
        // Store objects are 0444 and fs::copy carries modes across, so without an
        // explicit mode policy every materialized workspace would be read-only
        // and every build would fail.
        let (d, s) = store();
        let src = d.path().join("proj");
        fixture(&src);
        let digest = s.ingest_dir(&src).unwrap();
        let out = d.path().join("ws");
        s.materialize_tree(&digest, &out, Intent::PrivateInode).unwrap();

        fs::write(out.join("src/main.rs"), b"fn main(){ /* edited */ }")
            .expect("workspace file must be writable");
        // The store is unaffected.
        s.get_tree(&digest).unwrap();
    }

    #[test]
    fn symlinks_are_recorded_not_followed() {
        let (d, s) = store();
        let src = d.path().join("proj");
        fs::create_dir_all(&src).unwrap();
        // A link pointing outside the tree: following it would suck /etc into
        // the store.
        std::os::unix::fs::symlink("/etc/passwd", src.join("escape")).unwrap();

        let digest = s.ingest_dir(&src).unwrap();
        let out = d.path().join("out");
        s.materialize_tree(&digest, &out, Intent::PrivateInode).unwrap();

        let meta = fs::symlink_metadata(out.join("escape")).unwrap();
        assert!(meta.file_type().is_symlink());
        assert_eq!(
            fs::read_link(out.join("escape")).unwrap(),
            Path::new("/etc/passwd"),
            "target must be reproduced verbatim"
        );
    }

    #[test]
    fn materialize_refuses_an_existing_destination() {
        let (d, s) = store();
        let src = d.path().join("proj");
        fixture(&src);
        let digest = s.ingest_dir(&src).unwrap();

        let out = d.path().join("out");
        fs::create_dir(&out).unwrap();
        assert!(matches!(
            s.materialize_tree(&digest, &out, Intent::PrivateInode),
            Err(CasError::DestinationExists(_))
        ));
    }

    #[test]
    fn materialize_fails_loudly_when_a_blob_is_missing() {
        let (d, s) = store();
        let src = d.path().join("proj");
        fixture(&src);
        let digest = s.ingest_dir(&src).unwrap();

        // Evict one blob, as an over-eager GC would.
        let tree = s.get_tree(&digest).unwrap();
        let victim = tree.blob_digests().next().unwrap();
        fs::remove_file(s.object_path(&victim)).unwrap();

        // Silently producing a workspace with a missing file would be far worse
        // than failing.
        assert!(matches!(
            s.materialize_tree(&digest, &d.path().join("out"), Intent::PrivateInode),
            Err(CasError::Missing(_))
        ));
    }

    #[test]
    fn writable_materialization_never_shares_inodes_with_the_store() {
        use std::os::unix::fs::MetadataExt;
        let (d, s) = store();
        let src = d.path().join("proj");
        fixture(&src);
        let digest = s.ingest_dir(&src).unwrap();
        let out = d.path().join("out");
        s.materialize_tree(&digest, &out, Intent::PrivateInode).unwrap();

        let tree = s.get_tree(&digest).unwrap();
        for blob in tree.blob_digests() {
            let store_ino = fs::metadata(s.object_path(&blob)).unwrap().ino();
            for f in ["Cargo.toml", "build.sh"] {
                let ino = fs::metadata(out.join(f)).unwrap().ino();
                assert_ne!(ino, store_ino, "{f} aliases a store object");
            }
        }
    }

    #[test]
    fn rejects_unsupported_file_types() {
        let (d, s) = store();
        let src = d.path().join("proj");
        fs::create_dir_all(&src).unwrap();
        let fifo = src.join("pipe");
        // mkfifo via rustix; if it is not permitted here, the check is moot.
        if rustix::fs::mknodat(
            rustix::fs::CWD,
            &fifo,
            rustix::fs::FileType::Fifo,
            rustix::fs::Mode::from_raw_mode(0o644),
            0,
        )
        .is_err()
        {
            return;
        }
        assert!(matches!(
            s.ingest_dir(&src),
            Err(CasError::UnsupportedFileType(_))
        ));
    }

    // ------------------------------------------------------------------ gc

    #[test]
    fn gc_keeps_reachable_and_deletes_the_rest() {
        let (d, s) = store();
        let src = d.path().join("proj");
        fixture(&src);
        let root = s.ingest_dir(&src).unwrap();

        let orphan = s.put_bytes(b"nobody references me").unwrap();
        assert!(s.has(&orphan));

        // min_age zero: sweep everything unreferenced immediately.
        let stats = s.gc(&[root], Duration::ZERO).unwrap();
        assert_eq!(stats.deleted, 1);
        assert!(!s.has(&orphan));

        // Everything reachable from the root must have survived, transitively.
        let out = d.path().join("out");
        s.materialize_tree(&root, &out, Intent::PrivateInode)
            .expect("gc must not have broken the live tree");
    }

    #[test]
    fn gc_spares_young_objects() {
        // The race this closes: an in-flight step's outputs are unreachable
        // until its tree is committed. Deleting them would corrupt a live build.
        let (_d, s) = store();
        let inflight = s.put_bytes(b"just written by a running step").unwrap();

        let stats = s.gc(&[], Duration::from_secs(3600)).unwrap();
        assert_eq!(stats.deleted, 0);
        assert_eq!(stats.spared_young, 1);
        assert!(s.has(&inflight), "young unreferenced object must survive");
    }

    #[test]
    fn gc_on_empty_store_is_a_noop() {
        let (_d, s) = store();
        let stats = s.gc(&[], Duration::ZERO).unwrap();
        assert_eq!(stats, GcStats::default());
    }

    #[test]
    fn stats_report_placement_quality() {
        let (d, s) = store();
        let src = d.path().join("proj");
        fixture(&src);
        let digest = s.ingest_dir(&src).unwrap();

        let stats = s
            .materialize_tree(&digest, &d.path().join("lower"), Intent::OverlayLower)
            .unwrap();

        // The fixture holds 4 files, one of them executable. Under a lowerdir the
        // 3 plain files are placed for free; the executable one needs its own
        // inode to carry the +x bit, so it costs a reflink or a copy.
        assert_eq!(stats.files(), 4);
        assert_eq!(stats.copied + stats.reflinked, 1, "only build.sh should cost");
        assert!(stats.cheap_ratio() >= 0.75);
        assert!(stats.bytes > 0);

        // On a reflink-capable filesystem nothing should ever be copied.
        if s.capabilities().reflink_supported() == Some(true) {
            assert_eq!(stats.copied, 0, "reflink available but bytes were copied");
        }
    }
}
