//! TCP socket tuning.
//!
//! Two profiles, because control traffic and bulk transfer want opposite
//! settings and a single compromise is bad at both:
//!
//! * [`TcpTuning::control`] -- small frames, latency-critical. Nagle off.
//! * [`TcpTuning::bulk`] -- CAS objects, throughput-critical. Corked so a run of
//!   spliced objects coalesces into full-MSS segments instead of dribbling.
//!
//! # Everything here is best-effort, and reports what happened
//!
//! Most of these options fail on some kernel, container runtime, or hardened
//! seccomp profile. Silently ignoring that is how a deployment ends up believing
//! it runs BBR when it does not, so [`apply`](TcpTuning::apply) returns a
//! [`TuningReport`] listing what took effect and what was refused. Log it at
//! startup; the difference is diagnosable in one line instead of a week of
//! wondering why WAN transfers are slow.
//!
//! # What is deliberately *not* set
//!
//! `SO_SNDBUF` / `SO_RCVBUF` are left alone. Setting either one **disables
//! Linux's buffer autotuning**, and autotuning beats a hand-picked constant
//! across the range of links a real deployment sees. This is the most common
//! self-inflicted TCP wound. Raise `net.ipv4.tcp_rmem` / `tcp_wmem` ceilings
//! instead and let the kernel use the headroom.
//!
//! `TCP_QUICKACK` is also skipped: on Linux it is not sticky (it must be re-set
//! after every `recv`), so honouring it properly means a syscall per read for a
//! marginal gain.

use std::io;
use std::os::fd::{AsFd, AsRawFd, BorrowedFd};
use std::time::Duration;

/// Options the kernel headers define but `rustix` does not wrap.
mod raw {
    /// Cap on unsent bytes queued in the socket. See [`super::TcpTuning`].
    pub const TCP_NOTSENT_LOWAT: i32 = 25;
    /// Server side: enables the TFO queue with the given length.
    pub const TCP_FASTOPEN: i32 = 23;
    /// Client side: allows `connect` to carry data in the SYN.
    pub const TCP_FASTOPEN_CONNECT: i32 = 30;
    /// Microseconds of NIC polling before yielding.
    pub const SO_BUSY_POLL: i32 = 46;
}

/// Keepalive parameters. Detects peers that vanished without a FIN, which is the
/// normal outcome behind NAT and in cloud networks.
#[derive(Debug, Clone, Copy)]
pub struct KeepAlive {
    pub idle: Duration,
    pub interval: Duration,
    pub count: u32,
}

impl Default for KeepAlive {
    fn default() -> Self {
        KeepAlive {
            idle: Duration::from_secs(30),
            interval: Duration::from_secs(10),
            count: 3,
        }
    }
}

#[derive(Debug, Clone)]
pub struct TcpTuning {
    /// Disable Nagle. Correct for control frames, wrong for bulk.
    pub nodelay: bool,
    /// Coalesce small writes into full segments. The inverse of `nodelay`;
    /// applied around a batch of splices and released after.
    pub cork: bool,
    /// Congestion control algorithm. `bbr` substantially outperforms CUBIC on
    /// lossy or buffer-bloated WAN paths, which is where cross-region CAS
    /// transfer lives. Requires `tcp_bbr` to be available.
    pub congestion: Option<&'static str>,
    /// Cap on unsent bytes held in the socket buffer.
    ///
    /// The most valuable option here and the least known. Without it the
    /// multiplexer dumps megabytes into the send buffer and **loses the ability
    /// to prioritize**: a cancellation queued behind 8 MiB of bulk data waits
    /// for all of it to drain. This is the same mechanism browsers use for
    /// HTTP/2 prioritization.
    pub notsent_lowat: Option<u32>,
    /// Bound on how long the kernel retransmits before declaring the peer dead.
    /// Without it, a vanished worker occupies its slot for the default ~15
    /// minute retransmit budget -- far too slow to reschedule a build around.
    pub user_timeout: Option<Duration>,
    pub keepalive: Option<KeepAlive>,
    /// Server-side TFO queue length.
    ///
    /// Enabled because it is nearly free, but do not expect much: TFO saves one
    /// round trip on connection *setup*, and JRP uses long-lived multiplexed
    /// connections, so it saves one RTT per worker lifetime. It is also
    /// frequently stripped by middleboxes. Fast setup matters for protocols that
    /// churn connections, which this one is designed not to do.
    pub fastopen_queue: Option<u32>,
    /// Client-side counterpart to `fastopen_queue`.
    pub fastopen_connect: bool,
    /// Busy-poll the NIC for this many microseconds before yielding. Trades CPU
    /// for latency; sensible under a pinned shard-per-core runtime, wasteful on
    /// an idle self-hosted box, so it stays opt-in.
    pub busy_poll: Option<u32>,
}

