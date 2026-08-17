//! The internal JRP listener, for the `jet` CLI and for workers.
//!
//! Separate port from HTTP, and that separation is a security boundary rather
//! than tidiness: the HTTP listener must face the internet to receive webhooks,
//! while this one should only ever be reachable by workers and authenticated
//! CLIs. Two ports are trivial to firewall differently; two paths on one port are
//! not.

use std::sync::Arc;

use jet_proto::msg::{self, Validate, code};
use jet_proto::{Flags, JrpConn, MsgType};
use tokio::net::{TcpListener, TcpStream};

use crate::state::SharedState;

/// Per-connection session.
struct Session {
    conn: JrpConn,
    /// Set once [`MsgType::Auth`] succeeds. Until then only handshake messages
    /// are served; see [`MsgType::allowed_preauth`].
    authenticated: bool,
    peer: String,
}

/// Accept JRP connections until `shutdown` resolves.
pub async fn serve(
    listener: TcpListener,
    state: SharedState,
    mut shutdown: tokio::sync::watch::Receiver<bool>,
) -> std::io::Result<()> {
    let local = listener.local_addr()?;
    tracing::info!(%local, "jrp listener started");

    loop {
        tokio::select! {
            res = listener.accept() => {
                let (sock, peer) = match res {
                    Ok(v) => v,
                    Err(e) => {
                        // A failed accept is usually per-connection (EMFILE, the
                        // peer vanishing between SYN and accept); tearing down the
                        // listener would turn that into an outage.
                        tracing::warn!(error = %e, "jrp accept failed");
                        continue;
                    }
                };
                let state = Arc::clone(&state);
                tokio::spawn(async move {
                    if let Err(e) = handle(sock, peer.to_string(), state).await {
                        tracing::debug!(%peer, error = %e, "jrp session ended");
                    }
                });
            }
            _ = shutdown.changed() => {
                if *shutdown.borrow() {
                    tracing::info!("jrp listener shutting down");
                    return Ok(());
                }
            }
        }
    }
}

