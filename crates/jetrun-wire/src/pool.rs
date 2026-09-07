use std::sync::Arc;
use std::time::Duration;

use dashmap::DashMap;
use tokio::net::TcpStream;
use tokio::sync::Semaphore;
use tokio::time::timeout;

use crate::codec::FrameCodec;
use crate::error::WireError;

/// Connection pool for persistent TCP connections to a peer service.
/// Pre-opens connections and reuses them to avoid TCP handshake overhead.
pub struct ConnectionPool {
    addr: String,
    max_size: usize,
    semaphore: Arc<Semaphore>,
    idle: tokio::sync::Mutex<Vec<FrameCodec>>,
    connect_timeout: Duration,
}

impl ConnectionPool {
    pub fn new(addr: String, max_size: usize) -> Self {
        Self {
            addr,
            max_size,
            semaphore: Arc::new(Semaphore::new(max_size)),
            idle: tokio::sync::Mutex::new(Vec::with_capacity(max_size)),
            connect_timeout: Duration::from_secs(5),
        }
    }

    /// Acquire a connection from the pool.
    /// Returns an idle connection or creates a new one.
    pub async fn acquire(&self) -> Result<PooledConnection, WireError> {
        let _permit = self
            .semaphore
            .acquire()
            .await
            .map_err(|_| WireError::PoolExhausted)?;

        // Try to get an idle connection
        let codec = {
            let mut idle = self.idle.lock().await;
            idle.pop()
        };

        let codec = match codec {
            Some(c) => c,
            None => self.connect().await?,
        };

        Ok(PooledConnection { codec: Some(codec) })
    }

    /// Return a connection to the pool.
    pub async fn release(&self, codec: FrameCodec) {
        let mut idle = self.idle.lock().await;
        if idle.len() < self.max_size {
            idle.push(codec);
        }
        // If pool is full, connection is dropped (TCP FIN)
    }

    async fn connect(&self) -> Result<FrameCodec, WireError> {
        let stream = timeout(self.connect_timeout, TcpStream::connect(&self.addr))
            .await
            .map_err(|_| WireError::Timeout)?
            .map_err(WireError::Io)?;

        // Disable Nagle's algorithm for low latency
        stream.set_nodelay(true)?;

        Ok(FrameCodec::new(stream))
    }

    pub fn addr(&self) -> &str {
        &self.addr
    }
}

/// A connection checked out from the pool.
/// When dropped, the connection is NOT automatically returned — call `release()` explicitly.
pub struct PooledConnection {
    codec: Option<FrameCodec>,
}

impl PooledConnection {
    pub fn codec(&mut self) -> &mut FrameCodec {
        self.codec.as_mut().expect("connection already taken")
    }

    /// Take the codec out (for returning to pool or custom handling)
    pub fn take(mut self) -> FrameCodec {
        self.codec.take().expect("connection already taken")
    }
}

/// Multi-peer connection pool manager.
/// Maintains a pool per peer address.
pub struct PoolManager {
    pools: DashMap<String, Arc<ConnectionPool>>,
    default_pool_size: usize,
}

impl PoolManager {
    pub fn new(default_pool_size: usize) -> Self {
        Self {
            pools: DashMap::new(),
            default_pool_size,
        }
    }

    /// Get or create a connection pool for the given address.
    pub fn pool(&self, addr: &str) -> Arc<ConnectionPool> {
        self.pools
            .entry(addr.to_string())
            .or_insert_with(|| Arc::new(ConnectionPool::new(addr.to_string(), self.default_pool_size)))
            .clone()
    }
}
