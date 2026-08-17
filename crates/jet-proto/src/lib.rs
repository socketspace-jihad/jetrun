//! JRP -- the jetrun wire protocol.
//!
//! A framed binary protocol on bare TCP, used for **internal** traffic only:
//! control plane <-> worker, `jet` CLI <-> server, and CAS transfer. The public
//! API is deliberately HTTP/JSON instead (see `jet-server`), because webhooks,
//! browsers, and third-party integrations need ecosystem compatibility and gain
//! nothing from a custom protocol.
//!
//! # Why not gRPC here
//!
//! The strongest argument is not shaving HPACK or protobuf overhead off small
//! messages -- it is that **gRPC forces every CAS byte through userspace**. JRP's
//! bulk path moves objects from the page cache straight to the socket with
//! `splice`, zero copies. On a warm cache-affinity transfer of a multi-gigabyte
//! workspace that is the difference that matters; the control-frame savings are a
//! rounding error by comparison.
//!
//! The cost is real and worth stating: no `grpcurl`, no Envoy, no off-the-shelf
//! load balancing or tracing, and we own compatibility forever. That is why
//! [`frame`] puts magic **and** version in every header and the handshake
//! negotiates explicitly -- a worker fleet has to be upgradable one node at a
//! time.
//!
//! ## Layout
//!
//! * [`frame`] -- 16-byte fixed header, message codes, flags, size limits.
//! * [`msg`] -- rkyv payloads, validated on decode.
//! * [`conn`] -- async framed connection and handshake.
//! * [`tuning`] -- socket options, applied best-effort and reported.

pub mod conn;
pub mod frame;
pub mod msg;
pub mod tuning;

pub use conn::{ConnError, Frame, JrpConn};
pub use frame::{
    Flags, FrameError, FrameHeader, HEADER_LEN, MAGIC, MAX_PAYLOAD, MIN_VERSION, MsgType, VERSION,
};
pub use msg::{Invalid, Validate, caps, code, limits};
pub use tuning::{CorkGuard, KeepAlive, TcpTuning, TuningReport, set_reuseport};

/// Default port for the JRP listener.
///
/// Distinct from the HTTP port: the two protocols have different exposure
/// profiles. HTTP must face the internet to receive webhooks; JRP should be
/// reachable only by workers and authenticated CLIs, which is much easier to
/// enforce when it is a separate port to firewall.
pub const DEFAULT_JRP_PORT: u16 = 7433;

/// Default port for the public HTTP/JSON listener.
pub const DEFAULT_HTTP_PORT: u16 = 7432;
