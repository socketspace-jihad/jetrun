//! Framed JRP connection and handshake.

use std::io;

use rkyv::util::AlignedVec;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

use crate::frame::{FrameError, FrameHeader, Flags, HEADER_LEN, MIN_VERSION, MsgType, VERSION};
use crate::msg::{self, CodecError, Hello, HelloAck, Invalid, Validate, caps, code};
use crate::tuning::{TcpTuning, TuningReport};

/// A decoded frame: header plus its (aligned) payload bytes.
pub struct Frame {
    pub header: FrameHeader,
    payload: AlignedVec,
}

impl std::fmt::Debug for Frame {
    /// Prints the payload length rather than the payload. Frames carry tokens
    /// and log content, and a `Debug` that dumps them would leak secrets into
    /// error messages and traces.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Frame")
            .field("msg_type", &self.header.msg_type)
            .field("stream_id", &self.header.stream_id)
            .field("flags", &self.header.flags)
            .field("payload_len", &self.payload.len())
            .finish()
    }
}

impl Frame {
    pub fn msg_type(&self) -> MsgType {
        self.header.msg_type
    }

    pub fn stream_id(&self) -> u32 {
        self.header.stream_id
    }

    pub fn payload(&self) -> &AlignedVec {
        &self.payload
    }

    /// Decode the payload as `T`, with validation.
    pub fn decode<T>(&self) -> Result<T, CodecError>
    where
        T: rkyv::Archive,
        T::Archived: for<'a> rkyv::bytecheck::CheckBytes<
                rkyv::api::high::HighValidator<'a, rkyv::rancor::Error>,
            > + rkyv::Deserialize<T, rkyv::api::high::HighDeserializer<rkyv::rancor::Error>>,
    {
        msg::decode(&self.payload)
    }
}

/// A JRP connection over TCP.
pub struct JrpConn {
    stream: TcpStream,
    /// Version agreed during the handshake. Both sides stamp it into every frame.
    version: u16,
    peer_caps: u32,
    next_stream_id: u32,
    /// Client streams are odd, server streams even, so both ends allocate ids
    /// independently without a shared counter.
    client_side: bool,
}

impl JrpConn {
    /// Wrap an accepted socket (server side) and apply the control profile.
    pub fn server(stream: TcpStream) -> (Self, TuningReport) {
        let report = TcpTuning::control().apply(&stream);
        (
            JrpConn {
                stream,
                version: VERSION,
                peer_caps: 0,
                next_stream_id: 2,
                client_side: false,
            },
            report,
        )
    }

    /// Wrap a connected socket (client side) and apply the control profile.
    pub fn client(stream: TcpStream) -> (Self, TuningReport) {
        let report = TcpTuning::control().apply(&stream);
        (
            JrpConn {
                stream,
                version: VERSION,
                peer_caps: 0,
                next_stream_id: 1,
                client_side: true,
            },
            report,
        )
    }

    pub fn version(&self) -> u16 {
        self.version
    }

    pub fn peer_capabilities(&self) -> u32 {
        self.peer_caps
    }

    pub fn peer_supports(&self, cap: u32) -> bool {
        self.peer_caps & cap == cap
    }

    /// Allocate the next stream id for this side.
    pub fn next_stream(&mut self) -> u32 {
        let id = self.next_stream_id;
        // Step by two to stay on this side's parity.
        self.next_stream_id = self.next_stream_id.wrapping_add(2).max(if self.client_side {
            1
        } else {
            2
        });
        id
    }