async fn handle(sock: TcpStream, peer: String, state: SharedState) -> Result<(), SessionError> {
    let (conn, tuning) = JrpConn::server(sock);
    // Logged once per connection at debug: a fleet silently running without BBR or
    // without TCP_NOTSENT_LOWAT is otherwise invisible.
    tracing::debug!(%peer, tuning = %tuning.summary(), "jrp connection accepted");

    let session_id = jet_core::id::Ulid::generate().to_string();
    let mut s = Session {
        conn,
        authenticated: false,
        peer,
    };

    let hello = s
        .conn
        .server_handshake(concat!("jetrun-server/", env!("CARGO_PKG_VERSION")), &session_id)
        .await?;
    tracing::debug!(peer = %s.peer, agent = %hello.agent, "jrp handshake complete");

    loop {
        let frame = match s.conn.recv().await {
            Ok(f) => f,
            // A closed connection is the normal way a session ends.
            Err(jet_proto::ConnError::Io(e))
                if e.kind() == std::io::ErrorKind::UnexpectedEof
                    || e.kind() == std::io::ErrorKind::ConnectionReset =>
            {
                return Ok(());
            }
            Err(e) => return Err(e.into()),
        };

        // The single gate for pre-auth access. Checked here rather than in each
        // handler, because a handler that forgot the check would look exactly like
        // the ones that did not.
        if !s.authenticated && !frame.msg_type().allowed_preauth() {
            s.conn
                .send_error(
                    frame.stream_id(),
                    code::AUTH_REQUIRED,
                    "authenticate before issuing this request",
                )
                .await?;
            continue;
        }

        let stream_id = frame.stream_id();
        match frame.msg_type() {
            MsgType::Ping => {
                let p: msg::Ping = frame.decode()?;
                s.conn
                    .send(MsgType::Pong, stream_id, &msg::Pong { nonce: p.nonce }, Flags::NONE)
                    .await?;
            }

            MsgType::Auth => {
                let auth: msg::Auth = frame.decode()?;
                // Bound the token before Argon2 sees it.
                if let Err(e) = auth.validate() {
                    s.conn
                        .send_error(stream_id, code::BAD_REQUEST, &e.to_string())
                        .await?;
                    continue;
                }
                match state.authenticate_token(&auth.token).await {
                    Ok(()) => {
                        s.authenticated = true;
                        s.conn
                            .send(
                                MsgType::AuthOk,
                                stream_id,
                                &msg::AuthOk {
                                    org: String::new(),
                                    principal_kind: "service_account".into(),
                                    principal_id: String::new(),
                                    permissions: vec![],
                                },
                                Flags::END_STREAM,
                            )
                            .await?;
                    }
                    Err(e) => {
                        // Deliberately uniform: a malformed token and an unknown
                        // token produce the same wire response, so the error does
                        // not become an oracle for which prefixes exist.
                        tracing::debug!(peer = %s.peer, error = %e, "jrp auth failed");
                        s.conn
                            .send_error(stream_id, code::UNAUTHENTICATED, "authentication failed")
                            .await?;
                    }
                }
            }

            MsgType::SubmitRun => {
                let sub: msg::SubmitRun = frame.decode()?;
                if let Err(e) = sub.validate() {
                    s.conn
                        .send_error(stream_id, code::BAD_REQUEST, &e.to_string())
                        .await?;
                    continue;
                }
                // TODO(jet-sched): content-address the definition, resolve the
                // pipeline, insert the run.
                s.conn
                    .send_error(stream_id, code::INTERNAL, "run submission is not wired up yet")
                    .await?;
            }

            MsgType::GetRun | MsgType::CancelRun | MsgType::TailLogs | MsgType::CasQuery => {
                s.conn
                    .send_error(stream_id, code::INTERNAL, "not implemented yet")
                    .await?;
            }

            MsgType::Goodbye => return Ok(()),

            other => {
                s.conn
                    .send_error(
                        stream_id,
                        code::BAD_REQUEST,
                        &format!("{other:?} is not valid from a client"),
                    )
                    .await?;
            }
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum SessionError {
    #[error(transparent)]
    Conn(#[from] jet_proto::ConnError),
    #[error(transparent)]
    Codec(#[from] jet_proto::msg::CodecError),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::ServerState;
    use jet_proto::msg::caps;

    /// Start a listener on an ephemeral port and return its address plus a
    /// shutdown handle.
    async fn spawn_server() -> (std::net::SocketAddr, tokio::sync::watch::Sender<bool>) {
        let state = Arc::new(ServerState::for_test("secret").await);
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let (tx, rx) = tokio::sync::watch::channel(false);
        tokio::spawn(serve(listener, state, rx));
        (addr, tx)
    }

    async fn connect(addr: std::net::SocketAddr) -> JrpConn {
        let sock = TcpStream::connect(addr).await.unwrap();
        JrpConn::client(sock).0
    }

    #[tokio::test]
    async fn handshake_succeeds_against_the_real_listener() {
        let (addr, _sd) = spawn_server().await;
        let mut c = connect(addr).await;
        let ack = c.client_handshake("jet-cli/test").await.unwrap();
        assert_eq!(ack.version, jet_proto::VERSION);
        assert!(!ack.session.is_empty(), "server should assign a session id");
        assert!(ack.capabilities & caps::CAS != 0);
    }

    #[tokio::test]
    async fn ping_works_before_authentication() {
        // Liveness must not require credentials, or a worker cannot report health
        // while its token is being rotated.
        let (addr, _sd) = spawn_server().await;
        let mut c = connect(addr).await;
        c.client_handshake("jet-cli/test").await.unwrap();
        assert_eq!(c.ping(42).await.unwrap(), 42);
    }

    #[tokio::test]
    async fn data_requests_are_refused_before_authentication() {
        // The central authorization gate on this listener.
        let (addr, _sd) = spawn_server().await;
        let mut c = connect(addr).await;
        c.client_handshake("jet-cli/test").await.unwrap();

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
        assert_eq!(frame.msg_type(), MsgType::Error);
        let e: msg::ErrorMsg = frame.decode().unwrap();
        assert_eq!(e.code, code::AUTH_REQUIRED);
    }

    #[tokio::test]
    async fn every_privileged_message_is_gated() {
        let (addr, _sd) = spawn_server().await;
        let mut c = connect(addr).await;
        c.client_handshake("jet-cli/test").await.unwrap();

        // GetRun, CancelRun, TailLogs, CasQuery must all be refused pre-auth.
        for (ty, send) in [
            (MsgType::GetRun, 0u8),
            (MsgType::CancelRun, 1),
            (MsgType::TailLogs, 2),
            (MsgType::CasQuery, 3),
        ] {
            let sid = c.next_stream();
            match send {
                0 => c
                    .send(ty, sid, &msg::GetRun { run_id: "r".into() }, Flags::NONE)
                    .await
                    .unwrap(),
                1 => c
                    .send(ty, sid, &msg::CancelRun { run_id: "r".into() }, Flags::NONE)
                    .await
                    .unwrap(),
                2 => c
                    .send(
                        ty,
                        sid,
                        &msg::TailLogs {
                            run_id: "r".into(),
                            step: None,
                            from_offset: 0,
                            follow: false,
                        },
                        Flags::NONE,
                    )
                    .await
                    .unwrap(),
                _ => c
                    .send(ty, sid, &msg::CasQuery { digests: vec![] }, Flags::NONE)
                    .await
                    .unwrap(),
            }
            let frame = c.recv().await.unwrap();
            let e: msg::ErrorMsg = frame.decode().unwrap();
            assert_eq!(e.code, code::AUTH_REQUIRED, "{ty:?} was not gated");
        }
    }

    #[tokio::test]
    async fn bad_token_is_rejected_uniformly() {
        // A malformed token and an unknown one must be indistinguishable on the
        // wire, or the error becomes an oracle for which prefixes exist.
        let (addr, _sd) = spawn_server().await;
        let mut c = connect(addr).await;
        c.client_handshake("jet-cli/test").await.unwrap();

        let mut responses = Vec::new();
        for token in ["garbage", "jetr_abcd1234_wrongsecret"] {
            let sid = c.next_stream();
            c.send(
                MsgType::Auth,
                sid,
                &msg::Auth {
                    token: token.into(),
                },
                Flags::NONE,
            )
            .await
            .unwrap();
            let frame = c.recv().await.unwrap();
            let e: msg::ErrorMsg = frame.decode().unwrap();
            responses.push((e.code, e.message));
        }
        assert_eq!(responses[0], responses[1], "auth failures must be uniform");
        assert_eq!(responses[0].0, code::UNAUTHENTICATED);
    }

    #[tokio::test]
    async fn oversized_token_is_refused_before_the_kdf() {
        let (addr, _sd) = spawn_server().await;
        let mut c = connect(addr).await;
        c.client_handshake("jet-cli/test").await.unwrap();

        let sid = c.next_stream();
        c.send(
            MsgType::Auth,
            sid,
            &msg::Auth {
                token: "x".repeat(jet_proto::limits::MAX_TOKEN_LEN + 1),
            },
            Flags::NONE,
        )
        .await
        .unwrap();
        let frame = c.recv().await.unwrap();
        let e: msg::ErrorMsg = frame.decode().unwrap();
        assert_eq!(e.code, code::BAD_REQUEST);
    }

    #[tokio::test]
    async fn non_jrp_bytes_do_not_wedge_the_listener() {
        // Someone points curl at the internal port; the listener must survive and
        // keep serving real clients.
        let (addr, _sd) = spawn_server().await;
        {
            use tokio::io::AsyncWriteExt;
            let mut junk = TcpStream::connect(addr).await.unwrap();
            junk.write_all(b"GET / HTTP/1.1\r\n\r\n").await.unwrap();
            junk.flush().await.unwrap();
        }
        let mut c = connect(addr).await;
        assert!(c.client_handshake("jet-cli/test").await.is_ok());
    }

    #[tokio::test]
    async fn many_concurrent_sessions_are_served() {
        let (addr, _sd) = spawn_server().await;
        let mut tasks = Vec::new();
        for i in 0..16u64 {
            tasks.push(tokio::spawn(async move {
                let mut c = connect(addr).await;
                c.client_handshake("jet-cli/test").await.unwrap();
                c.ping(i).await.unwrap()
            }));
        }
        for (i, t) in tasks.into_iter().enumerate() {
            assert_eq!(t.await.unwrap(), i as u64);
        }
    }

    #[tokio::test]
    async fn shutdown_signal_stops_the_listener() {
        let (addr, sd) = spawn_server().await;
        let mut c = connect(addr).await;
        c.client_handshake("jet-cli/test").await.unwrap();

        sd.send(true).unwrap();
        // Give the select! a moment to observe the change.
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        // New connections should no longer be accepted.
        let result = tokio::time::timeout(std::time::Duration::from_millis(200), async {
            let sock = TcpStream::connect(addr).await?;
            let (mut conn, _) = JrpConn::client(sock);
            conn.client_handshake("late").await.map_err(std::io::Error::other)
        })
        .await;
        assert!(
            result.is_err() || result.unwrap().is_err(),
            "listener should have stopped accepting"
        );
    }

    #[tokio::test]
    async fn client_disconnect_is_not_an_error() {
        let (addr, _sd) = spawn_server().await;
        let mut c = connect(addr).await;
        c.client_handshake("jet-cli/test").await.unwrap();
        drop(c);
        // The server task should end cleanly; nothing to assert beyond not
        // panicking, which a failed task would surface in the runtime.
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
}