impl TcpTuning {
    /// Latency profile: control streams.
    pub fn control() -> Self {
        TcpTuning {
            nodelay: true,
            cork: false,
            congestion: Some("bbr"),
            // 128 KiB: enough to keep the pipe full, small enough that a
            // high-priority frame waits at most that many bytes.
            notsent_lowat: Some(128 << 10),
            user_timeout: Some(Duration::from_secs(20)),
            keepalive: Some(KeepAlive::default()),
            fastopen_queue: Some(256),
            fastopen_connect: true,
            busy_poll: None,
        }
    }

    /// Throughput profile: bulk CAS transfer on its own connection.
    ///
    /// A separate connection is not an optimization but a correctness
    /// requirement. Application-level multiplexing cannot fix **TCP's**
    /// head-of-line blocking: one lost packet stalls every stream sharing the
    /// byte stream. That is the HTTP/2 flaw QUIC exists to fix. Putting a 2 GiB
    /// object on its own connection means a drop mid-transfer cannot stall a
    /// heartbeat or a cancellation.
    pub fn bulk() -> Self {
        TcpTuning {
            nodelay: false,
            cork: true,
            congestion: Some("bbr"),
            // Much larger: throughput wants a deep queue, and there is no
            // competing stream on this connection to starve.
            notsent_lowat: Some(2 << 20),
            user_timeout: Some(Duration::from_secs(120)),
            keepalive: Some(KeepAlive::default()),
            fastopen_queue: None,
            fastopen_connect: false,
            busy_poll: None,
        }
    }

    /// Apply to a socket, collecting what succeeded and what did not.
    pub fn apply<F: AsFd>(&self, sock: F) -> TuningReport {
        let fd = sock.as_fd();
        let mut r = TuningReport::default();

        r.note("TCP_NODELAY", rustix::net::sockopt::set_tcp_nodelay(fd, self.nodelay));

        if self.cork {
            r.note("TCP_CORK", rustix::net::sockopt::set_tcp_cork(fd, true));
        }

        if let Some(algo) = self.congestion {
            // Fails with ENOENT if the module is absent, which is normal on
            // minimal kernels -- CUBIC then remains in use.
            r.note(
                "TCP_CONGESTION",
                rustix::net::sockopt::set_tcp_congestion(fd, algo),
            );
        }

        if let Some(bytes) = self.notsent_lowat {
            r.note("TCP_NOTSENT_LOWAT", set_int(fd, libc::IPPROTO_TCP, raw::TCP_NOTSENT_LOWAT, bytes as i32));
        }

        if let Some(t) = self.user_timeout {
            r.note(
                "TCP_USER_TIMEOUT",
                rustix::net::sockopt::set_tcp_user_timeout(fd, t.as_millis() as u32),
            );
        }

        if let Some(ka) = self.keepalive {
            r.note("SO_KEEPALIVE", rustix::net::sockopt::set_socket_keepalive(fd, true));
            r.note("TCP_KEEPIDLE", rustix::net::sockopt::set_tcp_keepidle(fd, ka.idle));
            r.note(
                "TCP_KEEPINTVL",
                rustix::net::sockopt::set_tcp_keepintvl(fd, ka.interval),
            );
            r.note("TCP_KEEPCNT", rustix::net::sockopt::set_tcp_keepcnt(fd, ka.count));
        }

        if let Some(q) = self.fastopen_queue {
            r.note(
                "TCP_FASTOPEN",
                set_int(fd, libc::IPPROTO_TCP, raw::TCP_FASTOPEN, q as i32),
            );
        }
        if self.fastopen_connect {
            r.note(
                "TCP_FASTOPEN_CONNECT",
                set_int(fd, libc::IPPROTO_TCP, raw::TCP_FASTOPEN_CONNECT, 1),
            );
        }
        if let Some(us) = self.busy_poll {
            r.note(
                "SO_BUSY_POLL",
                set_int(fd, libc::SOL_SOCKET, raw::SO_BUSY_POLL, us as i32),
            );
        }

        r
    }
}

