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

    #[error("Bad magic bytes: expected 'JR', got [{0:#04x}, {1:#04x}]")]
    BadMagic(u8, u8),

    #[error("Protocol version mismatch: got {got}, expected {expected}")]
    VersionMismatch { got: u16, expected: u16 },

    #[error("Connection closed")]
    ConnectionClosed,

    #[error("Pool exhausted: no available connections")]
    PoolExhausted,

    #[error("Request timeout")]
    Timeout,
}
