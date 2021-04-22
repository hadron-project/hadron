use anyhow::Result;
use async_trait::async_trait;
use proto::v1::StreamSubDelivery;

/// A type capable of handling a subscription delivery.
#[async_trait]
pub trait Handler: Send + Sync + 'static {
    async fn handle(&self, payload: StreamSubDelivery) -> Result<()>;
}
