use thiserror::Error;

#[derive(Debug, Error)]
pub enum WireError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Invalid frame: {0}")]
    InvalidFrame(String),

    #[error("Payload too large: {size} bytes (max {max})")]
    PayloadTooLarge { size: u32, max: u32 },

    #[error("Deserialization error: {0}")]
    Deserialize(String),

    #[error("Serialization error: {0}")]
    Serialize(String),

    #[error("Connection closed")]
    ConnectionClosed,

    #[error("Pool exhausted: no available connections")]
    PoolExhausted,

    #[error("Request timeout")]
    Timeout,
}
