//! The jetrun binary: server, worker, and CLI in one artifact.
//!
//! One binary rather than three because the executor must be identical in all of
//! them -- `jet run` on a laptop and a worker in a datacenter share `jet-exec` and
//! the same CAS, which is what makes a local run share cache hits with CI. Jenkins
//! fundamentally cannot reproduce a build on your machine; that property falls out
//! of shipping one artifact and never letting the executor depend on the server.

use std::net::SocketAddr;
use std::sync::Arc;

use clap::{Parser, Subcommand};
use jet_proto::{DEFAULT_HTTP_PORT, DEFAULT_JRP_PORT, JrpConn, MsgType, msg};
use jet_server::{ServerState, WebhookSecrets};
use jet_server::webhook::Provider;

// The workload is allocation-heavy (hashing buffers, tree manifests, trace
// records), which is where the system allocator's central locks hurt most under a
// thread-per-core layout. See the allocator discussion in the plan: this is the
// default, and it is a one-line change to A/B against jemalloc or snmalloc once
// the tracer exists and there is something meaningful to measure.
#[global_allocator]
static ALLOC: mimalloc::MiMalloc = mimalloc::MiMalloc;

#[derive(Parser)]
#[command(name = "jetrun", version, about = "A blazingly fast build system")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Run the control plane: public HTTP listener plus internal JRP listener.
    Serve {
        /// Public HTTP/JSON listener. Faces the internet to receive webhooks.
        #[arg(long, default_value_t = DEFAULT_HTTP_PORT, env = "JETRUN_HTTP_PORT")]
        http_port: u16,

        /// Internal JRP listener for the CLI and workers. Should be firewalled to
        /// trusted networks -- it is a separate port precisely so that it can be.
        #[arg(long, default_value_t = DEFAULT_JRP_PORT, env = "JETRUN_JRP_PORT")]
        jrp_port: u16,

        /// Bind address. Defaults to loopback rather than 0.0.0.0 so an
        /// accidental `serve` does not expose the control plane to the network.
        #[arg(long, default_value = "127.0.0.1", env = "JETRUN_BIND")]
        bind: String,

        #[arg(long, default_value = ".jetrun/jetrun.db", env = "JETRUN_DB")]
        db: String,

        /// GitHub webhook secret (HMAC-SHA256 key).
        #[arg(long, env = "JETRUN_GITHUB_WEBHOOK_SECRET")]
        github_secret: Option<String>,

        /// GitLab webhook token. Note GitLab does not sign payloads, so this is a
        /// bearer secret and TLS is mandatory rather than advisable.
        #[arg(long, env = "JETRUN_GITLAB_WEBHOOK_SECRET")]
        gitlab_secret: Option<String>,

        /// Bitbucket webhook secret (HMAC-SHA256 key).
        #[arg(long, env = "JETRUN_BITBUCKET_WEBHOOK_SECRET")]
        bitbucket_secret: Option<String>,

        /// Map a repository to a pipeline, as `repo=project/pipeline`. Repeatable.
        #[arg(long = "map", value_name = "REPO=PROJECT/PIPELINE")]
        maps: Vec<String>,
    },

    /// Check connectivity to a server over JRP.
    Ping {
        #[arg(long, default_value = "127.0.0.1", env = "JETRUN_SERVER")]
        host: String,
        #[arg(long, default_value_t = DEFAULT_JRP_PORT)]
        port: u16,
    },

    /// Submit a pipeline definition over JRP.
    Submit {
        #[arg(long, default_value = "127.0.0.1", env = "JETRUN_SERVER")]
        host: String,
        #[arg(long, default_value_t = DEFAULT_JRP_PORT)]
        port: u16,
        #[arg(long)]
        project: String,
        #[arg(long)]
        pipeline: String,
        /// Path to the pipeline YAML. Sent inline, so an uncommitted definition
        /// still runs -- and is still content-addressed and reproducible.
        #[arg(long, default_value = "jetrun.yaml")]
        file: String,
        #[arg(long, env = "JETRUN_TOKEN")]
        token: Option<String>,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_env("JETRUN_LOG")
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    match Cli::parse().command {
        Command::Serve {
            http_port,
            jrp_port,
            bind,
            db,
            github_secret,
            gitlab_secret,
            bitbucket_secret,
            maps,
        } => {
            let mut secrets = WebhookSecrets::default();
            for (provider, secret) in [
                (Provider::GitHub, github_secret),
                (Provider::GitLab, gitlab_secret),
                (Provider::Bitbucket, bitbucket_secret),
            ] {
                match secret {
                    Some(s) if !s.is_empty() => secrets.set(provider, s),
                    // Not configured means that provider's endpoint refuses every
                    // delivery, which is the correct posture: an unconfigured
                    // secret must never mean "accept anything from the internet".
                    _ => tracing::warn!(
                        %provider,
                        "no webhook secret configured; deliveries for this provider will be refused"
                    ),
                }
            }

            let database = jet_store::Db::open(&db).await?;
            jet_store::bootstrap(&database).await?;
            let mut state = ServerState::new(database, secrets);

            for m in &maps {
                let (repo, target) = m
                    .split_once('=')
                    .ok_or_else(|| anyhow::anyhow!("--map expects REPO=PROJECT/PIPELINE, got {m:?}"))?;
                let (project, pipeline) = target.split_once('/').ok_or_else(|| {
                    anyhow::anyhow!("--map target expects PROJECT/PIPELINE, got {target:?}")
                })?;
                state.map_repo(repo, project, pipeline);
                tracing::info!(%repo, %project, %pipeline, "mapped repository");
            }

            let http_addr: SocketAddr = format!("{bind}:{http_port}").parse()?;
            let jrp_addr: SocketAddr = format!("{bind}:{jrp_port}").parse()?;

            let (tx, rx) = tokio::sync::watch::channel(false);
            tokio::spawn(async move {
                if tokio::signal::ctrl_c().await.is_ok() {
                    tracing::info!("shutdown requested");
                    let _ = tx.send(true);
                }
            });

            jet_server::serve(Arc::new(state), http_addr, jrp_addr, rx).await?;
            Ok(())
        }

        Command::Ping { host, port } => {
            let mut conn = connect(&host, port).await?;
            let ack = conn
                .client_handshake(concat!("jet-cli/", env!("CARGO_PKG_VERSION")))
                .await?;
            let nonce = std::process::id() as u64;
            let echoed = conn.ping(nonce).await?;
            anyhow::ensure!(echoed == nonce, "server echoed the wrong nonce");
            println!(
                "ok  jrp v{}  session {}  server {}",
                ack.version, ack.session, ack.agent
            );
            Ok(())
        }

        Command::Submit {
            host,
            port,
            project,
            pipeline,
            file,
            token,
        } => {
            let definition = std::fs::read(&file)
                .map_err(|e| anyhow::anyhow!("cannot read {file}: {e}"))?;

            let mut conn = connect(&host, port).await?;
            conn.client_handshake(concat!("jet-cli/", env!("CARGO_PKG_VERSION")))
                .await?;

            if let Some(token) = token {
                let sid = conn.next_stream();
                conn.send(
                    MsgType::Auth,
                    sid,
                    &msg::Auth { token },
                    jet_proto::Flags::NONE,
                )
                .await?;
                let frame = conn.recv().await?;
                if frame.msg_type() == MsgType::Error {
                    let e: msg::ErrorMsg = frame.decode()?;
                    anyhow::bail!("authentication failed: {}", e.message);
                }
            }

            let sid = conn.next_stream();
            conn.send(
                MsgType::SubmitRun,
                sid,
                &msg::SubmitRun {
                    project,
                    pipeline,
                    definition,
                    commit_sha: None,
                    reference: None,
                    // Makes a retried submit return the original run instead of
                    // starting a second build.
                    idempotency_key: Some(jet_core::id::Ulid::generate().to_string()),
                },
                jet_proto::Flags::NONE,
            )
            .await?;

            let frame = conn.recv().await?;
            match frame.msg_type() {
                MsgType::RunAccepted => {
                    let acc: msg::RunAccepted = frame.decode()?;
                    println!(
                        "run #{} accepted ({}){}",
                        acc.number,
                        acc.run_id,
                        if acc.deduplicated { " [deduplicated]" } else { "" }
                    );
                    Ok(())
                }
                MsgType::Error => {
                    let e: msg::ErrorMsg = frame.decode()?;
                    anyhow::bail!("server refused the run (code {}): {}", e.code, e.message)
                }
                other => anyhow::bail!("unexpected reply {other:?}"),
            }
        }
    }
}

async fn connect(host: &str, port: u16) -> anyhow::Result<JrpConn> {
    let sock = tokio::net::TcpStream::connect((host, port))
        .await
        .map_err(|e| anyhow::anyhow!("cannot reach jetrun at {host}:{port}: {e}"))?;
    let (conn, tuning) = JrpConn::client(sock);
    tracing::debug!(tuning = %tuning.summary(), "connected");
    Ok(conn)
}
