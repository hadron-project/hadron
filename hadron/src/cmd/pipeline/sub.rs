//! Subscribe to a pipeline stage.

use std::sync::Arc;

use anyhow::{Context, Result};
use bytes::Bytes;
use hadron::{PipelineSubDelivery, PipelineSubscription};
use structopt::StructOpt;

use crate::Hadron;

/// Subscribe to a pipeline stage.
#[derive(StructOpt)]
#[structopt(name = "sub")]
pub struct Sub {
    /// The namespace/pipeline to which the subscription should be made.
    pipeline: String,
    /// The pipeline stage to process.
    stage: String,
}

impl Sub {
    pub async fn run(&self, base: &Hadron) -> Result<()> {
        // Destructure the target namespace & pipeline.
        let mut splits = self.pipeline.splitn(2, '/');
        let (ns, pipeline) = (splits.next().unwrap_or(""), splits.next().unwrap_or(""));

        tracing::info!("subscribing to pipeline {}/{} on stage {}", ns, pipeline, self.stage);
        let handler = Arc::new(StdoutHandler {});
        let client = base.get_client().await?;
        let sub = client.pipeline(handler, ns, pipeline, &self.stage);
        let _ = tokio::signal::ctrl_c().await;
        sub.cancel().await;
        Ok(())
    }

    /// Handle a subscription payload delivery.
    async fn handle_delivery_payload() -> Result<()> {
        Ok(())
    }
}

struct StdoutHandler {}

#[hadron::async_trait]
impl hadron::PipelineHandler for StdoutHandler {
    #[tracing::instrument(level = "debug", skip(self, payload))]
    async fn handle(&self, payload: PipelineSubDelivery) -> Result<Bytes> {
        let mut data = vec![];
        for (key, record) in payload.inputs.iter() {
            match std::str::from_utf8(&record) {
                Ok(strdata) => {
                    data.push((key, strdata));
                }
                Err(_) => {
                    data.push((key, "[binary data]"));
                }
            }
        }
        data.sort_by(|a, b| a.0.cmp(b.0));
        tracing::info!(stage = ?payload.stage, offset = payload.offset, inputs = ?data, "handling pipeline stage delivery");
        Ok(Bytes::new())
    }
}
