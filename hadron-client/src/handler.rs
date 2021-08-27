use anyhow::Result;
use async_trait::async_trait;

use crate::grpc::stream::{NewEvent, PipelineSubscribeResponse, StreamSubscribeResponse};

/// A type capable of handling a stream subscription delivery.
#[async_trait]
pub trait StreamHandler: Send + Sync + 'static {
    /// A method to handle a stream subscription delivery.
    ///
    /// Returning a `Result::Ok` will automatically `ack` the delivery, while returning a
    /// `Result::Err` will automatically `nack` the delivery.
    async fn handle(&self, payload: StreamSubscribeResponse) -> Result<()>;
}

/// A type capable of handling a pipeline subscription delivery.
#[async_trait]
pub trait PipelineHandler: Send + Sync + 'static {
    /// A method to handle a pipeline stage subscription delivery.
    ///
    /// Returning a `Result::Ok` will automatically `ack` the delivery providing the returned
    /// Event output as the output for this pipeline instance's stage, while returning a
    /// `Result::Err` will automatically `nack` the delivery.
    async fn handle(&self, payload: PipelineSubscribeResponse) -> Result<NewEvent>;
}
