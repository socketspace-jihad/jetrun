use bytes::BytesMut;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

use crate::error::WireError;
use crate::frame::Frame;

/// Buffered frame reader/writer over a TCP stream.
/// Handles partial reads and frame reassembly.
pub struct FrameCodec {
    stream: TcpStream,
    read_buf: BytesMut,
    write_buf: BytesMut,
}

impl FrameCodec {
    pub fn new(stream: TcpStream) -> Self {
        Self {
            stream,
            read_buf: BytesMut::with_capacity(16384),
            write_buf: BytesMut::with_capacity(16384),
        }
    }

    /// Read the next frame from the stream.
    /// Returns None if the connection is closed.
    pub async fn read_frame(&mut self) -> Result<Option<Frame>, WireError> {
        loop {
            // Try to decode a frame from buffered data
            if let Some(frame) = Frame::decode(&mut self.read_buf)? {
                return Ok(Some(frame));
            }

            // Need more data — read from TCP
            let n = self.stream.read_buf(&mut self.read_buf).await?;
            if n == 0 {
                if self.read_buf.is_empty() {
                    return Ok(None); // Clean close
                } else {
                    return Err(WireError::ConnectionClosed);
                }
            }
        }
    }

    /// Write a frame to the stream.
    pub async fn write_frame(&mut self, frame: &Frame) -> Result<(), WireError> {
        self.write_buf.clear();
        frame.encode(&mut self.write_buf);
        self.stream.write_all(&self.write_buf).await?;
        Ok(())
    }

    /// Flush the underlying TCP stream.
    pub async fn flush(&mut self) -> Result<(), WireError> {
        self.stream.flush().await?;
        Ok(())
    }

    /// Write a frame and flush immediately.
    pub async fn send_frame(&mut self, frame: &Frame) -> Result<(), WireError> {
        self.write_frame(frame).await?;
        self.flush().await
    }

    /// Get mutable reference to the underlying stream (for shutdown, etc.)
    pub fn stream_mut(&mut self) -> &mut TcpStream {
        &mut self.stream
    }
}
