use std::future::Future;
use std::sync::Arc;

use bytes::Bytes;
use tokio::net::TcpListener;

use crate::codec::FrameCodec;
use crate::error::WireError;
use crate::frame::{Frame, MessageType};
use crate::message::{WireRequest, WireResponse};

/// Handler function type for processing wire protocol requests.
pub type RequestHandler = Arc<
    dyn Fn(WireRequest) -> std::pin::Pin<Box<dyn Future<Output = WireResponse> + Send>>
        + Send
        + Sync,
>;

/// Wire protocol server. Listens on a TCP port and dispatches
/// incoming requests to a handler function.
pub struct WireServer {
    listener: TcpListener,
    handler: RequestHandler,
}

impl WireServer {
    /// Bind to an address and create a new server.
    pub async fn bind(addr: &str, handler: RequestHandler) -> Result<Self, WireError> {
        let listener = TcpListener::bind(addr).await?;
        tracing::info!(addr = %addr, "jetrun-wire server listening");
        Ok(Self { listener, handler })
    }

    /// Accept connections and process requests.
    pub async fn serve(self) -> Result<(), WireError> {
        loop {
            let (stream, peer_addr) = self.listener.accept().await?;
            stream.set_nodelay(true)?;

            tracing::debug!(peer = %peer_addr, "wire connection accepted");

            let handler = self.handler.clone();
            tokio::spawn(async move {
                if let Err(e) = handle_connection(FrameCodec::new(stream), handler).await {
                    tracing::debug!(peer = %peer_addr, error = %e, "wire connection ended");
                }
            });
        }
    }
}

async fn handle_connection(
    mut codec: FrameCodec,
    handler: RequestHandler,
) -> Result<(), WireError> {
    loop {
        let frame = match codec.read_frame().await? {
            Some(f) => f,
            None => return Ok(()), // Clean close
        };

        match frame.msg_type {
            MessageType::Ping => {
                codec.send_frame(&Frame::pong()).await?;
            }
            MessageType::Request => {
                let request_id = frame.request_id;

                // Deserialize request
                let request =
                    rkyv::from_bytes::<WireRequest, rkyv::rancor::Error>(&frame.payload)
                        .map_err(|e| WireError::Deserialize(e.to_string()))?;

                // Dispatch to handler
                let response = (handler)(request).await;

                // Serialize response
                let payload = rkyv::to_bytes::<rkyv::rancor::Error>(&response)
                    .map_err(|e| WireError::Serialize(e.to_string()))?;

                let response_frame = Frame::new(
                    MessageType::Response,
                    request_id,
                    Bytes::from(payload.into_vec()),
                );

                codec.send_frame(&response_frame).await?;
            }
            other => {
                tracing::warn!(msg_type = ?other, "unexpected frame type from client");
            }
        }
    }
}