    /// Send a typed message.
    pub async fn send<T>(
        &mut self,
        msg_type: MsgType,
        stream_id: u32,
        value: &T,
        flags: Flags,
    ) -> Result<(), ConnError>
    where
        T: for<'a> rkyv::Serialize<
                rkyv::api::high::HighSerializer<
                    AlignedVec,
                    rkyv::ser::allocator::ArenaHandle<'a>,
                    rkyv::rancor::Error,
                >,
            >,
    {
        let payload = msg::encode(value)?;
        if payload.len() as u64 > crate::frame::MAX_PAYLOAD as u64 {
            return Err(ConnError::PayloadTooLarge(payload.len()));
        }
        let mut header = FrameHeader::new(msg_type, stream_id, payload.len() as u32);
        header.version = self.version;
        header.flags = flags;

        // One write for header+payload. Two writes with TCP_NODELAY set would put
        // a 16-byte segment on the wire ahead of the body.
        let mut buf = Vec::with_capacity(HEADER_LEN + payload.len());
        buf.extend_from_slice(&header.encode());
        buf.extend_from_slice(payload.as_slice());
        self.stream.write_all(&buf).await?;
        self.stream.flush().await?;
        Ok(())
    }

    /// Read one frame.
    pub async fn recv(&mut self) -> Result<Frame, ConnError> {
        let mut hdr = [0u8; HEADER_LEN];
        self.stream.read_exact(&mut hdr).await?;
        let header = FrameHeader::decode(&hdr)?;

        // The header decoder already refused anything over MAX_PAYLOAD, so this
        // reservation is bounded by a value we chose, not one the peer chose.
        let mut payload = AlignedVec::with_capacity(header.payload_len as usize);
        payload.resize(header.payload_len as usize, 0);
        if header.payload_len > 0 {
            self.stream.read_exact(payload.as_mut_slice()).await?;
        }
        Ok(Frame { header, payload })
    }

    /// Client half of the handshake.
    pub async fn client_handshake(&mut self, agent: &str) -> Result<HelloAck, ConnError> {
        let stream_id = self.next_stream();
        self.send(
            MsgType::Hello,
            stream_id,
            &Hello {
                agent: agent.to_owned(),
                min_version: MIN_VERSION,
                max_version: VERSION,
                capabilities: caps::ALL,
            },
            Flags::NONE,
        )
        .await?;

        let frame = self.recv().await?;
        match frame.msg_type() {
            MsgType::HelloAck => {
                let ack: HelloAck = frame.decode()?;
                if ack.version < MIN_VERSION || ack.version > VERSION {
                    return Err(ConnError::Frame(FrameError::UnsupportedVersion(ack.version)));
                }
                self.version = ack.version;
                self.peer_caps = ack.capabilities;
                Ok(ack)
            }
            MsgType::Error => {
                let e: msg::ErrorMsg = frame.decode()?;
                Err(ConnError::Peer {
                    code: e.code,
                    message: e.message,
                })
            }
            other => Err(ConnError::Unexpected(other)),
        }
    }

    /// Server half of the handshake.
    ///
    /// Negotiates the highest mutually supported version. A peer whose range does
    /// not overlap ours gets an explicit `VERSION_MISMATCH` rather than a
    /// confusing parse failure three frames later.
    pub async fn server_handshake(&mut self, agent: &str, session: &str) -> Result<Hello, ConnError> {
        let frame = self.recv().await?;
        if frame.msg_type() != MsgType::Hello {
            self.send_error(
                frame.stream_id(),
                code::BAD_REQUEST,
                "expected Hello as the first frame",
            )
            .await?;
            return Err(ConnError::Unexpected(frame.msg_type()));
        }
        let hello: Hello = frame.decode()?;

        // Decoding proved the bytes were well-formed; this proves the values are
        // usable. Skipping it would let a garbage frame through as a Hello with
        // nonsense version bounds -- see `msg::limits`.
        if let Err(e) = hello.validate() {
            self.send_error(frame.stream_id(), code::BAD_REQUEST, &e.to_string())
                .await?;
            return Err(ConnError::Invalid(e));
        }

        // Overlap check: the highest version we both speak.
        let negotiated = VERSION.min(hello.max_version);
        if negotiated < MIN_VERSION || negotiated < hello.min_version {
            self.send_error(
                frame.stream_id(),
                code::VERSION_MISMATCH,
                &format!(
                    "peer speaks {}..={}, this server speaks {MIN_VERSION}..={VERSION}",
                    hello.min_version, hello.max_version
                ),
            )
            .await?;
            return Err(ConnError::Frame(FrameError::UnsupportedVersion(
                hello.max_version,
            )));
        }

        self.version = negotiated;
        self.peer_caps = hello.capabilities;

        self.send(
            MsgType::HelloAck,
            frame.stream_id(),
            &HelloAck {
                agent: agent.to_owned(),
                version: negotiated,
                // Intersection: never claim a capability the peer cannot use.
                capabilities: caps::ALL & hello.capabilities,
                session: session.to_owned(),
            },
            Flags::NONE,
        )
        .await?;
        Ok(hello)
    }

