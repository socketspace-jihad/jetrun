//! TCP socket tuning profiles for control (latency-critical) and bulk (throughput-critical) connections.

use std::os::fd::AsRawFd;
use tokio::net::TcpStream;

/// TCP socket tuning configuration.
#[derive(Debug, Clone)]
pub struct TcpTuning {
    pub nodelay: bool,
    pub cork: bool,
    pub notsent_lowat: Option<u32>,
    pub user_timeout_ms: Option<u32>,
    pub keepalive_idle_secs: Option<u32>,
    pub keepalive_interval_secs: Option<u32>,
    pub keepalive_count: Option<u32>,
}

impl TcpTuning {
    /// Control profile: optimized for RPC, heartbeats, cancellation (latency-critical).
    pub fn control() -> Self {
        Self {
            nodelay: true,
            cork: false,
            notsent_lowat: Some(128 * 1024),       // 128KB
            user_timeout_ms: Some(20_000),          // 20 seconds
            keepalive_idle_secs: Some(30),
            keepalive_interval_secs: Some(10),
            keepalive_count: Some(3),
        }
    }

    /// Bulk profile: optimized for large transfers (throughput-critical).
    pub fn bulk() -> Self {
        Self {
            nodelay: false,
            cork: true,
            notsent_lowat: Some(2 * 1024 * 1024),  // 2MB
            user_timeout_ms: Some(120_000),         // 120 seconds
            keepalive_idle_secs: None,
            keepalive_interval_secs: None,
            keepalive_count: None,
        }
    }

    /// Apply all tuning options to a TCP stream. Returns a report of what succeeded/failed.
    pub fn apply(&self, stream: &TcpStream) -> TuningReport {
        let mut report = TuningReport::default();

        // TCP_NODELAY — available via std
        match stream.set_nodelay(self.nodelay) {
            Ok(()) => report.nodelay = OptionResult::Applied(self.nodelay),
            Err(e) => report.nodelay = OptionResult::Failed(e.to_string()),
        }

        // TCP_CORK (Linux) / TCP_NOPUSH (macOS)
        report.cork = apply_cork(stream, self.cork);

        // TCP_NOTSENT_LOWAT
        if let Some(val) = self.notsent_lowat {
            report.notsent_lowat = apply_notsent_lowat(stream, val);
        }

        // TCP_USER_TIMEOUT
        if let Some(val) = self.user_timeout_ms {
            report.user_timeout = apply_user_timeout(stream, val);
        }

        // Keepalive settings
        if let Some(idle) = self.keepalive_idle_secs {
            report.keepalive_idle = apply_keepalive_idle(stream, idle);
        }
        if let Some(interval) = self.keepalive_interval_secs {
            report.keepalive_interval = apply_keepalive_interval(stream, interval);
        }
        if let Some(count) = self.keepalive_count {
            report.keepalive_count = apply_keepalive_count(stream, count);
        }

        // Enable SO_KEEPALIVE if any keepalive parameter is set
        if self.keepalive_idle_secs.is_some()
            || self.keepalive_interval_secs.is_some()
            || self.keepalive_count.is_some()
        {
            let fd = stream.as_raw_fd();
            let enabled: libc::c_int = 1;
            let res = unsafe {
                libc::setsockopt(
                    fd,
                    libc::SOL_SOCKET,
                    libc::SO_KEEPALIVE,
                    &enabled as *const _ as *const libc::c_void,
                    std::mem::size_of_val(&enabled) as libc::socklen_t,
                )
            };
            if res != 0 {
                tracing::warn!("failed to set SO_KEEPALIVE: {}", std::io::Error::last_os_error());
            }
        }

        report
    }
}

/// Result of applying a single socket option.
#[derive(Debug, Clone, Default)]
pub enum OptionResult {
    #[default]
    Skipped,
    Applied(bool),
    AppliedU32(u32),
    Failed(String),
    Unsupported,
}

impl OptionResult {
    pub fn is_ok(&self) -> bool {
        matches!(self, OptionResult::Skipped | OptionResult::Applied(_) | OptionResult::AppliedU32(_))
    }
}

/// Report of all socket option application results.
#[derive(Debug, Clone, Default)]
pub struct TuningReport {
    pub nodelay: OptionResult,
    pub cork: OptionResult,
    pub notsent_lowat: OptionResult,
    pub user_timeout: OptionResult,
    pub keepalive_idle: OptionResult,
    pub keepalive_interval: OptionResult,
    pub keepalive_count: OptionResult,
}

impl TuningReport {
    /// Returns true if all applied options succeeded (no failures).
    pub fn all_ok(&self) -> bool {
        self.nodelay.is_ok()
            && self.cork.is_ok()
            && self.notsent_lowat.is_ok()
            && self.user_timeout.is_ok()
            && self.keepalive_idle.is_ok()
            && self.keepalive_interval.is_ok()
            && self.keepalive_count.is_ok()
    }
}

/// RAII guard that corks a socket on creation and uncorks on drop.
pub struct CorkGuard {
    fd: std::os::fd::RawFd,
}

impl CorkGuard {
    /// Cork the given TCP stream. The socket will be uncorked when this guard is dropped.
    pub fn new(stream: &TcpStream) -> Self {
        let fd = stream.as_raw_fd();
        set_cork(fd, true);
        Self { fd }
    }
}

impl Drop for CorkGuard {
    fn drop(&mut self) {
        set_cork(self.fd, false);
    }
}

// ── Platform-specific setsockopt helpers ──

