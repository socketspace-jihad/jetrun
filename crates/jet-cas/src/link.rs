//! Getting bytes from the object store into a workspace without copying them.
//!
//! Three mechanisms, in descending order of preference:
//!
//! | mechanism | cost        | safe to expose to a writer? |
//! |-----------|-------------|-----------------------------|
//! | reflink   | metadata    | yes -- COW, writes diverge  |
//! | hardlink  | metadata    | **only under an overlay lowerdir** |
//! | copy      | O(bytes)    | yes                         |
//!
//! # The hardlink trap
//!
//! A hardlink is not a copy: it is a second name for the same inode. If a build
//! step opens a hardlinked file for writing, it mutates **the object store
//! itself**, and every future step on the machine that materializes that digest
//! receives the corrupted bytes. The digest no longer matches its content, so
//! the store is silently lying -- the single worst failure mode this system has.
//!
//! It is nonetheless safe in exactly one place. Under an overlayfs *lowerdir*,
//! a write triggers copy-up: the kernel duplicates the file into the upperdir
//! and redirects the write there, leaving the lower inode untouched. Since
//! jetrun materializes workspaces as overlay lowerdirs, hardlinks are both safe
//! and free in the common path.
//!
//! That distinction is enforced by [`Intent`] rather than by comment, because
//! it is far too dangerous to leave as a convention someone can forget.

use std::fs;
use std::io;
use std::path::Path;
use std::sync::atomic::{AtomicU8, Ordering};

/// Permissions applied to every object in the store: read-only for everyone.
///
/// Defence in depth behind [`Intent`]. It will not stop a process running as
/// the store's owner (which can simply `chmod`), but it turns the common
/// accident -- a build step redirecting output onto a materialized path -- into
/// an `EACCES` at the point of the mistake instead of silent store corruption.
pub const OBJECT_MODE: u32 = 0o444;

/// Whether the two files may safely share an inode.
///
/// This is the guard on the hardlink trap described in the module docs. There is
/// no default: every caller states which case it is in.
///
/// Note this is a property of **either** side. It is tempting to name the
/// variants after whether the *destination* is writable, but the source can be
/// the mutable one: ingesting a file the developer will edit next minute has
/// exactly the same aliasing hazard as handing a store object to a build step.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Intent {
    /// The destination becomes part of an overlayfs **lowerdir**, and the source
    /// is an immutable store object. Writes copy up, so no write can reach the
    /// shared inode and hardlinks are both safe and free.
    OverlayLower,
    /// Either side may be modified independently, so the two must not alias:
    /// a writable workspace, an artifact export, or **ingest of a file the user
    /// may still edit**. Hardlinks are forbidden; reflink or copy.
    PrivateInode,
}

/// How a materialization actually happened.
///
/// Recorded and surfaced in metrics: a deployment that has silently fallen back
/// to `Copy` for everything has lost most of the CAS's speed advantage, and the
/// only symptom is that things feel slow. Worth an explicit warning at startup.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkMode {
    Reflink,
    Hardlink,
    Copy,
}

impl LinkMode {
    /// Whether this mode duplicated the bytes.
    pub const fn is_copy(self) -> bool {
        matches!(self, LinkMode::Copy)
    }
}

/// Whether the filesystem under the store supports `FICLONE`.
///
/// Probed once and cached. Without the cache, every single file on an ext4
/// store would burn an ioctl that is guaranteed to return `EOPNOTSUPP` --
/// 50,000 wasted syscalls per workspace, which is precisely the overhead this
/// crate exists to remove.
#[derive(Debug)]
pub struct Capabilities {
    reflink: AtomicU8,
}

const CAP_UNKNOWN: u8 = 0;
const CAP_YES: u8 = 1;
const CAP_NO: u8 = 2;

impl Default for Capabilities {
    fn default() -> Self {
        Self::new()
    }
}

impl Capabilities {
    pub const fn new() -> Self {
        Capabilities {
            reflink: AtomicU8::new(CAP_UNKNOWN),
        }
    }

    /// Force a known state. Used by tests to exercise each fallback branch on
    /// whatever filesystem the test happens to run on.
    pub fn with_reflink(supported: bool) -> Self {
        Capabilities {
            reflink: AtomicU8::new(if supported { CAP_YES } else { CAP_NO }),
        }
    }

    pub fn reflink_supported(&self) -> Option<bool> {
        match self.reflink.load(Ordering::Relaxed) {
            CAP_YES => Some(true),
            CAP_NO => Some(false),
            _ => None,
        }
    }

    fn note_reflink(&self, supported: bool) {
        self.reflink.store(
            if supported { CAP_YES } else { CAP_NO },
            Ordering::Relaxed,
        );
    }

