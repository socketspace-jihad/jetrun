use async_trait::async_trait;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum BrokerError {
    #[error("Connection error: {0}")]
    Connection(String),

    #[error("Publish error: {0}")]
    Publish(String),

    #[error("Subscribe error: {0}")]
    Subscribe(String),

    #[error("Serialization error: {0}")]
    Serialization(String),
}

/// A message received from the broker
pub struct Message {
    pub id: String,
    pub subject: String,
    pub payload: Vec<u8>,
    /// Opaque handle for acking — broker-specific
    pub(crate) ack_handle: Option<Box<dyn std::any::Any + Send + Sync>>,
}

/// Stream of messages from a subscription
#[async_trait]
pub trait MessageStream: Send {
    async fn next(&mut self) -> Option<Message>;
}

/// Message broker trait — compile-time backend selection via feature flags.
///
/// ```bash
/// cargo build --features nats      # NATS JetStream (default)
/// cargo build --features pg-queue  # PostgreSQL job queue
/// ```
#[async_trait]
pub trait MessageBroker: Send + Sync {
    /// Publish a message to a subject/queue
    async fn publish(&self, subject: &str, payload: &[u8]) -> Result<(), BrokerError>;

    /// Subscribe to a subject/queue. Returns a stream of messages.
    async fn subscribe(&self, subject: &str) -> Result<Box<dyn MessageStream>, BrokerError>;

    /// Acknowledge a message (for at-least-once delivery).
    /// Call after successfully processing a message.
    async fn ack(&self, msg: &Message) -> Result<(), BrokerError>;
}
