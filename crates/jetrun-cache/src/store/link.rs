use std::path::Path;
use std::sync::atomic::{AtomicU8, Ordering};

/// Permission mode for objects in the CAS — read-only for all.
pub const OBJECT_MODE: u32 = 0o444;

/// Describes how the materialized file will be used.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Intent {
    /// File will be used as a read-only overlay lower layer.
    /// Hardlinks are safe here because the file won't be modified.
    OverlayLower,
    /// File needs its own inode (may be modified by the build).
    /// Must use reflink or copy to avoid corrupting the CAS.
    PrivateInode,
}

/// How a file was materialized.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkMode {
    Reflink,
    Hardlink,
    Copy,
}

// Internal state for the capabilities probe.
const PROBE_UNKNOWN: u8 = 0;
const PROBE_SUPPORTED: u8 = 1;
const PROBE_UNSUPPORTED: u8 = 2;

/// Caches whether the filesystem supports reflink (copy-on-write) clones.
///
/// The probe is performed once and the result is cached atomically.
pub struct Capabilities {
    reflink: AtomicU8,
}

impl Capabilities {
    /// Create un-probed capabilities (will probe on first use).
    pub fn new() -> Self {
        Self {
            reflink: AtomicU8::new(PROBE_UNKNOWN),
        }
    }

    /// Probe the given directory for reflink support.
    ///
    /// Creates a temporary file pair, attempts a clone, then cleans up.
    pub fn probe(dir: &Path) -> Self {
        let caps = Self::new();
        let _ = caps.probe_reflink(dir);
        caps
    }

    pub fn supports_reflink(&self) -> bool {
        self.reflink.load(Ordering::Relaxed) == PROBE_SUPPORTED
    }

    fn probe_reflink(&self, dir: &Path) -> std::io::Result<bool> {
        let src = dir.join(".jetrun_probe_src");
        let dst = dir.join(".jetrun_probe_dst");

        // Write a small probe file
        std::fs::write(&src, b"probe")?;

        let result = try_reflink(&src, &dst);

        // Cleanup
        let _ = std::fs::remove_file(&dst);
        let _ = std::fs::remove_file(&src);

        let supported = result.is_ok();
        self.reflink.store(
            if supported {
                PROBE_SUPPORTED
            } else {
                PROBE_UNSUPPORTED
            },
            Ordering::Relaxed,
        );
        Ok(supported)
    }
}

impl Default for Capabilities {
    fn default() -> Self {
        Self::new()
    }
}

/// Materialize a file from `src` to `dst` using the best available strategy.
///
/// Strategy selection:
/// 1. If reflink is supported, try reflink (always safe, CoW).
/// 2. If intent is `OverlayLower`, try hardlink (file won't be modified).
/// 3. Fall back to a full byte copy.
pub fn materialize(
    src: &Path,
    dst: &Path,
    intent: Intent,
    caps: &Capabilities,
) -> std::io::Result<LinkMode> {
    // Try reflink first
    if caps.supports_reflink() {
        if try_reflink(src, dst).is_ok() {
            return Ok(LinkMode::Reflink);
        }
    }

    // Hardlink is only safe for read-only overlay usage
    if intent == Intent::OverlayLower {
        if std::fs::hard_link(src, dst).is_ok() {
            return Ok(LinkMode::Hardlink);
        }
    }

    // Fallback: full copy
    std::fs::copy(src, dst)?;
    Ok(LinkMode::Copy)
}

/// Attempt a reflink (CoW clone) from src to dst.
///
/// - macOS: uses `clonefile(2)` via libc
/// - Linux: uses `FICLONE` ioctl via libc
/// - Other: always returns Err
#[cfg(target_os = "macos")]
fn try_reflink(src: &Path, dst: &Path) -> std::io::Result<()> {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;

    let src_c = CString::new(src.as_os_str().as_bytes())
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, e))?;
    let dst_c = CString::new(dst.as_os_str().as_bytes())
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, e))?;

    // clonefile(const char *src, const char *dst, int flags)
    let ret = unsafe { libc::clonefile(src_c.as_ptr(), dst_c.as_ptr(), 0) };
    if ret == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

#[cfg(target_os = "linux")]
fn try_reflink(src: &Path, dst: &Path) -> std::io::Result<()> {
    use std::fs::OpenOptions;
    use std::os::unix::io::AsRawFd;

    // FICLONE ioctl number: _IOW(0x94, 9, int) = 0x40049409
    const FICLONE: libc::c_ulong = 0x40049409;

    let src_file = std::fs::File::open(src)?;
    let dst_file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(dst)?;

    let ret = unsafe { libc::ioctl(dst_file.as_raw_fd(), FICLONE, src_file.as_raw_fd()) };
    if ret == 0 {
        Ok(())
    } else {
        // Remove the empty dst file we created
        let _ = std::fs::remove_file(dst);
        Err(std::io::Error::last_os_error())
    }
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn try_reflink(_src: &Path, _dst: &Path) -> std::io::Result<()> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "reflink not supported on this platform",
    ))
}