    /// Probe by attempting a real clone inside `dir`.
    ///
    /// Filesystem support is not enough on its own -- XFS needs `reflink=1` at
    /// mkfs time, and a bind mount can differ from its parent -- so this tries
    /// the actual operation rather than inspecting the fstype.
    pub fn probe(&self, dir: &Path) -> io::Result<bool> {
        if let Some(known) = self.reflink_supported() {
            return Ok(known);
        }
        let src = dir.join(".jetrun-reflink-probe.src");
        let dst = dir.join(".jetrun-reflink-probe.dst");
        let _ = fs::remove_file(&src);
        let _ = fs::remove_file(&dst);

        fs::write(&src, b"probe")?;
        let result = raw_reflink(&src, &dst);
        let _ = fs::remove_file(&src);
        let _ = fs::remove_file(&dst);

        let supported = match result {
            Ok(()) => true,
            Err(e) if is_unsupported(&e) => false,
            Err(e) => return Err(e),
        };
        self.note_reflink(supported);
        Ok(supported)
    }
}

/// `EOPNOTSUPP`/`ENOTTY`/`EXDEV`/`EINVAL` all mean "this filesystem will never
/// clone", as opposed to a transient failure worth reporting.
fn is_unsupported(e: &io::Error) -> bool {
    matches!(
        e.raw_os_error(),
        Some(libc::EOPNOTSUPP)
            | Some(libc::ENOTTY)
            | Some(libc::EXDEV)
            | Some(libc::EINVAL)
            | Some(libc::ENOSYS)
    )
}

/// Attempt a single `FICLONE`. `dst` must not exist.
fn raw_reflink(src: &Path, dst: &Path) -> io::Result<()> {
    let src_f = fs::File::open(src)?;
    // create_new: never clobber, and never clone onto a path we do not own.
    let dst_f = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(dst)?;

    match rustix::fs::ioctl_ficlone(&dst_f, &src_f) {
        Ok(()) => Ok(()),
        Err(e) => {
            drop(dst_f);
            // Leave no partial file behind for the caller to fall back onto.
            let _ = fs::remove_file(dst);
            Err(io::Error::from_raw_os_error(e.raw_os_error()))
        }
    }
}