/// Enable `SO_REUSEPORT` before binding.
///
/// This is what makes the network layer shard-per-core: each shard binds its own
/// listening socket, and the kernel hashes a connection's 4-tuple to one of
/// them, so a connection is accepted and served entirely on one core with no
/// shared accept queue and no cross-core handoff.
///
/// Note the remaining gap: plain `SO_REUSEPORT` picks a listener by hash, which
/// is not necessarily the shard running on the CPU whose NIC queue received the
/// packet. Closing that requires `SO_ATTACH_REUSEPORT_CBPF` to steer by CPU,
/// plus RSS/RPS/XPS and IRQ affinity aligned to the shard CPUs. Worth doing when
/// the shard runtime lands; `SO_REUSEPORT` alone already removes the accept-queue
/// contention, which is the larger win.
pub fn set_reuseport<F: AsFd>(sock: F) -> io::Result<()> {
    rustix::net::sockopt::set_socket_reuseport(sock.as_fd(), true).map_err(to_io)
}

/// Uncork a socket, flushing whatever the cork was holding.
///
/// The bulk sender's pattern is: cork, splice N objects, uncork. Without the
/// final uncork the tail of the last object sits in the kernel until the next
/// write or a 200 ms timer -- a stall that looks like a network problem.
pub fn uncork<F: AsFd>(sock: F) -> io::Result<()> {
    rustix::net::sockopt::set_tcp_cork(sock.as_fd(), false).map_err(to_io)
}

/// RAII cork: corks on construction, uncorks on drop.
///
/// Using the guard rather than paired calls means an early return or a `?` on
/// the splice path cannot leave the socket corked.
pub struct CorkGuard<'f> {
    fd: BorrowedFd<'f>,
}

impl<'f> CorkGuard<'f> {
    pub fn new(fd: BorrowedFd<'f>) -> io::Result<Self> {
        rustix::net::sockopt::set_tcp_cork(fd, true).map_err(to_io)?;
        Ok(CorkGuard { fd })
    }
}

impl Drop for CorkGuard<'_> {
    fn drop(&mut self) {
        let _ = rustix::net::sockopt::set_tcp_cork(self.fd, false);
    }
}

fn set_int(fd: BorrowedFd<'_>, level: i32, name: i32, value: i32) -> rustix::io::Result<()> {
    // SAFETY: `fd` is a valid borrowed socket for the duration of the call, and
    // `value` is a live i32 whose size is passed correctly. setsockopt does not
    // retain the pointer.
    let rc = unsafe {
        libc::setsockopt(
            fd.as_raw_fd(),
            level,
            name,
            &value as *const i32 as *const libc::c_void,
            std::mem::size_of::<i32>() as libc::socklen_t,
        )
    };
    if rc == 0 {
        Ok(())
    } else {
        Err(rustix::io::Errno::from_raw_os_error(
            io::Error::last_os_error().raw_os_error().unwrap_or(0),
        ))
    }
}

fn to_io(e: rustix::io::Errno) -> io::Error {
    io::Error::from_raw_os_error(e.raw_os_error())
}

/// What actually took effect.
#[derive(Debug, Default, Clone)]
pub struct TuningReport {
    pub applied: Vec<&'static str>,
    pub refused: Vec<(&'static str, i32)>,
}

impl TuningReport {
    fn note(&mut self, what: &'static str, res: rustix::io::Result<()>) {
        match res {
            Ok(()) => self.applied.push(what),
            Err(e) => self.refused.push((what, e.raw_os_error())),
        }
    }

    pub fn applied(&self, what: &str) -> bool {
        self.applied.contains(&what)
    }

