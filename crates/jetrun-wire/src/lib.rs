//! jetrun-wire: Custom binary protocol for inter-service communication.
//!
//! ## Why not gRPC?
//!
//! gRPC adds HTTP/2 framing, HPACK header encoding, and protobuf deserialization
//! overhead on every call (~50-100us). For a CI/CD system where cache lookups and
//! log streaming happen thousands of times per build, this overhead is significant.
//!
//! jetrun-wire uses:
//! - **16-byte fixed header** with magic bytes, version, and 4-billion stream IDs
//! - **rkyv zero-copy deserialization** (vs protobuf allocating owned structs)
//! - **Raw TCP with connection pooling** and platform-specific socket tuning
//! - **Separate control (low-latency) and bulk (high-throughput) connection profiles**
//! - **~5-10us per call** (vs ~50-100us for gRPC)
//!
//! ## Usage
//!
//! ```rust,no_run
//! use jetrun_wire::{WireClient, WireServer, WireRequest, WireResponse};
//! use jetrun_wire::pool::ConnectionPool;
//! use std::sync::Arc;
//!
//! // Server
//! let handler = Arc::new(|req: WireRequest| Box::pin(async move {
//!     WireResponse::Ok
//! }) as std::pin::Pin<Box<dyn std::future::Future<Output = WireResponse> + Send>>);
//! // let server = WireServer::bind("0.0.0.0:9010", handler).await?;
//!
//! // Client
//! let pool = Arc::new(ConnectionPool::new("127.0.0.1:9010".into(), 4));
//! let client = WireClient::new(pool);
//! // let response = client.call(&WireRequest::CacheStats).await?;
//! ```

pub mod client;
pub mod codec;
pub mod error;
pub mod frame;
pub mod message;
pub mod pool;
pub mod server;
pub mod tuning;

pub use client::WireClient;
pub use error::WireError;
pub use frame::{Flags, Frame, MessageType};
pub use message::{WireRequest, WireResponse};
pub use server::WireServer;
