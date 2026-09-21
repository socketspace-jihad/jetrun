use async_trait::async_trait;
use async_nats::jetstream;

use crate::traits::{BrokerError, Message, MessageBroker, MessageStream};

/// NATS JetStream-backed message broker.
pub struct NatsBroker {
    client: async_nats::Client,
    jetstream: jetstream::Context,
}

impl NatsBroker {
    pub async fn connect(url: &str) -> Result<Self, BrokerError> {
        let client = async_nats::connect(url)
            .await
            .map_err(|e| BrokerError::Connection(e.to_string()))?;

        let jetstream = jetstream::new(client.clone());

        // Create or get the stream for repo jobs
        let _ = jetstream
            .get_or_create_stream(jetstream::stream::Config {
                name: "JETRUN".to_string(),
                subjects: vec!["jetrun.>".to_string()],
                retention: jetstream::stream::RetentionPolicy::WorkQueue,
                max_age: std::time::Duration::from_secs(86400),
                ..Default::default()
            })
            .await
            .map_err(|e| BrokerError::Connection(format!("stream setup: {}", e)))?;

        tracing::info!("connected to NATS JetStream at {}", url);

        Ok(Self { client, jetstream })
    }
}

#[async_trait]
impl MessageBroker for NatsBroker {
    async fn publish(&self, subject: &str, payload: &[u8]) -> Result<(), BrokerError> {
        self.jetstream
            .publish(subject.to_string(), payload.to_vec().into())
            .await
            .map_err(|e| BrokerError::Publish(e.to_string()))?
            .await
            .map_err(|e| BrokerError::Publish(e.to_string()))?;

        tracing::debug!(subject = %subject, size = payload.len(), "published message");
        Ok(())
    }

    async fn subscribe(&self, subject: &str) -> Result<Box<dyn MessageStream>, BrokerError> {
        let stream = self.jetstream
            .get_stream("JETRUN")
            .await
            .map_err(|e| BrokerError::Subscribe(e.to_string()))?;

        // Delete any stale consumer from previous crash loops
        let consumer_name = subject.replace('.', "-");
        let _ = stream.delete_consumer(&consumer_name).await;

        // Create fresh consumer
        let consumer = stream
            .create_consumer(jetstream::consumer::pull::Config {
                durable_name: Some(consumer_name.clone()),
                filter_subject: subject.to_string(),
                ack_policy: jetstream::consumer::AckPolicy::Explicit,
                ack_wait: std::time::Duration::from_secs(120),
                ..Default::default()
            })
            .await
            .map_err(|e| BrokerError::Subscribe(format!("create consumer: {}", e)))?;

        let messages = consumer
            .messages()
            .await
            .map_err(|e| BrokerError::Subscribe(format!("messages stream: {}", e)))?;

        tracing::info!(subject = %subject, consumer = %consumer_name, "subscribed to NATS stream");

        Ok(Box::new(NatsMessageStream { messages }))
    }

    async fn ack(&self, msg: &Message) -> Result<(), BrokerError> {
        if let Some(handle) = &msg.ack_handle {
            if let Some(nats_msg) = handle.downcast_ref::<async_nats::jetstream::Message>() {
                nats_msg
                    .ack()
                    .await
                    .map_err(|e| BrokerError::Publish(format!("ack failed: {}", e)))?;
            }
        }
        Ok(())
    }
}

struct NatsMessageStream {
    messages: jetstream::consumer::pull::Stream,
}

#[async_trait]
impl MessageStream for NatsMessageStream {
    async fn next(&mut self) -> Option<Message> {
        use futures::StreamExt;
        match self.messages.next().await {
            Some(Ok(msg)) => {
                let payload = msg.payload.to_vec();
                let subject = msg.subject.to_string();
                let id = msg
                    .info()
                    .map(|i| format!("{}", i.stream_sequence))
                    .unwrap_or_default();

                Some(Message {
                    id,
                    subject,
                    payload,
                    ack_handle: Some(Box::new(msg)),
                })
            }
            Some(Err(e)) => {
                tracing::warn!(error = %e, "NATS message error");
                None
            }
            None => None,
        }
    }
}
