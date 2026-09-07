use std::sync::Arc;
use std::sync::atomic::{AtomicU16, Ordering};

use bytes::Bytes;

use crate::error::WireError;
use crate::frame::{Frame, MessageType};
use crate::message::{WireRequest, WireResponse};
use crate::pool::ConnectionPool;

/// RPC client for sending requests over the jetrun wire protocol.
/// Uses a connection pool for persistent TCP connections.
pub struct WireClient {
    pool: Arc<ConnectionPool>,
    next_request_id: AtomicU16,
}

impl WireClient {
    pub fn new(pool: Arc<ConnectionPool>) -> Self {
        Self {
            pool,
            next_request_id: AtomicU16::new(1),
        }
    }

    /// Send a request and wait for the response.
    pub async fn call(&self, request: &WireRequest) -> Result<WireResponse, WireError> {
        let request_id = self.next_request_id.fetch_add(1, Ordering::Relaxed);

        // Serialize with rkyv
        let payload = rkyv::to_bytes::<rkyv::rancor::Error>(request)
            .map_err(|e| WireError::Serialize(e.to_string()))?;

        let frame = Frame::new(
            MessageType::Request,
            request_id,
            Bytes::from(payload.into_vec()),
        );

        // Acquire connection, send frame, read response
        let mut conn = self.pool.acquire().await?;
        let codec = conn.codec();

        codec.send_frame(&frame).await?;

        // Read response frame
        let response_frame = codec
            .read_frame()
            .await?
            .ok_or(WireError::ConnectionClosed)?;

        // Return connection to pool
        let codec = conn.take();
        self.pool.release(codec).await;

        // Deserialize response (zero-copy access)
        let response = rkyv::from_bytes::<WireResponse, rkyv::rancor::Error>(&response_frame.payload)
            .map_err(|e| WireError::Deserialize(e.to_string()))?;

        Ok(response)
    }

    /// Send a request that returns a stream of responses.
    /// Returns a receiver that yields response frames until StreamEnd.
    pub async fn call_stream(
        &self,
        request: &WireRequest,
    ) -> Result<StreamReceiver, WireError> {
        let request_id = self.next_request_id.fetch_add(1, Ordering::Relaxed);

        let payload = rkyv::to_bytes::<rkyv::rancor::Error>(request)
            .map_err(|e| WireError::Serialize(e.to_string()))?;

        let frame = Frame::new(
            MessageType::Request,
            request_id,
            Bytes::from(payload.into_vec()),
        );

        let mut conn = self.pool.acquire().await?;
        let codec = conn.codec();
        codec.send_frame(&frame).await?;

        let (tx, rx) = tokio::sync::mpsc::channel(256);

        // Spawn a task to read stream frames
        let mut codec = conn.take();
        tokio::spawn(async move {
            loop {
                match codec.read_frame().await {
                    Ok(Some(frame)) => {
                        let is_last = frame.flags.contains(crate::frame::Flags::LAST_FRAME)
                            || frame.msg_type == MessageType::StreamEnd;

                        let _ = tx.send(Ok(frame)).await;

                        if is_last {
                            break;
                        }
                    }
                    Ok(None) => break,
                    Err(e) => {
                        let _ = tx.send(Err(e)).await;
                        break;
                    }
                }
            }
            // Connection is dropped here (not returned to pool for stream connections)
        });

        Ok(StreamReceiver { rx })
    }
}

/// Receives streaming frames from a wire protocol stream call.
pub struct StreamReceiver {
    rx: tokio::sync::mpsc::Receiver<Result<Frame, WireError>>,
}

impl StreamReceiver {
    /// Receive the next frame. Returns None when the stream ends.
    pub async fn recv(&mut self) -> Option<Result<Frame, WireError>> {
        self.rx.recv().await
    }
}