fn setsockopt_int(fd: std::os::fd::RawFd, level: libc::c_int, name: libc::c_int, value: libc::c_int) -> std::io::Result<()> {
    let res = unsafe {
        libc::setsockopt(
            fd,
            level,
            name,
            &value as *const _ as *const libc::c_void,
            std::mem::size_of_val(&value) as libc::socklen_t,
        )
    };
    if res == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

// TCP_CORK (Linux) / TCP_NOPUSH (macOS)
fn apply_cork(stream: &TcpStream, enable: bool) -> OptionResult {
    let fd = stream.as_raw_fd();
    let val = if enable { 1 } else { 0 };

    #[cfg(target_os = "linux")]
    {
        match setsockopt_int(fd, libc::IPPROTO_TCP, libc::TCP_CORK, val) {
            Ok(()) => OptionResult::Applied(enable),
            Err(e) => OptionResult::Failed(e.to_string()),
        }
    }

    #[cfg(target_os = "macos")]
    {
        match setsockopt_int(fd, libc::IPPROTO_TCP, libc::TCP_NOPUSH, val) {
            Ok(()) => OptionResult::Applied(enable),
            Err(e) => OptionResult::Failed(e.to_string()),
        }
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        let _ = (fd, val);
        OptionResult::Unsupported
    }
}

fn set_cork(fd: std::os::fd::RawFd, enable: bool) {
    let val: libc::c_int = if enable { 1 } else { 0 };

    #[cfg(target_os = "linux")]
    {
        let _ = setsockopt_int(fd, libc::IPPROTO_TCP, libc::TCP_CORK, val);
    }

    #[cfg(target_os = "macos")]
    {
        let _ = setsockopt_int(fd, libc::IPPROTO_TCP, libc::TCP_NOPUSH, val);
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        let _ = (fd, val);
    }
}

// TCP_NOTSENT_LOWAT — not always exposed by the libc crate, define manually.
// Linux: 25, macOS: 0x201
#[cfg(target_os = "linux")]
const TCP_NOTSENT_LOWAT: libc::c_int = 25;
#[cfg(target_os = "macos")]
const TCP_NOTSENT_LOWAT: libc::c_int = 0x201;

fn apply_notsent_lowat(stream: &TcpStream, value: u32) -> OptionResult {
    let fd = stream.as_raw_fd();

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    {
        match setsockopt_int(fd, libc::IPPROTO_TCP, TCP_NOTSENT_LOWAT, value as libc::c_int) {
            Ok(()) => OptionResult::AppliedU32(value),
            Err(e) => OptionResult::Failed(e.to_string()),
        }
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        let _ = (fd, value);
        OptionResult::Unsupported
    }
}

// TCP_USER_TIMEOUT (Linux only)
fn apply_user_timeout(stream: &TcpStream, ms: u32) -> OptionResult {
    let fd = stream.as_raw_fd();

    #[cfg(target_os = "linux")]
    {
        // TCP_USER_TIMEOUT = 18
        const TCP_USER_TIMEOUT: libc::c_int = 18;
        match setsockopt_int(fd, libc::IPPROTO_TCP, TCP_USER_TIMEOUT, ms as libc::c_int) {
            Ok(()) => OptionResult::AppliedU32(ms),
            Err(e) => OptionResult::Failed(e.to_string()),
        }
    }

    #[cfg(not(target_os = "linux"))]
    {
        let _ = (fd, ms);
        OptionResult::Unsupported
    }
}

// TCP_KEEPIDLE (Linux) / TCP_KEEPALIVE (macOS)
fn apply_keepalive_idle(stream: &TcpStream, secs: u32) -> OptionResult {
    let fd = stream.as_raw_fd();

    #[cfg(target_os = "linux")]
    {
        match setsockopt_int(fd, libc::IPPROTO_TCP, libc::TCP_KEEPIDLE, secs as libc::c_int) {
            Ok(()) => OptionResult::AppliedU32(secs),
            Err(e) => OptionResult::Failed(e.to_string()),
        }
    }

    #[cfg(target_os = "macos")]
    {
        match setsockopt_int(fd, libc::IPPROTO_TCP, libc::TCP_KEEPALIVE, secs as libc::c_int) {
            Ok(()) => OptionResult::AppliedU32(secs),
            Err(e) => OptionResult::Failed(e.to_string()),
        }
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        let _ = (fd, secs);
        OptionResult::Unsupported
    }
}

// TCP_KEEPINTVL
fn apply_keepalive_interval(stream: &TcpStream, secs: u32) -> OptionResult {
    let fd = stream.as_raw_fd();

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    {
        match setsockopt_int(fd, libc::IPPROTO_TCP, libc::TCP_KEEPINTVL, secs as libc::c_int) {
            Ok(()) => OptionResult::AppliedU32(secs),
            Err(e) => OptionResult::Failed(e.to_string()),
        }
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        let _ = (fd, secs);
        OptionResult::Unsupported
    }
}

// TCP_KEEPCNT
fn apply_keepalive_count(stream: &TcpStream, count: u32) -> OptionResult {
    let fd = stream.as_raw_fd();

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    {
        match setsockopt_int(fd, libc::IPPROTO_TCP, libc::TCP_KEEPCNT, count as libc::c_int) {
            Ok(()) => OptionResult::AppliedU32(count),
            Err(e) => OptionResult::Failed(e.to_string()),
        }
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        let _ = (fd, count);
        OptionResult::Unsupported
    }
}