    /// One-line summary for a startup log.
    pub fn summary(&self) -> String {
        let refused: Vec<String> = self
            .refused
            .iter()
            .map(|(what, errno)| format!("{what}(errno {errno})"))
            .collect();
        if refused.is_empty() {
            format!("tcp tuning: {} options applied", self.applied.len())
        } else {
            format!(
                "tcp tuning: {} applied, refused: {}",
                self.applied.len(),
                refused.join(", ")
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{TcpListener, TcpStream};

    fn loopback_pair() -> (TcpStream, TcpStream) {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = l.local_addr().unwrap();
        let client = TcpStream::connect(addr).unwrap();
        let (server, _) = l.accept().unwrap();
        (client, server)
    }

    #[test]
    fn control_profile_applies_the_options_that_matter() {
        let (c, _s) = loopback_pair();
        let report = TcpTuning::control().apply(&c);

        // These must work on any Linux we support; if they do not, something is
        // wrong with the socket, not the option.
        assert!(report.applied("TCP_NODELAY"), "{}", report.summary());
        assert!(report.applied("SO_KEEPALIVE"), "{}", report.summary());
        assert!(report.applied("TCP_USER_TIMEOUT"), "{}", report.summary());
        // The prioritization-critical one.
        assert!(report.applied("TCP_NOTSENT_LOWAT"), "{}", report.summary());
    }

    #[test]
    fn nodelay_actually_takes_effect() {
        let (c, _s) = loopback_pair();
        TcpTuning::control().apply(&c);
        assert!(rustix::net::sockopt::tcp_nodelay(&c).unwrap());
    }

    #[test]
    fn bulk_profile_corks_and_control_does_not() {
        let (c, _s) = loopback_pair();
        TcpTuning::bulk().apply(&c);
        assert!(
            rustix::net::sockopt::tcp_cork(&c).unwrap(),
            "bulk transfers should coalesce into full segments"
        );

        let (c2, _s2) = loopback_pair();
        TcpTuning::control().apply(&c2);
        assert!(
            !rustix::net::sockopt::tcp_cork(&c2).unwrap(),
            "control frames must not wait to be coalesced"
        );
    }

    #[test]
    fn cork_guard_releases_on_drop() {
        // The property that matters: an early return on the splice path must not
        // leave data stuck in the kernel.
        let (c, _s) = loopback_pair();
        {
            let _g = CorkGuard::new(c.as_fd()).unwrap();
            assert!(rustix::net::sockopt::tcp_cork(&c).unwrap());
        }
        assert!(
            !rustix::net::sockopt::tcp_cork(&c).unwrap(),
            "CorkGuard must uncork when it goes out of scope"
        );
    }

    #[test]
    fn keepalive_parameters_are_written_through() {
        let (c, _s) = loopback_pair();
        let ka = KeepAlive {
            idle: Duration::from_secs(45),
            interval: Duration::from_secs(5),
            count: 4,
        };
        let mut t = TcpTuning::control();
        t.keepalive = Some(ka);
        t.apply(&c);

        assert_eq!(rustix::net::sockopt::tcp_keepidle(&c).unwrap(), ka.idle);
        assert_eq!(rustix::net::sockopt::tcp_keepintvl(&c).unwrap(), ka.interval);
        assert_eq!(rustix::net::sockopt::tcp_keepcnt(&c).unwrap(), ka.count);
    }

    #[test]
    fn report_records_refusals_rather_than_hiding_them() {
        // A bogus congestion algorithm stands in for any option the kernel
        // refuses. The point is that it lands in `refused` and is visible in the
        // summary, instead of being silently swallowed.
        let (c, _s) = loopback_pair();
        let mut t = TcpTuning::control();
        t.congestion = Some("definitely-not-a-real-algo");
        let report = t.apply(&c);

        assert!(!report.applied("TCP_CONGESTION"));
        assert!(report.refused.iter().any(|(w, _)| *w == "TCP_CONGESTION"));
        assert!(report.summary().contains("TCP_CONGESTION"));
    }

    #[test]
    fn bbr_availability_is_reported_not_assumed() {
        // Informational: BBR is the largest WAN throughput win available, and a
        // deployment silently running CUBIC should be able to find that out.
        let (c, _s) = loopback_pair();
        let report = TcpTuning::control().apply(&c);
        if report.applied("TCP_CONGESTION") {
            assert_eq!(rustix::net::sockopt::tcp_congestion(&c).unwrap(), "bbr");
        } else {
            eprintln!("note: BBR unavailable on this kernel; {}", report.summary());
        }
    }

    #[test]
    fn reuseport_can_be_set_before_bind() {
        // Two listeners on one port, which is what per-shard listening requires.
        use std::net::SocketAddr;
        let probe = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr: SocketAddr = probe.local_addr().unwrap();
        drop(probe);

        let s1 = rustix::net::socket(
            rustix::net::AddressFamily::INET,
            rustix::net::SocketType::STREAM,
            None,
        )
        .unwrap();
        set_reuseport(&s1).unwrap();
        if rustix::net::bind(&s1, &rustix::net::SocketAddrV4::new(
            std::net::Ipv4Addr::LOCALHOST,
            addr.port(),
        ))
        .is_err()
        {
            return; // port raced away; nothing to assert
        }

        let s2 = rustix::net::socket(
            rustix::net::AddressFamily::INET,
            rustix::net::SocketType::STREAM,
            None,
        )
        .unwrap();
        set_reuseport(&s2).unwrap();
        rustix::net::bind(
            &s2,
            &rustix::net::SocketAddrV4::new(std::net::Ipv4Addr::LOCALHOST, addr.port()),
        )
        .expect("SO_REUSEPORT should permit a second bind on the same port");
    }
}