/// Place the contents of `src` at `dst`, choosing the cheapest mechanism that
/// is safe for `intent`.
///
/// `dst`'s parent must already exist. An existing `dst` is replaced.
pub fn materialize(
    src: &Path,
    dst: &Path,
    intent: Intent,
    caps: &Capabilities,
) -> io::Result<LinkMode> {
    // Replace rather than fail: re-materializing a workspace over a previous
    // one is routine, and the content is identical by construction anyway.
    if let Err(e) = fs::remove_file(dst)
        && e.kind() != io::ErrorKind::NotFound
    {
        return Err(e);
    }

    // 1. reflink -- cheapest and always safe.
    if caps.reflink_supported() != Some(false) {
        match raw_reflink(src, dst) {
            Ok(()) => {
                caps.note_reflink(true);
                return Ok(LinkMode::Reflink);
            }
            Err(e) if is_unsupported(&e) => caps.note_reflink(false),
            Err(e) => return Err(e),
        }
    }

    // 2. hardlink -- free, but only where a write cannot reach the inode.
    if intent == Intent::OverlayLower {
        match fs::hard_link(src, dst) {
            Ok(()) => return Ok(LinkMode::Hardlink),
            // EMLINK: inode is at its link limit. EXDEV: different filesystem.
            // Both are permanent for this file; fall through to copy.
            Err(e)
                if matches!(
                    e.raw_os_error(),
                    Some(libc::EMLINK) | Some(libc::EXDEV) | Some(libc::EPERM)
                ) => {}
            Err(e) => return Err(e),
        }
    }

    // 3. copy. std::fs::copy uses copy_file_range, so the kernel may still
    //    avoid moving bytes through userspace even here.
    //
    // Deliberately no chmod: mode policy belongs to the caller, which knows
    // whether it is producing a store object (0444), an overlay lowerdir entry,
    // or a writable workspace file. Setting a mode here would silently apply to
    // whichever inode we ended up sharing.
    fs::copy(src, dst)?;
    Ok(LinkMode::Copy)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::os::unix::fs::{MetadataExt, PermissionsExt};

    fn tmp() -> tempfile::TempDir {
        tempfile::tempdir().unwrap()
    }

    fn write(path: &Path, data: &[u8]) {
        let mut f = fs::File::create(path).unwrap();
        f.write_all(data).unwrap();
        let mut p = f.metadata().unwrap().permissions();
        p.set_mode(OBJECT_MODE);
        fs::set_permissions(path, p).unwrap();
    }

    #[test]
    fn probe_is_cached_and_consistent() {
        let d = tmp();
        let caps = Capabilities::new();
        assert_eq!(caps.reflink_supported(), None);
        let first = caps.probe(d.path()).unwrap();
        assert_eq!(caps.reflink_supported(), Some(first));
        // Second probe must not re-run the ioctl; it returns the cached answer.
        assert_eq!(caps.probe(d.path()).unwrap(), first);
    }

    #[test]
    fn probe_leaves_no_droppings() {
        let d = tmp();
        Capabilities::new().probe(d.path()).unwrap();
        let leftovers: Vec<_> = fs::read_dir(d.path())
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert!(leftovers.is_empty(), "probe left {leftovers:?} behind");
    }

    #[test]
    fn copy_fallback_produces_identical_bytes() {
        let d = tmp();
        let src = d.path().join("src");
        let dst = d.path().join("dst");
        write(&src, b"payload");

        // Force the copy path regardless of the host filesystem.
        let caps = Capabilities::with_reflink(false);
        let mode = materialize(&src, &dst, Intent::PrivateInode, &caps).unwrap();
        assert_eq!(mode, LinkMode::Copy);
        assert_eq!(fs::read(&dst).unwrap(), b"payload");
    }

    #[test]
    fn writable_intent_never_hardlinks() {
        // The core safety property: a writable destination must not share an
        // inode with the store, or a build step would corrupt the CAS.
        let d = tmp();
        let src = d.path().join("obj");
        let dst = d.path().join("workspace-file");
        write(&src, b"original");

        let caps = Capabilities::with_reflink(false);
        let mode = materialize(&src, &dst, Intent::PrivateInode, &caps).unwrap();
        assert_eq!(mode, LinkMode::Copy);

        let src_ino = fs::metadata(&src).unwrap().ino();
        let dst_ino = fs::metadata(&dst).unwrap().ino();
        assert_ne!(src_ino, dst_ino, "writable file shares an inode with the store");

        // And prove the source survives a write through the destination. The
        // chmod is the caller's job now -- `materialize` sets no modes, since
        // only the caller knows whether it is producing a store object, a
        // lowerdir entry, or a workspace file.
        fs::set_permissions(&dst, fs::Permissions::from_mode(0o644)).unwrap();
        fs::write(&dst, b"clobbered").unwrap();
        assert_eq!(fs::read(&src).unwrap(), b"original");
    }

    #[test]
    fn overlay_lower_intent_hardlinks_when_reflink_is_absent() {
        let d = tmp();
        let src = d.path().join("obj");
        let dst = d.path().join("lower-file");
        write(&src, b"shared");

        let caps = Capabilities::with_reflink(false);
        let mode = materialize(&src, &dst, Intent::OverlayLower, &caps).unwrap();
        assert_eq!(mode, LinkMode::Hardlink);
        assert_eq!(
            fs::metadata(&src).unwrap().ino(),
            fs::metadata(&dst).unwrap().ino(),
            "hardlink should share the inode -- that is the point"
        );
    }

    #[test]
    fn materialize_overwrites_existing_destination() {
        let d = tmp();
        let src = d.path().join("obj");
        let dst = d.path().join("dst");
        write(&src, b"new");
        fs::write(&dst, b"stale").unwrap();

        let caps = Capabilities::with_reflink(false);
        materialize(&src, &dst, Intent::PrivateInode, &caps).unwrap();
        assert_eq!(fs::read(&dst).unwrap(), b"new");
    }

    #[test]
    fn materialize_sets_no_mode_policy_of_its_own() {
        // Objects are 0444 and fs::copy carries that across. `materialize` must
        // leave it alone rather than guessing: the caller decides, because a
        // chmod here would land on whichever inode we ended up sharing. The
        // store's own mode policy is tested in store.rs.
        let d = tmp();
        let src = d.path().join("obj");
        let dst = d.path().join("dst");
        write(&src, b"data");

        let caps = Capabilities::with_reflink(false);
        materialize(&src, &dst, Intent::PrivateInode, &caps).unwrap();
        assert_eq!(
            fs::metadata(&dst).unwrap().permissions().mode() & 0o777,
            OBJECT_MODE,
            "materialize should not invent a mode"
        );
    }

    #[test]
    fn reflink_diverges_on_write_when_supported() {
        let d = tmp();
        let caps = Capabilities::new();
        if !caps.probe(d.path()).unwrap() {
            // ext4 and friends: nothing to assert here.
            return;
        }
        let src = d.path().join("obj");
        let dst = d.path().join("dst");
        write(&src, b"original");
        let mode = materialize(&src, &dst, Intent::PrivateInode, &caps).unwrap();
        assert_eq!(mode, LinkMode::Reflink);
        fs::write(&dst, b"diverged").unwrap();
        assert_eq!(fs::read(&src).unwrap(), b"original");
    }
}
