//! jetrun-wire: Custom binary protocol for inter-service communication.
//!
//! ## Why not gRPC?
//!
//! gRPC adds HTTP/2 framing, HPACK header encoding, and protobuf deserialization
//! overhead on every call (~50-100μs). For a CI/CD system where cache lookups and
//! log streaming happen thousands of times per build, this overhead is significant.
//!
//! jetrun-wire uses:
//! - **8-byte fixed header** (vs 9+ bytes HTTP/2 frame + headers)
//! - **rkyv zero-copy deserialization** (vs protobuf allocating owned structs)
//! - **Raw TCP with connection pooling** (vs HTTP/2 stream multiplexing)
//! - **~5-10μs per call** (vs ~50-100μs for gRPC)
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

pub use client::WireClient;
pub use error::WireError;
pub use frame::{Flags, Frame, MessageType};
pub use message::{WireRequest, WireResponse};
pub use server::WireServer;