    pub async fn send_error(
        &mut self,
        stream_id: u32,
        code: u16,
        message: &str,
    ) -> Result<(), ConnError> {
        self.send(
            MsgType::Error,
            stream_id,
            &msg::ErrorMsg {
                code,
                message: message.to_owned(),
            },
            Flags::END_STREAM,
        )
        .await
    }

    pub async fn ping(&mut self, nonce: u64) -> Result<u64, ConnError> {
        let stream_id = self.next_stream();
        self.send(MsgType::Ping, stream_id, &msg::Ping { nonce }, Flags::NONE)
            .await?;
        let frame = self.recv().await?;
        if frame.msg_type() != MsgType::Pong {
            return Err(ConnError::Unexpected(frame.msg_type()));
        }
        let pong: msg::Pong = frame.decode()?;
        Ok(pong.nonce)
    }

    pub fn into_inner(self) -> TcpStream {
        self.stream
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ConnError {
    #[error("io error: {0}")]
    Io(#[from] io::Error),
    #[error(transparent)]
    Frame(#[from] FrameError),
    #[error(transparent)]
    Codec(#[from] CodecError),
    #[error("unexpected message {0:?}")]
    Unexpected(MsgType),
    #[error("peer reported error {code}: {message}")]
    Peer { code: u16, message: String },
    #[error("payload of {0} bytes exceeds the frame limit")]
    PayloadTooLarge(usize),
    #[error("peer sent a semantically invalid message: {0}")]
    Invalid(#[from] Invalid),
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::net::TcpListener;

    /// Spin up a loopback pair of connected JRP endpoints.
    async fn pair() -> (JrpConn, JrpConn) {
        let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = l.local_addr().unwrap();
        let accept = tokio::spawn(async move { l.accept().await.unwrap().0 });
        let client = TcpStream::connect(addr).await.unwrap();
        let server = accept.await.unwrap();
        (JrpConn::client(client).0, JrpConn::server(server).0)
    }

    #[tokio::test]
    async fn handshake_negotiates_and_exchanges_capabilities() {
        let (mut c, mut s) = pair().await;
        let srv = tokio::spawn(async move {
            let hello = s.server_handshake("jetrun-server/test", "sess-1").await.unwrap();
            (hello, s)
        });
        let ack = c.client_handshake("jet-cli/test").await.unwrap();
        let (hello, _s) = srv.await.unwrap();

        assert_eq!(ack.version, VERSION);
        assert_eq!(ack.session, "sess-1");
        assert_eq!(hello.agent, "jet-cli/test");
        assert_eq!(c.version(), VERSION);
        assert!(c.peer_supports(caps::CAS));
        assert!(c.peer_supports(caps::LOG_TAIL));
    }

    #[tokio::test]
    async fn version_mismatch_is_reported_explicitly() {
        // A future client talking to this server must get a clear error rather
        // than a mysterious framing failure -- this is the rolling-upgrade path.
        let (mut c, mut s) = pair().await;
        let srv = tokio::spawn(async move { s.server_handshake("srv", "sess").await });

        let sid = c.next_stream();
        c.send(
            MsgType::Hello,
            sid,
            &Hello {
                agent: "from-the-future".into(),
                min_version: 99,
                max_version: 99,
                capabilities: 0,
            },
            Flags::NONE,
        )
        .await
        .unwrap();

        let frame = c.recv().await.unwrap();
        assert_eq!(frame.msg_type(), MsgType::Error);
        let e: msg::ErrorMsg = frame.decode().unwrap();
        assert_eq!(e.code, code::VERSION_MISMATCH);
        assert!(e.message.contains("99"), "{}", e.message);
        assert!(srv.await.unwrap().is_err());
    }

    #[tokio::test]
    async fn capabilities_are_intersected_not_asserted() {
        // The server must not claim a capability the client cannot use.
        let (mut c, mut s) = pair().await;
        let srv = tokio::spawn(async move { s.server_handshake("srv", "sess").await.map(|_| s) });

        let sid = c.next_stream();
        c.send(
            MsgType::Hello,
            sid,
            &Hello {
                agent: "minimal-client".into(),
                min_version: MIN_VERSION,
                max_version: VERSION,
                capabilities: caps::LOG_TAIL, // only this one
            },
            Flags::NONE,
        )
        .await
        .unwrap();

        let frame = c.recv().await.unwrap();
        let ack: HelloAck = frame.decode().unwrap();
        assert_eq!(ack.capabilities, caps::LOG_TAIL);
        assert_eq!(ack.capabilities & caps::CAS, 0);
        let _ = srv.await.unwrap();
    }

    #[tokio::test]
    async fn first_frame_must_be_hello() {
        let (mut c, mut s) = pair().await;
        let srv = tokio::spawn(async move { s.server_handshake("srv", "sess").await });

        let sid = c.next_stream();
        c.send(
            MsgType::SubmitRun,
            sid,
            &msg::SubmitRun {
                project: "p".into(),
                pipeline: "ci".into(),
                definition: vec![],
                commit_sha: None,
                reference: None,
                idempotency_key: None,
            },
            Flags::NONE,
        )
        .await
        .unwrap();

        let frame = c.recv().await.unwrap();
        let e: msg::ErrorMsg = frame.decode().unwrap();
        assert_eq!(e.code, code::BAD_REQUEST);
        assert!(srv.await.unwrap().is_err());
    }

    #[tokio::test]
    async fn round_trips_a_real_message() {
        let (mut c, mut s) = pair().await;
        let srv = tokio::spawn(async move {
            s.server_handshake("srv", "sess").await.unwrap();
            let f = s.recv().await.unwrap();
            assert_eq!(f.msg_type(), MsgType::SubmitRun);
            let sub: msg::SubmitRun = f.decode().unwrap();
            s.send(
                MsgType::RunAccepted,
                f.stream_id(),
                &msg::RunAccepted {
                    run_id: "run-1".into(),
                    number: 1,
                    definition_digest: format!("b3:{}", sub.definition.len()),
                    deduplicated: false,
                },
                Flags::END_STREAM,
            )
            .await
            .unwrap();
        });

        c.client_handshake("cli").await.unwrap();
        let sid = c.next_stream();
        c.send(
            MsgType::SubmitRun,
            sid,
            &msg::SubmitRun {
                project: "web".into(),
                pipeline: "ci".into(),
                definition: b"jobs: {}".to_vec(),
                commit_sha: None,
                reference: None,
                idempotency_key: None,
            },
            Flags::NONE,
        )
        .await
        .unwrap();

        let frame = c.recv().await.unwrap();
        assert_eq!(frame.stream_id(), sid, "response must match the request stream");
        assert!(frame.header.flags.contains(Flags::END_STREAM));
        let acc: msg::RunAccepted = frame.decode().unwrap();
        assert_eq!(acc.definition_digest, "b3:8");
        srv.await.unwrap();
    }

    #[tokio::test]
    async fn ping_pong_preserves_the_nonce() {
        let (mut c, mut s) = pair().await;
        let srv = tokio::spawn(async move {
            s.server_handshake("srv", "sess").await.unwrap();
            let f = s.recv().await.unwrap();
            let p: msg::Ping = f.decode().unwrap();
            s.send(
                MsgType::Pong,
                f.stream_id(),
                &msg::Pong { nonce: p.nonce },
                Flags::NONE,
            )
            .await
            .unwrap();
        });
        c.client_handshake("cli").await.unwrap();
        assert_eq!(c.ping(0xDEADBEEF).await.unwrap(), 0xDEADBEEF);
        srv.await.unwrap();
    }

    #[tokio::test]
    async fn stream_ids_keep_their_parity() {
        let (mut c, mut s) = pair().await;
        for _ in 0..5 {
            assert_eq!(c.next_stream() % 2, 1, "client streams must be odd");
            assert_eq!(s.next_stream() % 2, 0, "server streams must be even");
        }
    }

    #[tokio::test]
    async fn zero_length_payload_is_valid() {
        let (c, mut s) = pair().await;
        let srv = tokio::spawn(async move {
            let f = s.recv().await.unwrap();
            assert_eq!(f.header.payload_len, 0);
            f.msg_type()
        });
        // Goodbye with an empty reason still encodes to a non-empty archive, so
        // craft a genuinely empty frame by hand.
        let hdr = FrameHeader::new(MsgType::Goodbye, 1, 0);
        let mut raw = c.into_inner();
        raw.write_all(&hdr.encode()).await.unwrap();
        raw.flush().await.unwrap();
        assert_eq!(srv.await.unwrap(), MsgType::Goodbye);
    }

    #[tokio::test]
    async fn garbage_on_the_wire_is_rejected_at_the_header() {
        // Someone pointing curl at the JRP port.
        let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = l.local_addr().unwrap();
        let srv = tokio::spawn(async move {
            let (sock, _) = l.accept().await.unwrap();
            let (mut conn, _) = JrpConn::server(sock);
            conn.recv().await
        });

        let mut raw = TcpStream::connect(addr).await.unwrap();
        raw.write_all(b"GET / HTTP/1.1\r\nHost: x\r\n\r\n").await.unwrap();
        raw.flush().await.unwrap();

        let err = srv.await.unwrap().expect_err("must reject non-JRP bytes");
        assert!(
            matches!(err, ConnError::Frame(FrameError::BadMagic(_))),
            "expected BadMagic, got {err:?}"
        );
    }

    #[tokio::test]
    async fn oversized_declared_payload_is_refused_without_reading_it() {
        let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = l.local_addr().unwrap();
        let srv = tokio::spawn(async move {
            let (sock, _) = l.accept().await.unwrap();
            let (mut conn, _) = JrpConn::server(sock);
            conn.recv().await
        });

        let mut raw = TcpStream::connect(addr).await.unwrap();
        let mut hdr = FrameHeader::new(MsgType::SubmitRun, 1, 0).encode();
        hdr[12..16].copy_from_slice(&u32::MAX.to_le_bytes());
        raw.write_all(&hdr).await.unwrap();
        raw.flush().await.unwrap();

        let err = srv.await.unwrap().expect_err("must refuse a hostile length");
        assert!(matches!(
            err,
            ConnError::Frame(FrameError::PayloadTooLarge(_))
        ));
    }

    #[tokio::test]
    async fn tuning_is_applied_to_accepted_sockets() {
        let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = l.local_addr().unwrap();
        let accept = tokio::spawn(async move { l.accept().await.unwrap().0 });
        let _c = TcpStream::connect(addr).await.unwrap();
        let (_conn, report) = JrpConn::server(accept.await.unwrap());
        assert!(report.applied("TCP_NODELAY"), "{}", report.summary());
        assert!(report.applied("TCP_NOTSENT_LOWAT"), "{}", report.summary());
    }
}