/// Tracks how files were materialized — useful for logging and metrics.
#[derive(Debug, Default)]
pub struct MaterializeStats {
    pub reflinks: u64,
    pub hardlinks: u64,
    pub copies: u64,
    pub dirs: u64,
    pub symlinks: u64,
    pub bytes: u64,
}

impl MaterializeStats {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record that a file was materialized with the given mode and byte count.
    pub fn record(&mut self, mode: LinkMode, file_bytes: u64) {
        match mode {
            LinkMode::Reflink => self.reflinks += 1,
            LinkMode::Hardlink => self.hardlinks += 1,
            LinkMode::Copy => self.copies += 1,
        }
        self.bytes += file_bytes;
    }

    pub fn record_dir(&mut self) {
        self.dirs += 1;
    }

    pub fn record_symlink(&mut self) {
        self.symlinks += 1;
    }

    /// Total number of files materialized.
    pub fn total_files(&self) -> u64 {
        self.reflinks + self.hardlinks + self.copies
    }

    /// Fraction of files that were cheaply materialized (reflink or hardlink).
    /// Returns 0.0 if no files have been materialized.
    pub fn cheap_ratio(&self) -> f64 {
        let total = self.total_files();
        if total == 0 {
            return 0.0;
        }
        (self.reflinks + self.hardlinks) as f64 / total as f64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_copy_fallback_always_works() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("src.bin");
        let dst = dir.path().join("dst.bin");

        std::fs::write(&src, b"hello copy fallback").unwrap();

        // Capabilities with reflink explicitly not supported
        let caps = Capabilities::new();
        caps.reflink.store(PROBE_UNSUPPORTED, Ordering::Relaxed);

        let mode = materialize(&src, &dst, Intent::PrivateInode, &caps).unwrap();
        assert_eq!(mode, LinkMode::Copy);

        let content = std::fs::read(&dst).unwrap();
        assert_eq!(content, b"hello copy fallback");
    }

    #[test]
    fn test_hardlink_for_overlay_lower() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("src.bin");
        let dst = dir.path().join("dst.bin");

        std::fs::write(&src, b"overlay data").unwrap();

        let caps = Capabilities::new();
        caps.reflink.store(PROBE_UNSUPPORTED, Ordering::Relaxed);

        let mode = materialize(&src, &dst, Intent::OverlayLower, &caps).unwrap();
        assert_eq!(mode, LinkMode::Hardlink);

        let content = std::fs::read(&dst).unwrap();
        assert_eq!(content, b"overlay data");
    }

    #[test]
    fn test_materialize_stats_tracking() {
        let mut stats = MaterializeStats::new();
        assert_eq!(stats.total_files(), 0);
        assert_eq!(stats.cheap_ratio(), 0.0);

        stats.record(LinkMode::Reflink, 1000);
        stats.record(LinkMode::Hardlink, 500);
        stats.record(LinkMode::Copy, 2000);
        stats.record_dir();
        stats.record_symlink();

        assert_eq!(stats.reflinks, 1);
        assert_eq!(stats.hardlinks, 1);
        assert_eq!(stats.copies, 1);
        assert_eq!(stats.dirs, 1);
        assert_eq!(stats.symlinks, 1);
        assert_eq!(stats.bytes, 3500);
        assert_eq!(stats.total_files(), 3);

        // 2 cheap (reflink + hardlink) out of 3 total
        let ratio = stats.cheap_ratio();
        assert!((ratio - 2.0 / 3.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_capabilities_default_unknown() {
        let caps = Capabilities::new();
        // Before probing, reflink is not supported (unknown = not supported)
        assert!(!caps.supports_reflink());
    }

    #[test]
    fn test_probe_runs_without_panic() {
        let dir = tempfile::tempdir().unwrap();
        // probe should not panic regardless of filesystem support
        let caps = Capabilities::probe(dir.path());
        // Result depends on filesystem, just verify we get a bool answer
        let _ = caps.supports_reflink();
    }

    #[test]
    fn test_private_inode_skips_hardlink() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("src.bin");
        let dst = dir.path().join("dst.bin");

        std::fs::write(&src, b"private data").unwrap();

        let caps = Capabilities::new();
        caps.reflink.store(PROBE_UNSUPPORTED, Ordering::Relaxed);

        // PrivateInode should NOT use hardlink, should fall through to copy
        let mode = materialize(&src, &dst, Intent::PrivateInode, &caps).unwrap();
        assert_eq!(mode, LinkMode::Copy);
    }
}
